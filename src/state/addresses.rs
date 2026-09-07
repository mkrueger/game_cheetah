use std::sync::atomic::Ordering;

use i18n_embed_fl::fl;

use crate::{AddressSpec, GameCheetahEngine, ModuleCatalog, PendingAddress, SearchMode, SearchResult};

impl GameCheetahEngine {
    /// Rebuild session-local results from persistent definitions. Pending
    /// absolute addresses require explicit editing, never an automatic retry.
    pub fn resolve_table_addresses(&mut self, modules: &ModuleCatalog, new_process: bool) -> bool {
        let mut any_changed = false;
        for index in 0..self.searches.len() {
            let search = &self.searches[index];
            if search.searching != SearchMode::None {
                continue;
            }
            if !new_process && search.address_overrides.is_empty() && search.unresolved_addresses.is_empty() {
                continue;
            }
            let old = search.collect_results();
            let mut candidates: Vec<_> = old
                .iter()
                .map(|result| {
                    (
                        search
                            .address_overrides
                            .get(&(result.addr, result.search_type))
                            .cloned()
                            .unwrap_or_else(|| AddressSpec::absolute(result.addr)),
                        result.search_type,
                        !new_process,
                    )
                })
                .collect();
            candidates.extend(
                search
                    .unresolved_addresses
                    .iter()
                    .map(|entry| (entry.address.clone(), entry.search_type, false)),
            );
            let mut pending = Vec::new();
            let mut overrides = std::collections::HashMap::new();
            let mut results = Vec::new();
            for (spec, ty, allow_absolute) in candidates {
                let resolved = if !allow_absolute && matches!(spec, AddressSpec::Absolute(_)) {
                    Err(fl!(crate::LANGUAGE_LOADER, "address-stale-absolute"))
                } else {
                    modules.resolve_for_process(&spec, ty.fixed_byte_length().unwrap_or(1), self.pid)
                };
                match resolved {
                    Ok(addr) => {
                        if overrides.get(&(addr, ty)).is_some_and(|other| other != &spec) {
                            pending.push(PendingAddress {
                                address: spec,
                                search_type: ty,
                                reason: fl!(crate::LANGUAGE_LOADER, "address-duplicate"),
                            });
                        } else {
                            overrides.insert((addr, ty), spec);
                            results.push(SearchResult::new(addr, ty));
                        }
                    }
                    Err(reason) => pending.push(PendingAddress {
                        address: spec,
                        search_type: ty,
                        reason,
                    }),
                }
            }
            results.sort_by_key(|r| (r.addr, r.search_type as u8));
            results.dedup_by_key(|r| (r.addr, r.search_type as u8));
            let identity_changed = old.iter().any(|result| {
                let key = (result.addr, result.search_type);
                search.address_overrides.get(&key).is_some_and(|spec| overrides.get(&key) != Some(spec))
            });
            let changed = new_process
                || identity_changed
                || results.len() != old.len()
                || results.iter().zip(old.iter()).any(|(a, b)| a.addr != b.addr || a.search_type != b.search_type);
            if changed {
                self.remove_freezes(index);
            }
            let search = &mut self.searches[index];
            search.address_overrides = overrides;
            search.unresolved_addresses = pending;
            if changed {
                search.set_cached_results(results);
                // An old undo entry contains addresses/baselines from a different
                // mapping and must never reintroduce them after re-resolution.
                search.old_results.clear();
                search.clear_memory_snapshot();
                search.clear_previous_unknown_values();
                search.unknown_comparison = None;
                search.search_complete.store(true, Ordering::Release);
                any_changed = true;
            }
        }
        any_changed
    }

    pub fn validate_result_address(&self, search_index: usize, result: &SearchResult) -> Result<(), String> {
        self.validate_result_range(search_index, result, result.search_type.fixed_byte_length().unwrap_or(1))
    }

    pub fn validate_result_range(&self, search_index: usize, result: &SearchResult, width: usize) -> Result<(), String> {
        if self.pid == 0 {
            return Err(fl!(crate::LANGUAGE_LOADER, "address-no-process"));
        }
        if let Some(spec) = self.searches[search_index].address_overrides.get(&(result.addr, result.search_type))
            && spec.is_relative()
        {
            let modules = ModuleCatalog::for_process(self.pid)?;
            if modules.resolve_for_process(spec, width, self.pid)? != result.addr {
                return Err(fl!(crate::LANGUAGE_LOADER, "address-changed"));
            }
        }
        Ok(())
    }

    pub(super) fn reject_pointer_scan(&mut self, index: usize) -> bool {
        if self.searches.get(index).is_some_and(|search| search.has_pointer_addresses()) {
            self.push_error(fl!(crate::LANGUAGE_LOADER, "pointer-scan-warning"));
            true
        } else {
            false
        }
    }
}
