# ![Logo](/build/linux/128x128.png) Game Cheetah

**Game Cheetah** is a high-performance memory scanner and game trainer for Linux, Windows, and macOS. It allows users to search, modify, and freeze values in running processes, particularly games, to create cheats, trainers, or analyze program behavior.

Make yourself more memory, better stats or more lives. Single player games store the game state in memory where multiplayer games don't. So, this utility is not useful for multiplayer games.

## Key Features

- **Multi-Platform Support**: Works on Linux, Windows, and macOS with platform-specific optimizations
- **Advanced Memory Search**: 
  - Multiple data types (integers, floats, doubles, strings, arrays)
  - SIMD-optimized search algorithms for blazing-fast performance
  - Parallel search using all CPU cores
  - Smart memory region filtering to skip system libraries
- **Real-time Value Manipulation**:
  - Modify values directly in memory
  - Freeze values to prevent games from changing them
  - Multiple search tabs for different values
  - Undo/redo functionality
- **Intuitive GUI**: Built with Iced framework for a responsive, modern interface
- **Memory Editor**: Hex editor view for direct memory inspection and editing
- **Internationalization**: Multi-language support via Fluent localization

## Technical Highlights

- Written in Rust for memory safety and performance
- Lock-free data structures for efficient multi-threaded operations
- SIMD instructions (SSE2/AVX2) for accelerated searches
- Zero-copy memory access where possible
- Optimized for both speed and low memory usage

## Use Cases

- Creating game trainers and cheats
- Debugging and reverse engineering
- Educational purposes to understand memory management
- Game modding and analysis
- Performance analysis of applications

**Similar to**: Cheat Engine, ArtMoney, or GameGuardian, but with a focus on performance, safety, and cross-platform compatibility.

Keep in mind that altering a game memory contents may lead to game and/or computer crashes. Use at your own risk.

# Game Cheetah in action

[![Watch the video](https://img.youtube.com/vi/ng_1LBaUS48/maxresdefault.jpg)](https://youtu.be/ng_1LBaUS48)

# Installing 

ARCH LINUX: ``` yay -S game-cheetah ```

All other OSes: Easiest way is to use cargo or grab a binary. 

See: 
https://doc.rust-lang.org/cargo/getting-started/installation.html

Then install it with: `cargo install game-cheetah`
Ensure that the cargo bin path is in your PATH (but cargo tells you about it)

# Get binaries

Alternatively get the latest release here:
https://github.com/mkrueger/game_cheetah/releases/latest

# Build

Just install rust and compile with "cargo build --release".
Executable will be in target/release/game_cheetah

Just follow https://gtk-rs.org/gtk4-rs/git/book/installation.html

Note: You may need the nightly toolchain of rust

rustup toolchain install nightly

From project directory:
rustup override set nightly

https://doc.rust-lang.org/book/appendix-07-nightly-rust.html