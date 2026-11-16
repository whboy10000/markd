# Markd

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm_Noncommercial_1.0.0-blue)](./LICENSE)

[简体中文](README.md) | **English**

A Markdown editor built from scratch in pure Rust + egui, with an interface and interactions aligned with mainstream WYSIWYG editors.

## About

Markd is a desktop Markdown editor. Markdown parsing, block-level live rendering, code highlighting, and diagram drawing are all built in-house — no third-party editor kernel involved. Open and write: **the current block shows its source, all other blocks render live** (Hybrid View).

## Features

- **Hybrid View**: click any paragraph to edit it; the current block shows Markdown source while every other block renders as rich text, with lossless round-tripping.
- **Block-level WYSIWYG**: headings, paragraphs, quotes, lists (including task lists), tables, code blocks (in-house syntax highlighting + one-click copy + language switching), math, Mermaid diagrams, images, footnotes, YAML front matter, `[TOC]`.
- **Workspace**: open a folder, scan a Markdown file tree, jump via outline, drag-and-drop file opening.
- **Writing modes**: source mode, focus mode, typewriter mode, auto save.
- **Extras**: command palette, quick open, workspace-wide full-text search, bidirectional link graph, writing statistics, version snapshots (time machine).
- **Integrated terminal**: PTY-based interactive shell (VS Code style) with colored output and alternate-screen apps (vim / less / htop); panel resizing syncs rows and columns automatically.
- **6 themes**: Default Light / Default Dark / GitHub / Parchment / Nord / Solarized.
- **Export**: self-contained HTML (printable to PDF from a browser).
- **macOS native menu bar** and custom title bar: to the right of the traffic lights sit quick buttons for the command palette / quick open / search / save / terminal.

## Install

