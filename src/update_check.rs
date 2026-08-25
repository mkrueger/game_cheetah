//! Best-effort check for a newer GitHub release.
//!
//! Runs in a background blocking task on app startup (when the user has not
//! disabled it in Settings). Failures are silent — the worst case is the
//! "update available" hint simply does not appear.

use std::time::Duration;

const RELEASE_URL: &str = "https://api.github.com/repos/mkrueger/game_cheetah/releases/latest";

/// Fetches the `tag_name` of the latest GitHub release. Returns `None` on
/// network error, non-200 response, or unparseable body. Blocks the calling
/// thread; should be called from `Task::perform` with `smol::unblock` or a
/// regular thread.
pub fn fetch_latest_version() -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(5)))
        .user_agent(concat!("game-cheetah/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let body = agent
        .get(RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    parse_tag_name(&body)
}

/// Pulls `tag_name` out of the GitHub release JSON without dragging in a
/// full JSON parser. The field is always a top-level string.
fn parse_tag_name(body: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let i = body.find(key)?;
    let rest = &body[i + key.len()..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let q1 = rest.find('"')?;
    let rest = &rest[q1 + 1..];
    let q2 = rest.find('"')?;
    Some(rest[..q2].to_string())
}

/// Compares two version strings (with or without leading `v`) component-wise.
/// Non-numeric suffixes (e.g. `-rc1`) are ignored; only the numeric prefix
/// dotted segments are considered.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse::<u32>().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    let n = l.len().max(c.len());
    for i in 0..n {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv != cv {
            return lv > cv;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tag_name() {
        let body = r#"{"url":"...","tag_name":"v1.2.3","name":"1.2.3"}"#;
        assert_eq!(parse_tag_name(body).as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn parses_tag_name_with_whitespace() {
        let body = "{\n  \"tag_name\" : \"0.7.0\",\n  \"draft\": false\n}";
        assert_eq!(parse_tag_name(body).as_deref(), Some("0.7.0"));
    }

    #[test]
    fn parse_tag_name_missing() {
        assert_eq!(parse_tag_name(r#"{"foo":"bar"}"#), None);
    }

    #[test]
    fn newer_detects_patch_bump() {
        assert!(is_newer("v0.6.2", "0.6.1"));
    }

    #[test]
    fn newer_detects_minor_bump() {
        assert!(is_newer("0.7.0", "v0.6.99"));
    }

    #[test]
    fn newer_rejects_equal() {
        assert!(!is_newer("0.6.1", "0.6.1"));
        assert!(!is_newer("v0.6.1", "0.6.1"));
    }

    #[test]
    fn newer_rejects_older() {
        assert!(!is_newer("0.6.0", "0.6.1"));
    }

    #[test]
    fn newer_handles_extra_segments() {
        assert!(is_newer("0.6.1.1", "0.6.1"));
        assert!(!is_newer("0.6.1", "0.6.1.0"));
    }
}
