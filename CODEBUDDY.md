# CODEBUDDY.md

This file provides guidance to CodeBuddy Code when working with code in this repository.

## 项目

`Markd`：纯 Rust + egui 0.37 从零实现的 Markdown 编辑器。Markdown 解析、块级实时渲染、代码高亮与流程图绘制均为自研，不依赖现成编辑器内核。GUI 使用 eframe 的 glow 后端（`default-features = false`，关闭默认 wgpu 以缩短编译）。**注意：关掉 default-features 后必须显式保留 `x11` / `wayland` features，否则 Linux 上 winit 编译报 "The platform you're compiling for is not supported"。**仓库是 **Cargo workspace**，含 4 个 crate；根 `Cargo.toml` 是虚拟清单（`members = ["crates/*"]`，无根 `src/`），`edition = "2024"`（需 Rust ≥ 1.85）。

## 常用命令

- 构建全部：`cargo build --workspace`；发布构建：`cargo build --release --workspace`
- 运行：`cargo run -p markd`；打开指定文档：`cargo run -p markd -- path/to/note.md`
- 测试全部：`cargo test --workspace`
- 运行单个测试：`cargo test -p markd-core ast::tests::source_roundtrip`
- 只构建/测试某个 crate：`cargo build -p markd-core`、`cargo test -p markd-ui`
- 格式化 / 静态检查 / 仅类型检查：`cargo fmt --all`、`cargo clippy --workspace`、`cargo check --workspace`
- macOS 打包：`./scripts/package-macos.sh`——交叉编译 aarch64 + x86_64 两个 release（脚本自动 `rustup target add`）+ lipo 合成通用二进制 + 组装 .app（Info.plist + Resources/AppIcon.icns + ad-hoc 签名），产出**三种 DMG**：`Markd-<version>-macos-intel.dmg` / `-apple-silicon.dmg` / `-universal.dmg`；`dist/Markd.app` 保留通用二进制供本机直接运行。CI 的 macOS job 直接调用同一脚本，与本地逻辑一致。Dock 图标只能来自 .app 包，直接跑 `target/release/markd` 是裸可执行文件。三平台打包素材在 `packaging/`（macOS `Info.plist`、Windows `installer.nsi`、Linux `markd.desktop`）。Linux 的 `.deb` / `.rpm` / `.tar.gz` 由 CI（`.github/workflows/build.yml`）在 ubuntu runner 上产出，RPM 用 `apt install rpm` 后 rpmbuild 打包（staged root + `AutoReqProv: no`）。
- Linux 构建依赖：`sudo apt-get install -y libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libxcursor-dev libxinerama-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-icccm4-dev libxcb-keysyms1-dev libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev libwayland-dev libgtk-3-dev`
- CI（`.github/workflows/build.yml`，runner 固定 `ubuntu-26.04`）：`test` job 跑 `cargo test --workspace`；`build` job 在 macOS/Windows/Linux 三平台编译 + 打包，产物传 Actions Artifacts；master 滚动构建发 `latest` Release，推 `v*` 或 `20*`（如 `2026.11.19.1`）标签时发带版本号的 Release 并附三平台安装包。**版本号用 `MARKD_VERSION` 环境变量在 workflow 里解析后传给打包脚本**（tag 名去掉 `v` 前缀，master 构建回落读 Cargo.toml 的 `version`）；不要把四段版本号写进 Cargo.toml——`cargo` 会因 semver 校验直接拒绝。产物文件名与 Release tag 一致。`package-macos.sh` 用 `${MARKD_VERSION:-$(从 Cargo.toml 读)}` 取值。

