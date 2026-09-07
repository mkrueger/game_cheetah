//! Persistent address definitions. Resolved addresses are session-local only.
use std::{collections::BTreeMap, ops::Range};

use i18n_embed_fl::fl;
use process_memory::{CopyAddress, TryIntoProcessHandle};
use serde::{Deserialize, Serialize};

use crate::{SearchResult, SearchType};

/// Absolute strings retain compatibility with v1 tables. Module offsets are
/// relative to the image's first mapping, NOT its writable/executable segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum AddressSpec {
    Absolute(String),
    Module {
        module: String,
        offset: String,
    },
    Pointer {
        module: String,
        offset: String,
        offsets: Vec<String>,
        pointer_width: PointerWidth,
    },
}

/// Explicit target pointer size, independent of the scanner's architecture and
/// the final value type. Supported desktop targets use little-endian pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PointerWidth {
    #[serde(rename = "32")]
    Bits32,
    #[default]
    #[serde(rename = "64")]
    Bits64,
}

impl PointerWidth {
    pub fn bytes(self) -> usize {
        match self {
            Self::Bits32 => 4,
            Self::Bits64 => 8,
        }
    }
}

pub const MAX_POINTER_DEPTH: usize = 16;

#[derive(Debug, Clone)]
pub struct PointerStep {
    pub location: usize,
    pub pointer: usize,
    pub offset: String,
    pub target: usize,
}

#[derive(Debug, Clone)]
pub struct ResolvedAddress {
    pub address: usize,
    pub steps: Vec<PointerStep>,
}

impl AddressSpec {
    pub fn absolute(address: usize) -> Self {
        Self::Absolute(format!("0x{address:X}"))
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Absolute(address) => parse_hex(address).map(|_| ()),
            Self::Module { module, offset } | Self::Pointer { module, offset, .. } => {
                if module.trim().is_empty() {
                    return Err(fl!(crate::LANGUAGE_LOADER, "address-module-missing"));
                }
                parse_hex(offset)?;
                if let Self::Pointer { offsets, .. } = self {
                    if offsets.is_empty() || offsets.len() > MAX_POINTER_DEPTH {
                        return Err(fl!(crate::LANGUAGE_LOADER, "pointer-depth"));
                    }
                    for offset in offsets {
                        parse_pointer_offset(offset)?;
                    }
                }
                Ok(())
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Absolute(address) => address.clone(),
            Self::Module { module, offset } => format!("{} + {offset}", module.rsplit(['/', '\\']).next().unwrap_or(module)),
            Self::Pointer { module, offset, offsets, .. } => {
                format!("{} + {offset} -> [{}]", module.rsplit(['/', '\\']).next().unwrap_or(module), offsets.join(", "))
            }
        }
    }

    pub fn is_relative(&self) -> bool {
        !matches!(self, Self::Absolute(_))
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Self::Pointer { .. })
    }
}

fn parse_pointer_offset(text: &str) -> Result<(bool, usize), String> {
    let text = text.trim();
    let (negative, magnitude) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    Ok((negative, parse_hex(magnitude)?))
}

pub fn parse_hex(text: &str) -> Result<usize, String> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(fl!(crate::LANGUAGE_LOADER, "address-invalid-hex", value = text));
    }
    usize::from_str_radix(digits, 16).map_err(|_| fl!(crate::LANGUAGE_LOADER, "address-invalid-hex", value = text))
}

#[derive(Debug, Clone)]
pub struct PendingAddress {
    pub address: AddressSpec,
    pub search_type: SearchType,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub path: String,
    pub base: usize,
    pub ranges: Vec<Range<usize>>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModuleCatalog {
    pub modules: Vec<LoadedModule>,
}

impl ModuleCatalog {
    pub fn for_process(pid: process_memory::Pid) -> Result<Self, String> {
        if pid == 0 {
            return Ok(Self::default());
        }
        let maps = proc_maps::get_process_maps(pid).map_err(|err| fl!(crate::LANGUAGE_LOADER, "address-map-error", error = err.to_string()))?;
        let mut groups: BTreeMap<String, Vec<&proc_maps::MapRange>> = BTreeMap::new();
        for map in &maps {
            if let Some(path) = map.filename().and_then(|path| path.to_str())
                && !path.starts_with('[')
                && !path.ends_with(" (deleted)")
            {
                groups.entry(path.to_owned()).or_default().push(map);
            }
        }
        let mut modules = Vec::new();
        for (path, maps) in groups {
            // Do not promote ordinary mapped save files/assets to modules.
            if !maps.iter().any(|map| map.is_exec()) {
                continue;
            }
            #[cfg(target_os = "linux")]
            let bases: Vec<usize> = maps.iter().filter(|map| map.offset == 0).map(|map| map.start()).collect();
            #[cfg(not(target_os = "linux"))]
            let bases: Vec<usize> = maps.iter().map(|map| map.start()).min().into_iter().collect();
            let Some(base) = bases.iter().copied().min() else {
                continue;
            };
            modules.push(LoadedModule {
                path,
                base,
                ambiguous: bases.len() != 1,
                // Deliberately exclude anonymous BSS/heap mappings rather than
                // guessing ownership from proximity to an image.
                ranges: maps
                    .iter()
                    .filter(|map| map.is_read())
                    .map(|map| map.start()..map.start().saturating_add(map.size()))
                    .collect(),
            });
        }
        Ok(Self { modules })
    }

