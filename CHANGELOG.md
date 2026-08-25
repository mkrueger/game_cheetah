# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The memory editor fills the available width: the byte count per row adapts
  to the window in whole 8-byte groups, keeping the top address in place when
  the window is resized.
- Memory editor keyboard navigation gained Home/End for the current row,
  Ctrl+Home/Ctrl+End for the region, and Page Up/Page Down.
- The memory editor inspector can be collapsed to give the hex grid its
  height back, switches byte order from its header, marks the row matching
  the search type, and adds `hex` plus a `ptr` row that resolves the value
  against the target's memory map. Value fields grow with the column and only
  show their frame on hover or focus.

### Changed

- The result table extends an empty interaction column to the window edge, so
  wheel scrolling works across the otherwise-unused space while data columns
  remain compact. The search field is no longer stretched across the window,
  and the hit count is rendered prominently. Result lists too large to browse
  are collapsed behind a hint pointing at the next useful step, with an option
  to show them anyway.
- Search input, data type, actions and the compact result count now share one
  wrapping toolbar instead of being split across a card and a second row.
  Text table headings are bold and left-aligned with their data.
- Search tabs show their close icon only on hover, while the trailing `+`
  icon uses the same height and hit target as the tabs. Rename guidance stays
  in the tab tooltip.
- The process topbar now shows a compact `name · PID` identity, treats Save
  and Load as secondary actions, and separates Close visually. Save/Load
  status moved out of the bar into a dismissible four-second toast.
- In-process keyboard controls now update a search with Enter, cancel result
  editing with Escape, undo the last search with Ctrl/Cmd+Z, and remove the
  hovered result row with Delete.
- Empty searches and searches without hits now show a concise next-step
  message, centered prominently in the remaining window, and return keyboard
  focus to the value field.
- Result rows carry their actions as icons that appear on hover — × next to
  the address removes the row, ✏ next to the value opens the memory editor —
  now limited to their respective cells. A row context menu copies address or
  value, toggles freeze, or opens the memory editor; double-clicking an address
  opens it directly. Both the header and each row use a clickable ❄ for
  freeze/unfreeze, with the wording moved into tooltips.
- CI runs the checks and tests on every commit; Linux, Windows and macOS
  packages are now built when a `v*` tag is pushed and attached to that tag's
  release. The dependency audit moved to a weekly schedule.
- Updated dependencies: egui/eframe 0.36, sysinfo 0.39, proc-maps 0.5, ureq 3
  and the remaining crates to their current releases. The minimum supported
  Rust version is now 1.89.

### Fixed

- Escape no longer closes the memory editor. It now does what the inspector
  advertises: cancel the current edit, or clear the hex grid selection. The
  editor is left through its Close button.
- Memory editor rows are drawn at the exact height the scroll area advances
  by, so "Origin", arrow keys and paging land where they should instead of
  drifting. Rows are tighter, the caret is always scrolled into view with the
  smallest possible movement, and the bottom edge no longer leaves a blank
  strip where the next row belongs.
- Opening the memory editor and jumping back to Origin now center the origin
  row after adaptive column layout, so the highlighted address is visible on
  the first frame.
- Result rows always show the value read from the process while drawing. A
  cell could stay frozen on an old edit buffer — its change highlight still
  flashed, but the number only refreshed after focusing and unfocusing it.
