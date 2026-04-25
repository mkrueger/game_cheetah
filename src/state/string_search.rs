use crate::{SearchResult, SearchType};
use memchr::memmem;

fn is_ascii_printable(b: u8) -> bool {
    (0x20..=0x7E).contains(&b) || b == b'\t' || b == b'\r' || b == b'\n'
}

fn is_boundary_byte(b: u8) -> bool {
    b == 0 || !is_ascii_printable(b)
}

fn is_printable_u16(u: u16) -> bool {
    // Conservative: ASCII printable + common whitespace
    (0x20..=0x7E).contains(&u) || u == 0x09 || u == 0x0A || u == 0x0D
}

fn encode_utf16_le(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

// Replace/implement string search with alignment + boundary checks
pub(super) fn search_string_in_memory(memory_data: &[u8], search_str: &str, base_addr: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Minimum length to reduce random matches
    if search_str.len() < 3 {
        return results;
    }
    // UTF-8 exact substring with boundary checks
    let utf8_bytes = search_str.as_bytes();
    if !utf8_bytes.is_empty() {
        let finder = memmem::Finder::new(utf8_bytes);
        for pos in finder.find_iter(memory_data) {
            // Boundary before
            let ok_prev = if pos == 0 { true } else { is_boundary_byte(memory_data[pos - 1]) };
            // Boundary after
            let end = pos + utf8_bytes.len();
            let ok_next = if end >= memory_data.len() { true } else { is_boundary_byte(memory_data[end]) };

            if ok_prev && ok_next {
                results.push(SearchResult::new(base_addr + pos, SearchType::String));
            }
        }
    }

    // UTF-16LE exact substring with even alignment and boundary checks
    let utf16le_bytes = encode_utf16_le(search_str);
    if utf16le_bytes.len() >= 2 {
        let finder16 = memmem::Finder::new(&utf16le_bytes);
        for pos in finder16.find_iter(memory_data) {
            // Must be even-aligned in the buffer for UTF-16LE
            if pos % 2 != 0 {
                continue;
            }

            // Boundary before (u16)
            let ok_prev = if pos < 2 {
                true
            } else {
                let prev = u16::from_le_bytes([memory_data[pos - 2], memory_data[pos - 1]]);
                prev == 0 || !is_printable_u16(prev)
            };

            // Boundary after (u16)
            let end = pos + utf16le_bytes.len();
            let ok_next = if end + 1 >= memory_data.len() {
                true
            } else {
                let next = u16::from_le_bytes([memory_data[end], memory_data[end + 1]]);
                next == 0 || !is_printable_u16(next)
            };

            if ok_prev && ok_next {
                results.push(SearchResult::new(base_addr + pos, SearchType::StringUtf16));
            }
        }
    }

    results
}
