//! Best-effort permission/privilege diagnostics for process attach failures.
//!
//! When the user picks a process we silently rely on [`process_memory`] and
//! [`proc_maps`] to do the right thing. Both crates report opaque OS errors,
//! so this module probes the target process and turns common failure modes
//! into a single platform-specific hint that gets surfaced through the typed
//! engine error queue.
//!
//! The probe is intentionally cheap: open a handle, list memory regions, and
//! read one byte from the first readable region. Anything more would slow
//! down the attach path that runs on every process selection.

use super::AppError;
use proc_maps::get_process_maps;
use process_memory::{Pid, TryIntoProcessHandle, copy_address};

/// Run a one-shot attach probe against `pid`.
///
/// Returns `Ok(())` if memory is readable, otherwise a human-readable hint
/// suitable for direct display in the engine error banner.
pub fn diagnose_attach(pid: Pid) -> Result<(), AppError> {
    let handle = pid
        .try_into_process_handle()
        .map_err(|e| attach_error(&e, format!("Failed to attach to PID {pid}: {e}")))?;

    let maps = get_process_maps(pid).map_err(|e| attach_error(&e, format!("Failed to read memory map of PID {pid}: {e}")))?;

    // Pick the first plausibly-readable region. On Linux maps without the
    // 'r' bit are unreadable; on other platforms `proc_maps` reports the
    // bits anyway, so use the same filter everywhere.
    let probe_region = maps.iter().find(|m| m.is_read() && m.size() > 0);
    let Some(region) = probe_region else {
        // Empty map list is itself a sign the process is gone or restricted.
        return Err(AppError::AttachDiagnostic {
            message: format!("PID {pid} reports no readable memory regions."),
        });
    };

    if let Err(e) = copy_address(region.start(), 1, &handle) {
        return Err(attach_error(&e, format!("Failed to read memory of PID {pid} at 0x{:X}: {e}", region.start())));
    }

    Ok(())
}

fn attach_error(error: &std::io::Error, message: String) -> AppError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        AppError::AccessDenied { source: message }
    } else {
        AppError::AttachDiagnostic { message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_classification_uses_os_error_kind() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(attach_error(&denied, "context".into()), AppError::AccessDenied { source } if source == "context"));
        let unreadable = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        assert!(matches!(attach_error(&unreadable, "short read".into()), AppError::AttachDiagnostic { .. }));
    }
}
