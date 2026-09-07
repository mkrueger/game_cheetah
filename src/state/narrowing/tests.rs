use std::{cell::RefCell, io, ops::Range};

use super::*;

const BASE: usize = 0x1000;

struct Memory {
    bytes: Vec<u8>,
    unreadable: Vec<Range<usize>>,
    reads: RefCell<Vec<(usize, usize)>>,
}

impl Memory {
    fn new(len: usize) -> Self {
        Self {
            bytes: vec![0; len],
            unreadable: Vec::new(),
            reads: RefCell::new(Vec::new()),
        }
    }

    fn put(&mut self, offset: usize, ty: SearchType, text: &str) -> SearchResult {
        let bytes = ty.from_string(text).unwrap().1;
        self.bytes[offset..offset + bytes.len()].copy_from_slice(&bytes);
        SearchResult::new(BASE + offset, ty)
    }
}

impl CopyAddress for Memory {
    fn copy_address(&self, addr: usize, buf: &mut [u8]) -> io::Result<()> {
        self.reads.borrow_mut().push((addr, buf.len()));
        let end = addr.checked_add(buf.len()).ok_or_else(|| io::Error::other("overflow"))?;
        if self.unreadable.iter().any(|range| addr < range.end && end > range.start) {
            // Simulate an OS read that modified the destination before failing.
            buf.fill(0);
            buf[0] = 42;
            return Err(io::Error::other("unreadable"));
        }
        let offset = addr.checked_sub(BASE).ok_or_else(|| io::Error::other("below base"))?;
        let source = self.bytes.get(offset..offset + buf.len()).ok_or_else(|| io::Error::other("outside memory"))?;
        buf.copy_from_slice(source);
        Ok(())
    }

    fn get_pointer_width(&self) -> process_memory::Architecture {
        process_memory::Architecture::Arch64Bit
    }
}

fn keys(results: &[SearchResult]) -> Vec<(usize, SearchType)> {
    results.iter().map(|r| (r.addr, r.search_type)).collect()
}

#[test]
fn numeric_predicates_share_reads_and_retry_unreadable_gaps() {
    let mut memory = Memory::new(128);
    let old = [
        memory.put(0, SearchType::Int, "-1"),
        memory.put(16, SearchType::Int64, "9007199254740993"),
        memory.put(32, SearchType::Double, "0.5"),
    ];
    let prepared = PreparedSearch::numeric(crate::NumericFilter::parse(crate::NumericComparison::GreaterEqual, "0", "").unwrap());
    assert_eq!(keys(&prepared.update_results(&old, &memory)), keys(&old[1..]));
    assert_eq!(memory.reads.borrow().len(), 1);
    memory.reads.borrow_mut().clear();
    memory.unreadable.push(BASE + 8..BASE + 12);
    assert_eq!(keys(&prepared.update_results(&old, &memory)), keys(&old[1..]));
    assert_eq!(memory.reads.borrow().len(), 4);
    memory.unreadable.push(BASE + 16..BASE + 24);
    assert_eq!(keys(&prepared.update_results(&old, &memory)), keys(&old[2..]));
}

#[test]
fn numeric_filter_excludes_non_numeric_and_overflowing_addresses() {
    let memory = Memory::new(0);
    let old = [SearchResult::new(BASE, SearchType::String), SearchResult::new(usize::MAX, SearchType::Int64)];
    let prepared = PreparedSearch::numeric(crate::NumericFilter::parse(crate::NumericComparison::NotEqual, "0", "").unwrap());
    assert!(prepared.update_results(&old, &memory).is_empty());
    assert!(memory.reads.borrow().is_empty());
}

