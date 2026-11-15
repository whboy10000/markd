//! 应用外壳：窗口布局、菜单、快捷键、状态栏、各功能面板的整合。

use crate::document::{Document, OutlineItem};
use crate::editor::{self, EditorEvent, EditorState};
use crate::features::{
    Command, GraphPanel, Palette, PaletteMode, SearchPanel, SnapshotStore, WritingStats,
};
use crate::find::FindPanel;
use crate::render::{Action, ImageCache};
use crate::sidebar::{self, SideAction};
use crate::terminal::Terminal;
use crate::theme::{Theme, ThemeKind};
use crate::workspace::{self, Config, FileNode};
use egui::{self, Color32, RichText};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 这份文档是否应该被自动保存：开关打开、有未保存修改、且已经落过盘。
/// 未命名文档（没有路径）不自动写盘，要等用户按保存时再选路径。
fn should_autosave(enabled: bool, dirty: bool, has_path: bool) -> bool {
    enabled && dirty && has_path
}

/// 距上次自动保存是否已经够久（间隔下限 5 秒，和设置里的范围一致）。
fn autosave_due(now: u64, last: u64, interval: u64) -> bool {
    now.saturating_sub(last) >= interval.max(5)
}

/// 把 paths[i] 指定的位置写回第 i 个标签里所有有修改的文档，返回真正写过的路径。
/// 没有路径或没有修改的标签跳过；退出前的批量保存都走这里，便于单测覆盖。
fn write_dirty_tabs(tabs: &mut [Tab], paths: &[Option<PathBuf>]) -> std::io::Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for i in 0..tabs.len() {
        let Some(st) = tabs[i].state.as_mut() else {
            continue;
        };
        if !st.doc.dirty {
            continue;
        }
        let Some(path) = paths.get(i).cloned().flatten() else {
            continue;
        };
        st.doc.path = Some(path.clone());
        st.doc.save()?;
        tabs[i].path = Some(path.clone());
        written.push(path);
    }
    Ok(written)
}

/// 关闭请求是否需要先弹「有未保存的修改」确认框：
/// 用户已经选过「不保存」（force_quit）时直接放行。
fn needs_exit_confirm(force_quit: bool, close_requested: bool, unsaved: usize) -> bool {
    !force_quit && close_requested && unsaved > 0
}

/// 工具条上的自绘图标按钮（放大镜 / 橡皮擦）：不依赖 emoji 字体，
/// 尺寸按普通按钮算（文字高度 + 上下内边距），底色与描边色都跟随主题。
fn icon_button(
    ui: &mut egui::Ui,
    hover: &str,
    draw: fn(&egui::Painter, egui::Rect, egui::Color32, egui::Color32),
) -> bool {
    let pad = ui.spacing().button_padding;
    let h = (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * pad.y)
        .max(ui.spacing().interact_size.y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(h + 2.0, h), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&resp);
        // 底色用 weak_bg_fill（和普通 Button 同一个字段）；描边色用主题文字色：
        // theme.apply() 设的是 override_text_color，不读它自制图标会和按钮文字不同色。
        draw(
            ui.painter(),
            rect,
            visuals.weak_bg_fill,
            ui.visuals().text_color(),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::Default)
        .on_hover_text(hover)
        .clicked()
}

/// 把向量按给定角度旋转（屏幕坐标 y 向下）。
fn rotate(v: egui::Vec2, angle: f32) -> egui::Vec2 {
    let (s, c) = angle.sin_cos();
    egui::vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// 橡皮擦图标（参考 WPS 的擦除格式）：一块斜放的橡皮 + 分界线 + 下面的擦拭痕迹。
fn painter_eraser_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    let stroke = egui::Stroke::new(1.6, fg);
    let h = rect.height();
    let c = rect.center() + egui::vec2(0.0, -h * 0.07);
    // 橡皮形体：长条矩形绕中心转 -35°，长边 half.x、短边 half.y
    let angle = -0.62;
    let long = rotate(egui::vec2(h * 0.36, 0.0), angle);
    let short = rotate(egui::vec2(0.0, h * 0.21), angle);
    let body = vec![
        c - long - short,
        c + long - short,
        c + long + short,
        c - long + short,
    ];
    // 先给橡胶那一端（左下、贴着纸的那头）填一层浅色再描边：
    // 两段深浅不同才像橡皮，不像一块斜放的方框
    let t = c - long * 0.25;
    let rubber = vec![c - long - short, t - short, t + short, c - long + short];
    painter.add(egui::Shape::convex_polygon(
        rubber,
        fg.gamma_multiply(0.35),
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::closed_line(body, stroke));
    // 橡胶与外壳的分界
    painter.line_segment([t - short, t + short], stroke);
    // 底下的擦拭痕迹
    let y = rect.center().y + h * 0.37;
    painter.line_segment(
        [
            egui::pos2(rect.center().x - h * 0.2, y),
            egui::pos2(rect.center().x + h * 0.2, y),
        ],
        stroke,
    );
}

/// 在给定矩形里画一个放大镜：左上偏的圆 + 右下斜柄，圆内留空。
fn painter_search_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    let stroke = egui::Stroke::new(1.8, fg);
    let h = rect.height();
    let r = h * 0.27;
    let c = rect.center() + egui::vec2(-2.0, -2.0);
    painter.circle_stroke(c, r, stroke);
    // 斜柄从圆的右下 45° 处向外伸，长度略大于半径，看起来才像放大镜而不是字母 Q。
    let d = std::f32::consts::FRAC_1_SQRT_2;
    let a = c + egui::vec2(r * d, r * d);
    let b = a + egui::vec2(r * 1.3 * d, r * 1.3 * d);
    painter.line_segment([a, b], stroke);
}

