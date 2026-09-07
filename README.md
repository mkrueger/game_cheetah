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

## Numeric limits and result filters

Result values are written live by default. Enable **Write values only on Enter**
in **Settings** to buffer result-table edits until Enter; Escape or leaving the
field discards unconfirmed input. This option is off for new and existing users.
The memory inspector's existing Enter-to-write behavior is unchanged.

`Int` is a signed 32-bit integer, limited to −2,147,483,648 through 2,147,483,647.
This is a storage limit, not an artificial digit limit. For example, if a game
stores resources scaled by 100,000, a displayed 18,000 is stored as 1,800,000,000.
Do not assume that a larger resource count can safely be written by selecting a
wider type: writing eight bytes to a four-byte variable can overwrite its neighbor.
Use **Check data type…** when an edit is invalid, or click a result's type cell
to inspect the address. Type changes remain manual and reinterpret the same memory.

**Number / Guess** tries Int32, Int64, Float32 and Float64. Several types can match
the same address, especially the low four bytes of a small Int64. These are
possible interpretations, not reliable identification of the game's variable type.

After a numeric search, expand **Filter results** beside **Undo** and **Reset search**.
Choose `=`, `≠`, `<`, `≤`, `>`, `≥`, or **between**, then **Apply filter**.
For example, `≥ 0` removes negative values; **between 1000 and 5000** keeps both
endpoints. All current results are checked, even when the table is hidden.

Filters read current memory without writing it. Like other narrowing passes, they
release this search's freezes; **Undo** restores the result set, not freezes.
Unknown-value comparisons retain their previous comparison baseline. Bounds accept
Int64 integers or finite decimal numbers with a decimal point and optional exponent.
Comparisons are exact, not epsilon-based; Float32 bounds use Float32 precision.
The numeric value filter excludes unreadable, non-numeric and non-finite values.

Each filter can be enabled independently; enabled filters are combined:
- **Data types** keeps only selected interpretations (UInt8, Int16, Int32, Int64,
  Float32, Float64), without changing types or writing memory. Use **None** then
  select a type to keep only that type.
- **Stable values only** observes matching results for 3 seconds by default
  (adjustable from 1 to 30 seconds). Every observed byte change or failed read
  permanently removes that candidate, even if it subsequently returns to its old value.
  Observation starts after **Apply**, not retrospectively. Sampling is roughly
  every 200 ms, slower for large lists; changes between samples can be missed.
  **Cancel and restore results** aborts the observation and restores the previous
  result set. Freezes belonging to other searches remain active and can make
  a value appear stable.

## Cancelling searches

**Cancel and restore results** is available during numeric and text scans,
snapshot capture, unknown-value comparisons, refinement and stability observation.
It restores the previous results, comparison baseline and progress state, including
large unknown-value result sets. Released freezes stay off.

Switching tabs leaves a search running; closing its tab or detaching cancels its
background work. Cancellation also releases workers waiting to deliver results.
An OS memory read already in progress may finish, but its results cannot affect
the restored state or a new search.

If the target exits or its PID is reused, searches are cancelled and restored,
all freezes are cleared, and a clear error is shown. A pass in which every attempted
memory read fails also restores the previous state instead of reporting a successful
empty result; individually unreadable regions or candidates may still be skipped.

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