    pub fn resolve(&self, spec: &AddressSpec, width: usize) -> Result<usize, String> {
        spec.validate()?;
        if spec.is_pointer() {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-needs-process"));
        }
        let AddressSpec::Module { module, offset } = spec else {
            if let AddressSpec::Absolute(address) = spec {
                return parse_hex(address);
            }
            unreachable!();
        };
        let matches: Vec<_> = self
            .modules
            .iter()
            .filter(|loaded| loaded.path == *module || (!module.contains(['/', '\\']) && loaded.path.rsplit(['/', '\\']).next() == Some(module.as_str())))
            .collect();
        if matches.is_empty() {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-module-not-found", module = module.as_str()));
        }
        if matches.len() != 1 || matches[0].ambiguous {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-module-ambiguous", module = module.as_str()));
        }
        let loaded = matches[0];
        let address = loaded
            .base
            .checked_add(parse_hex(offset)?)
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "address-outside-module"))?;
        let end = address
            .checked_add(width.max(1))
            .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "address-outside-module"))?;
        if !loaded.ranges.iter().any(|range| range.start <= address && end <= range.end) {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-outside-module"));
        }
        Ok(address)
    }

    pub fn resolve_for_process(&self, spec: &AddressSpec, width: usize, pid: process_memory::Pid) -> Result<usize, String> {
        self.trace_for_process(spec, width, pid).map(|resolved| resolved.address)
    }

    pub fn trace_for_process(&self, spec: &AddressSpec, width: usize, pid: process_memory::Pid) -> Result<ResolvedAddress, String> {
        if !spec.is_pointer() {
            return self.resolve(spec, width).map(|address| ResolvedAddress { address, steps: Vec::new() });
        }
        if pid == 0 {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-no-process"));
        }
        let handle = pid.try_into_process_handle().map_err(|err| err.to_string())?;
        self.resolve_with_reader(spec, width, &crate::state::memory_reader::ExactProcessReader(&handle))
    }

    /// Read a pointer at the module root, add offsets[0], then repeat at
    /// the resulting address. The final target contains the VALUE, not a pointer.
    /// The reader must reject short reads; production uses ExactProcessReader.
    pub fn resolve_with_reader<T: CopyAddress>(&self, spec: &AddressSpec, width: usize, reader: &T) -> Result<ResolvedAddress, String> {
        spec.validate()?;
        let AddressSpec::Pointer {
            module,
            offset,
            offsets,
            pointer_width,
        } = spec
        else {
            return self.resolve(spec, width).map(|address| ResolvedAddress { address, steps: Vec::new() });
        };
        let root = AddressSpec::Module {
            module: module.clone(),
            offset: offset.clone(),
        };
        let mut address = self.resolve(&root, pointer_width.bytes())?;
        let mut steps = Vec::with_capacity(offsets.len());
        for (index, offset) in offsets.iter().enumerate() {
            let stage = (index + 1).to_string();
            let mut bytes = [0; 8];
            checked_pointer_range(address, pointer_width.bytes(), *pointer_width)?;
            reader.copy_address(address, &mut bytes[..pointer_width.bytes()]).map_err(|err| {
                fl!(
                    crate::LANGUAGE_LOADER,
                    "pointer-read-error",
                    step = stage.as_str(),
                    address = format!("0x{address:X}"),
                    error = err.to_string()
                )
            })?;
            let raw = match pointer_width {
                PointerWidth::Bits32 => u64::from(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
                PointerWidth::Bits64 => u64::from_le_bytes(bytes),
            };
            if raw == 0 {
                return Err(fl!(crate::LANGUAGE_LOADER, "pointer-null", step = stage));
            }
            let pointer = usize::try_from(raw).map_err(|_| fl!(crate::LANGUAGE_LOADER, "pointer-overflow"))?;
            let (negative, delta) = parse_pointer_offset(offset)?;
            let target = if negative { pointer.checked_sub(delta) } else { pointer.checked_add(delta) }
                .filter(|target| *target != 0)
                .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-overflow"))?;
            checked_pointer_range(target, 1, *pointer_width)?;
            steps.push(PointerStep {
                location: address,
                pointer,
                offset: offset.clone(),
                target,
            });
            address = target;
        }
        // Bounded validation of the entire final value. No stale or partially
        // readable target may be reported as a successfully resolved entry.
        checked_pointer_range(address, width.max(1), *pointer_width)?;
        if width > 1024 * 1024 {
            return Err(fl!(crate::LANGUAGE_LOADER, "pointer-overflow"));
        }
        reader.copy_address(address, &mut vec![0; width.max(1)]).map_err(|err| {
            fl!(
                crate::LANGUAGE_LOADER,
                "pointer-target-error",
                address = format!("0x{address:X}"),
                error = err.to_string()
            )
        })?;
        Ok(ResolvedAddress { address, steps })
    }

    pub fn suggest(&self, result: &SearchResult) -> AddressSpec {
        let width = result.search_type.fixed_byte_length().unwrap_or(1);
        for module in &self.modules {
            if !module.ambiguous
                && module.ranges.iter().any(|range| range.contains(&result.addr))
                && let Some(offset) = result.addr.checked_sub(module.base)
            {
                let spec = AddressSpec::Module {
                    module: module.path.clone(),
                    offset: format!("0x{offset:X}"),
                };
                if self.resolve(&spec, width).ok() == Some(result.addr) {
                    return spec;
                }
            }
        }
        AddressSpec::absolute(result.addr)
    }
}

fn checked_pointer_range(address: usize, width: usize, pointer_width: PointerWidth) -> Result<(), String> {
    let last = address
        .checked_add(width.saturating_sub(1))
        .ok_or_else(|| fl!(crate::LANGUAGE_LOADER, "pointer-overflow"))?;
    if pointer_width == PointerWidth::Bits32 && u32::try_from(last).is_err() {
        return Err(fl!(crate::LANGUAGE_LOADER, "pointer-overflow"));
    }
    Ok(())
}
