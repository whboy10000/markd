# Markd

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm_Noncommercial_1.0.0-blue)](./LICENSE)
[![Build](https://github.com/whboy10000/markd/actions/workflows/build.yml/badge.svg)](https://github.com/whboy10000/markd/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/whboy10000/markd?display_name=tag&sort=date)](https://github.com/whboy10000/markd/releases/latest)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](#install)

[简体中文](README.md) | **English**

A desktop Markdown editor built from scratch in pure Rust + egui. **No web kernel, no Tauri/Electron, no off-the-shelf editor component** — from Markdown parsing to every pixel on screen, Markd draws it all itself.

## Why Markd

| Dimension | Markd | Electron / WebView-based editors |
| --- | --- | --- |
| Binary size | Single-file executable, a few MB | 100 MB+, bundles Chromium |
| Memory | Native GUI, tens of MB | One Chromium process to start |
| Rendering | egui immediate mode, straight to GPU | DOM + CSS layout, layered indirection |
| Long documents | 500k-character files stay smooth | Scrolling/typing stutter is common |
| Parsing | In-house AST, lossless round-trip | Round-trip through HTML, format drift |

## Core concept: Hybrid View

**The current block shows source; every other block renders live.** Click any paragraph to edit it — that block turns into Markdown source so you can edit characters precisely; move focus away and it returns to rich text. You keep the intuitiveness of WYSIWYG editors like Typora without auto-formatting "eating" details in your source file (indentation, blank lines, original marker order).

## Feature overview

### Editing experience
- **Hybrid View block-level WYSIWYG**: headings, paragraphs, quotes, lists (including task lists), tables, code blocks, math, Mermaid, images, footnotes, YAML front matter, `[TOC]`.
- **Cross-block drag selection**: drag from a heading into a paragraph, across code blocks / images / formulas — the selection stays continuous; the active block stays in edit mode during the drag so layout doesn't jump.
- **Right-click context menu**: Paragraph / Format / Insert submenus, Cut / Copy / Paste, plus "Copy as / Paste as" with multiple targets (Markdown, plain text, HTML).
- **Full shortcut system**: 30+ shortcuts for heading levels, inline formats, block insertion, mode switches, command palette, etc. — `F1` opens the cheatsheet any time.
- **Inline highlight `==text==`**: a first-class marker alongside bold / italic / strikethrough / inline code, fully nestable.

### Markdown syntax
- **Basics**: ATX/Setext headings, paragraphs, ordered / unordered / nested lists, quotes (with nested blocks), links, images, horizontal rules, hard breaks.
- **GFM extensions**: tables (with alignment), strikethrough, task lists `- [ ]` / `- [x]`, fenced code blocks with language tags.
- **Inline formats**: `**bold**`, `*italic*`, `` `code` ``, `~~strike~~`, `==highlight==`, super/subscript, `[link](url)`, `![image](src)`, autolinks.
- **Footnotes**: `[^1]` references + `[^1]: content` definitions, auto-numbered.
- **Front matter**: a top YAML block rendered separately, never mixed into the body.

### Math (RaTeX native display list)
- **Three syntaxes**: inline `$...$`, display `$$...$$`, fenced ` ```math ... ``` `.
- **Zero rasterization**: RaTeX lays LaTeX out into a display list (glyph outlines, Bézier paths, line segments, rectangles); Markd tessellates it with lyon under the NonZero / EvenOdd winding rules into triangles and hands them to epaint `Mesh` for direct GPU rendering. **No PNG, no texture upload, no bitmap cache.**
- **Baseline-aligned**: inline math embeds into the text stream and aligns to the text baseline; display math sits on its own centered line and scales down uniformly if too wide.
- **Display mode**: inline uses `\textstyle`, display uses `\displaystyle`, matching LaTeX behavior.
- **Cached results**: tessellated shapes are cached by `(formula, font size, display mode)` — every frame is just a Mesh draw.

### Mermaid (25+ diagram types, all self-rendered)
No mermaid.js — every diagram type has its own Rust renderer:
`flowchart`, `sequence`, `class`, `er`, `gantt`, `journey`, `gitGraph`, `mindmap`, `timeline`, `pie`, `quadrantChart`, `radar`, `sankey`, `treemap`, `xychart`, `kanban`, `C4`, `requirementDiagram`, `cynefin`, `packet`, `railroad`, `swimlane`, `blockDiagram`, `boxes`, and more.

### Code blocks
- In-house lexical highlighting across 10 language families: Rust / Python / JavaScript / C-like / Markup / CSS / data formats / Shell / SQL / generic.
- One-click copy, switchable language tag, line numbers, current-line highlight.

### Workspace
- Open a folder → scan a Markdown file tree (capped at 4000 files / 60k entries as a safety net).
- Outline panel: built from heading levels, click to jump.
- Quick open `Cmd/Ctrl+Shift+O`: fuzzy search across workspace files.
- Backlink graph: `[[wiki-link]]` entries auto-build a graph, visualizing how notes interconnect.
- Full-text search across files.
- Writing statistics: word count, paragraphs, reading time.
- Version snapshots (time machine): automatic / manual snapshots, review history any time.
- Drag a file onto the window to open it.

### Writing modes
- **Source mode**: plain text editing, no rendering.
- **Focus mode `F8`**: only the current paragraph stays bright, the rest fades.
- **Typewriter mode `F9`**: the editing line stays centered in the viewport.
- **Auto save**: configurable interval.

### Integrated terminal
- PTY-based interactive shell (VS Code style) with colored output and alternate-screen apps (vim / less / htop).
- Panel resizing syncs terminal rows and columns automatically (`SIGWINCH`).
- When the terminal has focus, every app shortcut except `Cmd/Ctrl+J` is handed to the shell.

### Export
- **Self-contained HTML**: CSS inlined, printable to PDF from any browser.
- Math, code highlighting, Mermaid, table styles are all inlined — no external resources needed.

### Appearance
- **2 themes**: Light / Dark, syntax highlighting and math colors follow the theme.
- **macOS native menu bar** + custom title bar: to the right of the traffic lights sit quick buttons for the command palette / quick open / search / save / terminal.
- 220+ sidebar SVG icons embedded at compile time, rasterized once into a texture cache.

## Install

Download the installer for your platform from [Releases](https://github.com/whboy10000/markd/releases/latest):

- **macOS** (pick one of the three builds):
  - `Markd-*-macos-intel.dmg`: Intel chips
  - `Markd-*-macos-apple-silicon.dmg`: Apple Silicon (M-series) chips
  - `Markd-*-macos-universal.dmg`: universal binary, runs on both
  - Open the DMG and drag Markd into Applications. If macOS says "cannot verify developer" on first run: `xattr -d com.apple.quarantine /Applications/Markd.app`.
- **Windows**: `Markd-*-windows-setup.exe`, an install wizard with optional directory choice; Start-menu and desktop shortcuts are created automatically.
- **Linux**:
  - `Markd-*-linux-amd64.deb`: `sudo dpkg -i Markd-*.deb`
  - `Markd-*-linux-amd64.rpm`: `sudo rpm -i Markd-*.rpm`
  - Both register an application menu entry; or run the binary straight from the `.tar.gz`.

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
┌─────────────────────────────────────────────────────────────┐
│ markd (bin)                                                 │  Entry point + icon resources
└─────────────────────────────────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ markd-ui                                                    │
│  ├─ app/        App state, action dispatch, dialogs, tabs   │
│  ├─ editor/     Hybrid View editor, selection, caret, undo  │
│  ├─ render/     Block rendering (math / diagram / inline)   │
│  ├─ terminal/   PTY terminal                                │
│  ├─ sidebar.rs  File tree / outline / search                │
│  └─ icons.rs    220+ compile-time SVG icons                 │
└─────────────────────────────────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ markd-platform                                              │  macOS native menu (muda) / font loading
└─────────────────────────────────────────────────────────────┘
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ markd-core                                                  │
│  ├─ ast.rs        In-house Markdown AST                     │
│  ├─ document.rs   Document model + undo stack               │
│  ├─ command.rs    Command abstraction (edit/insert/nav)     │
│  ├─ export.rs     HTML export                               │
│  ├─ workspace.rs  Workspace scan / backlink index           │
│  ├─ image_host.rs Image-host upload                         │
│  └─ theme.rs      Theme & syntax highlight palette          │
└─────────────────────────────────────────────────────────────┘
```

## Performance: opens 500k-character documents smoothly

Markd's rendering pipeline is designed specifically for very long documents — **single files with hundreds of thousands of characters open instantly, with smooth typing and scrolling throughout**. Key optimizations:

- **Parse once, edit incrementally**. The whole document is parsed once on open; afterwards every keystroke re-parses **only the block being edited** (microseconds, ~3µs) — never the full text. Only cases like pasting blank lines that split one block into several fall back to a single full re-parse. This is the core of "editing doesn't slow down as the document grows".
- **Per-frame cost is independent of document size**. Each frame only clones the active block's source for editing; non-active blocks are laid out read-only. There is no path that does O(n) string concatenation over the full text every frame.
- **No virtual scrolling**. We tried "estimated height + placeholder" virtual scrolling, but the gap between estimated and measured heights made the scrollbar bounce back repeatedly (scrolling felt broken). Markd renders every block and makes single-block rendering fast enough: measured ~9.5ms per frame for a 100k-character / 989-block document (worst ~13ms), well within a 60fps budget.
- **Explicit-width layout**. Every text block is pre-laid out into a galley at a fixed width before being handed to `Label`, avoiding `Label` falling back to infinite width inside horizontal layouts, which caused long paragraphs not to wrap and re-layouts every frame.
- **Throttled undo stack**. Snapshots are recorded with a 450ms throttle (capped at 200 entries), so continuous typing never floods the undo stack nor copies the full text on every keystroke.
- **Lazy workspace**. File-tree scans are capped at 4000 files / 60k entries as a safety net; quick open and the link graph only scan in the background when their panels open — never synchronously at startup.
- **Zero runtime icon overhead**. 220+ sidebar SVG icons are embedded at compile time, rasterized once into a texture cache, then drawn as textures every frame.
- **Zero rasterization for math**. RaTeX display lists convert straight to epaint Meshes; shapes are cached by `(formula, font size, display mode)` so each frame is just a triangle draw.
- **Fast dev builds too**. `[profile.dev.package."*"] opt-level = 3` fully optimizes dependencies, so `cargo run` is usable day-to-day while your own code stays debuggable.

Measured baseline (100k characters / 989 blocks): full-text parse ~11ms (once on open); incremental single-block write-back ~3µs; editor frame ~9.5ms. A 500k-character document is roughly 5× that size and stays equally fluid.

## Configuration

Configuration and version snapshots are stored under `~/.markd/` in the user directory, never inside the project folder.

```
~/.markd/
├── config.toml      # Theme, font, auto-save, etc.
├── fonts/           # Custom fonts (Regular weight preferred)
└── snapshots/       # Version snapshots (time machine)
```

## Fonts

Body text uses **Alibaba PuHuiTi 3.0** by default: it is detected automatically when installed; otherwise place the font file in `~/.markd/fonts/` (Regular weight preferred) or install it into a system font directory. If nothing is found it falls back to system fonts. Code blocks always use a monospace font.

## Build

GitHub Actions builds on macOS / Windows / Linux automatically (`.github/workflows/build.yml`):

- Every push to `main` / `master` and every PR runs compile & tests;
- Build artifacts are uploaded as Actions Artifacts, ready to download and run;
- Pushing a `v*` / `20*` tag creates a Release automatically: macOS produces Intel / Apple Silicon / universal DMGs (`scripts/package-macos.sh`, same logic locally and in CI), Linux produces `.deb` / `.rpm` / `.tar.gz`, Windows produces an NSIS installer.

CI runners are pinned (`ubuntu-26.04` / `macos-latest` / `windows-latest`) — we don't chase `*-latest` to avoid GitHub migration banners.

## License

This project is released under the [PolyForm Noncommercial License 1.0.0](LICENSE): any **noncommercial use** (personal study, research, hobby projects, use by charities / educational institutions / government bodies, etc.) may freely use, modify, and distribute the software; **commercial use requires separate authorization from the author**. See [`LICENSE`](LICENSE) for the full license text.

Copyright (c) 2026 aurelianus
