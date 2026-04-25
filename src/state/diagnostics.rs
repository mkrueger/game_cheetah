//! Best-effort permission/privilege diagnostics for process attach failures.
//!
//! When the user picks a process we silently rely on [`process_memory`] and
//! [`proc_maps`] to do the right thing. Both crates report opaque OS errors,
//! so this module probes the target process and turns common failure modes
//! into a single platform-specific hint that gets surfaced through
//! `state.error_text`.
//!
//! The probe is intentionally cheap: open a handle, list memory regions, and
//! read one byte from the first readable region. Anything more would slow
//! down the attach path that runs on every process selection.

use proc_maps::get_process_maps;
use process_memory::{Pid, TryIntoProcessHandle, copy_address};

/// Run a one-shot attach probe against `pid`.
///
/// Returns `Ok(())` if memory is readable, otherwise a human-readable hint
/// suitable for direct display in `state.error_text`.
pub fn diagnose_attach(pid: Pid) -> Result<(), String> {
    let handle = pid
        .try_into_process_handle()
        .map_err(|e| format!("Failed to attach to PID {pid}: {e}{}", platform_attach_hint()))?;

    let maps = get_process_maps(pid).map_err(|e| format!("Failed to read memory map of PID {pid}: {e}{}", platform_attach_hint()))?;

    // Pick the first plausibly-readable region. On Linux maps without the
    // 'r' bit are unreadable; on other platforms `proc_maps` reports the
    // bits anyway, so use the same filter everywhere.
    let probe_region = maps.iter().find(|m| m.is_read() && m.size() > 0);
    let Some(region) = probe_region else {
        // Empty map list is itself a sign the process is gone or restricted.
        return Err(format!("PID {pid} reports no readable memory regions.{}", platform_attach_hint()));
    };

    if let Err(e) = copy_address(region.start(), 1, &handle) {
        return Err(format!(
            "Failed to read memory of PID {pid} at 0x{:X}: {e}{}",
            region.start(),
            platform_attach_hint()
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn platform_attach_hint() -> &'static str {
    "\nHint: on Linux this usually means ptrace is restricted. Try one of:\n  \
     - run game-cheetah with the same UID as the target process\n  \
     - sudo sysctl -w kernel.yama.ptrace_scope=0  (until next reboot)\n  \
     - run game-cheetah with sudo"
}

#[cfg(target_os = "macos")]
fn platform_attach_hint() -> &'static str {
    "\nHint: on macOS task_for_pid is restricted. Try one of:\n  \
     - run game-cheetah with sudo\n  \
     - codesign game-cheetah with the com.apple.security.cs.debugger entitlement\n  \
     - disable SIP for development (not recommended)"
}

#[cfg(target_os = "windows")]
fn platform_attach_hint() -> &'static str {
    "\nHint: on Windows the target may require elevated privileges. Try one of:\n  \
     - run game-cheetah as Administrator\n  \
     - confirm the target is not a protected process (anti-cheat / system process)"
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_attach_hint() -> &'static str {
    ""
}