// Independent per-hit reference implementation of the original comparison
// semantics. It deliberately reparses and rereads for each candidate.
fn reference(old: &[SearchResult], text: &str, memory: &Memory) -> Vec<SearchResult> {
    old.iter()
        .copied()
        .filter(|result| {
            let Ok(target) = result.search_type.from_string(text) else { return false };
            let Some(len) = result.search_type.fixed_byte_length() else { return false };
            let Ok(bytes) = process_memory::copy_address(result.addr, len, memory) else {
                return false;
            };
            match result.search_type {
                SearchType::Float => {
                    let current = f32::from_le_bytes(bytes.try_into().unwrap());
                    let target = f32::from_le_bytes(target.1.try_into().unwrap());
                    if current.is_finite() && target.is_finite() {
                        (current - target).abs() <= get_epsilon_f32(target)
                    } else {
                        current == target || (current.is_nan() && target.is_nan())
                    }
                }
                SearchType::Double => {
                    let current = f64::from_le_bytes(bytes.try_into().unwrap());
                    let target = f64::from_le_bytes(target.1.try_into().unwrap());
                    if current.is_finite() && target.is_finite() {
                        (current - target).abs() <= get_epsilon_f64(target)
                    } else {
                        current == target || (current.is_nan() && target.is_nan())
                    }
                }
                _ => bytes == target.1,
            }
        })
        .collect()
}

#[test]
fn dense_hits_share_bounded_reads() {
    let mut memory = Memory::new(8192);
    let old = (0..1024).map(|i| memory.put(i * 8, SearchType::Int, "42")).collect::<Vec<_>>();
    let actual = PreparedSearch::new(&old, "42").update_results(&old, &memory);
    assert_eq!(keys(&actual), keys(&old));
    assert_eq!(memory.reads.borrow().len(), 2);
    assert!(memory.reads.borrow().iter().all(|(_, len)| *len <= READ_WINDOW));
}

#[test]
fn sparse_hits_do_not_read_whole_pages() {
    let mut memory = Memory::new(4 * READ_WINDOW);
    let old = (0..4).map(|i| memory.put(i * READ_WINDOW, SearchType::Int, "42")).collect::<Vec<_>>();
    assert_eq!(PreparedSearch::new(&old, "42").update_results(&old, &memory).len(), 4);
    assert_eq!(memory.reads.borrow().len(), 4);
    assert!(memory.reads.borrow().iter().all(|(_, len)| *len == 4));
}

#[test]
fn unreadable_gap_falls_back_without_losing_readable_hits() {
    let mut memory = Memory::new(64);
    let old = [memory.put(0, SearchType::Int, "42"), memory.put(16, SearchType::Int, "42")];
    memory.unreadable.push(BASE + 8..BASE + 12);
    assert_eq!(keys(&PreparedSearch::new(&old, "42").update_results(&old, &memory)), keys(&old));
    assert_eq!(&*memory.reads.borrow(), &[(BASE, 20), (BASE, 4), (BASE + 16, 4)]);
}

#[test]
fn failed_partial_reads_never_produce_matches() {
    let mut memory = Memory::new(64);
    let old = [memory.put(0, SearchType::Int, "42"), memory.put(8, SearchType::Int, "99")];
    memory.unreadable.push(BASE + 8..BASE + 12);
    let prepared = PreparedSearch::new(&old, "42");
    assert_eq!(keys(&prepared.update_results(&old, &memory)), keys(&old[..1]));
    assert!(prepared.update_results(&old[1..], &memory).is_empty());
}

#[test]
fn mixed_types_unsorted_and_duplicate_hits_match_reference() {
    let mut memory = Memory::new(16384);
    let types = [
        SearchType::Byte,
        SearchType::Short,
        SearchType::Int,
        SearchType::Int64,
        SearchType::Float,
        SearchType::Double,
    ];
    let mut old = (0..1500)
        .map(|i| memory.put(i * 8 + 1, types[i % types.len()], if i % 3 == 0 { "42" } else { "99" }))
        .collect::<Vec<_>>();
    old.push(old[0]);
    old.push(SearchResult::new(old[0].addr, SearchType::Float));
    for text in ["42", "99", "-1", "300", "42.0", "invalid"] {
        let expected = reference(&old, text, &memory);
        assert_eq!(keys(&PreparedSearch::new(&old, text).update_results(&old, &memory)), keys(&expected));
    }
    old.reverse();
    assert_eq!(
        keys(&PreparedSearch::new(&old, "42").update_results(&old, &memory)),
        keys(&reference(&old, "42", &memory))
    );
}

