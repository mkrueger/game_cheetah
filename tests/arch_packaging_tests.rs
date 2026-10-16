#[cfg(target_os = "linux")]
mod arch {
    use std::process::{Command, Output};

    const PKGBUILD: &str = include_str!("../build/arch/PKGBUILD");

    fn prepare(version: &str) -> Output {
        Command::new("bash")
            .args([
                "-euc",
                r#"
source build/arch/PKGBUILD
srcdir=.
CARCH=x86_64
rustc() { printf '%s\n' "$TEST_RUST_VERSION"; }
error() { printf '%s\n' "$*" >&2; }
cd() { :; }
cargo() { printf 'cargo %s\n' "$*"; }
prepare
"#,
            ])
            .env("TEST_RUST_VERSION", version)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("run AUR prepare() with bash")
    }

    #[test]
    fn aur_accepts_unversioned_rust_providers() {
        assert!(PKGBUILD.contains("makedepends=('rust' 'cargo')"));
        assert!(PKGBUILD.contains(&format!("pkgver={}", env!("CARGO_PKG_VERSION"))));
        assert!(include_str!("../publish_aur.sh").contains("PKGBUILD_SOURCE=\"build/arch/PKGBUILD\""));
    }

    #[test]
    fn prepare_accepts_minimum_and_newer_compilers() {
        let msrv = env!("CARGO_PKG_RUST_VERSION");
        for version in [
            format!("rustc {msrv}.0 (test 2026-01-01)"),
            "rustc 1.96.0 (test 2026-01-01)".into(),
            "rustc 1.98.0-nightly (test 2026-01-01)".into(),
            "rustc 2.0.0 (test 2026-01-01)".into(),
        ] {
            let output = prepare(&version);
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("cargo fetch --locked --target x86_64-unknown-linux-gnu"));
        }
    }

    #[test]
    fn prepare_rejects_old_or_unrecognized_compilers_before_fetching() {
        for version in ["rustc 1.94.9 (test 2026-01-01)", "rustc 1.9.0", "rustc 0.99.0", "invalid", ""] {
            let output = prepare(version);
            assert!(!output.status.success(), "unexpectedly accepted {version:?}");
            assert!(output.stdout.is_empty(), "must reject the compiler before fetching dependencies");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains(&format!("Rust {} or newer is required", env!("CARGO_PKG_RUST_VERSION"))));
            assert!(stderr.contains("rustup update stable"));
        }
    }
}