/// 保存路径统一补上 .md 扩展名。
fn with_md_ext(path: PathBuf) -> PathBuf {
    let has_ext = path
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_lowercase();
            e == "md" || e == "markdown"
        })
        .unwrap_or(false);
    if has_ext {
        return path;
    }
    let mut p = path;
    p.set_extension("md");
    p
}

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
            Command::New,
            Command::OpenWorkspace,
            Command::Save,
            Command::SaveAs,
            Command::ExportHtml,
            Command::Snapshot,
            Command::Quit,
        ],
        MenuKind::Edit => vec![
            Command::Undo,
            Command::Redo,
            Command::Bold,
            Command::Italic,
            Command::Strike,
            Command::Code,
            Command::Link,
            Command::Image,
            Command::Paragraph,
            Command::H1,
            Command::H2,
            Command::H3,
            Command::H4,
            Command::H5,
            Command::H6,
            Command::Quote,
            Command::Bullet,
            Command::Ordered,
            Command::Task,
            Command::Table,
            Command::Rule,
            Command::Math,
            Command::Mermaid,
            Command::Toc,
            Command::Footnote,
            Command::FrontMatter,
        ],
        MenuKind::View => vec![
            Command::ToggleSource,
            Command::ToggleFocus,
            Command::ToggleTypewriter,
            Command::ToggleSidebar,
            Command::ToggleOutline,
            Command::ToggleAutoSave,
            Command::ToggleSidebarHeader,
            Command::ToggleWorkspaceBar,
            Command::ToggleTerminal,
            Command::OpenFileDialog,
            Command::Search,
            Command::Graph,
            Command::Stats,
        ],
        MenuKind::Theme => vec![Command::ThemeNext],
        MenuKind::Help => vec![Command::Shortcuts, Command::Settings, Command::About],
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
                let name = p
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                m.insert(name, links);
            }
        }
    }
    m
}

/// 一个标签页的完整文档状态。除活动标签外，其余标签的状态都暂存在这里，
/// 切换时与 MarkdApp 上的 doc/editor/outline/stats 字段整体换出换入。
struct TabState {
    doc: Document,
    editor: EditorState,
    outline: Vec<OutlineItem>,
    stats: crate::document::Stats,
}

/// 一个标签页：只有元信息常驻，重状态在 state 里（活动标签的 state 为 None）。
struct Tab {
    /// 标签唯一编号，用于区分 egui 状态下不同标签的编辑控件。
    uid: u64,
    /// 已落盘的文档路径；未命名的新文档为 None。
    path: Option<PathBuf>,
    /// 未命名文档的编号，用来区分多个空白标签并生成默认文件名。
    seq: usize,
    state: Option<TabState>,
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
    /// 文档内查找/替换弹窗（Cmd/Ctrl+F、Ctrl+H）。
    find: FindPanel,
    graph: GraphPanel,
    stats: WritingStats,
    snapshots: SnapshotStore,
    status: String,
    about: bool,
    shortcuts: bool,
    table_dialog: bool,
    table_rows: usize,
    table_cols: usize,
    link_docs: HashMap<String, Vec<String>>,
    /// 后台线程算好的链接图，算完通过它回传。
    link_rx: Option<std::sync::mpsc::Receiver<HashMap<String, Vec<String>>>>,
    last_autosave: u64,
    /// 关闭窗口前的「有未保存文件」确认弹窗。
    exit_dialog: bool,
    /// 用户已选择「不保存」，下次关闭请求直接放行。
    force_quit: bool,
    theme_dirty: bool,
    menu_guard: bool,
    workspace_dialog: bool,
    quick_cache_dir: String,
    quick_cache: Vec<PathBuf>,
    menu_queue: crate::native_menu::CommandQueue,
    terminal: Terminal,
    settings: crate::settings::Settings,
    /// 首页：显示欢迎页与最近打开的文件，点「首页」按钮或标签栏的首页页签切换。
    home: bool,
    /// 所有打开的标签页；为空时只显示首页。
    tabs: Vec<Tab>,
    /// 活动标签下标（tabs 非空时有效）。
    active: usize,
    /// 未命名文档编号自增器。
    untitled_seq: usize,
    /// 标签编号自增器。
    next_uid: u64,
    frame_count: u64,
}

/// 首页上可点的入口。
enum HomeAction {
    New,
    OpenFile,
    OpenWorkspace,
    Recent(PathBuf),
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

