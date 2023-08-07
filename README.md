# ![Logo](/build/linux/128x128.png) game_cheetah
Game cheetah is an utility to modifiy the state of a game process.

Make yourself more memory, better stats or more lifes.

Single player games store the game state in memory where multi player games
don't. So, this utility is not useful for multiplayer games.

Features:
 * Easy to use UI
 * Supports multiple searches
 * Guesses the data type of the searched value
 * Game Cheetah runs natively on Linux, Mac and Windows computers.

Keep in mind that altering a game memory contents may lead to game and/or computer crashes. Use at your own risk.

# Game Cheetah in action

[![Watch the video](https://img.youtube.com/vi/ng_1LBaUS48/maxresdefault.jpg)](https://youtu.be/ng_1LBaUS48)

# Get binaries

Get the latest release here:
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