Download the installer for your platform from [Releases](https://github.com/whboy10000/markd/releases/latest):

- **macOS** (pick one of the three builds):
  - `Markd-*-macos-intel.dmg`: Intel chips
  - `Markd-*-macos-apple-silicon.dmg`: Apple Silicon (M-series) chips
  - `Markd-*-macos-universal.dmg`: universal binary, runs on both
  - Open the DMG and drag Markd into Applications.
- **Windows**: `Markd-*-windows-setup.exe`, an install wizard with optional directory choice; Start-menu and desktop shortcuts are created automatically.
- **Linux**: `Markd-*-linux-amd64.deb` (`sudo dpkg -i Markd-*.deb`) or `Markd-*-linux-amd64.rpm` (`sudo rpm -i Markd-*.rpm`), both registering an application menu entry; or run the binary straight from the `.tar.gz`.

## Run

Requires **Rust 1.85+** (the workspace uses `edition = "2024"`).

```bash
# Launch directly
cargo run -p markd

# Open a specific document
cargo run -p markd -- path/to/note.md
```

Release build:

```bash
cargo build --release -p markd
# Output: target/release/markd (target\release\markd.exe on Windows)
```

### Linux runtime dependencies

```bash
sudo apt-get install -y libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev \
  libwayland-dev libgtk-3-dev
```

## Shortcuts

| Shortcut | Action |
| --- | --- |
| `Cmd/Ctrl + S` / `Shift + S` | Save / Save as |
| `Cmd/Ctrl + O` | Open file |
| `Cmd/Ctrl + Shift + O` | Quick open |
| `Cmd/Ctrl + N` | New document |
| `Cmd/Ctrl + P` | Command palette |
| `Cmd/Ctrl + 1 / 2 / 3` | Heading level 1 / 2 / 3 |
| `Cmd/Ctrl + B / I / K` | Bold / Italic / Link (paste URL in a popup) |
| ``Cmd/Ctrl + Shift + ` `` | Inline code |
| ``Ctrl + Shift + ` `` | Strikethrough |
| `Cmd + Option + C` / `B` | Code block / Math block |
| `Cmd + Option + T` / `Q` | Table / Quote |
| `Cmd + Option + O` / `U` | Ordered / Unordered list |
| `Cmd/Ctrl + /` | Toggle source mode |
| `Cmd/Ctrl + F` | Full-text search |
| `Cmd/Ctrl + Shift + L` | Show / hide sidebar |
| `Cmd + Ctrl + 1` | Show outline |
| `F8` / `F9` | Focus / Typewriter mode |
| `Cmd/Ctrl + J` | Show / hide terminal |
| `Cmd/Ctrl + Z` / `Shift + Z` | Undo / Redo |
| `Cmd/Ctrl + Shift + E` | Export HTML |
| `F1` | Shortcut cheatsheet |

> When the terminal has keyboard focus, every app shortcut except `Cmd/Ctrl + J` is handed to the shell so interactive commands are never interrupted.

## Project structure

A Cargo workspace with strictly one-way dependency flow:

```text
markd-core → markd-platform → markd-ui → markd (bin)
```

| crate | Role |
| --- | --- |
| `markd-core` | Markdown parsing (in-house AST), document model & undo stack, `Command` definitions, themes, workspace & config persistence, HTML export |
| `markd-platform` | macOS native menu bar (muda) and system CJK font loading |
| `markd-ui` | egui UI: block rendering, Hybrid View editor, sidebar, PTY terminal, overlays |
| `markd` | Binary entry point |

## Performance: opens 500k-character documents smoothly

Markd's rendering pipeline is designed specifically for very long documents — **single files with hundreds of thousands of characters open instantly, with smooth typing and scrolling throughout**. Key optimizations:

- **Parse once, edit incrementally**. The whole document is parsed once on open; afterwards every keystroke re-parses **only the block being edited** (microseconds, ~3µs) — never the full text. Only cases like pasting blank lines that split one block into several fall back to a single full re-parse. This is the core of "editing doesn't slow down as the document grows".
- **Per-frame cost is independent of document size**. Each frame only clones the active block's source for editing; non-active blocks are laid out read-only. There is no path that does O(n) string concatenation over the full text every frame.
- **No virtual scrolling**. We tried "estimated height + placeholder" virtual scrolling, but the gap between estimated and measured heights made the scrollbar bounce back repeatedly (scrolling felt broken). Markd renders every block and makes single-block rendering fast enough: measured ~9.5ms per frame for a 100k-character / 989-block document (worst ~13ms), well within a 60fps budget.
- **Explicit-width layout**. Every text block is pre-laid out into a galley at a fixed width before being handed to `Label`, avoiding `Label` falling back to infinite width inside horizontal layouts, which caused long paragraphs not to wrap and re-layouts every frame.
- **Throttled undo stack**. Snapshots are recorded with a 450ms throttle (capped at 200 entries), so continuous typing never floods the undo stack nor copies the full text on every keystroke.
- **Lazy workspace**. File-tree scans are capped at 4000 files / 60k entries as a safety net; quick open and the link graph only scan in the background when their panels open — never synchronously at startup.
- **Zero runtime icon overhead**. 220+ sidebar SVG icons are embedded at compile time, rasterized once into a texture cache, then drawn as textures every frame.
- **Fast dev builds too**. `[profile.dev.package."*"] opt-level = 3` fully optimizes dependencies, so `cargo run` is usable day-to-day while your own code stays debuggable.

Measured baseline (100k characters / 989 blocks): full-text parse ~11ms (once on open); incremental single-block write-back ~3µs; editor frame ~9.5ms. A 500k-character document is roughly 5× that size and stays equally fluid.

## Configuration

Configuration and version snapshots are stored under `~/.markd/` in the user directory, never inside the project folder.

## Fonts

Body text uses **Alibaba PuHuiTi 3.0** by default: it is detected automatically when installed; otherwise place the font file in `~/.markd/fonts/` (Regular weight preferred) or install it into a system font directory. If nothing is found it falls back to system fonts. Code blocks always use a monospace font.

## Build

GitHub Actions builds on macOS / Windows / Linux automatically (`.github/workflows/build.yml`):

- Every push to `main` / `master` and every PR runs compile & tests;
- Build artifacts are uploaded as Actions Artifacts, ready to download and run;
- Pushing a `v*` / `20*` tag creates a Release automatically: macOS produces Intel / Apple Silicon / universal DMGs (`scripts/package-macos.sh`, same logic locally and in CI), Linux produces `.deb` / `.rpm` / `.tar.gz`, Windows produces an NSIS installer.

## License

This project is released under the [PolyForm Noncommercial License 1.0.0](LICENSE): any **noncommercial use** (personal study, research, hobby projects, use by charities / educational institutions / government bodies, etc.) may freely use, modify, and distribute the software; **commercial use requires separate authorization from the author**. See [`LICENSE`](LICENSE) for the full license text.

Copyright (c) 2026 aurelianus