#[test]
fn floats_preserve_epsilon_nan_infinity_and_signed_zero() {
    let mut memory = Memory::new(512);
    let mut old = Vec::new();
    for ty in [SearchType::Float, SearchType::Double] {
        for value in ["42", "42.00001", "42.01", "0", "-0", "NaN", "inf", "-inf", "1e30"] {
            old.push(memory.put(old.len() * 16, ty, value));
        }
    }
    for text in ["42", "0", "-0", "NaN", "inf", "-inf", "1e30"] {
        let expected = reference(&old, text, &memory);
        assert_eq!(keys(&PreparedSearch::new(&old, text).update_results(&old, &memory)), keys(&expected));
    }
}

#[test]
fn overlapping_types_and_window_crossing_values_are_preserved() {
    let mut memory = Memory::new(READ_WINDOW * 2);
    let wide = memory.put(READ_WINDOW - 3, SearchType::Int64, "42");
    let old = [
        memory.put(0, SearchType::Byte, "42"),
        SearchResult::new(wide.addr, SearchType::Byte),
        SearchResult::new(wide.addr, SearchType::Int),
        wide,
    ];
    assert_eq!(keys(&PreparedSearch::new(&old, "42").update_results(&old, &memory)), keys(&old));
    assert_eq!(&*memory.reads.borrow(), &[(BASE, 1), (wide.addr, 8)]);
}

#[test]
fn empty_unsupported_invalid_and_overflowing_candidates_do_not_read() {
    let memory = Memory::new(8);
    let old = [
        SearchResult::new(BASE, SearchType::String),
        SearchResult::new(BASE, SearchType::StringUtf16),
        SearchResult::new(BASE, SearchType::Unknown),
        SearchResult::new(BASE, SearchType::Guess),
        SearchResult::new(usize::MAX, SearchType::Double),
    ];
    assert!(PreparedSearch::new(&old, "42").update_results(&old, &memory).is_empty());
    assert!(PreparedSearch::new(&[], "42").update_results(&[], &memory).is_empty());
    let invalid = [SearchResult::new(BASE, SearchType::Byte)];
    assert!(PreparedSearch::new(&invalid, "256").update_results(&invalid, &memory).is_empty());
    assert!(memory.reads.borrow().is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn real_short_read_across_protected_page_cannot_match_stale_bytes() {
    use process_memory::TryIntoProcessHandle;

    struct Mapping(*mut libc::c_void, usize);
    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: This guard owns the mapping until drop, including its
            // protected page. No references to its contents are created.
            unsafe { libc::munmap(self.0, self.1) };
        }
    }

    // SAFETY: sysconf has no pointer arguments and only queries the page size.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page > 0);
    let page = page as usize;
    // SAFETY: Request a fresh anonymous mapping; the returned pointer is checked
    // before use and owned by the guard. Anonymous pages are zero initialized.
    let pointer = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            2 * page,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(pointer, libc::MAP_FAILED);
    let mapping = Mapping(pointer, 2 * page);
    let base = mapping.0 as usize;
    // SAFETY: The second page is page-aligned and belongs to this mapping.
    assert_eq!(unsafe { libc::mprotect((base + page) as *mut libc::c_void, page, libc::PROT_NONE) }, 0);
    let handle = (std::process::id() as process_memory::Pid).try_into_process_handle().unwrap();
    let reader = super::super::memory_reader::ExactProcessReader(&handle);
    let mut buffer = [0u8; 24];
    let error = reader.copy_address(base + page - 8, &mut buffer).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    let old = [
        SearchResult::new(base + page - 8, SearchType::Int),
        SearchResult::new(base + page + 8, SearchType::Int),
    ];
    // A zero-filled or stale tail would incorrectly match the inaccessible hit.
    assert_eq!(keys(&crate::update_results(&old, "0", &handle)), keys(&old[..1]));
}
