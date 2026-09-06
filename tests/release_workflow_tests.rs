#[test]
fn ci_pins_the_declared_minimum_rust_version() {
    let msrv = env!("CARGO_PKG_RUST_VERSION");
    let toolchain = if msrv.split('.').count() == 2 { format!("{msrv}.0") } else { msrv.to_owned() };
    let workflow = include_str!("../.github/workflows/ci.yml");
    assert!(
        workflow.contains(&format!("toolchain: \"{toolchain}\"")),
        "CI must test the declared MSRV {toolchain}"
    );
}

// The release guard runs on Ubuntu and requires the same Cargo/jq/bash tools
// available there. Other platforms test the application, not the Linux guard.
#[cfg(target_os = "linux")]
mod release_guard {
    use std::process::{Command, Output};

    fn run(args: &[&str]) -> Output {
        Command::new("bash")
            .arg("tools/check_release_version.sh")
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("run release version guard with bash, Cargo and jq")
    }

    #[test]
    fn accepts_tag_matching_cargo_version() {
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        let output = run(&[&tag]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("Release tag verified: {tag}")));
    }

    #[test]
    fn rejects_mismatched_missing_prefix_and_branch_names() {
        for tag in ["v0.0.0", env!("CARGO_PKG_VERSION"), "main", "", "v0.0.0-rc.1"] {
            let output = run(&[tag]);
            assert!(!output.status.success(), "unexpectedly accepted {tag:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("does not match Cargo package version"));
        }
    }

    #[test]
    fn rejects_missing_or_extra_arguments() {
        for args in [vec![], vec!["v0.7.1", "extra"]] {
            let output = run(&args);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
        }
    }
}
