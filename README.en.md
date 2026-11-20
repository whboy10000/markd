# Markd

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm_Noncommercial_1.0.0-blue)](./LICENSE)
[![Build](https://github.com/whboy10000/markd/actions/workflows/build.yml/badge.svg)](https://github.com/whboy10000/markd/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/whboy10000/markd?display_name=tag&sort=date)](https://github.com/whboy10000/markd/releases/latest)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)](#install)

[简体中文](README.md) | **English**

The full manual — features, workflows, shortcuts and every preference — lives in [docs/MANUAL.md](docs/MANUAL.md) (Chinese).

A desktop Markdown editor built from scratch in pure Rust + egui. **No web kernel, no Tauri/Electron, no off-the-shelf editor component** — from Markdown parsing to every pixel on screen, Markd draws it all itself.

![Markd light theme](assets/screenshots/main-light.png)

<details>
<summary>Dark theme</summary>

![Markd dark theme](assets/screenshots/main-dark.png)

</details>

## Why Markd

| Dimension | Markd | Electron / WebView-based editors |
| --- | --- | --- |
| Binary size | Single-file executable, 8–19 MB installers | 100 MB+, bundles Chromium |
| Memory | Native GUI, tens of MB | One Chromium process to start |
| Rendering | egui immediate mode, straight to GPU | DOM + CSS layout, layered indirection |
| Long documents | 10-million-character files stay editable (1.5 ms idle frame) | Scrolling/typing stutter is common |
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
- **Front matter**: the top YAML block renders with the same code-block chrome (highlighted as `yaml`, with fold / copy / delete controls), never mixed into the body.

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
- In-house lexical highlighting: 10 colour families (Rust / Python / JavaScript / C-like / Markup / CSS / data formats / Shell / SQL / generic) plus per-language comment syntax (`#`, `//`, `--`, `%`, `;`, `::`/`rem`, `dnl`, `\*`, `%%`) and block forms (`/* */`, `<!-- -->`, `{- -}`, `(* *)`, `{ }`, `--[[ ]]`, `#= =#`, `(: :)`).
- Dedicated keyword tables for Dockerfile, Makefile, CMake, Terraform/HCL, Nginx, Lua, Haskell, Ruby, Go, LaTeX, MATLAB, Julia, R, Elixir, Clojure, Solidity and GraphQL.
- 460+ rows in the language dropdown: every spelling is its own entry — `cpp`/`c++`/`cxx`/`cc`, `dart`/`flutter`, `yaml`/`yml`/`k8s`/`github-actions` — and clicking a row writes exactly that spelling into the fence. Aliases highlight identically (lookup keys are lowercase, so hand-written ```CPP and ```C++ work too).
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
│  ├─ editor/     Hybrid View editor, culling, selection, undo│
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

## Performance: measured on a 10-million-character file

Stress corpus: 10,000,045 characters / 17.6 MB / 241,046 blocks (headings, paragraphs, tables, code blocks, math and Mermaid mixed). Same file, same machine (macOS x86_64, release build, 1280 px viewport):

| Scenario (10M characters) | Before | Now |
| --- | --- | --- |
| Cold first frame | 13,862.9 ms | **34.8 ms** |
| Idle frame | 5,703.8 ms (worst 9,042.6) | **1.5 ms** (worst 4.0) |
| Scroll frame | 3,864.4 ms (worst 8,446.7) | **5.5 ms** (worst 9.0) |
| Keypress frame | 3,471.1 ms (worst 3,583.0) | **2.7 ms** (worst 8.3) |
| Shapes per frame | 754,878 | **182** |
| Resident memory | 3,152 MB | **271 MB** |
| Open pipeline | read 44 + parse 1,246 + outline 9.2 + stats 290 ms | read 44 + parse **553** + outline 4.2 + stats **0.5** ms |

The point of all of this is that **per-frame cost does not scale with document length**:

- **Block layout table + viewport culling** (`editor/layout.rs`). Per-block heights and a prefix sum of top edges let a frame lay out only the blocks inside the viewport (plus 24 blocks of margin above and below). Heights converge on measured values: unmeasured blocks start from a fallback estimate and are replaced the moment they are laid out, so the scrollbar never bounces from the gap between estimated and real heights.
- **Parse once, edit incrementally**. The document is parsed in full on open (553 ms ≈ 18M characters/s); every later keystroke re-parses **only the block being edited**. Only cases like pasting blank lines that split one block into several fall back to a single full re-parse. Inline markup parsing is sharded across blocks.
- **The undo stack stores diffs, not copies**. Each record is a splice — block index plus the old and new text — and only a whole-document replacement degrades to a full snapshot. On the 10M-character file, 200 records cost **0.09 MB** instead of **3,300 MB**, and a full undo/redo drops from 1,483 / 1,497 ms to **the sub-millisecond range**. On a 1M-character file, 200 records stop resident memory climbing from 29 MB to 388 MB and hold it flat at **26 MB**, with a 2.0 ms lossless full undo. Snapshots are still throttled at 450 ms and capped at 200 groups.
- **Per-block incremental statistics**. `Stats` is a sum over cached per-block stats, so editing one block recomputes one block: 290 ms → **0.5 ms** on 10M characters, with an equivalence test against a full-text reference implementation.
- **Parallel search**. Hit scanning is sharded across blocks with `min(cores, 8)` threads and returns byte-identical results to the serial path (including the hit cap): 93.9 ms → **20.9 ms** on 10M characters.
- **Syntax highlighting only for visible blocks**. Warm code-block frame 63.0 ms → **1.4 ms** (100k-character corpus, same below).
- **Source mode caches by document revision**. The full source string, line-start table, character count and longest-line width are recomputed only when the document actually changes, and line numbers are drawn only for visible rows: 113 ms → **15.7 ms** per frame on 10M characters.
- **Virtualized sidebar lists**. Backlink target index: 307 ms / 254 MB → **0.25 ms / 0.2 MB**.
- **Three size-independent tricks stay**: explicit-width layout (a `Label` inside a horizontal layout otherwise falls back to infinite width and re-lays out every frame), icons embedded at compile time and rasterized once, and math drawn straight from RaTeX display lists with zero rasterization.
- **Lazy workspace**: file-tree scans are capped at 4,000 files / 60,000 entries; quick open and the link graph only scan in the background when their panels open — never synchronously at startup.
- **Dev builds are usable too**: `[profile.dev.package."*"] opt-level = 3` fully optimizes dependencies, so `cargo run` is day-to-day fast while your own code stays debuggable.

Reproduce the measurements (they require `MARKD_STRESS=1`, so plain `cargo test` skips them):

```bash
MARKD_STRESS=1 MARKD_STRESS_TARGET=10000000 \
  cargo test --release -p markd-ui stress -- --nocapture --test-threads=1
```

Reference point at 1M characters / 24,420 blocks (same probes): open pipeline read 4.4 + parse 49.9 + outline 0.8 + stats 0.1 ms; idle frame 1.2 ms (worst 3.2), scroll 4.0 ms, keypress 2.5 ms; 137 MB resident; HTML export 189.9 ms; full-text scan 2.6 ms parallel vs 9.9 ms serial.

Known costs, only perceptible at the ten-million-character scale: a cold source-mode frame takes 6.6 s (egui lays out all 17.6 MB once; later frames are 15.7 ms), typing in source mode costs about 950 ms per keypress (full re-parse), and HTML export takes 1.9 s. The default hybrid view is not affected by any of the three.

## Configuration

Configuration and version snapshots are stored under `~/.markd/` in the user directory, never inside the project folder.

```
~/.markd/
├── config.json      # Theme, font, font size, auto-save, image-host token, etc.
├── folds.json       # Block fold state, keyed by absolute document path
├── fonts/           # Custom fonts (Regular weight preferred)
├── assets/          # Where inserted images are written locally
└── snapshots/       # Version snapshots (time machine), indexed by snapshots/index.json
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