        let initial = initial.filter(|p| p.is_file());
        // 没有指定要打开的文件时先亮首页（欢迎页 + 最近打开），像 VS Code 那样。
        let start_home = initial.is_none();
        // 没有命令行参数时，按设置恢复上次打开的文件
        let initial = initial.or_else(|| {
            if config.restore_last_file {
                config
                    .last_file
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_file())
            } else {
                None
            }
        });
        let start_file = initial.clone();
        let (doc, root) = if let Some(p) = initial {
            let text = workspace::read(&p).unwrap_or_else(|_| WELCOME.to_string());
            let r = p.parent().map(|x| x.to_path_buf());
            (Document::new(&text, Some(p)), r)
        } else {
            let r = config
                .workspace
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .or_else(|| std::env::current_dir().ok())
                // Finder / Dock 启动时 cwd 是根目录，不能拿来当工作区：
                // 建树与链接图会遍历整块磁盘，启动直接卡死。根目录没有父目录。
                .filter(|p| p.parent().is_some());
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
        // 链接图要遍历整个工作区目录，桌面上目录一多就要好几秒。
        // 它只有「双向链接关系图」用得到，所以启动时不算，点开面板再后台算。
        let link_docs = HashMap::new();
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
        let mut app = Self {
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
            find: FindPanel::default(),
            graph: GraphPanel::default(),
            snapshots: SnapshotStore::load(),
            status: String::new(),
            about: false,
            shortcuts: false,
            table_dialog: false,
            table_rows: 3,
            table_cols: 3,
            link_docs,
            link_rx: None,
            last_autosave: now_secs(),
            exit_dialog: false,
            force_quit: false,
            theme_dirty: false,
            menu_guard: false,
            workspace_dialog: false,
            quick_cache_dir: String::new(),
            quick_cache: Vec::new(),
            menu_queue,
            terminal,
            settings: Default::default(),
            home: start_home,
            // 启动时带了文件就把该文件做成第一个标签，否则只显示首页、没有标签。
            tabs: start_file
                .map(|p| Tab {
                    uid: 1,
                    path: Some(p),
                    seq: 0,
                    state: None,
                })
                .into_iter()
                .collect(),
            active: 0,
            untitled_seq: 0,
            next_uid: 1,
            frame_count: 0,
        };
        // TEMP 调试：模拟点击「+」新建文档
        if std::env::var("MARKD_AUTO_NEW").as_deref() == Ok("1") {
            app.new_document();
        }
        app
    }

    /// 把文件记进「最近打开」：最新在前、去重、最多 10 条。
    fn push_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.config.recent.retain(|x| x != &s);
        self.config.recent.insert(0, s);
        self.config.recent.truncate(10);
        self.config.save();
    }

    /// 首页：欢迎信息 + 三个入口 + 最近打开列表。
    fn home_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<HomeAction> {
        let mut act = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let w = ui.available_width().min(620.0);
                let pad = ((ui.available_width() - w) / 2.0).max(0.0);
                ui.add_space(56.0);
                ui.horizontal_top(|ui| {
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.label(
                            RichText::new("Markd")
                                .size(36.0)
                                .strong()
                                .color(self.theme.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("纯 Rust 实现的 Markdown 编辑器")
                                .size(13.0)
                                .color(self.theme.dim),
                        );
                        ui.add_space(22.0);
                        ui.horizontal(|ui| {
                            if ui.button("新建文档").clicked() {
                                act = Some(HomeAction::New);
                            }
                            if ui.button("打开文件…").clicked() {
                                act = Some(HomeAction::OpenFile);
                            }
                            if ui.button("打开工作区…").clicked() {
                                act = Some(HomeAction::OpenWorkspace);
                            }
                        });
                        ui.add_space(30.0);
                        ui.label(RichText::new("最近打开").color(self.theme.dim));
                        ui.add_space(8.0);
                        if self.config.recent.is_empty() {
                            ui.label(RichText::new("还没有打开过文件").color(self.theme.dim));
                        }
                        for p in self.config.recent.clone() {
                            let path = PathBuf::from(&p);
                            let exists = path.is_file();
                            let name = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| p.clone());
                            let dir = path
                                .parent()
                                .map(|d| d.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let color = if exists {
                                self.theme.text
                            } else {
                                self.theme.dim
                            };
                            let row = ui
                                .horizontal(|ui| {
                                    let r = ui.add(
                                        egui::Label::new(RichText::new(name).color(color))
                                            .sense(egui::Sense::click()),
                                    );
                                    ui.label(RichText::new(dir).small().color(self.theme.dim));
                                    r
                                })
                                .inner;
                            if row.hovered() && exists {
                                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if row.clicked() && exists {
                                act = Some(HomeAction::Recent(path.clone()));
                            }
                            ui.add_space(3.0);
                        }
                    });
                });
            });
        act
    }

    /// 把配置里的外观项应用到主题/缩放/编辑器。
    fn apply_config(&mut self, ctx: &egui::Context) {
        self.theme = Theme::get(self.config.theme);
        self.theme.body = self.config.font_size;
        self.theme.mono = (self.config.font_size * 0.84).max(11.0);
        self.theme_dirty = true;
        ctx.set_zoom_factor(self.config.ui_scale.clamp(0.8, 1.6));
        self.editor.tab_size = self.config.tab_size.max(1);
        self.editor.auto_pair = self.config.auto_pair;
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
        // 链接图按需生成（打开关系图时），这里不再同步扫盘。
        self.link_docs.clear();
    }

    /// 后台线程构建双向链接关系图，算完回填并唤醒界面。
    fn spawn_link_graph(&mut self, ctx: &egui::Context) {
        let Some(r) = self.root.clone() else {
            self.link_docs.clear();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.link_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = build_link_graph(Some(&r));
            let _ = tx.send(m);
            ctx.request_repaint();
        });
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

    /// 生成新的标签编号。
    fn next_tab_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    /// 把当前活动标签的状态存回 tabs[active]，切标签/关标签前调用。
    /// 存完后 self.doc 等字段是空占位值，必须紧跟 load_tab。
    fn stash_active(&mut self) {
        if self.active >= self.tabs.len() {
            return;
        }
        let tab = &mut self.tabs[self.active];
        tab.path = self.doc.path.clone();
        tab.state = Some(TabState {
            doc: std::mem::replace(&mut self.doc, Document::new("", None)),
            editor: std::mem::take(&mut self.editor),
            outline: std::mem::take(&mut self.outline),
            stats: std::mem::take(&mut self.stats_cache),
        });
    }

    /// 把 tabs[i] 暂存的状态换进活动字段。
    fn load_tab(&mut self, i: usize) {
        let Some(st) = self.tabs.get_mut(i).and_then(|t| t.state.take()) else {
            return;
        };
        self.active = i;
        self.doc = st.doc;
        self.editor = st.editor;
        self.outline = st.outline;
        self.stats_cache = st.stats;
        self.stats_at = std::time::Instant::now();
        self.stats.tick(self.stats_cache.words);
        self.config.last_file = self
            .doc
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
    }

    /// 标签标题：已保存的用文件名，未保存的用「未命名-N」。
    fn tab_label(&self, i: usize) -> String {
        let Some(t) = self.tabs.get(i) else {
            return String::new();
        };
        let path = t
            .path
            .clone()
            .or_else(|| t.state.as_ref().and_then(|s| s.doc.path.clone()));
        match path {
            Some(p) => p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string()),
            None => format!("未命名-{}", t.seq.max(1)),
        }
    }

    /// 标签是否有未保存的修改（活动标签的状态在 self.doc 里）。
    fn tab_dirty(&self, i: usize) -> bool {
        if i == self.active && i < self.tabs.len() {
            return self.doc.dirty;
        }
        self.tabs
            .get(i)
            .and_then(|t| t.state.as_ref())
            .map(|s| s.doc.dirty)
            .unwrap_or(false)
    }

    /// 切换到第 i 个标签；首页随之退出。
    fn switch_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.home = false;
        if i == self.active {
            return;
        }
        self.stash_active();
        self.load_tab(i);
    }

    /// 关闭第 i 个标签；关掉活动标签后由右侧（没有则左侧）接手，全关则回首页。
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let was_active = i == self.active;
        let label = self.tab_label(i);
        let dirty = self.tab_dirty(i);
        if was_active && self.tabs.len() == 1 {
            self.tabs.clear();
            self.doc = Document::new("", None);
            self.editor = EditorState::default();
            self.outline.clear();
            self.stats_cache = Default::default();
            self.active = 0;
            self.home = true;
            self.status = "已关闭全部标签".to_string();
            return;
        }
        if was_active {
            self.stash_active();
        }
        self.tabs.remove(i);
        if was_active {
            let next = i.min(self.tabs.len() - 1);
            self.load_tab(next);
        } else if i < self.active {
            self.active -= 1;
        }
        self.status = if dirty {
            format!("已关闭 {}（未保存的修改已丢弃）", label)
        } else {
            format!("已关闭 {}", label)
        };
    }

    /// 打开文件：已经开着的标签直接切过去，否则新建一个标签。
    fn open_file(&mut self, path: PathBuf) {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path.as_deref() == Some(path.as_path()))
        {
            self.switch_tab(i);
            self.status = format!(
                "已切换到 {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            return;
        }
        match workspace::read(&path) {
            Ok(text) => {
                self.home = false;
                self.push_recent(&path);
                self.stash_active();
                let doc = Document::new(&text, Some(path.clone()));
                let editor = EditorState {
                    focus_req: Some(0),
                    ..Default::default()
                };
                let outline = doc.outline();
                let uid = self.next_tab_uid();
                self.tabs.push(Tab {
                    uid,
                    path: Some(path.clone()),
                    seq: 0,
                    state: Some(TabState {
                        doc,
                        editor,
                        outline,
                        stats: Default::default(),
                    }),
                });
                let i = self.tabs.len() - 1;
                self.load_tab(i);
                self.refresh_stats();
                self.refresh_tree();
                // 状态栏只显示文件名，完整路径既长又乱。
                self.status = format!(
                    "已打开 {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.config.last_file = Some(path.to_string_lossy().to_string());
                self.config.save();
            }
            Err(e) => self.status = format!("打开失败: {}", e),
        }
    }

    /// 新建标签页：先在内存里建空白文档，Ctrl/Cmd+S 时才问保存路径。
    fn new_document(&mut self) {
        self.untitled_seq += 1;
        self.stash_active();
        let editor = EditorState {
            focus_req: Some(0),
            ..Default::default()
        };
        let uid = self.next_tab_uid();
        self.tabs.push(Tab {
            uid,
            path: None,
            seq: self.untitled_seq,
            state: Some(TabState {
                doc: Document::new("", None),
                editor,
                outline: Vec::new(),
                stats: Default::default(),
            }),
        });
        let i = self.tabs.len() - 1;
        self.load_tab(i);
        self.home = false;
        self.status = "已新建 Markdown 文档，Ctrl/Cmd+S 选择保存路径".to_string();
    }

    /// 保存：已有路径的文档直接原地写回（像 VS Code 一样，不再弹框）；
    /// 新建的未命名文档才需要先选保存路径。
    fn save(&mut self) {
        if self.tabs.is_empty() {
            self.status = "没有可保存的文档".to_string();
            return;
        }
        if let Some(p) = self.doc.path.clone() {
            self.save_to(p);
            return;
        }
        self.save_as();
    }

    /// 另存为：总是弹保存位置选择框（原生对话框），选好路径再写盘。
    fn save_as(&mut self) {
        if self.tabs.is_empty() {
            self.status = "没有可保存的文档".to_string();
            return;
        }
        let current = self
            .doc
            .path
            .clone()
            .or_else(|| self.tabs.get(self.active).and_then(|t| t.path.clone()));
        let default_name = current
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{}.md", self.tab_label(self.active)));
        let start_dir = current
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| self.root.clone());
        let mut dialog = rfd::FileDialog::new()
            .set_title("选择保存路径")
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(default_name);
        if let Some(d) = start_dir.filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(d);
        }
        match dialog.save_file() {
            Some(p) => self.save_to(p),
            None => self.status = "已取消保存".to_string(),
        }
    }

    /// 把当前文档写到指定路径，并同步标签、最近打开、文件树。
    fn save_to(&mut self, path: PathBuf) {
        let path = with_md_ext(path);
        self.doc.path = Some(path.clone());
        match self.doc.save() {
            Ok(()) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.path = Some(path.clone());
                }
                self.snapshots.capture(&self.doc);
                self.config.last_file = Some(path.to_string_lossy().to_string());
                self.push_recent(&path);
                self.refresh_tree();
                self.status = format!("已保存 {}", path.display());
            }
            Err(e) => self.status = format!("保存失败: {}", e),
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
        let i = all
            .iter()
            .position(|t| *t == self.config.theme)
            .unwrap_or(0);
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
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", url])
                .spawn();
            self.status = format!("已打开链接 {}", url);
        } else if let Some(p) =
            workspace::resolve(self.doc.path.as_ref().and_then(|x| x.parent()), url)
        {
            if p.is_file() {
                if p.extension()
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
                {
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
            Command::SaveAs => self.save_as(),
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
            Command::Paragraph => editor::make_paragraph(&mut self.doc, &mut self.editor),
            Command::H1 => editor::apply_prefix(&mut self.doc, &mut self.editor, "# "),
            Command::H2 => editor::apply_prefix(&mut self.doc, &mut self.editor, "## "),
            Command::H3 => editor::apply_prefix(&mut self.doc, &mut self.editor, "### "),
            Command::H4 => editor::apply_prefix(&mut self.doc, &mut self.editor, "#### "),
            Command::H5 => editor::apply_prefix(&mut self.doc, &mut self.editor, "##### "),
            Command::H6 => editor::apply_prefix(&mut self.doc, &mut self.editor, "###### "),
            Command::Bold => editor::wrap(&mut self.doc, &mut self.editor, "**", "**"),
            Command::Italic => editor::wrap(&mut self.doc, &mut self.editor, "*", "*"),
            Command::Strike => editor::wrap(&mut self.doc, &mut self.editor, "~~", "~~"),
            Command::Code => editor::wrap(&mut self.doc, &mut self.editor, "`", "`"),
            Command::Link => editor::wrap(&mut self.doc, &mut self.editor, "[", "](https://)"),
            Command::Image => {
                editor::insert_template(&mut self.doc, &mut self.editor, "![](./image.png)")
            }
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
            Command::Math => {
                editor::insert_template(&mut self.doc, &mut self.editor, "$$\nE = mc^2\n$$")
            }
            Command::Mermaid => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "```mermaid\ngraph TD\n  A[开始] --> B[结束]\n```",
            ),
            Command::Toc => editor::insert_template(&mut self.doc, &mut self.editor, "[TOC]"),
            Command::Footnote => {
                editor::insert_template(&mut self.doc, &mut self.editor, "[^1]: 脚注内容")
            }
            Command::FrontMatter => {
                editor::insert_template(&mut self.doc, &mut self.editor, "---\ntitle: 未命名\n---")
            }
            Command::Snapshot => {
                self.snapshots.capture(&self.doc);
                self.snapshots.open = true;
            }
            Command::Search => self.search.open = true,
            Command::Graph => {
                self.spawn_link_graph(ctx);
                self.graph.open = true;
            }
            Command::Stats => self.stats.open = true,
            Command::Shortcuts => self.shortcuts = true,
            Command::Settings => self.settings.open(),
            Command::About => self.about = true,
            Command::Quit => {
                // 不再静默保存：由 close_requested 的统一拦截决定要不要弹确认框。
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
            (
                i.modifiers.command,
                i.modifiers.shift,
                i.modifiers.alt,
                i.modifiers.ctrl,
            )
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
        // macOS 上 Ctrl 与 Cmd 是分开的两个键，用户习惯按 Ctrl+S，这里额外认一下。
        if mac && ctrl && !shift && key(egui::Key::S) {
            self.execute(Command::Save, ctx);
            return;
        }
        // 文档内查找 / 替换：Ctrl+F、Ctrl+H（macOS 上 Cmd+H 是系统「隐藏」，只能用 Ctrl+H）。
        if mac && ctrl && !shift && !alt && key(egui::Key::F) {
            self.find.open_for(false);
            return;
        }
        if mac && ctrl && !shift && !alt && key(egui::Key::H) {
            self.find.open_for(true);
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
            } else if key(egui::Key::F) {
                // macOS 上 Cmd+H 被系统占用来隐藏窗口，替换用 Cmd+Option+F。
                self.find.open_for(true);
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
            // 文档内查找（VS Code / Typora 习惯）；工作区全文搜索仍在「搜索」按钮里。
            self.find.open_for(false);
        } else if key(egui::Key::H) && !shift && !mac {
            self.find.open_for(true);
        } else if key(egui::Key::L) && shift {
            self.execute(Command::ToggleSidebar, ctx);
        } else if key(egui::Key::H) && shift {
            self.execute(Command::ToggleSidebarHeader, ctx);
        } else if key(egui::Key::Z) && shift {
            self.execute(Command::Redo, ctx);
        } else if key(egui::Key::Z) {
            self.execute(Command::Undo, ctx);
        } else if key(egui::Key::Comma) {
            self.execute(Command::Settings, ctx);
        } else if key(egui::Key::Num1) {
            self.execute(Command::H1, ctx);
        } else if key(egui::Key::Num2) {
            self.execute(Command::H2, ctx);
        } else if key(egui::Key::Num3) {
            self.execute(Command::H3, ctx);
        } else if key(egui::Key::Num4) {
            self.execute(Command::H4, ctx);
        } else if key(egui::Key::Num5) {
            self.execute(Command::H5, ctx);
        } else if key(egui::Key::Num6) {
            self.execute(Command::H6, ctx);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let drops: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        for d in drops {
            let path = d.path().to_path_buf();
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
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
            // 设置入口已移到「Markd」菜单与 Cmd/Ctrl+,，标题栏左侧不再放齿轮按钮。
            // 「首页」入口已移到编辑区上方的标签栏，标题栏这里不再重复放。
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
            // 左侧只放当前文档名（首页时显示「首页」）
            let title = if self.home {
                "首页".to_string()
            } else {
                self.doc.title()
            };
            ui.label(RichText::new(title).strong().color(self.theme.dim));

            // 右侧：命令面板 / 快速打开 / 搜索 / 保存 / 终端图标
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                // 搜索按钮已移到编辑区上方的工具条（统计右侧），这里只留保存 / 终端。
                if ui
                    .button("保存")
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    chosen = Some(Command::Save);
                }
                ui.add_space(2.0);
                if ui
                    .selectable_label(
                        self.terminal.open,
                        RichText::new(">_").monospace().size(13.0),
                    )
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .on_hover_text("终端 (Cmd/Ctrl+J)")
                    .clicked()
                {
                    self.terminal.toggle();
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
        // 按钮较多（13 个）：窗口变窄时换行，而不是把最右边的「搜索」挤到看不见。
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            // 功能区最左边：命令面板 / 快速打开
            if ui
                .button("命令面板")
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                self.palette.open_with(PaletteMode::Commands);
            }
            if ui
                .button("快速打开")
                .on_hover_cursor(egui::CursorIcon::Default)
                .clicked()
            {
                self.open_quick_palette();
            }
            ui.separator();
            // 标题级别收进一个下拉：先点选目标段落让它成为活动块，再选 H1–H6，
            // 整段会替换掉原有的 #/>/- 前缀变成对应标题（H1→H6 之间也能直接互转）。
            ui.menu_button("标题", |ui| {
                for (label, cmd) in [
                    // 「段落」把已选中的标题/引用/列表还原成普通段落。
                    ("段落 正文", Command::Paragraph),
                    ("H1 一级标题", Command::H1),
                    ("H2 二级标题", Command::H2),
                    ("H3 三级标题", Command::H3),
                    ("H4 四级标题", Command::H4),
                    ("H5 五级标题", Command::H5),
                    ("H6 六级标题", Command::H6),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            for (label, cmd) in [("B", Command::Bold), ("I", Command::Italic)] {
                if ui
                    .button(label)
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    chosen = Some(cmd);
                }
            }
            // 删除线：图标直接画「ABC 中间一条横线」，比写「删除线」三个字更直观。
            if ui
                .add(egui::Button::new(RichText::new("ABC").strikethrough()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("删除线 (Ctrl+Shift+`)")
                .clicked()
            {
                chosen = Some(Command::Strike);
            }
            // 清除格式（橡皮擦）：把光标所在块变回普通段落，并去掉行内加粗/删除线等标记。
            if icon_button(
                ui,
                "清除格式：当前块变回普通段落（像 WPS 橡皮擦）",
                painter_eraser_icon,
            ) {
                if editor::clear_format(&mut self.doc, &mut self.editor) {
                    self.on_changed();
                    self.status = "已清除格式".to_string();
                } else {
                    self.status = "这一块不是文本段落，没有可清除的格式".to_string();
                }
            }
            for (label, cmd) in [
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
                if ui
                    .button(label)
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    chosen = Some(cmd.clone());
                }
            }
            ui.separator();
            for (label, cmd) in [
                ("快照", Command::Snapshot),
                ("关系图", Command::Graph),
                ("统计", Command::Stats),
            ] {
                if ui
                    .button(label)
                    .on_hover_cursor(egui::CursorIcon::Default)
                    .clicked()
                {
                    chosen = Some(cmd.clone());
                }
            }
            // 放大镜 = 文档内查找/替换（和 Cmd/Ctrl+F、Ctrl+H 同一个面板）。
            // 工作区全文搜索仍在「视图」菜单与命令面板里（Command::Search）。
            if icon_button(ui, "查找/替换 (Cmd/Ctrl+F、Ctrl+H)", painter_search_icon) {
                self.find.open_for(false);
            }
        });
        if let Some(c) = chosen {
            self.execute(c, ctx);
        }
    }

    /// 编辑区顶部的标签栏：首页页签 + 每个打开的文件一个页签。
    /// 点页签切换，页签末尾的 × 关闭；它声明在 left 面板之后，所以只占编辑区宽度。
    fn tabbar(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().interaction.selectable_labels = false;
        ui.style_mut().spacing.item_spacing.x = 4.0;
        let mut switch: Option<usize> = None;
        let mut close: Option<usize> = None;
        ui.horizontal(|ui| {
            // 左侧留 10pt：侧栏右缘的拖动条占着靠里 8pt，避免抢它的点击。
            ui.add_space(10.0);
            let home_txt = if self.home {
                RichText::new("首页").strong().color(self.theme.accent)
            } else {
                RichText::new("首页").color(self.theme.dim)
            };
            if ui
                .selectable_label(self.home, home_txt)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("欢迎页与最近打开")
                .clicked()
            {
                self.home = true;
            }
            // 加号紧跟在「首页」右侧：新建一个 Markdown 文档标签（首次保存时再选路径）。
            if ui
                .button("+")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("新建 Markdown 文档 (Cmd/Ctrl+N)")
                .clicked()
            {
                self.new_document();
            }
            ui.separator();
            for i in 0..self.tabs.len() {
                let active = !self.home && i == self.active;
                let mut text = self.tab_label(i);
                if self.tab_dirty(i) {
                    text.push_str(" •");
                }
                let color = if active {
                    self.theme.text
                } else {
                    self.theme.dim
                };
                let tip = self.tabs[i]
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "未保存的文档".to_string());
                let resp = ui
                    .selectable_label(active, RichText::new(text).color(color))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(tip);
                if resp.clicked() {
                    switch = Some(i);
                }
                if resp.middle_clicked() {
                    close = Some(i);
                }
                let x = ui
                    .add(egui::Button::new(RichText::new("×").color(self.theme.dim)).frame(false));
                if x.on_hover_text("关闭标签").clicked() {
                    close = Some(i);
                }
            }
        });
        // 一帧内关闭优先：关掉标签后下标会变，不再处理切换。
        if let Some(i) = close {
            self.close_tab(i);
        } else if let Some(i) = switch {
            self.switch_tab(i);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().interaction.selectable_labels = false;
        if self.config.show_sidebar_header {
            ui.horizontal(|ui| {
                // 只显示当前页签名（文件 / 大纲），切换入口在底部状态栏。
                let name = match self.side_tab {
                    SideTab::Files => "文件",
                    SideTab::Outline => "大纲",
                };
                ui.label(RichText::new(name).strong().color(self.theme.text));
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
                    sidebar::file_tree(
                        ui,
                        &mut self.tree,
                        self.doc.path.as_deref(),
                        &self.theme,
                        &mut actions,
                    );
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
                    self.editor.scroll_top = true;
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
        // 只有快速打开面板真的打开时才扫盘。之前每帧都算一次，
        // 工作区在桌面上时要遍历整个目录，CPU 直接被这一个调用吃满。
        let files = if self.palette.open {
            self.quick_open_files()
        } else {
            Vec::new()
        };
        if let Some(cmd) = self.palette.show(ctx, &files) {
            self.execute(cmd, ctx);
        }
        if let Some((path, line)) = self.search.show(ctx, self.root.as_deref()) {
            self.open_file(path);
            self.editor.active = line.min(self.doc.blocks.len().saturating_sub(1));
        }
        if let Some(name) = self.graph.show(ctx, &self.link_docs, &self.theme) {
            if let Some(p) = self.files().into_iter().find(|p| {
                p.file_stem()
                    .map(|s| s.to_string_lossy() == name)
                    .unwrap_or(false)
            }) {
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

        let (settings_changed, settings_reset) =
            self.settings.show(ctx, &self.theme, &mut self.config);
        if settings_reset {
            self.config = Config::default();
            self.apply_config(ctx);
            self.config.save();
        } else if settings_changed {
            self.apply_config(ctx);
            self.config.save();
        }

        if self.shortcuts {
            egui::Window::new("快捷键一览")
                .open(&mut self.shortcuts)
                .collapsible(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    let rows = [
                        (
                            "Cmd/Ctrl + S（macOS 也认 Ctrl+S）",
                            "保存：弹窗选择保存路径",
                        ),
                        ("Cmd/Ctrl + Shift + S", "另存为：同样弹窗选择路径"),
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

        // 文档内查找/替换：命中的块滚到视野里，并把命中的文字选中（像 Typora 那样）。
        let fr = self.find.show(ctx, &mut self.doc, &self.theme);
        if let Some(h) = fr.jump {
            self.editor.active = h.block;
            self.editor.scroll_req = Some(h.block);
            self.editor.scroll_top = false;
            if let Some((a, b)) = self.find.hit_chars(&self.doc, h) {
                self.editor.select_req = Some((h.block, a, b));
            }
        }
        if fr.changed {
            self.on_changed();
        }

        if self.exit_dialog {
            self.exit_confirm(ctx);
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

    /// 定时自动保存：已落盘的文档按间隔直接写回，不需要再按 Ctrl/Cmd+S。
    /// 未命名文档（没有路径）保持脏状态，等用户按保存时再选路径。
    fn auto_save(&mut self) {
        let n = now_secs();
        if !autosave_due(n, self.last_autosave, self.config.auto_save_interval) {
            return;
        }
        self.last_autosave = n;
        let enabled = self.config.auto_save;
        let mut saved = false;
        if should_autosave(enabled, self.doc.dirty, self.doc.path.is_some())
            && self.doc.save().is_ok()
        {
            saved = true;
        }
        // 非活动标签的脏状态存在 state 里，同样按间隔写回。
        for t in self.tabs.iter_mut() {
            if let Some(st) = t.state.as_mut()
                && should_autosave(enabled, st.doc.dirty, st.doc.path.is_some())
                && st.doc.save().is_ok()
            {
                saved = true;
            }
        }
        if saved {
            self.status = "已自动保存".to_string();
        }
    }

    /// 有未保存修改的标签下标。
    fn unsaved_tabs(&self) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&i| self.tab_dirty(i))
            .collect()
    }

    /// 未保存文档的显示名：已命名的用文件名，未命名的用「未命名-N.md」。
    fn unsaved_label(&self, i: usize) -> String {
        match self.tabs.get(i) {
            Some(t) => match &t.path {
                Some(p) => p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string()),
                None => format!("未命名-{}.md", t.seq.max(1)),
            },
            None => String::new(),
        }
    }

    /// 退出前把有修改的标签全部写盘；未命名文档先问保存路径。
    /// 返回 false 表示用户取消了路径选择或写入失败，应当留在应用里。
    fn save_all_for_exit(&mut self) -> bool {
        if self.tabs.is_empty() {
            return true;
        }
        // 活动标签的状态先换回 tabs，随后统一遍历。
        self.stash_active();
        let active = self.active;
        // 第一遍只问路径：rfd 需要 &self，不能和 &mut tabs 同时借。
        let mut paths: Vec<Option<PathBuf>> = self.tabs.iter().map(|t| t.path.clone()).collect();
        let mut cancelled = false;
        for (i, slot) in paths.iter_mut().enumerate() {
            let dirty = self.tabs[i].state.as_ref().is_some_and(|s| s.doc.dirty);
            if !dirty || slot.is_some() {
                continue;
            }
            match self.pick_save_path(i) {
                Some(p) => *slot = Some(p),
                None => {
                    cancelled = true;
                    break;
                }
            }
        }
        // 第二遍统一写盘。写出哪几个文件由 write_dirty_tabs 返回，用于更新最近打开。
        let written = match cancelled {
            true => {
                self.status = "已取消保存".to_string();
                None
            }
            false => match write_dirty_tabs(&mut self.tabs, &paths) {
                Ok(list) => Some(list),
                Err(e) => {
                    self.status = format!("保存失败: {}", e);
                    None
                }
            },
        };
        if let Some(list) = &written {
            for p in list {
                self.config.last_file = Some(p.to_string_lossy().to_string());
                self.push_recent(p);
            }
        }
        if active < self.tabs.len() {
            self.load_tab(active);
        }
        written.is_some()
    }

    /// 给未命名标签挑保存路径（退出确认用）。
    fn pick_save_path(&self, i: usize) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_title("保存未保存的文档")
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(format!("{}.md", self.tab_label(i)));
        if let Some(d) = self.root.clone().filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(d);
        }
        dialog.save_file().map(with_md_ext)
    }

    /// 关闭窗口前的「有未保存文件」确认弹窗。
    fn exit_confirm(&mut self, ctx: &egui::Context) {
        let names: Vec<String> = self
            .unsaved_tabs()
            .iter()
            .map(|&i| self.unsaved_label(i))
            .collect();
        let mut open = true;
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        egui::Window::new("有未保存的修改")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("以下文件还没有保存到磁盘：");
                for n in &names {
                    ui.label(format!("· {}", n));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("保存并退出").clicked() {
                        save = true;
                    }
                    if ui.button("不保存").clicked() {
                        discard = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            if self.save_all_for_exit() {
                self.exit_dialog = false;
                self.force_quit = true;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                // 取消了路径选择或写入失败：收起弹窗，留在应用里。
                self.exit_dialog = false;
            }
        } else if discard {
            self.exit_dialog = false;
            self.force_quit = true;
            self.config.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if cancel || !open {
            self.exit_dialog = false;
        }
    }
}

impl eframe::App for MarkdApp {
    /// 清屏色跟随主题：eframe 默认是近黑的 (12,12,12)，任何透传的区域都会显得发黑。
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }

    fn ui(&mut self, root_ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root_ui.ctx().clone();
        // 关闭窗口（红绿灯、Cmd+Q、菜单退出）前先拦一道：有未保存的修改就弹确认框，
        // 而不是像以前那样直接写盘或悄悄丢掉。「不保存」会把 force_quit 置位，下一帧放行。
        if needs_exit_confirm(
            self.force_quit,
            ctx.input(|i| i.viewport().close_requested()),
            self.unsaved_tabs().len(),
        ) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.exit_dialog = true;
        }
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
        // 后台算完的链接图回填（启动时不再同步扫盘）。
        if let Some(rx) = self.link_rx.take() {
            match rx.try_recv() {
                Ok(m) => self.link_docs = m,
                Err(std::sync::mpsc::TryRecvError::Empty) => self.link_rx = Some(rx),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        // 设置里的编辑器选项每帧同步到编辑器状态
        self.editor.tab_size = self.config.tab_size.max(1);
        self.editor.auto_pair = self.config.auto_pair;
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
                // 底色显式用 theme.bg：Frame::NONE 是透明的，会露出窗口清屏色（近黑），
                // 暗色主题下侧栏/大纲就会比编辑区黑一截。
                .frame(
                    egui::Frame::NONE
                        .fill(self.theme.bg)
                        .inner_margin(egui::Margin {
                            left: 8,
                            right: 0,
                            top: 6,
                            bottom: 4,
                        }),
                )
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

        // 标签栏：紧贴编辑区上方，一直显示（首页页签与加号都在这条里）。
        // 声明在 left 之后，横向只占侧栏右侧的区域。
        {
            egui::Panel::top("tabbar")
                .frame(
                    egui::Frame::NONE
                        .fill(self.theme.panel)
                        .inner_margin(egui::Margin {
                            left: 0,
                            right: 6,
                            top: 3,
                            bottom: 3,
                        }),
                )
                .show(&mut *root_ui, |ui| self.tabbar(ui));
        }

        // 终端持有焦点时编辑区进入只读，避免它每帧把焦点抢回去。
        self.editor.read_only = self.terminal.has_focus();
        let show_home = self.home;
        let (event, actions, home_act) = egui::CentralPanel::default()
            .show(&mut *root_ui, |ui| {
                if show_home {
                    let actions: Vec<Action> = Vec::new();
                    let act = self.home_view(ui, &ctx);
                    return (EditorEvent::None, actions, act);
                }
                let mut actions = Vec::new();
                let base: Option<PathBuf> = self
                    .doc
                    .path
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf());
                let tab_key = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
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
                    tab_key,
                    &mut actions,
                );
                (ev, actions, None)
            })
            .inner;

        if let Some(a) = home_act {
            match a {
                HomeAction::New => self.new_document(),
                HomeAction::OpenFile => self.execute(Command::OpenFileDialog, &ctx),
                HomeAction::OpenWorkspace => self.execute(Command::OpenWorkspace, &ctx),
                HomeAction::Recent(p) => self.open_file(p),
            }
        }

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
                    self.editor.scroll_top = true;
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
                            let fence = if source.starts_with("~~~") {
                                "~~~"
                            } else {
                                "```"
                            };
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
                            if lang.is_empty() {
                                "纯文本"
                            } else {
                                lang.as_str()
                            }
                        );
                    }
                }
            }
        }

        if std::env::var("MARKD_TRACE_FOCUS").is_ok() {
            eprintln!(
                "[state] home={} tabs={} active={} blocks={} focused={:?}",
                self.home,
                self.tabs.len(),
                self.active,
                self.doc.blocks.len(),
                ctx.memory(|m| m.focused())
            );
        }
        self.windows(&ctx);
        self.auto_save();
        // TEMP 调试：模拟「在首页点新建文档」——CentralPanel 之后再建，下一帧才渲染编辑器
        if std::env::var("MARKD_AUTO_NEW").as_deref() == Ok("2") {
            self.frame_count += 1;
            if self.frame_count == 2 {
                self.new_document();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自动保存判定：开关 / 有修改 / 已落盘 三个条件缺一不可。
    #[test]
    fn autosave_requires_enabled_dirty_and_path() {
        assert!(should_autosave(true, true, true));
        assert!(!should_autosave(false, true, true));
        assert!(!should_autosave(true, false, true), "没改过不该写盘");
        assert!(
            !should_autosave(true, true, false),
            "未命名文档没有路径，不该自动写盘"
        );
    }

    /// 间隔判定：未到点不存、到点存；间隔下限 5 秒；时钟回拨不触发。
    #[test]
    fn autosave_interval_gate() {
        assert!(!autosave_due(19, 0, 20));
        assert!(autosave_due(20, 0, 20));
        assert!(autosave_due(1000, 20, 20));
        // 配置被改成 0 也至少等 5 秒，否则等于每帧写盘
        assert!(!autosave_due(4, 0, 0));
        assert!(autosave_due(5, 0, 0));
        assert!(!autosave_due(5, 100, 20), "时钟回拨时不该误触发");
    }

    /// 退出拦截：只有「有未保存修改 + 真的在关闭 + 未选择不保存」才弹确认框。
    #[test]
    fn exit_confirm_gate() {
        assert!(needs_exit_confirm(false, true, 1));
        assert!(
            !needs_exit_confirm(true, true, 1),
            "已选过「不保存」应直接放行"
        );
        assert!(!needs_exit_confirm(false, false, 1), "没点关闭不该弹窗");
        assert!(!needs_exit_confirm(false, true, 0), "没有未保存修改不弹窗");
    }

    /// 造一个标签：原内容 src，改成 edited 后处于脏状态；path 为 None 表示未命名。
    fn dirty_tab(src: &str, edited: &str, path: Option<PathBuf>) -> Tab {
        let mut doc = Document::new(src, path.clone());
        doc.edit_block(0, edited);
        assert!(doc.dirty);
        Tab {
            uid: 0,
            path,
            seq: 1,
            state: Some(TabState {
                doc,
                editor: EditorState::default(),
                outline: Vec::new(),
                stats: Default::default(),
            }),
        }
    }

    /// 造一个干净的（没改过）标签。
    fn clean_tab(src: &str, path: PathBuf) -> Tab {
        Tab {
            uid: 0,
            path: Some(path.clone()),
            seq: 1,
            state: Some(TabState {
                doc: Document::new(src, Some(path)),
                editor: EditorState::default(),
                outline: Vec::new(),
                stats: Default::default(),
            }),
        }
    }

    /// 退出批量保存：已命名的脏标签原地写回，未命名的写到刚选的路径；
    /// 没有修改的标签不碰磁盘；写完后脏标记清掉。
    #[test]
    fn write_dirty_tabs_writes_only_dirty_ones() {
        let dir = std::env::temp_dir().join(format!("markd-exit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        let c = dir.join("untouched.md");
        let mut tabs = vec![
            dirty_tab("旧 A", "新 A", Some(a.clone())),
            dirty_tab("旧 B", "新 B", None),
            clean_tab("干净", c.clone()),
        ];
        let paths = vec![Some(a.clone()), Some(b.clone()), Some(c.clone())];
        let written = write_dirty_tabs(&mut tabs, &paths).unwrap();
        assert_eq!(written, vec![a.clone(), b.clone()]);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "新 A");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "新 B");
        assert!(!c.exists(), "没改过的标签不该被写盘");
        assert!(!tabs[0].state.as_ref().unwrap().doc.dirty);
        assert!(!tabs[1].state.as_ref().unwrap().doc.dirty);
        assert_eq!(tabs[1].path, Some(b), "未命名标签保存后应记住路径");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没拿到路径（用户取消）的标签保持脏状态，下一次还得提示保存。
    #[test]
    fn write_dirty_tabs_skips_tabs_without_path() {
        let mut tabs = vec![dirty_tab("旧 B", "新 B", None)];
        let written = write_dirty_tabs(&mut tabs, &[None]).unwrap();
        assert!(written.is_empty());
        assert!(
            tabs[0].state.as_ref().unwrap().doc.dirty,
            "没有路径时不该清掉脏标记"
        );
    }

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
