//! Persistent user preferences (small, opt-in toggles only).
//!
//! Stored as TOML in [`crate::config_dir`]/`settings.toml`. Unknown fields are
//! ignored so older builds can read newer files (and vice versa with
//! `serde(default)`).
//!
//! This intentionally does *not* persist anything tied to a target process'
//! address space (search results, freeze state, addresses); see the note in
//! [`crate::cheat_table`] for why.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "settings.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    pub auto_reconnect: bool,
    pub check_for_updates: bool,
    pub confirm_value_writes: bool,
    pub enable_persistence: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            auto_reconnect: false,
            check_for_updates: true,
            confirm_value_writes: false,
            enable_persistence: false,
        }
    }
}

fn settings_path() -> PathBuf {
    crate::config_dir().join(SETTINGS_FILE)
}

impl UserSettings {
    /// Best-effort load. Returns defaults if the file is missing or unreadable.
    pub fn load() -> Self {
        let path = settings_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Best-effort save. Returns an error string the caller can surface; the
    /// app should not treat a failure here as fatal.
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| format!("Serialization error: {e}"))?;
        std::fs::write(&path, text).map_err(|e| format!("Cannot write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_fields() {
        let original = UserSettings {
            auto_reconnect: true,
            check_for_updates: false,
            confirm_value_writes: true,
            enable_persistence: true,
        };
        let text = toml::to_string_pretty(&original).unwrap();
        let parsed: UserSettings = toml::from_str(&text).unwrap();
        assert!(parsed.auto_reconnect);
        assert!(!parsed.check_for_updates);
        assert!(parsed.confirm_value_writes);
        assert!(parsed.enable_persistence);
    }

    #[test]
    fn unknown_fields_and_missing_fields_use_defaults() {
        let parsed: UserSettings = toml::from_str("auto_reconnect = true\nfuture_field = 42\n").unwrap();
        assert!(parsed.auto_reconnect);
        assert!(parsed.check_for_updates);
        assert!(!parsed.confirm_value_writes);
        assert!(!UserSettings::default().confirm_value_writes);
        assert!(!parsed.enable_persistence);
        assert!(!UserSettings::default().enable_persistence);
    }
}
