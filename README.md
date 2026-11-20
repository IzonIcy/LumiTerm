# Lumi-Term

Lumi-Term is a GPU-rendered terminal emulator in Rust for people who want to understand how a terminal works by reading one.

## Run it

```bash
cargo run
```

![Lumi-Term window showing the tab bar, a live shell prompt, a 256-color test strip, and real ls and wc output](docs/images/demo.png)

## What it does

- Runs a real shell in a PTY through `portable-pty`. Window resizes propagate to the PTY, so programs see the new grid.
- Parses VT escape sequences with the `vt100` crate: cursor movement, erase, scrolling, and SGR colors.
- Draws 16 ANSI colors, the 256-color index, and RGB colors, plus bold, dim, italic, underline, and inverse.
- Keeps 10,000 lines of scrollback by default and searches it with `Cmd+F`.
- Opens multiple tabs, one shell each.
- Reloads the config file about two seconds after you save it. A broken edit keeps the last good config instead of killing the session.
- Enables bracketed paste so shells can tell pasted text from typed text.

## How it compares

iTerm2 is the one you already have, and it does the things this does not: split panes, drag-to-select, mouse reporting to programs like vim and htop, clickable links, profiles, and far wider escape sequence coverage. Ghostty, Alacritty, and kitty are the other obvious picks, and all three have years of sequence coverage and platform integration behind them. Lumi-Term is a real terminal you can live in for a shell prompt, and it is not a replacement for any of those.

## Keys

| Shortcut | Action |
| --- | --- |
| `Cmd+F` or `Ctrl+F` | Open scrollback search |
| `Cmd+T` | New tab |
| `Cmd+Shift+W` | Close the active tab |
| `Cmd+Shift+C` or `Ctrl+Shift+C` | Copy the visible screen |
| `Shift+PageUp` / `Shift+PageDown` | Scroll ten lines |
| `Ctrl+End` | Jump back to live output |

## Config

On first launch it writes a TOML config to one of these paths:

- macOS: `~/Library/Application Support/com.lumi.lumi-term/lumi-term.toml`
- Linux: `$XDG_CONFIG_HOME/lumi-term/lumi-term.toml`

```toml
[window]
title = "Lumi-Term"
width = 1280.0
height = 760.0

[terminal]
font_size = 16.0
scrollback = 10000
# shell = "/bin/zsh"            # omit to use your default program
# working_directory = "/"       # omit to start in /

[theme]
background = [17, 17, 17]
foreground = [233, 233, 233]
```

Unknown keys are ignored, and the file is re-read every couple of seconds, so you can change the font size or theme without restarting.

## How it works

```
PTY process -> vt100 parser and screen state (src/pty.rs) -> egui draw (src/app.rs) -> pixels
```

The PTY spawns your shell, reads its output, and parses escape sequences into a screen snapshot. Each frame draws that snapshot with egui. A small C bridge carries tab and window chrome state across the FFI boundary, and Rust's build script compiles it at build time. I wrote the thing to learn the whole path from keystroke to lit pixel.

```
src/
├── main.rs           # eframe entry point, config load, viewport setup
├── app.rs            # app state, tabs, key encoding, grid and chrome drawing, config hot-reload
├── pty.rs            # PTY spawn, VT parsing, screen snapshots, scrollback search
├── config.rs         # TOML load-or-create, defaults
└── chrome_bridge.rs  # FFI declarations for the native C chrome bridge

native/
├── lumi_chrome.c     # tab/window chrome state across the FFI boundary
└── lumi_chrome.h
```

## What it does not do yet

- It is not xterm-complete. Common sequences work, uncommon ones do not, and it is not a drop-in replacement for xterm.
- No split panes. The split button in the title bar answers "Split panes are not available yet."
- No mouse. Nothing reads mouse events, so there is no drag-to-select and no mouse reporting for full-screen programs. Copy takes the whole visible screen.
- No clickable links. URLs on screen are just text.
- The menu button is a placeholder. It answers "Main menu actions coming soon."
- The theme section sets only background and foreground. You cannot restyle the 16 ANSI colors.

## Build and test

[![CI](https://github.com/IzonIcy/LumiTerm/actions/workflows/ci.yml/badge.svg)](https://github.com/IzonIcy/LumiTerm/actions/workflows/ci.yml)

```bash
cargo build --release
cargo clippy
cargo fmt --check
cargo test
```

CI runs fmt, clippy with warnings as errors, tests, and `cargo check` on Ubuntu. Tagged releases build tarballs for Apple silicon macOS and x86_64 Linux, and v0.1.0 has [both](https://github.com/IzonIcy/LumiTerm/releases/tag/v0.1.0).

## Requirements

Rust toolchain, a C11 compiler for the bridge, and macOS 12+ or Linux with Wayland or X11. On Linux the link step needs the X11 and Mesa development headers: `libx11-dev`, `libxext-dev`, `libxft-dev`, `libxinerama-dev`, `libxcursor-dev`, `libxrandr-dev`, `libxi-dev`, `libgl1-mesa-dev`, `libegl1-mesa-dev`.
