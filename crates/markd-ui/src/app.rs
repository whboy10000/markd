//! 应用外壳：窗口布局、菜单、快捷键、状态栏、各功能面板的整合。

use crate::document::{Document, OutlineItem};
use crate::editor::{self, EditorEvent, EditorState};
use crate::features::{
    Command, GraphPanel, Palette, PaletteMode, SearchPanel, SnapshotStore, WritingStats,
};
use crate::render::{Action, ImageCache};
use crate::sidebar::{self, SideAction};
use crate::terminal::Terminal;
use crate::theme::{Theme, ThemeKind};
use crate::workspace::{self, Config, FileNode};
use egui::{self, Color32, RichText};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const WELCOME: &str = r#"---
title: 欢迎使用 Markd
---

[TOC]

# 欢迎使用 Markd

这是一个用 **纯 Rust + egui** 从零搭建的 Markdown 编辑器，采用所见即所得的 Hybrid View，并加入了若干特色功能。

> 点击任意段落即可进入编辑；**当前块显示源码**，其余块实时渲染 —— 这就是 Hybrid View。

## 行内样式

支持 **粗体**、*斜体*、~~删除线~~、`行内代码`、[链接](https://example.com)、$E=mc^2$ 以及 [[双向链接]]。

## 列表

- 无序列表项
- 支持嵌套
  - 缩进用 Tab / Shift+Tab

1. 有序列表
2. 第二项

- [x] 已完成的任务
- [ ] 待办任务（点击复选框可切换）

## 表格

| 功能 | 状态 | 说明 |
| :--- | :---: | ---: |
| 实时预览 | 完成 | 块级 WYSIWYG |
| 代码高亮 | 完成 | 自研词法着色 |
| 导出 HTML | 完成 | 可打印为 PDF |

## 代码

```rust
fn main() {
    let msg = "Hello, Markdown!";
    println!("{}", msg);
}
```

## 数学与图表

$$
\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
$$

```mermaid
graph LR
  A[写 Markdown] --> B[AST 解析]
  B --> C[块级渲染]
  C --> D[所见即所得]
```

---

脚注示例[^1]，以及 `Ctrl/Cmd + P` 打开命令面板。

[^1]: 脚注内容会显示在文末。
"#;

#[derive(Clone, Copy, PartialEq)]
enum MenuKind {
    File,
    Edit,
    View,
    Theme,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SideTab {
    Files,
    Outline,
}

/// 大纲显隐开关的状态转移：返回 (新的 show_outline, 新的侧栏页签)。
fn outline_toggle(showing: bool) -> (bool, SideTab) {
    if showing {
        (false, SideTab::Files)
    } else {
        (true, SideTab::Outline)
    }
}

/// 「文件 / 大纲」入口被点击后的侧栏状态：(show_sidebar, side_tab, show_outline)。
/// 当前正显示该页签时再点一次 → 收起；否则展开并切到该页签。
fn sidebar_click(visible: bool, tab: SideTab, target: SideTab) -> (bool, SideTab, bool) {
    if visible && tab == target {
        (false, tab, tab == SideTab::Outline)
    } else {
        (true, target, target == SideTab::Outline)
    }
}

/// 生成 rows×cols 的空表格（含表头与分隔行）。
fn table_template(rows: usize, cols: usize) -> String {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let mut s = String::new();
    s.push('|');
    for c in 1..=cols {
        s.push_str(&format!(" 列{c} |"));
    }
    s.push('\n');
    s.push('|');
    for _ in 0..cols {
        s.push_str(" --- |");
    }
    s.push('\n');
    for _ in 0..rows {
        s.push('|');
        for _ in 0..cols {
            s.push_str("  |");
        }
        s.push('\n');
    }
    s.trim_end().to_string()
}

fn menu_commands(k: MenuKind) -> Vec<Command> {
    match k {
        MenuKind::File => vec![
            Command::New, Command::OpenWorkspace, Command::Save, Command::SaveAs,
            Command::ExportHtml, Command::Snapshot, Command::Quit,
        ],
        MenuKind::Edit => vec![
            Command::Undo, Command::Redo, Command::Bold, Command::Italic, Command::Strike,
            Command::Code, Command::Link, Command::Image, Command::H1, Command::H2, Command::H3,
            Command::Quote, Command::Bullet, Command::Ordered, Command::Task, Command::Table,
            Command::Rule, Command::Math, Command::Mermaid, Command::Toc, Command::Footnote,
            Command::FrontMatter,
        ],
        MenuKind::View => vec![
            Command::ToggleSource, Command::ToggleFocus, Command::ToggleTypewriter,
            Command::ToggleSidebar, Command::ToggleOutline, Command::ToggleAutoSave,
            Command::ToggleSidebarHeader, Command::ToggleWorkspaceBar,
            Command::ToggleTerminal, Command::OpenFileDialog,
            Command::Search, Command::Graph, Command::Stats,
        ],
        MenuKind::Theme => vec![Command::ThemeNext],
        MenuKind::Help => vec![Command::Shortcuts, Command::About],
    }
}

fn build_link_graph(root: Option<&Path>) -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    if let Some(r) = root {
        for p in workspace::collect_markdown(r) {
            if let Ok(s) = workspace::read(&p) {
                let blocks = crate::ast::parse(&s);
                let mut links = Vec::new();
                for b in &blocks {
                    for inl in crate::document::block_inlines(b) {
                        crate::ast::collect_wiki(std::slice::from_ref(inl), &mut links);
                    }
                }
                links.sort();
                links.dedup();
                let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                m.insert(name, links);
            }
        }
    }
    m
}

pub struct MarkdApp {
    doc: Document,
    config: Config,
    theme: Theme,
    editor: EditorState,
    images: ImageCache,
    root: Option<PathBuf>,
    tree: Vec<FileNode>,
    expanded: HashSet<PathBuf>,
    outline: Vec<OutlineItem>,
    /// 缓存统计结果：Document::stats() 是 O(全文) 的，不能每帧都算。
    stats_cache: crate::document::Stats,
    stats_at: std::time::Instant,
    side_tab: SideTab,
    menu: Option<MenuKind>,
    workspace_input: String,
    palette: Palette,
    search: SearchPanel,
    graph: GraphPanel,
    stats: WritingStats,
    snapshots: SnapshotStore,
    status: String,
    about: bool,
    shortcuts: bool,
    save_dialog: bool,
    save_input: String,
    table_dialog: bool,
    table_rows: usize,
    table_cols: usize,
    link_docs: HashMap<String, Vec<String>>,
    last_autosave: u64,
    theme_dirty: bool,
    menu_guard: bool,
    workspace_dialog: bool,
    quick_cache_dir: String,
    quick_cache: Vec<PathBuf>,
    menu_queue: crate::native_menu::CommandQueue,
    terminal: Terminal,
}

impl MarkdApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let config = Config::load();
        let mut theme = Theme::get(config.theme);
        theme.body = config.font_size;
        theme.mono = (config.font_size * 0.84).max(11.0);
        theme.apply(&cc.egui_ctx);
        // macOS 原生系统菜单栏（屏幕最左上角），事件通过队列转发。
        let menu_queue = crate::native_menu::install(&cc.egui_ctx);

        let (doc, root) = if let Some(p) = initial.filter(|p| p.is_file()) {
            let text = workspace::read(&p).unwrap_or_else(|_| WELCOME.to_string());
            let r = p.parent().map(|x| x.to_path_buf());
            (Document::new(&text, Some(p)), r)
        } else {
            let r = config
                .workspace
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .or_else(|| std::env::current_dir().ok());
            (Document::new(WELCOME, None), r)
        };

        let mut expanded = HashSet::new();
        if let Some(r) = &root {
            expanded.insert(r.clone());
        }
        let tree = root
            .as_ref()
            .map(|r| workspace::scan_tree(r, &expanded))
            .unwrap_or_default();
        let outline = doc.outline();
        let link_docs = build_link_graph(root.as_deref());
        // 终端默认在工作区目录下执行命令。
        let terminal = Terminal::new(
            root.as_ref()
                .cloned()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
        );
        let workspace_input = root
            .as_ref()
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default();

        let stats_cache = doc.stats();
        Self {
            stats: WritingStats::new(&doc),
            stats_cache,
            stats_at: std::time::Instant::now(),
            doc,
            config,
            theme,
            editor: EditorState::default(),
            images: ImageCache::new(),
            root,
            tree,
            expanded,
            outline,
            side_tab: SideTab::Files,
            menu: None,
            workspace_input,
            palette: Palette::default(),
            search: SearchPanel::default(),
            graph: GraphPanel::default(),
            snapshots: SnapshotStore::load(),
            status: String::new(),
            about: false,
            shortcuts: false,
            save_dialog: false,
            save_input: String::new(),
            table_dialog: false,
            table_rows: 3,
            table_cols: 3,
            link_docs,
            last_autosave: now_secs(),
            theme_dirty: false,
            menu_guard: false,
            workspace_dialog: false,
            quick_cache_dir: String::new(),
            quick_cache: Vec::new(),
            menu_queue,
            terminal,
        }
    }

    fn on_changed(&mut self) {
        self.outline = self.doc.outline();
        // stats() 要扫全文，连续输入时按 200ms 节流，避免每个按键都全量统计一遍。
        let now = std::time::Instant::now();
        if now.duration_since(self.stats_at).as_millis() >= 200 {
            self.refresh_stats();
        }
    }

    /// 立即刷新统计（打开文档时用，不受节流影响）。
    fn refresh_stats(&mut self) {
        self.stats_cache = self.doc.stats();
        self.stats_at = std::time::Instant::now();
        self.stats.tick(self.stats_cache.words);
    }

    fn refresh_tree(&mut self) {
        if let Some(r) = &self.root {
            self.tree = workspace::scan_tree(r, &self.expanded);
        }
        self.link_docs = build_link_graph(self.root.as_deref());
    }

    /// 大纲显隐开关：正在显示时切回文件树（隐藏），否则显示大纲面板。
    fn toggle_outline(&mut self) {
        let showing = self.config.show_sidebar && self.side_tab == SideTab::Outline;
        let (show_outline, tab) = outline_toggle(showing);
        self.config.show_outline = show_outline;
        self.side_tab = tab;
        if !showing {
            self.config.show_sidebar = true;
        }
        self.config.save();
    }

    fn files(&self) -> Vec<PathBuf> {
        self.root
            .as_ref()
            .map(|r| workspace::collect_markdown(r))
            .unwrap_or_default()
    }

    /// 快速打开：按面板里指定的目录扫描 MD 文件，用缓存避免每帧遍历磁盘。
    fn quick_open_files(&mut self) -> Vec<PathBuf> {
        let dir = self.palette.dir.trim().to_string();
        if dir.is_empty() {
            return self.files();
        }
        let root = PathBuf::from(&dir);
        if !root.is_dir() {
            return Vec::new();
        }
        if dir != self.quick_cache_dir {
            self.quick_cache_dir = dir;
            self.quick_cache = workspace::collect_markdown(&root);
        }
        self.quick_cache.clone()
    }

    /// 打开快速打开面板，路径默认填当前工作区。
    fn open_quick_palette(&mut self) {
        self.palette.open_with(PaletteMode::Files);
        if self.palette.dir.is_empty() {
            self.palette.dir = self
                .root
                .as_ref()
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_default();
        }
    }

    /// 直接新建文档：有工作区就在工作区里建一个不重名的 md 文件，
    /// 没有就建内存中的未命名文档（保存时再命名）。
    fn new_document(&mut self) {
        if let Some(root) = self.root.clone() {
            let mut n = 0usize;
            let path = loop {
                let name = if n == 0 {
                    "未命名.md".to_string()
                } else {
                    format!("未命名-{n}.md")
                };
                let p = root.join(name);
                if !p.exists() {
                    break p;
                }
                n += 1;
            };
            let _ = workspace::write(&path, "");
            self.open_file(path);
            self.refresh_tree();
        } else {
            self.doc = Document::new("", None);
            self.editor = EditorState::default();
            self.editor.focus_req = Some(0);
            self.outline = self.doc.outline();
            self.refresh_stats();
            self.status = "已新建空白文档（保存时可命名）".to_string();
        }
    }

    fn save(&mut self) {
        if self.doc.path.is_none() {
            self.save_dialog = true;
            self.save_input = self
                .root
                .as_ref()
                .map(|r| r.join("untitled.md").to_string_lossy().to_string())
                .unwrap_or_else(|| "untitled.md".to_string());
            return;
        }
        match self.doc.save() {
            Ok(()) => {
                self.snapshots.capture(&self.doc);
                self.status = "已保存，并生成版本快照".to_string();
            }
            Err(e) => self.status = format!("保存失败: {}", e),
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        match workspace::read(&path) {
            Ok(text) => {
                self.doc = Document::new(&text, Some(path.clone()));
                self.editor.active = 0;
                self.editor.focus_req = Some(0);
                self.on_changed();
                self.refresh_stats();
                self.refresh_tree();
                self.status = format!("已打开 {}", path.display());
            }
            Err(e) => self.status = format!("打开失败: {}", e),
        }
    }

    fn set_workspace(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.status = format!("不是有效目录: {}", path.display());
            return;
        }
        self.root = Some(path.clone());
        self.expanded.insert(path.clone());
        self.config.workspace = Some(path.to_string_lossy().to_string());
        self.config.save();
        self.workspace_input = path.to_string_lossy().to_string();
        self.terminal.set_cwd(path.clone());
        self.refresh_tree();
        self.status = format!("工作区: {}", path.display());
    }

    fn export_html(&mut self) {
        match crate::export::export_html(
            self.doc.path.as_deref(),
            &self.doc.source(),
            &self.doc.title(),
            self.theme.kind.is_dark(),
        ) {
            Ok(p) => self.status = format!("已导出 {}", p.display()),
            Err(e) => self.status = format!("导出失败: {}", e),
        }
    }

    fn next_theme(&mut self) {
        let all = ThemeKind::ALL;
        let i = all.iter().position(|t| *t == self.config.theme).unwrap_or(0);
        self.config.theme = all[(i + 1) % all.len()];
        let mut t = Theme::get(self.config.theme);
        t.body = self.config.font_size;
        t.mono = (self.config.font_size * 0.84).max(11.0);
        self.theme = t;
        self.config.save();
        self.theme_dirty = true;
        self.status = format!("主题: {}", self.theme.kind.name());
    }

    fn open_link(&mut self, url: &str) {
        if url.starts_with("http://") || url.starts_with("https://") {
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd").args(["/C", "start", url]).spawn();
            self.status = format!("已打开链接 {}", url);
        } else if let Some(p) = workspace::resolve(
            self.doc.path.as_ref().and_then(|x| x.parent()),
            url,
        ) {
            if p.is_file() {
                if p.extension().map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false) {
                    self.open_file(p);
                } else {
                    self.status = format!("附件: {}", p.display());
                }
            } else {
                self.status = format!("目标不存在: {}", p.display());
            }
        }
    }

    fn execute(&mut self, cmd: Command, ctx: &egui::Context) {
        match cmd {
            // 直接新建，不弹窗；命名留到保存时
            Command::New => self.new_document(),
            Command::OpenWorkspace => {
                self.config.show_sidebar = true;
                self.side_tab = SideTab::Files;
                self.workspace_dialog = true;
                self.status = "输入工作区文件夹路径".to_string();
            }
            Command::Save => self.save(),
            Command::SaveAs => {
                self.save_dialog = true;
                self.save_input = self
                    .doc
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "untitled.md".to_string());
            }
            Command::ExportHtml => self.export_html(),
            Command::Undo => {
                if self.doc.undo() {
                    self.on_changed();
                }
            }
            Command::Redo => {
                if self.doc.redo() {
                    self.on_changed();
                }
            }
            // 源码 / 专注 / 打字机三种模式互斥：打开一个会关掉另外两个
            Command::ToggleSource => {
                let on = !self.config.source_mode;
                self.config.source_mode = on;
                if on {
                    self.config.focus_mode = false;
                    self.config.typewriter = false;
                }
                self.config.save();
            }
            Command::ToggleFocus => {
                let on = !self.config.focus_mode;
                self.config.focus_mode = on;
                if on {
                    self.config.source_mode = false;
                    self.config.typewriter = false;
                }
                self.config.save();
            }
            Command::ToggleTypewriter => {
                let on = !self.config.typewriter;
                self.config.typewriter = on;
                if on {
                    self.config.source_mode = false;
                    self.config.focus_mode = false;
                }
                self.config.save();
            }
            Command::ToggleSidebar => {
                self.config.show_sidebar = !self.config.show_sidebar;
                self.config.save();
            }
            Command::ToggleOutline => self.toggle_outline(),
            Command::ToggleAutoSave => {
                self.config.auto_save = !self.config.auto_save;
                self.config.save();
            }
            Command::ToggleSidebarHeader => {
                self.config.show_sidebar_header = !self.config.show_sidebar_header;
                self.config.save();
                self.status = if self.config.show_sidebar_header {
                    "侧栏标签栏已显示".to_string()
                } else {
                    "侧栏标签栏已隐藏".to_string()
                };
            }
            Command::ToggleWorkspaceBar => {
                self.config.show_workspace_bar = !self.config.show_workspace_bar;
                self.config.save();
            }
            Command::ToggleTerminal => self.terminal.toggle(),
            Command::OpenFileDialog => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("打开 Markdown 文件")
                    .add_filter("Markdown", &["md", "markdown"]);
                let start = if self.palette.dir.trim().is_empty() {
                    self.root.clone()
                } else {
                    Some(PathBuf::from(self.palette.dir.trim()))
                };
                if let Some(d) = start.filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(d);
                }
                if let Some(p) = dialog.pick_file() {
                    self.open_file(p);
                }
            }
            Command::ThemeNext => self.next_theme(),
            Command::H1 => editor::apply_prefix(&mut self.doc, &mut self.editor, "# "),
            Command::H2 => editor::apply_prefix(&mut self.doc, &mut self.editor, "## "),
            Command::H3 => editor::apply_prefix(&mut self.doc, &mut self.editor, "### "),
            Command::Bold => editor::wrap(&mut self.doc, &mut self.editor, "**", "**"),
            Command::Italic => editor::wrap(&mut self.doc, &mut self.editor, "*", "*"),
            Command::Strike => editor::wrap(&mut self.doc, &mut self.editor, "~~", "~~"),
            Command::Code => editor::wrap(&mut self.doc, &mut self.editor, "`", "`"),
            Command::Link => editor::wrap(&mut self.doc, &mut self.editor, "[", "](https://)"),
            Command::Image => editor::insert_template(&mut self.doc, &mut self.editor, "![](./image.png)"),
            Command::Quote => editor::apply_prefix(&mut self.doc, &mut self.editor, "> "),
            Command::CodeBlock => {
                editor::insert_template(&mut self.doc, &mut self.editor, "```\n\n```")
            }
            Command::Bullet => editor::apply_prefix(&mut self.doc, &mut self.editor, "- "),
            Command::Ordered => editor::apply_prefix(&mut self.doc, &mut self.editor, "1. "),
            Command::Task => editor::insert_template(&mut self.doc, &mut self.editor, "- [ ] "),
            Command::Rule => editor::insert_template(&mut self.doc, &mut self.editor, "---"),
            // 和 Typora 一样：先问几行几列，再插入
            Command::Table => self.table_dialog = true,
            Command::Math => editor::insert_template(&mut self.doc, &mut self.editor, "$$\nE = mc^2\n$$"),
            Command::Mermaid => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "```mermaid\ngraph TD\n  A[开始] --> B[结束]\n```",
            ),
            Command::Toc => editor::insert_template(&mut self.doc, &mut self.editor, "[TOC]"),
            Command::Footnote => editor::insert_template(&mut self.doc, &mut self.editor, "[^1]: 脚注内容"),
            Command::FrontMatter => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "---\ntitle: 未命名\n---",
            ),
            Command::Snapshot => {
                self.snapshots.capture(&self.doc);
                self.snapshots.open = true;
            }
            Command::Search => self.search.open = true,
            Command::Graph => {
                self.link_docs = build_link_graph(self.root.as_deref());
                self.graph.open = true;
            }
            Command::Stats => self.stats.open = true,
            Command::Shortcuts => self.shortcuts = true,
            Command::About => self.about = true,
            Command::Quit => {
                let _ = self.doc.save();
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Command::OpenFile(p) => self.open_file(p),
        }
        self.on_changed();
    }

    /// 快捷键尽量与 Typora 保持一致，方便无损切换。
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mac = cfg!(target_os = "macos");
        let (cmd, shift, alt, ctrl) = ctx.input(|i| {
            (i.modifiers.command, i.modifiers.shift, i.modifiers.alt, i.modifiers.ctrl)
        });
        let key = |k: egui::Key| ctx.input(|i| i.key_pressed(k));

        // ---- 无修饰键：Typora 的 F8 专注 / F9 打字机 / F1 快捷键速查 ----
        if key(egui::Key::F8) {
            self.execute(Command::ToggleFocus, ctx);
            return;
        }
        if key(egui::Key::F9) {
            self.execute(Command::ToggleTypewriter, ctx);
            return;
        }
        if key(egui::Key::F1) {
            self.shortcuts = true;
            return;
        }
        // Typora: Ctrl+Shift+` 删除线（macOS 上 Ctrl 与 Cmd 是分开的）
        if mac && ctrl && shift && key(egui::Key::Backtick) {
            self.execute(Command::Strike, ctx);
            return;
        }

        if !cmd {
            return;
        }
        // Cmd/Ctrl+J 始终用于显隐终端；终端获得焦点后其余快捷键交给 PTY，
        // 避免 Ctrl+P / Ctrl+B 之类的按键被应用抢走而打断交互式命令输入。
        if key(egui::Key::J) {
            self.execute(Command::ToggleTerminal, ctx);
            return;
        }
        if self.terminal.has_focus() {
            return;
        }

        // ---- Typora 的 Command+Option 组合 ----
        if alt {
            if key(egui::Key::Q) {
                self.execute(Command::Quote, ctx);
            } else if key(egui::Key::C) {
                self.execute(Command::CodeBlock, ctx);
            } else if key(egui::Key::B) {
                self.execute(Command::Math, ctx);
            } else if key(egui::Key::T) {
                self.execute(Command::Table, ctx);
            } else if key(egui::Key::O) {
                self.execute(Command::Ordered, ctx);
            } else if key(egui::Key::U) {
                self.execute(Command::Bullet, ctx);
            }
            return;
        }

        // ---- Typora 的 Command+Control 组合（仅 macOS，其它平台 Cmd==Ctrl） ----
        if mac && ctrl {
            if key(egui::Key::I) {
                self.execute(Command::Image, ctx);
            } else if key(egui::Key::Num1) {
                self.config.show_sidebar = true;
                self.config.show_outline = true;
                self.side_tab = SideTab::Outline;
                self.config.save();
            }
            return;
        }

        // ---- 纯 Command 组合 ----
        if key(egui::Key::S) && shift {
            self.execute(Command::SaveAs, ctx);
        } else if key(egui::Key::S) {
            self.execute(Command::Save, ctx);
        } else if key(egui::Key::O) && shift {
            self.open_quick_palette();
        } else if key(egui::Key::O) {
            self.execute(Command::OpenFileDialog, ctx);
        } else if key(egui::Key::P) && shift {
            self.open_quick_palette();
        } else if key(egui::Key::P) {
            self.palette.open_with(PaletteMode::Commands);
        } else if key(egui::Key::N) {
            self.execute(Command::New, ctx);
        } else if key(egui::Key::B) {
            self.execute(Command::Bold, ctx);
        } else if key(egui::Key::I) {
            self.execute(Command::Italic, ctx);
        } else if key(egui::Key::K) {
            self.execute(Command::Link, ctx);
        } else if key(egui::Key::Backtick) && shift {
            self.execute(Command::Code, ctx);
        } else if key(egui::Key::Slash) {
            // Typora: Cmd+/ 切换源码模式
            self.execute(Command::ToggleSource, ctx);
        } else if key(egui::Key::E) && shift {
            self.execute(Command::ExportHtml, ctx);
        } else if key(egui::Key::E) {
            self.execute(Command::ToggleSource, ctx);
        } else if key(egui::Key::F) && shift {
            self.execute(Command::ToggleFocus, ctx);
        } else if key(egui::Key::F) {
            self.search.open = true;
        } else if key(egui::Key::L) && shift {
            self.execute(Command::ToggleSidebar, ctx);
        } else if key(egui::Key::H) && shift {
            self.execute(Command::ToggleSidebarHeader, ctx);
        } else if key(egui::Key::Z) && shift {
            self.execute(Command::Redo, ctx);
        } else if key(egui::Key::Z) {
            self.execute(Command::Undo, ctx);
        } else if key(egui::Key::Num1) {
            self.execute(Command::H1, ctx);
        } else if key(egui::Key::Num2) {
            self.execute(Command::H2, ctx);
        } else if key(egui::Key::Num3) {
            self.execute(Command::H3, ctx);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let drops: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        for d in drops {
            let path = d.path().to_path_buf();
            if path.extension().map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false) {
                self.open_file(path);
            } else if workspace::is_image(&path.to_string_lossy()) {
                let base = self.doc.path.as_ref().and_then(|p| p.parent());
                let rel = base
                    .and_then(|b| path.strip_prefix(b).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                editor::insert_template(&mut self.doc, &mut self.editor, &format!("![]({})", rel));
                self.on_changed();
            }
        }
    }

    /// 固定在窗口最顶部的单行菜单栏（VS Code 风格）。
    /// macOS 下这一行同时充当标题栏：系统红绿灯浮在左端，快捷按钮紧跟其右侧。
    fn menubar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        let mut chosen: Option<Command> = None;
        ui.horizontal(|ui| {
            // 给 macOS 系统红绿灯按钮留出空间；其它平台只用常规边距。
            ui.add_space(if cfg!(target_os = "macos") { 78.0 } else { 4.0 });
            // macOS 的菜单已移到屏幕左上角的系统菜单栏，窗口内不再重复显示。
            if !cfg!(target_os = "macos") {
                for (label, kind) in [
                    ("文件", MenuKind::File),
                    ("编辑", MenuKind::Edit),
                    ("视图", MenuKind::View),
                    ("主题", MenuKind::Theme),
                    ("帮助", MenuKind::Help),
                ] {
                    if ui
                        .add(egui::Button::new(label).frame(false))
                        .on_hover_cursor(egui::CursorIcon::Default)
                        .clicked()
                    {
                        let active = self.menu == Some(kind);
                        self.menu = if active { None } else { Some(kind) };
                        self.menu_guard = self.menu.is_some();
                    }
                }
                ui.separator();
            }
            // 左侧只放文件名
            ui.label(RichText::new(self.doc.title()).strong().color(self.theme.dim));

            // 右侧：命令面板 / 快速打开 / 搜索 / 保存 / 终端图标
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if ui
                    .selectable_label(self.terminal.open, RichText::new(">_").monospace().size(13.0))
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .on_hover_text("终端 (Cmd/Ctrl+J)")
                    .clicked()
                {
                    self.terminal.toggle();
                }
                ui.add_space(2.0);
                if ui.button("保存").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    chosen = Some(Command::Save);
                }
                if ui.button("搜索").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    self.search.open = true;
                }
                // 剩余空白 = 拖动区（双击 = 最大化）
                let r = ui.available_rect_before_wrap();
                let drag = ui.allocate_response(
                    egui::vec2(r.width().max(1.0), r.height().max(1.0)),
                    egui::Sense::click_and_drag(),
                );
                if drag.double_clicked() {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                } else if drag.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }

    /// 菜单栏下方的格式工具条。
    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        let mut chosen: Option<Command> = None;
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            // 功能区最左边：命令面板 / 快速打开
            if ui.button("命令面板").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                self.palette.open_with(PaletteMode::Commands);
            }
            if ui.button("快速打开").on_hover_cursor(egui::CursorIcon::Default).clicked() {
                self.open_quick_palette();
            }
            ui.separator();
            for (label, cmd) in [
                ("B", Command::Bold),
                ("I", Command::Italic),
                ("H1", Command::H1),
                ("H2", Command::H2),
                ("链接", Command::Link),
                ("代码", Command::Code),
                ("引用", Command::Quote),
                ("列表", Command::Bullet),
                ("任务", Command::Task),
                ("表格", Command::Table),
                ("公式", Command::Math),
                ("图表", Command::Mermaid),
                ("目录", Command::Toc),
            ] {
                if ui.button(label).on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    chosen = Some(cmd.clone());
                }
            }
            ui.separator();
            for (label, cmd) in [
                ("快照", Command::Snapshot),
                ("关系图", Command::Graph),
                ("统计", Command::Stats),
            ] {
                if ui.button(label).on_hover_cursor(egui::CursorIcon::Default).clicked() {
                    chosen = Some(cmd.clone());
                }
            }
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().interaction.selectable_labels = false;
        if self.config.show_sidebar_header {
            ui.horizontal(|ui| {
                // 只保留「刷新」；文件 / 大纲的切换入口在底部状态栏。
                let name = match self.side_tab {
                    SideTab::Files => "文件",
                    SideTab::Outline => "大纲",
                };
                ui.label(RichText::new(name).small().color(self.theme.dim));
                if ui
                    .button("刷新")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    self.refresh_tree();
                }
            });
            ui.separator();
        }
        let mut set_ws: Option<PathBuf> = None;
        if self.config.show_workspace_bar {
            ui.horizontal(|ui| {
                ui.label(RichText::new("工作区").small().color(self.theme.dim));
                let out = ui.text_edit_singleline(&mut self.workspace_input);
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .button("打开")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                    || (enter && out.lost_focus())
                {
                    set_ws = Some(PathBuf::from(self.workspace_input.trim()));
                }
                if ui
                    .button("+")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    self.new_document();
                }
            });
            ui.separator();
        }
        if let Some(p) = set_ws {
            self.set_workspace(p);
        }
        let mut actions = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| match self.side_tab {
            SideTab::Files => sidebar::file_tree(
                ui,
                &mut self.tree,
                self.doc.path.as_deref(),
                &self.theme,
                &mut actions,
            ),
            SideTab::Outline => {
                if self.config.show_outline {
                    sidebar::outline_panel(ui, &self.outline, &self.theme, &mut actions);
                } else {
                    // 隐藏大纲时回退到文件树，避免侧栏出现空白或提示文字。
                    sidebar::file_tree(ui, &mut self.tree, self.doc.path.as_deref(), &self.theme, &mut actions);
                }
            }
        });
        for a in actions {
            match a {
                SideAction::Open(p) => self.open_file(p),
                SideAction::ToggleDir(p) => {
                    if !self.expanded.remove(&p) {
                        self.expanded.insert(p);
                    }
                    self.refresh_tree();
                }
                SideAction::Jump(i) => {
                    self.editor.active = i.min(self.doc.blocks.len().saturating_sub(1));
                    self.editor.scroll_req = Some(self.editor.active);
                }
            }
        }
    }

    fn statusbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.style_mut().interaction.selectable_labels = false;
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let st = self.stats_cache.clone();
        let mut chosen: Option<Command> = None;
        ui.horizontal(|ui| {
            // 状态栏入口：点「文件」显示文件树，点「大纲」显示大纲。
            let files_on = self.config.show_sidebar && self.side_tab == SideTab::Files;
            let files_txt = if files_on {
                RichText::new("文件").strong().color(self.theme.accent)
            } else {
                RichText::new("文件").color(self.theme.dim)
            };
            if ui
                .selectable_label(files_on, files_txt)
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                let (show, tab, show_outline) =
                    sidebar_click(self.config.show_sidebar, self.side_tab, SideTab::Files);
                self.config.show_sidebar = show;
                self.side_tab = tab;
                self.config.show_outline = show_outline;
                self.config.save();
            }

            let outline_on = self.config.show_sidebar && self.side_tab == SideTab::Outline;
            let outline_txt = if outline_on {
                RichText::new("大纲").strong().color(self.theme.accent)
            } else {
                RichText::new("大纲").color(self.theme.dim)
            };
            if ui
                .selectable_label(outline_on, outline_txt)
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                let (show, tab, show_outline) =
                    sidebar_click(self.config.show_sidebar, self.side_tab, SideTab::Outline);
                self.config.show_sidebar = show;
                self.side_tab = tab;
                self.config.show_outline = show_outline;
                self.config.save();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} 字 · {} 字符 · {} 行 · 约 {} 分钟",
                        st.words, st.chars, st.lines, st.reading_min
                    ))
                    .color(self.theme.dim),
                );
                if !self.status.is_empty() {
                    ui.label(RichText::new(&self.status).color(self.theme.accent));
                }
                if self.doc.dirty {
                    ui.label(RichText::new("未保存").color(Color32::from_rgb(200, 120, 60)));
                }
                ui.label(
                    RichText::new(format!(
                        "块 {}/{} · {}",
                        self.editor.active + 1,
                        self.doc.blocks.len(),
                        self.theme.kind.name()
                    ))
                    .color(self.theme.dim),
                );
                ui.separator();
                for (label, active, cmd) in [
                    ("打字机", self.config.typewriter, Command::ToggleTypewriter),
                    ("专注", self.config.focus_mode, Command::ToggleFocus),
                    ("源码", self.config.source_mode, Command::ToggleSource),
                ] {
                    let txt = if active {
                        RichText::new(label).strong().color(self.theme.accent)
                    } else {
                        RichText::new(label).color(self.theme.dim)
                    };
                    if ui
                        .selectable_label(active, txt)
                        .on_hover_cursor(egui::CursorIcon::Default)
                        .clicked()
                    {
                        chosen = Some(cmd);
                    }
                }
            });
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let files = self.quick_open_files();
        if let Some(cmd) = self.palette.show(ctx, &files) {
            self.execute(cmd, ctx);
        }
        if let Some((path, line)) = self.search.show(ctx, self.root.as_deref()) {
            self.open_file(path);
            self.editor.active = line.min(self.doc.blocks.len().saturating_sub(1));
        }
        if let Some(name) = self.graph.show(ctx, &self.link_docs, &self.theme) {
            if let Some(p) = self
                .files()
                .into_iter()
                .find(|p| p.file_stem().map(|s| s.to_string_lossy() == name).unwrap_or(false))
            {
                self.open_file(p);
            }
        }
        if let Some(content) = self.snapshots.show(ctx) {
            self.doc.record();
            self.doc.set_source(&content);
            self.on_changed();
            self.status = "已恢复到所选版本".to_string();
        }
        self.stats.show(ctx, &self.stats_cache, &self.theme);

        if let Some(kind) = self.menu {
            let title = match kind {
                MenuKind::File => "文件",
                MenuKind::Edit => "编辑",
                MenuKind::View => "视图",
                MenuKind::Theme => "主题",
                MenuKind::Help => "帮助",
            };
            let guard = self.menu_guard;
            self.menu_guard = false;
            let mut chosen: Option<Command> = None;
            let mut dismiss = false;
            let mut open = true;
            let window_resp = egui::Window::new(title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_pos(egui::pos2(16.0, 64.0))
                .show(ctx, |ui| {
                    for c in menu_commands(kind) {
                        if ui.button(c.label()).clicked() {
                            chosen = Some(c);
                        }
                    }
                    if kind == MenuKind::Theme {
                        ui.separator();
                        for t in ThemeKind::ALL {
                            if ui.button(t.name()).clicked() {
                                self.config.theme = t;
                                let mut nt = Theme::get(t);
                                nt.body = self.config.font_size;
                                nt.mono = (self.config.font_size * 0.84).max(11.0);
                                self.theme = nt;
                                self.config.save();
                                self.theme_dirty = true;
                                dismiss = true;
                            }
                        }
                    }
                });
            if !open {
                dismiss = true;
            }
            // 打开或切换菜单的那一帧跳过，避免标题按钮的点击被当成「点击外部」。
            if !guard {
                if let Some(inner) = window_resp {
                    if inner.response.clicked_elsewhere() {
                        dismiss = true;
                    }
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                dismiss = true;
            }
            if let Some(c) = chosen {
                self.execute(c, ctx);
                dismiss = true;
            }
            if dismiss {
                self.menu = None;
            }
        }

        if self.about {
            egui::Window::new("关于 Markd")
                .open(&mut self.about)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new("Markd").heading());
                    ui.label("纯 Rust + egui 从零构建的 Markdown 编辑器");
                    ui.label("自研 AST 解析、块级实时渲染、代码高亮与流程图绘制");
                    ui.separator();
                    ui.label("特色：版本快照、命令面板、全文搜索、双向链接、写作统计");
                    ui.label(RichText::new("本作品为从零实现的独立 Markdown 编辑器").small());
                });
        }

        if self.shortcuts {
            egui::Window::new("快捷键一览")
                .open(&mut self.shortcuts)
                .collapsible(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    let rows = [
                        ("Cmd/Ctrl + S / Shift + S", "保存 / 另存为"),
                        ("Cmd/Ctrl + O", "打开文件"),
                        ("Cmd/Ctrl + Shift + O", "快速打开"),
                        ("Cmd/Ctrl + N", "新建文档"),
                        ("Cmd/Ctrl + P", "命令面板"),
                        ("Cmd/Ctrl + 1 / 2 / 3", "一级 / 二级 / 三级标题"),
                        ("Cmd/Ctrl + B / I / K", "粗体 / 斜体 / 链接"),
                        ("Cmd/Ctrl + Shift + `", "行内代码"),
                        ("Ctrl + Shift + `", "删除线"),
                        ("Cmd + Option + C / B", "代码块 / 公式块"),
                        ("Cmd + Option + T / Q", "表格 / 引用"),
                        ("Cmd + Option + O / U", "有序 / 无序列表"),
                        ("Cmd/Ctrl + /", "切换源码模式"),
                        ("Cmd/Ctrl + F", "全文搜索"),
                        ("Cmd/Ctrl + Shift + L", "显示 / 隐藏侧边栏"),
                        ("Cmd + Ctrl + 1", "显示大纲"),
                        ("F8 / F9", "专注 / 打字机模式"),
                        ("Cmd/Ctrl + J", "切换终端面板"),
                        ("Cmd/Ctrl + Z / Shift + Z", "撤销 / 重做"),
                        ("Cmd/Ctrl + Shift + E", "导出 HTML"),
                        ("Cmd/Ctrl + Shift + H", "显示 / 隐藏侧栏标签栏"),
                        ("F1", "快捷键一览"),
                        ("Enter / Shift + Tab", "拆分块 / 减少缩进"),
                        ("Ctrl + 点击链接", "打开链接"),
                    ];
                    for (k, v) in rows {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(k).monospace().color(self.theme.accent));
                            ui.label(v);
                        });
                    }
                });
        }

        if self.workspace_dialog {
            let mut input = std::mem::take(&mut self.workspace_input);
            let mut open = self.workspace_dialog;
            let mut cancel = false;
            let mut confirm = false;
            let mut pick = false;
            egui::Window::new("打开工作区")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("工作区文件夹路径：");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut input);
                        if ui
                            .button("选择…")
                            .on_hover_cursor(egui::CursorIcon::Default)
                            .clicked()
                        {
                            pick = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("打开").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            // 原生对话框在 UI 闭包结束后调用，避免阻塞 egui 的构建过程。
            if pick {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("选择工作区文件夹")
                    .pick_folder()
                {
                    input = p.to_string_lossy().to_string();
                }
            }
            if cancel {
                open = false;
            }
            self.workspace_input = input.clone();
            self.workspace_dialog = open;
            if confirm {
                self.set_workspace(PathBuf::from(input.trim()));
                self.workspace_dialog = false;
            }
        }

        if self.save_dialog {
            let mut input = std::mem::take(&mut self.save_input);
            let mut open = self.save_dialog;
            let mut cancel = false;
            let mut chosen = None;
            let mut pick = false;
            egui::Window::new("另存为")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("文件路径：");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut input);
                        if ui
                            .button("选择…")
                            .on_hover_cursor(egui::CursorIcon::Default)
                            .clicked()
                        {
                            pick = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("保存").clicked() {
                            chosen = Some(input.clone());
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            // 原生保存对话框：选中的路径回填到输入框，用户仍可再编辑后点「保存」。
            if pick {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("选择保存位置")
                    .add_filter("Markdown", &["md", "markdown"]);
                if let Some(r) = &self.root {
                    dialog = dialog.set_directory(r);
                }
                if let Some(p) = dialog.save_file() {
                    input = p.to_string_lossy().to_string();
                }
            }
            if cancel {
                open = false;
            }
            self.save_input = input;
            self.save_dialog = open;
            if let Some(s) = chosen {
                let p = PathBuf::from(s.trim());
                self.doc.path = Some(p.clone());
                self.save();
                self.save_dialog = false;
                self.refresh_tree();
            }
        }

        if self.table_dialog {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("插入表格")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("行数");
                        ui.add(egui::DragValue::new(&mut self.table_rows).range(1..=100));
                        ui.label("列数");
                        ui.add(egui::DragValue::new(&mut self.table_cols).range(1..=20));
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("插入").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if confirm {
                let src = table_template(self.table_rows, self.table_cols);
                editor::insert_template(&mut self.doc, &mut self.editor, &src);
                self.on_changed();
            }
            if confirm || cancel || !open {
                self.table_dialog = false;
            }
        }
    }

    fn auto_save(&mut self) {
        let n = now_secs();
        if self.config.auto_save
            && self.doc.dirty
            && self.doc.path.is_some()
            && n.saturating_sub(self.last_autosave) > 20
        {
            if self.doc.save().is_ok() {
                self.last_autosave = n;
            }
        }
    }
}

