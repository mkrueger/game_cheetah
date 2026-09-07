//! Linux read-only diagnostic scanner, deliberately independent of the UI.
//! Usage: find_pointer_chain PID HEX_TARGET [HEX_TARGET ...]
//! 64-bit little-endian, aligned pointers, positive offsets <= 0x1000,
//! <= 4 dereferences. Does not stop the game or write its memory/table.
use std::{collections::BTreeMap, fs::File, io, os::unix::fs::FileExt};

const MAX_OFFSET: u64 = 0x1000;
const MAX_BYTES: usize = 512 * 1024 * 1024;
const MAX_POINTERS: usize = 8_000_000;
const MAX_FRONTIER: usize = 100_000;

#[derive(Debug)]
struct Mapping {
    start: u64,
    end: u64,
    offset: u64,
    readable: bool,
    writable: bool,
    executable: bool,
    path: String,
}

fn mappings(pid: u32) -> io::Result<Vec<Mapping>> {
    std::fs::read_to_string(format!("/proc/{pid}/maps"))?
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            let (start, end) = fields.next().unwrap().split_once('-').unwrap();
            let perms = fields.next().unwrap().as_bytes();
            let offset = u64::from_str_radix(fields.next().unwrap(), 16).unwrap();
            fields.next();
            fields.next();
            Ok(Mapping {
                start: u64::from_str_radix(start, 16).unwrap(),
                end: u64::from_str_radix(end, 16).unwrap(),
                offset,
                readable: perms[0] == b'r',
                writable: perms[1] == b'w',
                executable: perms[2] == b'x',
                path: fields.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

fn mapped(maps: &[Mapping], address: u64) -> bool {
    let i = maps.partition_point(|map| map.start <= address);
    i > 0 && maps[i - 1].readable && address < maps[i - 1].end
}

fn module_root<'a>(maps: &'a [Mapping], address: u64) -> Option<(&'a str, u64)> {
    let map = maps.iter().find(|map| map.start <= address && address + 8 <= map.end)?;
    if !map.path.starts_with('/') || map.path.ends_with(" (deleted)") {
        return None;
    }
    if !maps.iter().any(|other| other.path == map.path && other.executable) {
        return None;
    }
    let mut roots = maps.iter().filter(|other| other.path == map.path && other.offset == 0);
    let base = roots.next()?.start;
    if roots.next().is_some() {
        return None;
    }
    Some((&map.path, address.checked_sub(base)?))
}

fn follow(mem: &File, root: u64, offsets: &[u64]) -> io::Result<u64> {
    let mut location = root;
    for offset in offsets {
        let mut bytes = [0; 8];
        mem.read_exact_at(&mut bytes, location)?;
        let pointer = u64::from_le_bytes(bytes);
        if pointer == 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        location = pointer.checked_add(*offset).ok_or(io::ErrorKind::InvalidData)?;
    }
    Ok(location)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let pid: u32 = args.next().ok_or("PID required")?.parse()?;
    let targets: Vec<u64> = args
        .map(|arg| u64::from_str_radix(arg.trim_start_matches("0x"), 16))
        .collect::<Result<_, _>>()?;
    if targets.is_empty() {
        return Err("At least one target required".into());
    }
    let maps = mappings(pid)?;
    let mem = File::open(format!("/proc/{pid}/mem"))?;
    for target in &targets {
        let mut value = [0; 4];
        mem.read_exact_at(&mut value, *target)?;
        println!("TARGET 0x{target:X} Int32={}", i32::from_le_bytes(value));
    }
    let mut pointers: Vec<(u64, u64)> = Vec::new();
    let mut bytes_read = 0;
    let mut failed_pages = 0;
    let mut capped = false;
    // Anonymous/heap plus image-backed writable storage; exclude device mappings.
    'scan: for map in maps
        .iter()
        .filter(|m| m.readable && m.writable && !m.path.starts_with("/dev/") && !m.path.contains("(deleted)"))
    {
        for start in (map.start..map.end).step_by(4096) {
            if bytes_read >= MAX_BYTES || pointers.len() >= MAX_POINTERS {
                capped = true;
                break 'scan;
            }
            let len = (map.end - start).min(4096) as usize;
            let mut page = [0; 4096];
            if mem.read_exact_at(&mut page[..len], start).is_err() {
                failed_pages += 1;
                continue;
            }
            bytes_read += len;
            for (i, chunk) in page[..len].chunks_exact(8).enumerate() {
                let pointer = u64::from_le_bytes(chunk.try_into().unwrap());
                if pointer != 0 && mapped(&maps, pointer) {
                    pointers.push((pointer, start + (i * 8) as u64));
                }
            }
        }
    }
    pointers.sort_unstable();
    println!(
        "INDEX bytes={bytes_read} pointers={} failed_pages={failed_pages} capped={capped}",
        pointers.len()
    );
    for target in targets {
        let mut frontier = BTreeMap::from([(target, Vec::<u64>::new())]);
        let mut found = 0;
        for depth in 1..=4 {
            let mut next = BTreeMap::new();
            let mut limited = false;
            let mut edges = 0;
            'level: for (destination, suffix) in frontier {
                let lo = pointers.partition_point(|&(value, _)| value < destination.saturating_sub(MAX_OFFSET));
                let hi = pointers.partition_point(|&(value, _)| value <= destination);
                for &(value, location) in &pointers[lo..hi] {
                    edges += 1;
                    if edges > 2_000_000 {
                        limited = true;
                        break 'level;
                    }
                    let mut offsets = vec![destination - value];
                    offsets.extend_from_slice(&suffix);
                    if let Some((path, offset)) = module_root(&maps, location) {
                        if follow(&mem, location, &offsets).ok() == Some(target) {
                            println!("CHAIN target=0x{target:X} module={path:?} base_offset=0x{offset:X} offsets={offsets:X?} root=0x{location:X}");
                            found += 1;
                            if found >= 24 {
                                break 'level;
                            }
                        }
                    } else if next.len() < MAX_FRONTIER {
                        // One representative suffix per location keeps the search bounded.
                        next.entry(location).or_insert(offsets);
                    } else {
                        limited = true;
                    }
                }
            }
            println!(
                "LEVEL target=0x{target:X} depth={depth} frontier={} limited={limited} found={found}",
                next.len()
            );
            if found >= 24 || next.is_empty() {
                break;
            }
            frontier = next;
        }
    }
    Ok(())
}
