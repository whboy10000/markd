# CODEBUDDY.md

This file provides guidance to CodeBuddy Code when working with code in this repository.

## 项目

`Markd`：纯 Rust + egui 0.37 从零实现的 Markdown 编辑器。Markdown 解析、块级实时渲染、代码高亮与流程图绘制均为自研，不依赖现成编辑器内核。GUI 使用 eframe 的 glow 后端（`default-features = false`，关闭默认 wgpu 以缩短编译）。仓库是 **Cargo workspace**，含 4 个 crate；根 `Cargo.toml` 是虚拟清单（`members = ["crates/*"]`，无根 `src/`），`edition = "2024"`（需 Rust ≥ 1.85）。

## 常用命令

- 构建全部：`cargo build --workspace`；发布构建：`cargo build --release --workspace`
- 运行：`cargo run -p markd`；打开指定文档：`cargo run -p markd -- path/to/note.md`
- 测试全部：`cargo test --workspace`
- 运行单个测试：`cargo test -p markd-core ast::tests::source_roundtrip`
- 只构建/测试某个 crate：`cargo build -p markd-core`、`cargo test -p markd-ui`
- 格式化 / 静态检查 / 仅类型检查：`cargo fmt --all`、`cargo clippy --workspace`、`cargo check --workspace`

首次构建需联网拉取 crates.io 依赖：`eframe`/`egui` 0.37、`rfd`、`muda`（仅 macOS target）、`image`、`serde`、`serde_json`、`portable-pty`、`vte`。版本集中在根 `Cargo.toml` 的 `[workspace.dependencies]`，各 crate 用 `xxx.workspace = true` 引用；`[workspace.package]` 统一 version/edition。

## Workspace 结构

依赖方向严格单向：

```text
markd-core → markd-platform → markd-ui → markd (bin)
```

| crate | 路径 | 职责 |
| --- | --- | --- |
| `markd-core` | `crates/markd-core` | 数据与逻辑层，不依赖 eframe/窗口：`ast`（自研 Markdown 解析，产出 `Vec<Block>`，每个 Block 保留原始 `source`，这是 Hybrid View 无损往返的基础）、`document`（源码拼接、节流式撤销/重做、统计、大纲）、`command`（`Command` 枚举，UI 与原生菜单共用）、`theme`（6 套主题与 Visuals）、`workspace`（文件树扫描、Markdown 读写、配置持久化、路径解析）、`export`（AST → 自包含 HTML）。注意：它仍链接 `egui`，因为 `theme.rs` 要用 `Color32`/`Visuals`/`Context`。 |
| `markd-platform` | `crates/markd-platform` | 平台集成：`native_menu`（macOS 系统菜单栏 + 命令队列）、`fonts`（系统中文字体加载）。`muda` 只在 `[target.'cfg(target_os = "macos")'.dependencies]` 中引入。 |
| `markd-ui` | `crates/markd-ui` | egui 界面层：`render`（富文本/表格/代码块高亮/公式/流程图/图片）、`editor`（Hybrid View 与按键交互）、`sidebar`（文件树/大纲）、`features`（版本快照、命令面板、全文搜索、双向链接关系图、写作统计）、`terminal`（PTY 交互式集成终端）、`app`（`eframe::App::ui` 入口、快捷键、命令分发、各浮层窗口）。 |
| `markd` | `crates/markd-app` | 二进制入口，只有 `main.rs`：初始化字体、`NativeOptions`、`eframe::run_native`。默认窗口 1000×600（`with_inner_size`）。 |

两个关键设计：
- `Command` 放在 core，是为了让 `markd-platform` 的原生菜单和 `markd-ui` 共享动作定义，避免 crate 间循环依赖。
- `markd-ui/src/lib.rs` 用 `pub use markd_core::{ast, command, document, export, theme, workspace};` 和 `pub use markd_platform::native_menu;` 把下层模块挂到本 crate 根，因此各模块内仍可沿用 `crate::ast`、`crate::theme`、`crate::native_menu` 等路径；`features.rs` 里再 `pub use markd_core::command::Command;` 保持 `crate::features::Command` 可用。新增模块时优先沿用这种 re-export 方式，不要逐个文件改 import。

**新增一个 `Command` 变体的完整链路**（漏改会出现 `match` 非穷尽或菜单里看不到）：
1. `markd-core/src/command.rs`：枚举变体 + `label()` + `all()`。
2. `markd-ui/src/app.rs`：`execute()` 的 `match`、菜单分类列表、需要时 `handle_shortcuts()`。
3. `markd-platform/src/native_menu.rs`：用 `mk(...)` 注册菜单项与 `Command` 映射（macOS 原生菜单）。
4. 命令面板由 `features.rs` 的 re-export + `Command::all()` 自动获得，无需单独改。

## 关键约束