impl eframe::App for MarkdApp {
    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        self.terminal.poll();
        // 消费 macOS 原生菜单栏产生的命令。
        let pending: Vec<Command> = match self.menu_queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for c in pending {
            self.execute(c, &ctx);
        }
        if self.theme_dirty {
            self.theme.apply(&ctx);
            self.theme_dirty = false;
        }
        self.handle_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        let ctx2 = ctx.clone();
        // 菜单栏固定在窗口最顶部（VS Code 风格的单行窄条），下方另起一条格式工具条。
        let menu_bg = self.theme.panel;
        let menubar = egui::Panel::top("menubar")
            .frame(egui::Frame::NONE.fill(menu_bg).inner_margin(4.0))
            .show(&mut *root_ui, |ui| self.menubar(ui, &ctx2));
        // 自定义标题栏：让系统红绿灯垂直居中于菜单栏这一行（非 macOS 为空操作）。
        let r = menubar.response.rect;
        frame.set_traffic_lights_position(&ctx, egui::Rangef::new(r.min.y, r.max.y), 12.0);
        egui::Panel::top("toolbar").show(&mut *root_ui, |ui| self.toolbar(ui, &ctx2));
        // bottom 必须先于 left 声明：bottom 只收窄父 Ui 的 y 轴，此时 x 仍是整窗宽度，
        // 状态栏才能横跨整个窗口；若放在 left 之后，x 起点会被推到侧栏右缘而只剩中央区域。
        egui::Panel::bottom("statusbar").show(&mut *root_ui, |ui| self.statusbar(ui, &ctx2));
        // 终端面板：位于状态栏上方、横跨整窗；高度同样由自己维护并可拖动上边缘调整。
        if self.terminal.open {
            let h = self.terminal.height.clamp(120.0, 600.0);
            let panel = egui::Panel::bottom("terminal_panel")
                .resizable(false)
                .show_separator_line(true)
                .exact_size(h)
                .show(&mut *root_ui, |ui| self.terminal.ui(ui, &ctx2, &self.theme));
            let r = panel.response.rect;
            let handle = egui::Rect::from_min_max(
                egui::pos2(r.min.x, r.min.y - 6.0),
                egui::pos2(r.max.x, r.min.y + 6.0),
            );
            #[allow(deprecated)]
            let resize_id = egui::Id::new("terminal_resize");
            let drag = root_ui.interact(handle, resize_id, egui::Sense::drag());
            if drag.hovered() || drag.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if drag.dragged() {
                self.terminal.height = (h - drag.drag_delta().y).clamp(120.0, 600.0);
            }
        }
        if self.config.show_sidebar {
            // 宽度完全由 `config.sidebar_width` 控制。0.37 的 `PanelState` 不会持久化尺寸，
            // 每帧传 `default_size` 会把拖动结果弹回去，所以这里关掉内置 resizable，
            // 改用 `exact_size` + 在右缘自绘一条可拖动的分隔条。
            let width = self.config.sidebar_width.clamp(160.0, 900.0);
            let panel = egui::Panel::left("sidebar")
                .resizable(false)
                .show_separator_line(true)
                // 右边距为 0：滚动条紧贴侧栏分隔线，视觉上和那条线重合
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    left: 8,
                    right: 0,
                    top: 6,
                    bottom: 4,
                }))
                .exact_size(width)
                .show(&mut *root_ui, |ui| self.sidebar(ui));

            let r = panel.response.rect;
            // 放在面板外侧，避免和侧栏滚动条抢同一块区域
            let handle = egui::Rect::from_min_max(
                egui::pos2(r.max.x, r.min.y),
                egui::pos2(r.max.x + 8.0, r.max.y),
            );
            #[allow(deprecated)]
            let resize_id = egui::Id::new("sidebar_resize");
            let drag = root_ui.interact(handle, resize_id, egui::Sense::drag());
            if drag.hovered() || drag.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if drag.dragged() {
                self.config.sidebar_width = (width + drag.drag_delta().x).clamp(160.0, 900.0);
            }
            if drag.drag_stopped() {
                self.config.save();
            }
        }

        // 终端持有焦点时编辑区进入只读，避免它每帧把焦点抢回去。
        self.editor.read_only = self.terminal.has_focus();
        let (event, actions) = egui::CentralPanel::default()
            .show(&mut *root_ui, |ui| {
                let mut actions = Vec::new();
                let base: Option<PathBuf> = self
                    .doc
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf());
                let ev = editor::show_editor(
                    ui,
                    &mut self.doc,
                    &mut self.editor,
                    &self.theme,
                    &ctx,
                    &mut self.images,
                    base.as_deref(),
                    &self.outline,
                    self.config.source_mode,
                    self.config.focus_mode,
                    self.config.typewriter,
                    &mut actions,
                );
                (ev, actions)
            })
            .inner;

        match event {
            EditorEvent::Changed => self.on_changed(),
            EditorEvent::None => {}
        }
        // 用户点回了编辑区：把键盘焦点从终端交还，下一帧编辑区恢复可编辑。
        if self.editor.clicked && self.terminal.has_focus() {
            self.terminal.blur(&ctx);
        }
        self.editor.clicked = false;
        for a in actions {
            match a {
                Action::ToggleTask(i) => {
                    editor::toggle_task(&mut self.doc, i);
                    self.on_changed();
                }
                Action::OpenLink(url) => self.open_link(&url),
                Action::Jump(i) => {
                    self.editor.active = i.min(self.doc.blocks.len().saturating_sub(1));
                    self.editor.scroll_req = Some(self.editor.active);
                }
                Action::Notify(msg) => self.status = msg,
                Action::Caret(i, off) => {
                    self.editor.active = i;
                    self.editor.focus_req = Some(i);
                    self.editor.pending_caret = Some(off);
                }
                Action::SetLang(i, lang) => {
                    let rebuilt = match self.doc.blocks.get(i) {
                        Some(crate::ast::Block::Code { code, source, .. }) => {
                            let fence = if source.starts_with("~~~") { "~~~" } else { "```" };
                            Some(format!("{}{}\n{}\n{}", fence, lang, code, fence))
                        }
                        _ => None,
                    };
                    if let Some(src) = rebuilt {
                        self.doc.record();
                        self.doc.edit_block(i, &src);
                        self.on_changed();
                        self.status = format!(
                            "代码语言：{}",
                            if lang.is_empty() { "纯文本" } else { lang.as_str() }
                        );
                    }
                }
            }
        }

        self.windows(&ctx);
        self.auto_save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 大纲显隐开关：点击隐藏、再点显示，且状态可往返。
    #[test]
    fn outline_toggle_roundtrip() {
        // 初始隐藏（文件树）→ 点击 → 显示大纲
        let (show, tab) = outline_toggle(false);
        assert!(show);
        assert_eq!(tab, SideTab::Outline);

        // 再点击 → 隐藏（回到文件树）
        let (show2, tab2) = outline_toggle(show && tab == SideTab::Outline);
        assert!(!show2);
        assert_eq!(tab2, SideTab::Files);

        // 第三次点击 → 又显示
        let (show3, tab3) = outline_toggle(show2 && tab2 == SideTab::Outline);
        assert!(show3);
        assert_eq!(tab3, SideTab::Outline);
    }

    /// 验证「文件 / 大纲」入口的四种点击组合：展开、收起、互切。
    #[test]
    fn sidebar_click_toggles_and_switches() {
        // 1) 收起状态点「文件」→ 展开文件树
        let (show, tab, outline) = sidebar_click(false, SideTab::Outline, SideTab::Files);
        assert!(show, "应收起->展开");
        assert_eq!(tab, SideTab::Files);
        assert!(!outline);

        // 2) 正显示文件树再点「文件」→ 收起
        let (show, tab, outline) = sidebar_click(true, SideTab::Files, SideTab::Files);
        assert!(!show, "再点当前页签应收起侧栏");
        assert_eq!(tab, SideTab::Files);
        assert!(!outline);

        // 3) 正显示文件树点「大纲」→ 切换到大纲
        let (show, tab, outline) = sidebar_click(true, SideTab::Files, SideTab::Outline);
        assert!(show, "切到另一页签应保持展开");
        assert_eq!(tab, SideTab::Outline);
        assert!(outline);

        // 4) 正显示大纲再点「大纲」→ 收起
        let (show, tab, outline) = sidebar_click(true, SideTab::Outline, SideTab::Outline);
        assert!(!show, "再点当前页签应收起侧栏");
        assert_eq!(tab, SideTab::Outline);
        assert!(outline);

        // 5) 收起状态点「大纲」→ 展开大纲
        let (show, tab, outline) = sidebar_click(false, SideTab::Files, SideTab::Outline);
        assert!(show);
        assert_eq!(tab, SideTab::Outline);
        assert!(outline);
    }
}
