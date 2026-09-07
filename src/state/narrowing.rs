use crate::{SearchResult, SearchType};
use process_memory::CopyAddress;

use super::simd::{get_epsilon_f32, get_epsilon_f64};

// Small, bounded reads: nearby hits share one call, sparse hits do not cause
// entire pages to be read. This is a grouping limit, not an OS page-size claim.
const READ_WINDOW: usize = 4096;
const MAX_GAP: usize = 64;
const TYPE_COUNT: usize = SearchType::StringUtf16 as usize + 1;

enum Needle {
    Any { len: usize },
    Predicate { ty: SearchType, filter: crate::NumericFilter },
    Integer { bytes: [u8; 8], len: usize },
    Float { target: f32, epsilon: f32 },
    Double { target: f64, epsilon: f64 },
}

impl Needle {
    fn len(&self) -> usize {
        match self {
            Self::Any { len } => *len,
            Self::Predicate { ty, .. } => ty.fixed_byte_length().unwrap(),
            Self::Integer { len, .. } => *len,
            Self::Float { .. } => 4,
            Self::Double { .. } => 8,
        }
    }

    fn matches(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Any { .. } => true,
            Self::Predicate { ty, filter } => filter.matches(*ty, bytes),
            Self::Integer { bytes: target, len } => bytes == &target[..*len],
            Self::Float { target, epsilon } => {
                let current = f32::from_le_bytes(bytes.try_into().expect("validated float width"));
                if current.is_finite() && target.is_finite() {
                    (current - target).abs() <= *epsilon
                } else {
                    current == *target || (current.is_nan() && target.is_nan())
                }
            }
            Self::Double { target, epsilon } => {
                let current = f64::from_le_bytes(bytes.try_into().expect("validated double width"));
                if current.is_finite() && target.is_finite() {
                    (current - target).abs() <= *epsilon
                } else {
                    current == *target || (current.is_nan() && target.is_nan())
                }
            }
        }
    }
}

pub(super) struct PreparedSearch {
    needles: [Option<Needle>; TYPE_COUNT],
    worker: Option<crate::search_task::SearchWorker>,
}

impl PreparedSearch {
    pub(super) fn numeric(filter: crate::NumericFilter) -> Self {
        Self::filtered(Some(filter), &SearchType::NUMERIC_TYPES)
    }

    pub(super) fn filtered(filter: Option<crate::NumericFilter>, types: &[SearchType]) -> Self {
        let mut needles = std::array::from_fn(|_| None);
        for &ty in types {
            if let Some(len) = ty.fixed_byte_length() {
                needles[ty as usize] = Some(match filter {
                    Some(filter) => Needle::Predicate { ty, filter },
                    None => Needle::Any { len },
                });
            }
        }
        Self { needles, worker: None }
    }

    pub(super) fn new(results: &[SearchResult], text: &str) -> Self {
        let mut needles = std::array::from_fn(|_| None);
        let mut seen = [false; TYPE_COUNT];
        for result in results {
            let ty = result.search_type;
            let index = ty as usize;
            if seen[index] {
                continue;
            }
            seen[index] = true;
            let Some(len) = ty.fixed_byte_length() else {
                continue;
            };
            let value = match ty.from_string(text) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Error converting {ty:?}: {err}");
                    continue;
                }
            };
            needles[index] = Some(match ty {
                SearchType::Float => {
                    let target = f32::from_le_bytes(value.1[..4].try_into().unwrap());
                    Needle::Float {
                        target,
                        epsilon: get_epsilon_f32(target),
                    }
                }
                SearchType::Double => {
                    let target = f64::from_le_bytes(value.1[..8].try_into().unwrap());
                    Needle::Double {
                        target,
                        epsilon: get_epsilon_f64(target),
                    }
                }
                _ => {
                    let mut bytes = [0; 8];
                    bytes[..len].copy_from_slice(&value.1);
                    Needle::Integer { bytes, len }
                }
            });
        }
        Self { needles, worker: None }
    }

    pub(super) fn with_worker(mut self, worker: crate::search_task::SearchWorker) -> Self {
        self.worker = Some(worker);
        self
    }

    pub(super) fn update_results<T: CopyAddress>(&self, old: &[SearchResult], handle: &T) -> Vec<SearchResult> {
        self.read_matching(old, handle, |_, _| true)
    }

    /// The visitor receives the input index and only fully read matching bytes.
    /// It can collect a baseline or reject values that changed since a prior read.
    pub(super) fn read_matching<T: CopyAddress>(&self, old: &[SearchResult], handle: &T, mut accept: impl FnMut(usize, &[u8]) -> bool) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let mut buffer = [0u8; READ_WINDOW];
        let mut from = 0;
        while from < old.len() {
            if self.worker.as_ref().is_some_and(|worker| worker.stopped()) {
                break;
            }
            let first = old[from];
            let Some(needle) = &self.needles[first.search_type as usize] else {
                from += 1;
                continue;
            };
            let start = first.addr;
            let Some(mut end) = start.checked_add(needle.len()) else {
                from += 1;
                continue;
            };
            let mut to = from + 1;
            // Production results are sorted. Unsorted callers remain correct:
            // stop a group at a descending address rather than reorder results.
            while let Some(next) = old.get(to) {
                let Some(next_needle) = &self.needles[next.search_type as usize] else {
                    break;
                };
                let Some(next_end) = next.addr.checked_add(next_needle.len()) else {
                    break;
                };
                if next.addr < old[to - 1].addr || next.addr.saturating_sub(end) > MAX_GAP || next_end - start > READ_WINDOW {
                    break;
                }
                end = end.max(next_end);
                to += 1;
            }

            if handle.copy_address(start, &mut buffer[..end - start]).is_ok() {
                for (index, result) in old.iter().enumerate().take(to).skip(from) {
                    let needle = self.needles[result.search_type as usize].as_ref().unwrap();
                    let offset = result.addr - start;
                    let bytes = &buffer[offset..offset + needle.len()];
                    if needle.matches(bytes) && accept(index, bytes) {
                        results.push(*result);
                    }
                }
            } else if to - from > 1 {
                // A gap, stale mapping or page boundary can make a grouped read
                // fail even though individual values remain readable. Retry each
                // hit and never compare a partially filled/failed buffer.
                for (index, result) in old.iter().enumerate().take(to).skip(from) {
                    let needle = self.needles[result.search_type as usize].as_ref().unwrap();
                    let bytes = &mut buffer[..needle.len()];
                    if handle.copy_address(result.addr, bytes).is_ok() && needle.matches(bytes) && accept(index, bytes) {
                        results.push(*result);
                    }
                }
            }
            from = to;
        }
        results
    }
}

#[cfg(test)]
mod tests;
