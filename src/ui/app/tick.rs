//! Per-frame housekeeping and the bulk change tracker.
//!
//! - [`App::tick`] runs each frame before the view code: suppresses
//!   debug overlays, refreshes the process list, drives the in-process
//!   change tracker, and schedules the next repaint.
//! - [`App::update_change_tracker`] does the actual byte-level diffing
//!   of the active search's results, in throttled, round-robin,
//!   page-grouped chunks.

use std::{
    collections::BTreeMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use process_memory::{ProcessHandle, TryIntoProcessHandle, copy_address};

use super::{App, AppState, CHANGE_HIGHLIGHT};
use crate::{SearchMode, SearchType};

/// Cadence at which the bulk change tracker runs. The UI repaints at ~30 Hz
/// for fluid live values, but humans can't perceive value flicker faster
/// than ~10 Hz — reading the process at the repaint rate just wastes
/// syscalls.
const TRACKER_INTERVAL: Duration = Duration::from_millis(100);

/// Per-tick budget for the round-robin bulk tracker. Each tick scans this
/// many *result rows* starting from a wrapping cursor; combined with
/// page-grouped reads this cycles through tens of thousands of addresses
/// well within a second on dense result sets.
const TRACKER_WINDOW: usize = 4096;

/// 4 KiB — the page size we bucket bulk-tracker addresses by so dense pages
/// are covered with one `copy_address` instead of one read per address.
const TRACKER_PAGE: usize = 4096;

/// Default-construct a tracker `last_run` timestamp that's safely in the
/// past so the first call doesn't get throttled.
pub(super) fn idle_last_run() -> Instant {
    Instant::now() - TRACKER_INTERVAL * 2
}

impl App {
    /// Periodic per-frame housekeeping run before the view code.
    pub(super) fn tick(&mut self, ctx: &egui::Context) {
        // Suppress egui's built-in debug paint overlays. Two of them are
        // enabled by default in debug builds and produce noisy red strokes
        // and orange "Unaligned" markers all over the result table during
        // scrolling:
        //
        // * `warn_if_rect_changes_id` — paints a 2 px **red** rect-stroke
        //   when the same screen rect appears with a different widget id
        //   between passes. `egui_extras::TableBuilder` recycles row ids
        //   as rows scroll in/out of view, which trips this constantly.
        //
        // * `show_unaligned` — paints orange "Unaligned" tick marks on
        //   widgets whose rect isn't pixel-aligned to the GUI rounding
        //   grid. Equally noisy during fractional scroll offsets.
        //
        // The `debug` field on `Style` is only present in debug builds,
        // so this whole block is cfg-gated.
        #[cfg(debug_assertions)]
        {
            let debug = ctx.global_style().debug;
            if debug.warn_if_rect_changes_id || debug.show_unaligned {
                ctx.global_style_mut(|style| {
                    style.debug.warn_if_rect_changes_id = false;
                    style.debug.show_unaligned = false;
                });
            }
        }

        // Search context state machine: cycle each context's search-mode
        // flag back to `None` once `search_complete` flips.
        for search_context in &mut self.state.searches {
            search_context.update_search_mode();
        }

        match self.app_state {
            AppState::ProcessSelection => {
                if self.last_process_refresh.elapsed() >= Duration::from_millis(1000) {
                    self.state.update_process_data();
                    self.last_process_refresh = Instant::now();
                }
                // Schedule the next repaint to coincide with the next
                // cache refresh. The list is cached, so painting more
                // often just wastes CPU.
                ctx.request_repaint_after(Duration::from_millis(1000));
            }
            AppState::InProcess => {
                self.state.detach_if_gone();
                if self.auto_reconnect
                    && self.state.pid == 0
                    && !self.state.process_name.is_empty()
                    && self.last_reattach_attempt.elapsed() >= Duration::from_secs(1)
                {
                    self.last_reattach_attempt = Instant::now();
                    self.state.update_process_data();
                    let target = self.state.process_name.clone();
                    if let Some(process) = self.state.processes.iter().find(|p| p.name == target).cloned() {
                        self.state.select_process(&process);
                    }
                }
                // Finalize in-flight searches so the tracker refresh below
                // sees the final result set.
                {
                    let ctx_search = &mut self.state.searches[self.state.current_search];
                    if !matches!(ctx_search.searching, SearchMode::None) {
                        let _ = ctx_search.collect_results();
                    }
                    if ctx_search.search_complete.load(Ordering::SeqCst) {
                        // Drain trailing batches so addresses don't keep
                        // shifting after the search ends.
                        loop {
                            let before = ctx_search.get_result_count();
                            let _ = ctx_search.collect_results();
                            let after = ctx_search.get_result_count();
                            if before == after {
                                break;
                            }
                        }
                        ctx_search.searching = SearchMode::None;
                    }
                }

                let search_running = self.state.searches.iter().any(|s| !matches!(s.searching, SearchMode::None));
                if !search_running && self.state.is_process_running() {
                    self.update_change_tracker();
                }

                // 30 Hz repaint for fluid live values.
                ctx.request_repaint_after(Duration::from_millis(33));
            }
            AppState::MemoryEditor => {
                self.memory_editor.tick(self.state.pid as process_memory::Pid);
                ctx.request_repaint_after(Duration::from_millis(33));
            }
            _ => {}
        }

        // Prune expired change highlights so rows return to their default
        // appearance at the next frame (and the tracker doesn't grow
        // unboundedly while scanning long sessions).
        self.changed_addresses.retain(|_, t| t.elapsed() < CHANGE_HIGHLIGHT);
    }

    /// Read current values for all results in the active search and record
    /// which addresses changed since the last call.
    ///
    /// The tracker has four important properties that together let it scale
    /// past the previous "give up over 4096 rows" behaviour:
    ///
    /// * **Throttled** — it runs at most every `TRACKER_INTERVAL` (10 Hz),
    ///   regardless of the UI repaint rate.
    /// * **Round-robin** — it scans `TRACKER_WINDOW` rows per invocation
    ///   starting from a wrapping cursor, so arbitrarily large result sets
    ///   eventually get covered.
    /// * **Page-grouped** — addresses inside the same 4 KiB page share a
    ///   single `copy_address` call. For dense result sets that's an order-
    ///   of-magnitude syscall reduction over per-address reads.
    /// * **Byte-diffed** — the cache stores raw process bytes; we never
    ///   format-then-string-compare just to detect a change.
    fn update_change_tracker(&mut self) {
        if self.last_change_tracker_run.elapsed() < TRACKER_INTERVAL {
            return;
        }
        self.last_change_tracker_run = Instant::now();

        let search_index = self.state.current_search;
        let results = self.state.searches[search_index].collect_results();
        let total = results.len();
        if total == 0 {
            return;
        }

        let pid = self.state.pid;
        let Some(handle) = self.ensure_process_handle(pid) else {
            return;
        };

        let search_value_text = self.state.searches[search_index].search_value_text.clone();
        let string_byte_len = search_value_text.len().max(1);
        let string_char_count = search_value_text.chars().count().max(1);

        // Tracking byte width per type. Strings get the user-typed string's
        // byte/unit count as the read window.
        let bytes_for = |ty: SearchType| -> usize {
            if let Some(n) = ty.fixed_byte_length() {
                return n;
            }
            match ty {
                SearchType::String => string_byte_len,
                SearchType::StringUtf16 => string_char_count.saturating_mul(2),
                _ => 0,
            }
        };

        // Round-robin window of the result list.
        let start = self.change_tracker_cursor.min(total.saturating_sub(1));
        let end = start.saturating_add(TRACKER_WINDOW).min(total);
        let wrapped = end == total;
        let window = &results[start..end];

        // Group window addresses by 4 KiB page so one syscall covers all
        // hits sharing a page.
        let mut by_page: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, r) in window.iter().enumerate() {
            by_page.entry(r.addr & !(TRACKER_PAGE - 1)).or_default().push(i);
        }

        let now = Instant::now();

        for (_page_base, idxs) in by_page.iter() {
            // Compute the [min, max) span covering every address in this page
            // bucket. The bucket already shares a 4 KiB page so the span is
            // bounded above by ~PAGE + max-type-len.
            let mut min_addr = usize::MAX;
            let mut max_end = 0usize;
            for &i in idxs {
                let r = &window[i];
                let len = bytes_for(r.search_type);
                if len == 0 {
                    continue;
                }
                min_addr = min_addr.min(r.addr);
                max_end = max_end.max(r.addr.saturating_add(len));
            }
            if min_addr == usize::MAX || max_end <= min_addr {
                continue;
            }
            // Cap span at 2 pages so a single very-long string entry can't
            // pull a huge read window.
            let span = (max_end - min_addr).min(TRACKER_PAGE * 2);

            let Ok(buf) = copy_address(min_addr, span, &handle) else { continue };

            for &i in idxs {
                let r = &window[i];
                let len = bytes_for(r.search_type);
                if len == 0 {
                    continue;
                }
                let offset = r.addr.wrapping_sub(min_addr);
                if offset + len > buf.len() {
                    continue;
                }
                let bytes = buf[offset..offset + len].to_vec();
                if let Some(prev) = self.value_change_tracker.put(r.addr, bytes.clone())
                    && prev != bytes
                {
                    self.changed_addresses.insert(r.addr, now);
                }
            }
        }

        self.change_tracker_cursor = if wrapped { 0 } else { end };

        // Only prune after a full sweep — otherwise we'd repeatedly drop
        // entries the round-robin still needs to track.
        if wrapped {
            let live: std::collections::HashSet<usize> = results.iter().map(|r| r.addr).collect();
            self.value_change_tracker.retain(|addr| live.contains(addr));
            self.changed_addresses.retain(|addr, _| live.contains(addr));
        }
    }

    /// Return (a copy of) the cached `ProcessHandle` for `pid`, opening a
    /// fresh one only when the pid has changed since last call. `ProcessHandle`
    /// is `Copy` on all supported platforms so handing back a copy is cheap;
    /// the cache simply avoids the per-tick `try_into_process_handle()` call.
    fn ensure_process_handle(&mut self, pid: process_memory::Pid) -> Option<ProcessHandle> {
        let matches = matches!(self.cached_process_handle, Some((p, _)) if p == pid);
        if !matches {
            let handle = pid.try_into_process_handle().ok()?;
            self.cached_process_handle = Some((pid, handle));
        }
        self.cached_process_handle.map(|(_, h)| h)
    }
}