- Aggregated process rows can now be expanded to attach to an individual
  member process. Previously only the group representative was searchable,
  so memory of the other processes (e.g. Chrome's renderer processes) could
  not be reached ([#27](https://github.com/mkrueger/game_cheetah/issues/27)).

## [0.7.0] - 2026-05-14

Feature release focused on in-process result-table responsiveness, changed-value visibility, and memory editor polish.

### Added

- In-process result rows now use a bounded LRU value cache, keeping live
  change tracking responsive without unbounded growth on very large result
  sets.
- Changed values in the in-process result table now fade out with the same
  orange highlight style as the memory editor.
- Optional GitHub release update checks can notify when a newer release is
  available and link to the latest release page.

### Changed

- In-process change tracking now runs at a throttled 10 Hz cadence, scans
  large result sets in round-robin windows, groups dense addresses by 4 KiB
  page, compares raw bytes instead of formatted strings, and reuses the
  target process handle between ticks.
- Visible result rows now always read fresh bytes before comparing against
  the previous-value cache, fixing stale display and change-detection edge
  cases.
- Memory editor ASCII rendering now uses painter-based cell drawing so
  highlighted ASCII bytes have seamless backgrounds without per-glyph
  borders.

### Fixed

- Result list values no longer flicker / briefly disappear during refresh.
  The virtualized scroll area used to rebuild every visible row 30 times a
  second because the cache key was tied to the refresh tick counter; it now
  hashes the actual cached value strings, freeze set and highlight set so
  rows only re-create when their displayed content really changed.
- One-frame blank rows the instant a search finished are gone. The tick
  handler now finalises in-flight searches *before* refreshing the live
  value cache, and it no longer wipes the cache when a scan starts — the
  previously read values persist across the scan so the first frame after
  it ends already has cached strings for every surviving address.
- String result rows now share the same live-value cache as numeric rows,
  so they no longer briefly read blank when a memory read transiently
  fails.
- Debug-build red/orange egui paint overlays no longer appear around
  process-view table rows while scrolling.

## [0.6.2] - 2026-05-09

Patch release focused on memory editor responsiveness and per-result data type editing.

### Added

- Memory editor toolbar now shows the opened search hit address (read-only) alongside a data-type pick list. Switching the type re-tags that result in the cached result list so the same address can be viewed/written as a different numeric type without re-scanning (issue #26).
- Inspector row matching the selected data type is highlighted so the active interpretation is obvious at a glance.
- Hex grid uses a custom focusable widget so it actually owns keyboard focus and draws a real focus ring around the scroll area. Click inside the grid to focus, click outside to blur.
- Fade-tick subscription drives change-highlight animations even when target memory is idle, so highlights smoothly fade out instead of waiting for the next memory refresh.

### Changed

- Memory editor reads visible rows in contiguous spans during the editor tick instead of issuing one `copy_address` per row per redraw, dropping the cost of a typical viewport from ~25 syscalls to roughly one per mapped region.
- Hex / inspector / undo / redo writes update the visible cache immediately instead of waiting for the next refresh tick, so direct edits no longer feel delayed.
- Small refinement passes (≤1024 addresses) now update inline on the UI thread; the worker-thread overhead was dominating the actual memory reads and left the UI sitting on "Updating N/N" until the next tick.

### Fixed

- Memory editor keyboard handling: arrow keys, page up/down, hex digits, Enter, Escape, and `Ctrl/Cmd+Z`/`Shift+Ctrl/Cmd+Z` now reliably reach the grid via the focusable area instead of being filtered by the global keyboard subscription.
- Default focus ring no longer appears as a tiny circle in the corner; the grid now draws a proper border around the scrollable area when focused.
- Escape key now reliably dismisses the About, Settings, and process picker dialogs even when a focused widget (e.g. the process filter input) had captured the event.

## [0.6.1] - 2026-05-08

Patch release focused on release packaging, cheat-table polish, and post-0.6.0 fixes.

### Added

- Cheat tables can now be saved and loaded as TOML files per target process.
- Result values can be toggled between decimal and hexadecimal display.
- Frozen rows now show a lock cue and a highlighted row style.
- Result rows briefly highlight when their live value changes.
- Search tabs can be renamed by double-clicking the tab label.
- Optional automatic reconnect is available as an off-by-default setting from the main menu.
- The Settings page shows the resolved configuration directory and explains how it is chosen.
- The Settings page now provides an Open button that reveals the configuration directory in the system file manager (powered by the `opener` crate).
- The Settings page now provides a Copy path button for the configuration directory.
- CI now checks i18n key parity between English and German translations.

### Changed

- Main menu actions now use consistent widths, with Start emphasized as the primary action and version/GitHub moved into a quieter footer.
- English UI labels now use more consistent sentence-style capitalization.
- Configuration directory resolution now uses the `dirs` crate. On Linux it moves from `~/.game-cheetah` to `~/.config/game-cheetah`, on macOS to `~/Library/Application Support/game-cheetah`, and on Windows to `%APPDATA%\game-cheetah`. Existing cheat tables under the old `.game-cheetah` directory must be moved manually.
- Cheat-table loading now validates the saved process name and schema version before loading addresses.
- Freeze state is no longer persisted in cheat tables. Freezing is treated as runtime state and must be re-enabled explicitly after loading.
- Result-table live value rendering reuses the change-tracker cache when available, reducing duplicate memory reads.
- Per-row change tracking is skipped during active searches and capped for large result sets to keep the UI responsive.
- Error reporting now uses a typed bounded error queue with a dismissible in-process error banner instead of a single global string.
- Region chunking for memory scans is shared through a helper to keep string, numeric, and snapshot scan behavior consistent.
- Updated dependencies where compatible with the project MSRV; `sysinfo` is pinned to `0.38.4` because `0.39.x` requires Rust 1.95.

### Fixed

- The English result table heading now says "Frozen" instead of "Freezed".
- macOS release bundles no longer appear damaged/crossed-out in Finder: `CFBundleExecutable` now matches the packaged binary, bundle versions are patched during CI, and the final universal app is ad-hoc signed after `lipo`.
- The committed macOS `Info.plist` version is checked against Cargo metadata by a regression test.
- Removed a committed Cargo config that forced Windows cross-compilation as the default target.
- Removed unused dependencies (`threadpool`, `sudo`, `boyer-moore-magiclen`).
- `bytes` was updated to fix RUSTSEC-2026-0007.
- CI Linux `.deb` packaging no longer masks `cargo deb` failures with `export DEB=$(...)`.
- German translations were added for save/load and hex display UI strings.

## [0.6.0] - 2026-04-25

A robustness, performance, and UX release.

### Added

- Result table now uses a virtual row renderer, so only visible rows are laid out instead of prebuilding every result row. Replaces the previous hard cap of 1000 visible results without requiring pagination.
- Memory editor uses the same virtual row renderer over the target process' readable memory regions, skipping unmapped holes while preserving region permissions and names in the inspector. Cursor navigation, page up/down, and address jumps now scroll smoothly with viewport-aware "ensure visible" behavior.
- Memory editor value inspector fields are editable. Users can write typed integer and floating-point values at the cursor address directly from the inspector. Invalid input is highlighted in the destructive color until it parses cleanly.
- Memory editor region info now sits in its own band above the hex grid, with a compact status strip below the grid showing the focused address and offset from the opened search hit (when applicable).
- Memory editor highlights bytes that change in the target process: each visible byte that updates is briefly tinted in the destructive color and fades back to normal, making it obvious which addresses the game is currently writing to.
- Memory editor undo/redo: `Ctrl/Cmd+Z` undoes the most recent edit (hex digit or inspector value), `Shift+Ctrl/Cmd+Z` redoes it. The cursor jumps to the affected address so the change is visible.
- Memory editor strings (toolbar, header, status strip, region access labels, and error messages) are now localized through Fluent and ship with English and German translations.
- Process selection list now defaults to sorting by memory size, descending. Users can still click any column header to override.
- Attach now probes the target process and surfaces a platform-specific hint when access is denied (Linux: `kernel.yama.ptrace_scope`; macOS: `task_for_pid` entitlements; Windows: Administrator / protected processes).
- Process exit / PID recycling detection. The engine captures the target's start time at attach and refuses to keep operating against a recycled PID, so the freeze loop can no longer write into an unrelated process when the target restarts.
- Criterion benchmark suite for the search hot paths, including a multi-region rayon macrobenchmark.
- Edge-case unit tests for the unknown-search comparator and the unique- vs shared-Arc finalization paths.

### Changed

- **No longer published on crates.io.** Game Cheetah depends on `icy_ui` (git-only), and `cargo publish` requires every dependency to be on crates.io. Until `icy_ui` is published, install via the prebuilt binaries on the [GitHub releases page](https://github.com/mkrueger/game_cheetah/releases/latest) or build from source. `cargo install game-cheetah` will keep working for older 0.5.x versions but will not pick up new releases.
- Float SIMD scan now uses the portable `wide` crate instead of x86\_64-only intrinsics. Roughly 15× faster on f32 / f64 scans and now runs on aarch64.
- Integer scans (`i16`, `i32`, `i64`, byte) use `memchr::memmem` (Two-Way + SIMD prefilter) in place of hand-rolled SIMD.
- `Guess` mode parses the user-typed needle once before the parallel region scan instead of re-parsing it inside every region's closure.
- `SearchResult` shrunk to `addr + search_type`. Previous-value bookkeeping for the unknown-search filter moved into a separate `(addr, type) -> [u8; 8]` table on the search context.
- Result channels are bounded (capacity 128) to apply backpressure on producers when the UI side falls behind.
- `state.rs` split into focused submodules (`memory_reader`, `simd`, `string_search`, `unknown`, `diagnostics`).

### Fixed

- Failed memory writes are no longer silent. Edits, hex-cell writes, and the in-process value editor now route their errors through `state.error_text` so the user can see when a write is refused (process gone, region not writable, attach lost).
- `SearchValue` `Display` no longer panics on malformed byte vectors. Returns `<invalid>` instead.
- Unknown-search finalization no longer panics on empty per-page chunks or on `Arc::try_unwrap` failure. Poisoned mutex contents are recovered instead of dropped.
- `SearchType::get_byte_length` (which could panic on `Guess`/`Unknown`/`String`) replaced with a fallible `fixed_byte_length`.
- Closing a search no longer leaves `current_search` pointing past the end of the list.
- Process refresh clock rollback is handled gracefully.
- Channel-send failures (the freeze thread dying) are no longer swallowed; they surface through `state.error_text`.
- Freeze loop retires individual addresses that fail to write `MAX_FREEZE_FAILURES` times in a row instead of retrying forever.
- `MemoryEditorJumpToAddress` reports invalid hex input through `state.error_text` and accepts the `0X` prefix.
- `Guess` payload UTF-8 round-trip no longer silently produces an empty needle list on malformed bytes.

### Internal

- All `unsafe` blocks now have explicit SAFETY comments documenting their invariants.
- CI: modernized Rust toolchain setup, unified release artifact versioning, joined CI workflows.

## [0.5.2] - 2025-12-27

Last release on the iced UI stack; switched workspace to `icy_ui`.