首次构建需联网拉取 crates.io 依赖：`eframe`/`egui` 0.37、`rfd`、`muda`（仅 macOS target）、`image`、`serde`、`serde_json`、`regex`、`ureq`+`base64`（图床 HTTP）、`arboard`（读系统剪贴板）、`pulldown-cmark`（仅用于识别 ```math 围栏，其余解析仍走自研 AST 以保留无损往返）、`chrono`（图床路径日期占位符）、RaTeX 全家桶（`ratex-parser`/`ratex-layout`/`ratex-render`/`ratex-types`/`ratex-font`/`ratex-font-loader`）、`ab_glyph`+`lyon`（公式字形轮廓细分成 Mesh）、`portable-pty`、`vte`。版本集中在根 `Cargo.toml` 的 `[workspace.dependencies]`，各 crate 用 `xxx.workspace = true` 引用；`[workspace.package]` 统一 version/edition。

## Workspace 结构

依赖方向严格单向：

```text
markd-core → markd-platform → markd-ui → markd (bin)
```

| crate | 路径 | 职责 |
| --- | --- | --- |
| `markd-core` | `crates/markd-core` | 数据与逻辑层，不依赖 eframe/窗口：`ast`（自研 Markdown 解析，产出 `Vec<Block>`，每个 Block 保留原始 `source`，这是 Hybrid View 无损往返的基础）、`document`（源码拼接、节流式撤销/重做、统计、大纲）、`command`（`Command` 枚举，UI 与原生菜单共用）、`theme`（2 套主题与 Visuals）、`workspace`（文件树扫描、Markdown 读写、配置持久化、路径解析）、`export`（AST → 自包含 HTML）、`image_host`（Gitee 图床上传 + 远程图片下载）。注意：它仍链接 `egui`，因为 `theme.rs` 和 `image_host.rs` 要用到 `Color32` / `ColorImage` / `Context`。 |
| `markd-platform` | `crates/markd-platform` | 平台集成：`native_menu`（macOS 系统菜单栏 + 命令队列）、`fonts`（系统中文字体加载）。`muda` 只在 `[target.'cfg(target_os = "macos")'.dependencies]` 中引入。 |
| `markd-ui` | `crates/markd-ui` | egui 界面层，主体已拆成目录模块：`app/`（窗口布局、菜单、快捷键、命令分发、各浮层窗口、标签页）、`editor/`（Hybrid View 与按键交互）、`render/`（富文本/表格/代码块高亮/公式/流程图/图片）、`sidebar`（文件树/大纲）、`find`（文档内查找/替换，支持正则与 $1）、`terminal/`（PTY 交互式集成终端）、`settings`（偏好设置弹窗）、`icons`、`image_insert`、`features`（快照、命令面板、全文搜索、关系图、写作统计）。 |
| `markd` | `crates/markd-app` | 二进制入口，只有 `main.rs`：初始化字体、`NativeOptions`、`eframe::run_native`。默认窗口 1000×600（`with_inner_size`）、最小 720×480，运行时用 `include_bytes!("assets/icon.png")` 设置窗口图标。 |

两个关键设计：
- `Command` 放在 core，是为了让 `markd-platform` 的原生菜单和 `markd-ui` 共享动作定义，避免 crate 间循环依赖。
- `markd-ui/src/lib.rs` 用 `pub use markd_core::{ast, command, document, export, image_host, theme, workspace};` 和 `pub use markd_platform::native_menu;` 把下层模块挂到本 crate 根，因此各模块内仍可沿用 `crate::ast`、`crate::theme`、`crate::image_host`、`crate::native_menu` 等路径。**`app` / `editor` / `render` / `terminal` 都是目录模块**：各自的 `mod.rs` 声明子模块并 `pub use` 重新导出，因此外部仍写 `crate::editor::{EditorState, EditorEvent}`、`crate::render::{Action, RenderEnv}`、`crate::terminal::Terminal`，不要改成逐文件改 import。新增子模块时优先沿用这种 re-export 方式。

- **快捷键对齐 Typora**（权威实现在 `app/actions.rs::handle_shortcuts`，改键先看它）：`Cmd+1/2/3` 标题（`Cmd+0` 段落）、`Cmd+B/I/K/U`、`Cmd+Shift+Backtick` 行内代码、`Ctrl+Shift+Backtick`（macOS）删除线、`Cmd+Shift+H` 高亮（`==文字==`）、`Cmd+Option+C/B/T/Q/O/U/X/Minus` 代码块/公式/表格/引用/有序/无序/任务列表/分隔线、`Cmd+Ctrl+I` 图片、`Cmd+Ctrl+1` 大纲、`Cmd+/` 或 `Cmd+E` 源码模式、`Cmd+Shift+E` 导出 HTML、`Cmd+F` 文档内查找、`Ctrl+H`（macOS 为 `Cmd+Option+F`）替换、`Cmd+Shift+Z` 重做、`Cmd+,` 偏好设置、`Cmd+Backslash` 清除格式、`F8/F9` 专注/打字机、`Cmd+Shift+F` 专注、`Cmd+Shift+L` 侧栏、`F1` 快捷键一览、`Cmd+J` 终端、`Cmd+N/P/Shift+P` 新建/命令面板/快速打开、`Cmd+Shift+O` 快速打开、`Cmd+O` 打开文件浏览。macOS 额外认 `Ctrl+S/C/X`（`Ctrl+C` 在终端里不抢）。终端聚焦时除 `Cmd/Ctrl+J` 外全部让位给 PTY。
- 状态栏已不再显示文件路径；侧栏头部只显示当前页签名（文件 / 大纲），没有刷新按钮。

**新增一个 `Command` 变体的完整链路**（漏改会出现 `match` 非穷尽或菜单里看不到）：
1. `markd-core/src/command.rs`：枚举变体 + `label()` + `all()`。
2. `markd-ui/src/app/actions.rs`：`execute()` 的 `match`（含 `handle_shortcuts()` 的按键映射）。
3. `markd-ui/src/app/menu.rs`：`menu_commands()` 的分类列表（`MenuKind` 定义在 `app/mod.rs`）。
4. `markd-platform/src/native_menu.rs`：用 `mk(...)` 注册菜单项与 `Command` 映射（macOS 原生菜单）。
5. 命令面板由 `features.rs` 的 re-export + `Command::all()` 自动获得，无需单独改。

## 关键约束

- 锁定 egui/eframe **0.37**，与 0.2x/0.3x 差异较大：`App` trait 实现 `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)` 而非 `update`；`Panel::show` 接收 `&mut Ui`，`Window::show` 仍接收 `&Context`；`Frame::NONE` / `Frame::corner_radius` / `painter.rect_stroke` 需 `StrokeKind`；`TextEditOutput.response` 是 `WidgetAtomResponse`（Deref 到 `Response`，光标在 `cursor_range`，用 `CharIndex.0` 取字符偏移）。改动 UI 前先查 `~/.cargo/registry/src` 下的 0.37 源码。
- 解析与渲染是两条独立路径：改解析器必须保证 `source()` 能 round-trip（`ast::tests::source_roundtrip` 覆盖此约束）；改渲染不能丢掉 Block 的 `source`。
- `Document::source()` = 所有 `Block::source()` 用 `\n` 拼接。**`edit_block` 走增量路径**：只 `parse(new_src)`，解析出单块就原地替换，不再全文重解析（10 万字时 26ms → ~5µs）；只有单块解析出多块（例如粘贴空行）才回退全文重解析。`insert_block_after` / `remove_block` 同样不重解析全文。`record()` 存整篇源码字符串（去重、上限 200 条），`maybe_record()` 按 450ms 节流。
- 配置、版本快照写在用户目录 `~/.markd/`，不写入项目目录（`config.json`、`panic.log`、`folds.json` 都在这里）。
- **公式与图表不再是「自绘近似」**：公式走 RaTeX 引擎做真正的 LaTeX 解析 + 布局（`render/math.rs`），把 `DisplayList` 的字形轮廓经 `lyon` 细分成 epaint `Mesh` 直接上 GPU，不走 PNG 栅格化/纹理上传，任意缩放清晰；行内与块级（`\displaystyle` vs `\textstyle`）分别用 `MathDisplay::Inline/Display`。仅用 `pulldown-cmark` 识别 ```math 围栏，其余仍走自研 AST 以无损往返。Mermaid 图表在 `render/diagram/` 按类型各占一个文件（流程图、时序、饼图、类图、ER、甘特、旅程、Git、看板、思维导图、桑基、雷达、时间线、树图、XY 图、C4、状态/框线图等 20+ 种），全部 egui 画布自绘，无法识别的源码走 `raw_code()` 原样显示（绝不乱画）。导出仍是自包含 HTML，PDF 由浏览器打印完成。
- `App::ui`（`impl eframe::App`，在 `app/update.rs`）中 panel 的声明顺序固定为 `top(menubar) → top(toolbar) → bottom(状态栏) → bottom(终端面板，条件) → left(侧栏) → top(tabbar，条件，声明在 left 之后) → CentralPanel`。0.37 的 `Panel` 按声明顺序切父 `Ui` 矩形：`bottom` 只收窄 y 轴，`left` 只把 x 起点推到侧栏右缘。若把 `bottom` 放到 `left` 之后，状态栏宽度会被侧栏截断、无法横跨整窗。
- macOS 使用**自定义标题栏**：`crates/markd-app/src/main.rs` 在 macOS 下设置 `with_fullsize_content_view(true)` + `with_titlebar_shown(false)` + `with_title_shown(false)`，让窗口内容延伸到标题栏下方；`App::ui` 每帧调用 `frame.set_traffic_lights_position()` 把系统红绿灯垂直居中于 menubar 这一行。`menubar` 左侧预留 78pt 给红绿灯，紧接是文档标题；右侧只固定「保存」和 `>_`（终端）两个按钮，其余空白用 `ViewportCommand::StartDrag` 支持拖动窗口。非 macOS 仍在窗口内显示「文件/编辑/视图/主题/帮助」菜单分类。
- 编辑区上方的格式工具条（`app/ui.rs::toolbar`）：最左是撤销 / 重做（「命令面板 / 快速打开」按钮已移除，打开面板仍走 `Cmd/Ctrl+P` 与 `Cmd/Ctrl+Shift+O`），中间是标题下拉与 B/I/删除线/高亮/链接…，右侧一组是「快照 / 关系图 / 统计」，最右端是**自绘放大镜图标**按钮（`icon_button` + `painter_search_icon`，圆 + 斜柄，不依赖 emoji 字体），点击等同 `Cmd/Ctrl+F`，打开 `find` 查找/替换面板。工具条上的「橡皮擦（清除格式）」按钮已移除（用户反馈作用不大）；清除格式仍可通过「格式」菜单与 `Command::ClearFormat` 使用，逻辑在 `editor::clear_format`。工具条用 `horizontal_wrapped`，窗口变窄时换行而不是把最右边的按钮挤没。工作区全文搜索（`Command::Search` / `SearchPanel`）只剩「视图」菜单与命令面板入口。
- 状态栏左侧有「文件 / 大纲」两个入口，统一走 `app/menu.rs::sidebar_click()` 的状态转移：点当前页签收起侧栏，点另一个则展开并切换。侧栏头部只有一行当前页签名（文件 / 大纲，已去掉刷新按钮与页签切换）。状态栏已不再显示文件路径（`Config.show_status_path` 字段与 `Command::ToggleStatusPath` 已删除）。
- 大纲 / TOC / 新建块的跳转统一用 `EditorState.scroll_req`：由 `show_editor` 在 ScrollArea 内 `take()` 后调用 `ui.scroll_to_rect`。**这个请求必须被消费**——只写不读就会表现成「点大纲标题完全没反应」。
- 侧栏宽度由 `Config.sidebar_width` 控制：在 `app/update.rs` 里用 `exact_size(width)` + 右缘自绘的 `Sense::drag()` 窄分隔条实现自由拖动，拖动结束写回配置。**不要改回依赖 `PanelState` / `default_size` + `resizable(true)`**——0.37 的 `PanelState` 不持久化尺寸，每帧 `default_size` 会把拖动结果弹回。
- 编辑区正文宽度是 `ui.available_width() * 0.9`（`editor/view.rs`），随窗口浮动。**所有文本块都经 `render` 的 `wrapped_label` 用 `RenderEnv.max_width` 显式排版成 galley 再交给 `Label`**——横向布局（引用/列表/脚注）里 `Label` 会退回 `Extend`（无限宽），长段落就永远不换行，所以不要改回直接 `ui.label(job)`。
- 编辑区渲染全部块，不要引入「估算高度 + `add_space` 占位」的虚拟滚动：估算高度会在滚动时被实测高度替换，内容总高反复变化，滚动条被不断夹回，表现为**鼠标滚轮滚不动/一直回弹**。性能改从别处拿：每帧只 clone 活动块源码（`active_buf`）、增量解析、依赖 `opt-level = 3`。10 万字 / 989 块实测单帧 ~9.5ms。
- **源码模式的 TextEdit 必须包在 ScrollArea 里**（`editor/source_mode.rs`）：TextEdit 随内容自动撑高，直接放在父 Ui 上会被裁剪且滚轮无处生效，表现就是源码模式鼠标滚不动。
- 编辑区 CentralPanel 的底色必须显式 `theme.bg`（`egui::Frame::new().inner_margin(8).fill(self.theme.bg)`）：`CentralPanel::default()` 默认填 `panel_fill`（浅色 #F3F3F3），会把编辑区染成侧栏灰。
- 浅色主题代码块背景 `code_bg` 为 **#F5F5F5**（`theme.rs` Light 分支元组第 6 项）。代码块左上角**不放语言文字标签**——右上角语言下拉已显示当前语言，再加文字是冗余（`render/block.rs` 的 Block::Code 分支）。
- 字体（`markd-platform/src/fonts.rs`）：优先加载**阿里巴巴普惠体 3.0**——依次扫描 `~/.markd/fonts/`、`~/Library/Fonts`、`/Library/Fonts`、`/usr/share/fonts` 等目录里文件名含 `puhuiti` 的 ttf/otf/ttc（Regular 字重优先），插到 `FontFamily::Proportional` 列表最前面；找不到再回退系统中文字体。等宽字体（代码块）不受影响。
- 活动块按 `EditKind`（`editor/edit_kind.rs`）**原地编辑**：标题只编辑文字、代码块只编辑纯代码（不含围栏），写回时由 `EditKind::rebuild` 补回 Markdown 标记。代码块编辑器不设 `return_key(None)`（Enter 换行），标题/普通块设 `None` 以便回车拆分块。**代码块回车行为对齐 Typora**：末行行尾回车新建行（`EditKind::Code::rebuild` 在闭合围栏前总是补 `\n`，否则缓冲末尾的空行写回后会丢失）；末尾空行上再按一次回车退出代码块、下方插入普通段落（判定条件：按键前缓冲以 `\n` 结尾 + 本帧改动恰为末尾追加一个换行 + 光标在缓冲末尾，见 `editor/view.rs`「代码块末尾空行回车」段）。
- 性能基线（10 万字 / 989 块，dev + 依赖 opt-level 3）：解析 ~11ms（仅打开时一次）、`edit_block` ~3µs、编辑器单帧 ~9.5ms（最差 ~13ms）。根 `Cargo.toml` 的 `[profile.dev.package."*"] opt-level = 3` 让 `cargo run` 也够快；改动编辑器时不要引入每帧 O(块数) 的渲染或 O(全文) 的字符串构建。
- 给 `Config`（`crates/markd-core/src/workspace.rs`）新增字段时必须加 `#[serde(default = "...")]`（现在是 33 处，每个新字段都有），否则旧的 `~/.markd/config.json` 反序列化失败会回退到全默认配置、丢掉用户已保存的主题与模式设置。偏好设置弹窗在 `markd-ui/src/settings.rs`（左侧 7 分类：通用/外观/编辑器/文件/图像/Markdown/导出），窗口 Id 带 `open_seq` 以便每次打开重新居中；其中「测试上传」等网络操作必须丢后台线程、用 `mpsc` 回传，不能阻塞 UI。
- macOS 的菜单栏是**原生系统菜单栏**（`crates/markd-platform/src/native_menu.rs` + `muda`），显示在屏幕最左上角。菜单分类对齐 **Typora**：Markd(应用) / 文件（新建、快速打开、命令面板、打开、工作区、保存、另存为、导出、快照、退出） / 编辑（撤销/重做/查找/自动保存） / 段落（块级结构：标题、列表、任务、表格、分隔线、代码块、代码组、公式块/代码块公式/行内公式、Mermaid、流程图、TOC、脚注、FrontMatter） / 格式（行内样式：加粗、斜体、下划线、删除线、高亮、行内代码、链接、图片、清除格式） / 视图（源码/专注/打字机、行号、侧栏/大纲/侧栏标签栏/工作区栏、终端、工作区搜索、关系图、统计） / 主题 / 窗口（最小化、最大化/还原） / 帮助（快捷键一览、偏好设置、关于）。窗口内菜单（非 macOS）由 `app/menu.rs::menu_commands()` 按同一结构渲染，分类由 `MenuKind`（`app/mod.rs`）驱动。`install()` 必须在主线程（`MarkdApp::new`）调用，`Menu` 需常驻（当前用 `Box::leak`）。菜单事件经 `MenuEvent` → 线程安全的 `CommandQueue` → `App::ui` 每帧取出并执行，线程会 `ctx.request_repaint()` 唤醒 UI。
- 终端面板（`crates/markd-ui/src/terminal/`，拆为 `mod`/`keys`/`screen`/`session`）是 **PTY 交互式集成终端**，不是逐条命令执行：`portable-pty` 启动持久 shell（`$SHELL`，Windows 用 `new_default_prog`），`vte::Parser` 把 ANSI 字节流解析成 `Screen` 字符网格（SGR 颜色、光标移动、擦除、备用屏 `?1049`、滚动区、CJK 双宽字符——宽字符占两列，右半格以 `'\0'` 占位），渲染按固定字符网格逐格绘制（比例排版会让宽字形叠压成「乱码」）；GUI 启动缺 `LANG` 时补 `en_US.UTF-8`，否则 C locale 下 `ls` 把中文文件名输出成 `?`。输出经后台线程 `mpsc` 回传、`App::ui` 每帧 `poll()`；键盘输入（文字/粘贴/回车/退格/Ctrl+C/方向键等）直接写回 PTY，面板尺寸变化时 `master.resize()` 同步 PTY 行数列数；线程用 `ctx.request_repaint()` 唤醒。终端获得焦点时 `handle_shortcuts` 只保留 Cmd/Ctrl+J，其余快捷键交给 PTY，避免打断命令输入。
- **启动阶段绝对不要同步扫盘**。从 Finder / Dock / DMG 启动时进程 cwd 是 `/`，把它当工作区会遍历整块磁盘（旧版表现就是图标一直转、窗口不出现）。三处约束：`MarkdApp::new`（`app/lifecycle.rs`）用 `filter(|p| p.parent().is_some())` 排除根目录；双向链接图只在打开「关系图」时用 `spawn_link_graph` 后台算；`quick_open_files` 必须挂在「快速打开面板已打开」的条件里，否则每帧扫一次工作区会把 CPU 吃满。`workspace::collect_markdown` 另有 4000 文件 / 6 万条目的上限兜底。
- 首页：`MarkdApp.home` 为真时 CentralPanel 走 `home_view`（`app/home.rs`，欢迎页 + 最近打开）。「首页」按钮只进入不切换（`home = true`），离开首页只由打开/新建文档触发。最近打开存在 `Config.recent`（去重、上限 10，`push_recent` 写入）。
- **多标签**：`MarkdApp.tabs` 里每个打开的文件一个 `Tab`，`active` 是当前标签（结构与 `TabState` 在 `app/mod.rs`，行为在 `app/tabs.rs`）。活动标签的重状态仍放在 app 顶层字段（`doc`/`editor`/`outline`/`stats_cache`），切标签靠 `stash_active()` 把顶层字段整体换进 `tabs[active].state`、`load_tab(i)` 再换出目标标签的 `TabState`；**不要改成给每个标签直接存 doc 再走访问器**，那要把上百处 `self.doc` 全改掉。`tabs` 为空 = 纯首页。标签栏是 `Panel::top("tabbar")`，**声明在 `left(sidebar)` 之后**，纵向位置在工具条下、横向只占侧栏右侧的编辑区；是否显示由 `Config.show_tabbar` 控制（偏好设置里可关）。最左固定是「首页」页签（无 ×），每个文件页签末尾的 × 关闭该页签（中键也可）；关掉活动页签由右侧接手、没有右侧取左侧，全关回首页。编辑器 TextEdit 的 Id 里带 `tab_key`（`Tab.uid`），否则不同标签会共用同一份光标/选中状态。标题栏 menubar 上 `首页` 右侧的 `+` 与 `Cmd/Ctrl+N` 都走 `new_document()`：**只在内存里建空白标签，不落盘**。
- 保存：`Ctrl/Cmd+S` 与「另存为」都统一走 `save()`——先弹原生保存路径选择框（rfd `save_file`，默认文件名取当前文件名或 `未命名-N.md`），确认后由 `save_to()` 写盘并同步标签路径/最近打开/文件树；取消则什么都不写。应用内的「另存为」窗口已删除。`auto_save` 仍然直接写回已有路径，不弹框。
- 主题配色：`ThemeKind::Light` / `Dark` 直接对齐 VS Code 默认 Light+ / Dark+（editor #FFFFFF / #1E1E1E、侧栏 #F3F3F3 / #252526、链接 #006AB1 / #3794FF、选区 #ADD6FF / #264F78）；浅色主题代码块背景为 **#F5F5F5**（`code_bg`）；代码块语法色由 `Theme::syntax()` 提供（关键字/字符串/数字/注释），浅色 #0000FF/#A31515/#098658/#008000，深色 #569CD6/#CE9178/#B5CEA8/#6A9955。当前行高亮为 VS Code 风格 `Theme::line_highlight()`（`theme.rs`，从行号栏右缘铺到编辑区右缘，`editor/view.rs` 与 `editor/source_mode.rs` 使用）；行号显隐由 `Config.show_line_numbers` / `Command::ToggleLineNumbers` 控制（源码模式整栏 + 混合模式逐块行号）。
- 工具条的「插入」是下拉（`app/ui.rs`）：图片/上传本地图片到图床/表格/代码块/代码组（多语言 Tab）/行内代码/引用/链接/列表/任务/分隔线/行内公式/公式块/图表/流程图/脚注/目录，新增插入类动作优先放进这个下拉，而不是再加一个工具条按钮。代码组用 `EditorState.code_group_tab`（块下标 → 标签下标）记录每块显示的标签，`render::Action::{CodeGroupTab, CodeGroupLang, CodeGroupAddTab, CodeGroupRemoveTab}` 驱动切换/改语言/增删，至少保留一个标签。
- 工具栏标题是下拉：`Command::H1`–`H6` 收在 `ui.menu_button("标题")` 里，全部走 `editor::apply_prefix`，会先剥掉原有的 `#`/`>`/`-`/数字前缀再套新级别。工具条上的删除线按钮用 `RichText::new("ABC").strikethrough()` 当图标（画出来是「ABC 中间一条横线」），不要改回文字标签。
- 暗色一致性：侧栏 `Frame` 必须显式 `fill(theme.bg)`（`Frame::NONE` 透明会露出 eframe 默认清屏色 12,12,12，比编辑区黑一截），并实现 `App::clear_color` 跟随主题。
- **图床**（`crates/markd-core/src/image_host.rs`）：当前只支持 Gitee 仓库（OpenAPI v5 写文件），设计成可扩展——加平台时在 `ImageHostKind` 加枚举值、在 `upload()` 加分派分支即可，配置与 UI 不用改。上传走阻塞式 HTTP（`ureq` + rustls），单图上限 20MB；保存路径模板支持 `{year}/{month}/{day}/{hour}/{minute}`（`chrono` 本地时区）。令牌明文存在本地 `~/.markd/config.json`，**绝不写进文档/仓库**。插入图片先写本地副本、后台线程上传完成后按 `ImageUpload`（`app/mod.rs`，带 `tab_uid` 防切标签串改）把旧链接替换成公网 URL；已有文档可用 `Command::UploadImages` 批量上传。远程图片由 `render/media.rs` 的 `ImageCache::remote_texture()` 异步下载（`image_host::fetch_remote_image`）+ 纹理缓存预览，下载失败显示占位框。
- **交互细节**：跨块选区（`EditorState.cross_sel`）在渲染态块上按住拖动产生，`render::Action::CrossDrag` / `WordSel` / `BlockSel` / `ShiftClick` 驱动选区；拖选后的复制/剪切走 egui 的 `Event::Copy`/`Event::Cut`（不能只按 `Key::C/X`，否则没反应）。编辑区空白处右键菜单由 `EditorState.context_menu` 记录位置、`app/context.rs::editor_context_menu()` 绘制（「只用左键判断点外部关闭」，否则右键一闪而过；粘贴走 `arboard` 主动读剪贴板）。剪贴板截图通过 `Event::PasteImage` 落盘到资源目录并插入图片链接。图片插入支持拖动右下角缩放（`Action::ResizeImage`）。代码块折叠状态（`editor/folds.rs`）按文档绝对路径存到 `~/.markd/folds.json`，键为折叠块的 `source` 字符串而非块下标（下标会因前面增删块而错位）；两个内容相同的代码块共享键属可接受边缘情况。
- **应用图标（`assets/morphe_aurora.icns`）的生成规则**：主体取画布 88%（900/1024），遮罩圆角半径必须**大于**素材自身圆角（素材 ≈0.276×边长，故用 ≈0.30×边长）——半径偏小会把白色底角切出来，表现就是「图标一圈乱七八糟的框」。不要做满版（会显得比系统图标大）也不要只抠白（会把白色 MD 字一起抠掉）。`package-macos.sh` 把 `assets/morphe_aurora.icns` 拷成 `Resources/AppIcon.icns`，并随版本号升版以便绕开图标缓存。窗口/任务栏图标另用 `assets/icon.png`（Windows EXE 资源段嵌入的是其中的 PNG 流，CI 有专门校验；Linux `.deb` 用 256×256 版本）。
- 打包成 .app 后没有终端，panic 会静默闪退：`markd-app` 的 `main` 里挂了 panic 钩子，写 `~/.markd/panic.log`（含调用栈），排查闪退先看它。

- 无参数启动显示的是 `crates/markd-ui/src/app/mod.rs` 的内置 `WELCOME` 常量（raw string），**不是**读取仓库根目录的 `untitled.md`；两者内容相同（示例文档）。改欢迎页必须改 `WELCOME`。
