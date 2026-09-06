# ![Logo](/build/linux/128x128.png) Game Cheetah

**Game Cheetah** is a fast, cross-platform memory scanner and game trainer for Linux, Windows, and macOS.

Use it to search values in a running process, narrow down result sets, edit memory, freeze values, and keep useful addresses in cheat tables. It is intended for single-player/offline games, debugging, reverse engineering, game modding, and educational use.

> Modifying another process can crash the target application or behave unpredictably. Use at your own risk. Game Cheetah is not intended for cheating in online or multiplayer games.

## Features

- **Cross-platform desktop app**
  - Linux, Windows, and macOS support
  - Native GUI built with Rust, `egui`, and `eframe`
  - English and German localization
- **Fast memory scanning**
  - Search integers, floats, doubles, strings, UTF-16 strings, byte arrays, and unknown values
  - Narrow results with exact, changed, unchanged, increased, decreased, and guessed-value workflows
  - Parallel search and SIMD-aware code paths where they help
  - Memory-region filtering to skip irrelevant or unsafe regions
- **Live process editing**
  - Expand grouped process entries to attach to a specific process instance
  - Edit search results directly in the result table
  - Freeze individual values or the complete result set
  - Changed-value highlighting for live rows
  - Multiple search tabs for independent searches
- **Memory editor**
  - Adaptive hex and ASCII view that fills the available width
  - Direct byte inspection and editing with keyboard navigation
  - Collapsible numeric inspector with little-/big-endian interpretation
  - Hex and mapped-pointer views plus result-type selection
  - Undo/redo and selected-result range highlighting
- **Cheat tables and workflow helpers**
  - Save and load cheat-table entries
  - Rename, close, and manage searches
  - Auto-reconnect to a process with the same name after restart
  - Optional update checks against GitHub releases

## Screenshots

### Find a process

![Filter and select a running process](assets/process_list.png)

### Search and refine values

![Search results with live values and freeze controls](assets/search_results.png)

### Change values in a running game

![Game Cheetah editing values while FTL is running](assets/change_value.png)

### Inspect memory

![Hex memory editor with the numeric inspector](assets/memory_editor.png)

## Video demo

[![Watch the video](https://img.youtube.com/vi/ng_1LBaUS48/maxresdefault.jpg)](https://youtu.be/ng_1LBaUS48)

## Installation

### Prebuilt binaries

The recommended installation method is to download a prebuilt package from the latest GitHub release:

https://github.com/mkrueger/game_cheetah/releases/latest

Release artifacts usually include:

- Linux AppImage
- Linux `.deb` package
- Windows x64 ZIP containing the executable
- macOS universal `.dmg`

### Install with Cargo

Game Cheetah is also published on crates.io:

```bash
cargo install game-cheetah
```

This requires Rust **1.95 or newer**. The minimum supported Rust version is also listed in `Cargo.toml` and tested in CI.

## Linux permissions

On Linux, reading or writing another process is controlled by the operating system. Game Cheetah can usually inspect processes owned by the same user, but some distributions restrict this through Yama `ptrace_scope`.

If Game Cheetah cannot open or read a process, check:

```bash
cat /proc/sys/kernel/yama/ptrace_scope
```

For local development or personal single-user systems, you can temporarily relax this setting with:

```bash
echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope
```

Changing this setting affects system security. Prefer the least-permissive setup that works for your use case.

## Build from source

Install Rust from https://www.rust-lang.org/tools/install, then run:

```bash
cargo build --release
```

The executable will be created at:

```bash
target/release/game-cheetah
```

On Linux, you may need development packages for native GUI, audio, and windowing dependencies. On Debian/Ubuntu-like systems, the CI build uses:

```bash
sudo apt install libgtk-3-dev libasound2-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Development

To preview the process picker directly, run `cargo run -- --processes`.
Single-click selects a row; double-click, Enter, or **Connect** attaches.
Use the arrow keys to navigate and Ctrl/Cmd+F to return to the filter.

Common checks:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
```

GitHub Actions runs Linux formatting/lint checks and tests on stable Rust, plus
native Windows/macOS builds and tests. A separate Linux matrix entry builds all
targets and runs the tests with the pinned minimum Rust version. Linux release
guard tests also require Bash and `jq` (available on GitHub's Ubuntu runners).
Dependency auditing runs in its own workflow.

Release packaging runs for version tags, or manually for an existing tag. Before
opening a release draft or building packages, the workflow verifies that the tag
is exactly `v` followed by the Cargo package version. All package builds use
`--locked`; Debian packaging uses a pinned, lockfile-resolved `cargo-deb` install.
The published application version is not changed automatically when tagging.

## Similar tools

Game Cheetah is similar in spirit to Cheat Engine, ArtMoney, and GameGuardian, with a focus on Rust, performance, safety, and cross-platform desktop support.

## License

Game Cheetah is licensed under the Apache License 2.0.
