# Lumi-Term — GPU Terminal Emulator

A terminal emulator in Rust. Real PTY shell, VT parsing via the `vt100` crate, egui for rendering. Ongoing work.

## Quick Start

- Build: `cargo build`
- Run: `cargo run`
- Test: `cargo test`
- Check: `cargo check`
- Clippy: `cargo clippy`
- Fmt: `cargo fmt`

## Where It Stands

It works. `cargo run` opens a window with a real shell in it, and keystrokes come back as characters on screen through the GPU. It handles most of the common escape sequences, but it is not xterm-complete — there is no full feature parity, and it isn't trying to be a drop-in `xterm` replacement.

Treat it as a real project with real users (mostly me), not a finished product. Bugs get fixed, features get added, things get refactored. There's no archive notice.

## Architecture Overview

```
build.rs            # compiles native/lumi_chrome.c via cc

src/
├── main.rs           # eframe entry point, config load, viewport setup
├── app.rs            # App state, tabs, key encoding, grid + chrome rendering, config hot-reload
├── pty.rs            # PTY spawn, VT parsing, screen snapshots, scrollback search
├── config.rs         # TOML load-or-create, defaults
└── chrome_bridge.rs  # FFI declarations for the native C chrome bridge

native/
├── lumi_chrome.c     # Tab/window chrome state across the FFI boundary
└── lumi_chrome.h
```

The data path: PTY output → `vt100` parser (in `pty.rs`) → screen snapshot → `app.rs` draws it with egui. Keyboard input goes the other way: egui key event → byte sequence in `app.rs` → written to the PTY.

Line counts for reference: `app.rs` ~1.8k, `pty.rs` ~700, `config.rs` ~230, `chrome_bridge.rs` ~170, `main.rs` ~40.

Tests sit in each module under `#[cfg(test)]` — 65 in total: key encoding, config hot-reload, and palette/grid resolution in `app.rs`; parser and snapshot behaviour in `pty.rs`; palette resolution in `chrome_bridge.rs`; config loading in `config.rs`.

## Background I Already Have

Coming out of the earlier work on this, I know:

- VT100/ANSI escape sequence parsing
- PTY management (portable-pty)
- GPU rendering with egui
- Cross-platform terminal concerns

This is prior knowledge, not a finished curriculum. It's here so you don't have to re-derive the basics — go straight to the part you're changing. The FFI boundary in particular is worth understanding before touching it, since the C bridge is compiled by the build script and mistakes there fail in ways Rust won't explain.

## Conventions

- Keep the split: parsing and screen state in `pty.rs`, drawing in `app.rs`, FFI in `chrome_bridge.rs`. If something straddles two, it probably wants to be one.
- Run `cargo clippy` and `cargo test` before calling anything done.
- `src/app.rs` is the largest file and the one that grows fastest. If a change makes it meaningfully worse, split it.