- 锁定 egui/eframe **0.37**，与 0.2x/0.3x 差异较大：`App` trait 实现 `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)` 而非 `update`；`Panel::show` 接收 `&mut Ui`，`Window::show` 仍接收 `&Context`；`Frame::NONE` / `Frame::corner_radius` / `painter.rect_stroke` 需 `StrokeKind`；`TextEditOutput.response` 是 `WidgetAtomResponse`（Deref 到 `Response`，光标在 `cursor_range`，用 `CharIndex.0` 取字符偏移）。改动 UI 前先查 `~/.cargo/registry/src` 下的 0.37 源码。
- 解析与渲染是两条独立路径：改解析器必须保证 `source()` 能 round-trip（`ast::tests::source_roundtrip` 覆盖此约束）；改渲染不能丢掉 Block 的 `source`。
- `Document::source()`（`document.rs:51`）= 所有 `Block::source()` 用 `\n` 拼接；`edit_block` / `insert_block_after` / `remove_block` 改完单块后都会 `ast::parse(&s)` 重解析全文。`record()` 存整篇源码字符串（去重、上限 200 条），`maybe_record()` 按 450ms 节流。
- 配置、版本快照写在用户目录 `~/.markd/`，不写入项目目录。
- 公式与图表是自绘近似，不是 MathJax/Mermaid 的完整渲染；导出格式为自包含 HTML，PDF 由浏览器打印完成。
- `App::ui` 中 panel 的声明顺序固定为 `top(menubar) → top(toolbar) → bottom(状态栏) → bottom(终端面板，条件) → left(侧栏) → CentralPanel`。0.37 的 `Panel` 按声明顺序切父 `Ui` 矩形：`bottom` 只收窄 y 轴，`left` 只把 x 起点推到侧栏右缘。若把 `bottom` 放到 `left` 之后，状态栏宽度会被侧栏截断、无法横跨整窗。
- macOS 使用**自定义标题栏**：`markd-app/src/main.rs` 在 macOS 下设置 `with_fullsize_content_view(true)` + `with_titlebar_shown(false)` + `with_title_shown(false)`，让窗口内容延伸到标题栏下方；`App::ui` 每帧调用 `frame.set_traffic_lights_position()` 把系统红绿灯垂直居中于 menubar 这一行。`menubar` 左侧预留 78pt 给红绿灯，其后依次是 `命令面板 → 快速打开 → 搜索 → 保存 → 终端` 五个按钮，文档标题靠右，最右侧空白区用 `ViewportCommand::StartDrag` 支持拖动窗口。非 macOS 仍在窗口内显示「文件/编辑/视图/主题/帮助」菜单分类。
- 侧栏宽度由 `Config.sidebar_width` 控制：在 `app::ui` 里用 `exact_size(width)` + 右缘自绘的 `Sense::drag()` 窄分隔条实现自由拖动，拖动结束写回配置。**不要改回依赖 `PanelState` / `default_size` + `resizable(true)`**——0.37 的 `PanelState` 不持久化尺寸，每帧 `default_size` 会把拖动结果弹回。
- 编辑区正文宽度是 `ui.available_width() * 0.9`（`editor.rs`），随窗口浮动。所有块（含活动块）都用 `TextEdit::multiline` + `desired_width(page_w)` + `return_key(None)` 按该宽度换行，避免 `singleline` 把长段落横向裁掉；`render.rs` 的 `build_layout` / `code_job` 设置 `wrap.break_anywhere = true`，表格 `Grid::max_col_width` 与流程图 `draw_flow` 的每行节点数也都按可用宽度自适应。
- 给 `Config`（`crates/markd-core/src/workspace.rs`）新增字段时必须加 `#[serde(default = "...")]`，否则旧的 `~/.markd/config.json` 反序列化失败会回退到全默认配置、丢掉用户已保存的主题与模式设置。状态栏最左侧路径由 `Config.show_status_path` 控制，**默认 false**。
- macOS 的菜单栏是**原生系统菜单栏**（`crates/markd-platform/src/native_menu.rs` + `muda`），显示在屏幕最左上角。`install()` 必须在主线程（`MarkdApp::new`）调用，`Menu` 需常驻（当前用 `Box::leak`）。菜单事件经 `MenuEvent` → 线程安全的 `CommandQueue` → `App::ui` 每帧取出并执行，线程会 `ctx.request_repaint()` 唤醒 UI。
- 终端面板（`crates/markd-ui/src/terminal.rs`）是 **PTY 交互式集成终端**，不是逐条命令执行：`portable-pty` 启动持久 shell（`$SHELL`，Windows 用 `new_default_prog`），`vte::Parser` 把 ANSI 字节流解析成 `Screen` 字符网格（SGR 颜色、光标移动、擦除、备用屏 `?1049`、滚动区），`TextFormat.background` 渲染彩色背景。输出经后台线程 `mpsc` 回传、`App::ui` 每帧 `poll()`；键盘输入（文字/粘贴/回车/退格/Ctrl+C/方向键等）直接写回 PTY，面板尺寸变化时 `master.resize()` 同步 PTY 行数列数；线程用 `ctx.request_repaint()` 唤醒。终端获得焦点时 `handle_shortcuts` 只保留 Cmd/Ctrl+J，其余快捷键交给 PTY，避免打断命令输入。
- 无参数启动显示的是 `crates/markd-ui/src/app.rs` 的内置 `WELCOME` 常量（raw string），**不是**读取仓库根目录的 `untitled.md`；两者内容相同（示例文档）。改欢迎页必须改 `WELCOME`。
