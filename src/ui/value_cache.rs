//! Tiny LRU cache mapping `addr -> last-seen value`.
//!
//! Used by the in-process view's change tracker so the per-row "what did this
//! value used to be?" lookup stays bounded even on million-result searches.
//! Every `put` refreshes the entry's recency; once the cache exceeds
//! `capacity`, the least-recently-touched entry is evicted in `O(log n)` via
//! a secondary tick-indexed `BTreeMap`.
//!
//! Generic over the stored value `V` so callers can pick the cheapest
//! representation — e.g. raw `Vec<u8>` for byte-level diffs of process memory.
//! Only what the call sites actually use is exposed; this is intentionally
//! small, no iterators or fancy APIs.

use std::collections::{BTreeMap, HashMap};

pub struct ValueCache<V> {
    map: HashMap<usize, (V, u64)>,
    /// Secondary index ordered by `tick`, used for O(log n) eviction of the
    /// oldest entry. The two indexes are kept in sync by every mutating op.
    by_tick: BTreeMap<u64, usize>,
    capacity: usize,
    tick: u64,
}

impl<V> ValueCache<V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity + 1),
            by_tick: BTreeMap::new(),
            capacity,
            tick: 0,
        }
    }

    /// Insert or update `addr -> value`. Returns the previous value for
    /// this key if any. Evicts the oldest entry when the cache exceeds
    /// `capacity` on a brand-new insertion.
    pub fn put(&mut self, addr: usize, value: V) -> Option<V> {
        self.tick = self.tick.wrapping_add(1);
        let new_tick = self.tick;
        let prev = self.map.insert(addr, (value, new_tick));
        if let Some((_, old_tick)) = &prev {
            self.by_tick.remove(old_tick);
        }
        self.by_tick.insert(new_tick, addr);
        let prev_value = prev.map(|(v, _)| v);
        if prev_value.is_none() && self.map.len() > self.capacity
            && let Some((&oldest_tick, &oldest_addr)) = self.by_tick.iter().next()
        {
            self.by_tick.remove(&oldest_tick);
            self.map.remove(&oldest_addr);
        }
        prev_value
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.by_tick.clear();
        self.tick = 0;
    }

    /// Drop entries whose key does not satisfy `predicate`. Keeps the two
    /// indexes in sync.
    pub fn retain(&mut self, mut predicate: impl FnMut(&usize) -> bool) {
        let mut dropped_ticks: Vec<u64> = Vec::new();
        self.map.retain(|k, (_, t)| {
            if predicate(k) {
                true
            } else {
                dropped_ticks.push(*t);
                false
            }
        });
        for t in dropped_ticks {
            self.by_tick.remove(&t);
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_returns_previous_value() {
        let mut cache: ValueCache<String> = ValueCache::new(4);
        assert_eq!(cache.put(0x10, "one".into()), None);
        assert_eq!(cache.put(0x10, "two".into()).as_deref(), Some("one"));
        assert_eq!(cache.put(0x10, "two".into()).as_deref(), Some("two"));
    }

    #[test]
    fn evicts_oldest_entry_when_over_capacity() {
        let mut cache: ValueCache<String> = ValueCache::new(2);
        cache.put(1, "a".into());
        cache.put(2, "b".into());
        cache.put(3, "c".into()); // should evict key 1
        assert_eq!(cache.len(), 2);
        // Re-insert 1 — should report no previous value because it was evicted.
        assert_eq!(cache.put(1, "a2".into()), None);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn put_refreshes_recency_so_evicted_key_is_oldest_untouched() {
        let mut cache: ValueCache<String> = ValueCache::new(2);
        cache.put(1, "a".into());
        cache.put(2, "b".into());
        // Touch key 1 — now key 2 is the oldest.
        cache.put(1, "a2".into());
        cache.put(3, "c".into()); // should evict key 2, not key 1
        // Key 1 should still be present.
        assert_eq!(cache.put(1, "a3".into()).as_deref(), Some("a2"));
        // Key 2 should be gone — put returns None.
        assert_eq!(cache.put(2, "b2".into()), None);
    }

    #[test]
    fn retain_keeps_both_indexes_consistent() {
        let mut cache: ValueCache<String> = ValueCache::new(8);
        for i in 0..6 {
            cache.put(i, format!("v{i}"));
        }
        cache.retain(|k| *k % 2 == 0);
        assert_eq!(cache.len(), 3);
        // Force an eviction-triggering scenario to ensure by_tick is in sync.
        for i in 100..106 {
            cache.put(i, format!("v{i}"));
        }
        assert_eq!(cache.len(), 8);
    }

    #[test]
    fn clear_resets_state() {
        let mut cache: ValueCache<String> = ValueCache::new(4);
        cache.put(1, "a".into());
        cache.put(2, "b".into());
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.put(1, "a".into()), None);
    }

    #[test]
    fn works_with_byte_slice_values() {
        let mut cache: ValueCache<Vec<u8>> = ValueCache::new(4);
        assert_eq!(cache.put(1, vec![0x1B, 0, 0, 0]), None);
        let prev = cache.put(1, vec![0x1C, 0, 0, 0]);
        assert_eq!(prev.as_deref(), Some(&[0x1B, 0, 0, 0][..]));
    }
}
