default: build-windows

# Build a Windows release .exe via mingw cross-compilation
build-windows:
    cargo build --release --target x86_64-pc-windows-gnu
    @echo "Output: target/x86_64-pc-windows-gnu/release/game-cheetah.exe"

# Build a native Linux release binary
build-linux:
    cargo build --release
    @echo "Output: target/release/game-cheetah"

# Run checks (fmt + clippy + tests)
check:
    cargo fmt --all -- --check
    cargo clippy --all-features -- -D warnings
    cargo test --all-features
