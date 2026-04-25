# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-04-25

A robustness, performance, and UX release.

### Added

- Result table now uses a virtual row renderer, so only visible rows are laid out instead of prebuilding every result row. Replaces the previous hard cap of 1000 visible results without requiring pagination.
- Memory editor uses the same virtual row renderer over the target process' readable memory regions, skipping unmapped holes while preserving region permissions and names in the inspector. Cursor navigation, page up/down, and address jumps now scroll smoothly with viewport-aware "ensure visible" behavior.
- Memory editor value inspector fields are editable. Users can write typed integer and floating-point values at the cursor address directly from the inspector.
- Memory editor status strip now shows the focused address, offset from the opened search hit, selected byte count, region permissions, read/write status, mapped region summary, and current region name.
- Process selection list now defaults to sorting by memory size, descending. Users can still click any column header to override.
- Attach now probes the target process and surfaces a platform-specific hint when access is denied (Linux: `kernel.yama.ptrace_scope`; macOS: `task_for_pid` entitlements; Windows: Administrator / protected processes).
- Process exit / PID recycling detection. The engine captures the target's start time at attach and refuses to keep operating against a recycled PID, so the freeze loop can no longer write into an unrelated process when the target restarts.
- Criterion benchmark suite for the search hot paths, including a multi-region rayon macrobenchmark.
- Edge-case unit tests for the unknown-search comparator and the unique- vs shared-Arc finalization paths.

### Changed

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
