//! Author: aurelianus <aurelianus@126.com>

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

/// 拖动缩放后重建「纯图片段落」的源码：第 `no` 张图片写入 `{width=N}`，
/// 其余图片保留原有属性。不是纯图片段落时返回 None。
fn resize_image_source(doc: &Document, block: usize, no: usize, width: u32) -> Option<String> {
    let crate::ast::Block::Paragraph { content, .. } = doc.blocks.get(block)? else {
        return None;
    };
    let imgs: Vec<(&String, &String, Option<u32>)> = content
        .iter()
        .filter_map(|i| match i {
            crate::ast::Inline::Image { alt, url, width } => Some((alt, url, *width)),
            _ => None,
        })
        .collect();
    // 只重建「纯图片段落」：混着正文的段落重建会把文字弄丢。
    let only_images = !imgs.is_empty()
        && content.iter().all(|i| {
            matches!(
                i,
                crate::ast::Inline::Image { .. } | crate::ast::Inline::Break
            )
        });
    if !only_images || no >= imgs.len() {
        return None;
    }
    Some(
        imgs.iter()
            .enumerate()
            .map(|(k, (alt, url, w))| {
                let w = if k == no { Some(width) } else { *w };
                match w {
                    Some(w) => format!("![{alt}]({url}){{width={w}}}"),
                    None => format!("![{alt}]({url})"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 关闭请求是否需要先弹「有未保存的修改」确认框：
/// 用户已经选过「不保存」（force_quit）时直接放行。
fn needs_exit_confirm(force_quit: bool, close_requested: bool, unsaved: usize) -> bool {
    !force_quit && close_requested && unsaved > 0
}

/// 工具条上的自绘图标按钮（放大镜等）：不依赖 emoji 字体，
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

/// 撤销 / 恢复图标：一段圆弧 + 箭头（WPS 那种弯曲的回退箭头）。
/// right = true 时左右镜像，用于「重做」。
fn painter_undo_redo(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
    right: bool,
) {
    painter.rect_filled(rect, egui::CornerRadius::same(4), bg);
    let stroke = egui::Stroke::new(1.5, fg);
    let h = rect.height();
    let c = rect.center() + egui::vec2(0.0, h * 0.08);
    let r = h * 0.21;
    // 圆弧从左下绕到右下（屏幕上就是一条向上的半圆）
    let a0 = std::f32::consts::PI * 0.92;
    let a1 = std::f32::consts::PI * 2.18;
    let mirror = |p: egui::Pos2| {
        if right {
            egui::pos2(2.0 * c.x - p.x, p.y)
        } else {
            p
        }
    };
    let mut pts: Vec<egui::Pos2> = Vec::new();
    let steps = 18;
    for i in 0..=steps {
        let t = a0 + (a1 - a0) * (i as f32 / steps as f32);
        pts.push(mirror(egui::pos2(c.x + r * t.cos(), c.y + r * t.sin())));
    }
    // 箭头画在 a0 那一端，指向圆弧外侧
    let tip_dir = {
        let d = pts[0] - pts[1];
        d.normalized()
    };
    let tip = pts[0] + tip_dir * (r * 0.75);
    let perp = egui::vec2(-tip_dir.y, tip_dir.x) * (r * 0.55);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, pts[0] + perp, pts[0] - perp],
        fg,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::line(pts, stroke));
}

/// 撤销图标（向左回退）
fn painter_undo_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter_undo_redo(painter, rect, bg, fg, false);
}

/// 重做图标（向右回退）
fn painter_redo_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    painter_undo_redo(painter, rect, bg, fg, true);
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
    Paragraph,
    Format,
    View,
    Theme,
    Window,
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

/// Mermaid 下拉的图类型与标准模板。渲染器支持流程图/时序图/饼图；
/// 其余类型插入后暂以代码块形式保真显示，不会乱画。
static MERMAID_TEMPLATES: &[(&str, &str)] = &[
    (
        "流程图",
        "```mermaid\nflowchart TD\n  A([开始]) --> B[填写表单]\n  B --> C{校验通过?}\n  C -->|是| D[提交订单]\n  C -->|否| B\n  D --> E([结束])\n```",
    ),
    (
        "时序图",
        "```mermaid\nsequenceDiagram\n  title 下单流程\n  participant U as 用户\n  participant S as 服务端\n  participant D as 数据库\n  U->>S: 提交订单\n  S->>D: 写入订单\n  D-->>S: 写入成功\n  S-->>U: 返回结果\n  Note over S,D: 事务保证\n```",
    ),
    (
        "饼图",
        "```mermaid\npie title 时间分配\n  \"写代码\" : 45\n  \"开会\" : 20\n  \"看文档\" : 15\n  \"休息\" : 20\n```",
    ),
    (
        "类图",
        "```mermaid\nclassDiagram\n  class Animal {\n    +String name\n    +eat()\n  }\n  class Dog {\n    +bark()\n  }\n  Animal <|-- Dog\n  Animal : +Owner owner\n  Owner ..> Animal : 喂养\n```",
    ),
    (
        "状态图",
        "```mermaid\nstateDiagram-v2\n  [*] --> 空闲\n  空闲 --> 运行中: 开始任务\n  运行中 --> 暂停: 暂停\n  暂停 --> 运行中: 恢复\n  运行中 --> [*]: 完成\n```",
    ),
    (
        "ER 图",
        "```mermaid\nerDiagram\n  USER {\n    string name\n    string email\n  }\n  ORDER {\n    int amount\n    string status\n  }\n  USER ||--o{ ORDER : 下单\n```",
    ),
    (
        "甘特图",
        "```mermaid\ngantt\n  title 项目排期\n  section 设计\n  需求评审 :a1, 2026-01-01, 7d\n  原型设计 :a2, after a1, 10d\n  section 开发\n  编码 :a3, after a2, 21d\n  联调测试 :after a3, 14d\n```",
    ),
    (
        "旅程图",
        "```mermaid\njourney\n  title 一次开心的写作\n  section 准备\n  打开 Markd: 5: 我\n  建好大纲: 4: 我\n  section 写作\n  沉浸输出: 5: 我\n  一键导出: 4: 我, 读者\n```",
    ),
    (
        "Git 图",
        "```mermaid\ngitGraph\n  commit\n  commit\n  branch dev\n  checkout dev\n  commit\n  commit\n  checkout main\n  merge dev\n  commit\n```",
    ),
];

/// 菜单栏结构对齐 Typora：文件 / 编辑 / 段落 / 格式 / 视图 / 主题 / 窗口 / 帮助。
/// 段落 = 块级结构（标题、列表、表格、代码块、公式块…），格式 = 行内样式（加粗、斜体、链接…）。
fn menu_commands(k: MenuKind) -> Vec<Command> {
    match k {
        MenuKind::File => vec![
            Command::New,
            Command::QuickOpen,
            Command::CommandPalette,
            Command::OpenFileDialog,
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
            Command::Find,
            Command::ToggleAutoSave,
        ],
        MenuKind::Paragraph => vec![
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
            Command::CodeBlock,
            Command::Math,
            Command::InlineMath,
            Command::Mermaid,
            Command::Flowchart,
            Command::Toc,
            Command::Footnote,
            Command::FrontMatter,
        ],
        MenuKind::Format => vec![
            Command::Bold,
            Command::Italic,
            Command::Underline,
            Command::Strike,
            Command::Code,
            Command::Link,
            Command::Image,
            Command::ClearFormat,
        ],
        MenuKind::View => vec![
            Command::ToggleSource,
            Command::ToggleFocus,
            Command::ToggleTypewriter,
            Command::ToggleLineNumbers,
            Command::ToggleSidebar,
            Command::ToggleOutline,
            Command::ToggleSidebarHeader,
            Command::ToggleWorkspaceBar,
            Command::ToggleTerminal,
            Command::Search,
            Command::Graph,
            Command::Stats,
        ],
        MenuKind::Theme => vec![Command::ThemeNext],
        MenuKind::Window => vec![Command::Minimize, Command::ToggleMaximize],
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

/// 插入链接弹窗：先把选中文字和选区记下来，用户粘贴 URL 后再写回。
/// 这样点工具条/菜单导致编辑框失焦、选区被清掉时也不会丢掉选中的文字。
#[derive(Default)]
struct LinkDialog {
    open: bool,
    /// 链接地址输入框（支持直接 Cmd/Ctrl+V 粘贴）。
    url: String,
    /// 显示文字；选中文字时预填，未选中文字时留空表示用 URL 当文字。
    text: String,
    /// (块下标, 起始字符, 结束字符)：打开弹窗时记住的选区。
    target: Option<(usize, usize, usize)>,
    /// 打开后的第一帧把焦点交给地址输入框，直接粘贴就能用。
    focus_url: bool,
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
    /// 侧栏文件树图标纹理缓存（SVG 在首次显示时栅格化）。
    icons: crate::icons::IconCache,
    root: Option<PathBuf>,
    tree: Vec<FileNode>,
    expanded: HashSet<PathBuf>,
    outline: Vec<OutlineItem>,
    /// 缓存统计结果：Document::stats() 是 O(全文) 的，不能每帧都算。
    stats_cache: crate::document::Stats,
    stats_at: std::time::Instant,
    side_tab: SideTab,
    /// 侧栏大纲里被折叠的条目下标（按 outline 顺序）。
    outline_folded: HashSet<usize>,
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
    /// 轻提示弹窗：(文案, 弹出时间)。复制代码等操作的浮动反馈，淡出后自动清除。
    toast: Option<(String, f64)>,
    about: bool,
    shortcuts: bool,
    table_dialog: bool,
    table_rows: usize,
    table_cols: usize,
    /// 插入链接弹窗。
    link_dialog: LinkDialog,
    link_docs: HashMap<String, Vec<String>>,
    /// 后台线程算好的链接图，算完通过它回传。
    link_rx: Option<std::sync::mpsc::Receiver<HashMap<String, Vec<String>>>>,
    last_autosave: u64,
    /// 关闭窗口前的「有未保存文件」确认弹窗。
    exit_dialog: bool,
    /// 待确认删除的代码块下标：点「删除」按钮后弹窗，确定才真正删除。
    code_delete_confirm: Option<usize>,
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
    /// 文件树右键菜单：显示位置 + 目标路径。
    ctx_menu: Option<(PathBuf, egui::Pos2)>,
    /// 文件剪贴板：(路径, 是否剪切)。剪切在粘贴成功后才会删除源文件。
    file_clipboard: Option<(PathBuf, bool)>,
    /// 重命名对话框：(原路径, 名称输入缓冲)。
    rename_dialog: Option<(PathBuf, String)>,
    /// 删除确认对话框：待删除路径。
    delete_confirm: Option<PathBuf>,
    /// 新建对话框：(父目录, 是否文件夹, 名称输入缓冲)。
    new_dialog: Option<(PathBuf, bool, String)>,
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
        let app = Self {
            stats: WritingStats::new(&doc),
            stats_cache,
            stats_at: std::time::Instant::now(),
            doc,
            config,
            theme,
            editor: EditorState::default(),
            images: ImageCache::new(),
            icons: crate::icons::IconCache::default(),
            root,
            tree,
            expanded,
            outline,
            side_tab: SideTab::Files,
            outline_folded: HashSet::new(),
            menu: None,
            workspace_input,
            palette: Palette::default(),
            search: SearchPanel::default(),
            find: FindPanel::default(),
            graph: GraphPanel::default(),
            snapshots: SnapshotStore::load(),
            status: String::new(),
            toast: None,
            about: false,
            shortcuts: false,
            table_dialog: false,
            table_rows: 3,
            table_cols: 3,
            link_dialog: LinkDialog::default(),
            link_docs,
            link_rx: None,
            last_autosave: now_secs(),
            exit_dialog: false,
            code_delete_confirm: None,
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
            ctx_menu: None,
            file_clipboard: None,
            rename_dialog: None,
            delete_confirm: None,
            new_dialog: None,
        };
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

    /// 插入磁盘上的图片：按设置复制到文档旁的资源目录（默认 imgs/），
    /// 再在光标所在块之后插入 Markdown 图片链接。
    fn insert_image_path(&mut self, src: &Path) {
        let (copy, assets, relative) = (
            self.config.insert_image_to_assets,
            self.config.image_assets_dir.clone(),
            self.config.image_relative,
        );
        match crate::image_insert::from_file(src, self.doc.path.as_deref(), copy, &assets, relative)
        {
            Ok(md) => {
                editor::insert_image(&mut self.doc, &mut self.editor, &md);
                self.on_changed();
                self.status = format!("已插入图片：{}", src.display());
            }
            Err(e) => self.status = format!("插入图片失败：{e}"),
        }
    }

    /// 插入剪贴板图片：保存为 PNG 到文档旁的资源目录，再插入 Markdown 链接。
    fn insert_clipboard_image(&mut self, img: &egui::ColorImage) {
        let (assets, relative) = (
            self.config.image_assets_dir.clone(),
            self.config.image_relative,
        );
        match crate::image_insert::from_clipboard(
            img.size,
            &img.pixels,
            self.doc.path.as_deref(),
            &assets,
            relative,
        ) {
            Ok(md) => {
                editor::insert_image(&mut self.doc, &mut self.editor, &md);
                self.on_changed();
                self.status = "已插入剪贴板图片".to_string();
            }
            Err(e) => self.status = format!("插入剪贴板图片失败：{e}"),
        }
    }

    fn refresh_tree(&mut self) {
        if let Some(r) = &self.root {
            self.tree = workspace::scan_tree(r, &self.expanded);
        }
        // 链接图按需生成（打开关系图时），这里不再同步扫盘。
        self.link_docs.clear();
    }

    // ---------------- 文件树右键菜单的文件操作 ----------------

    /// 复制文件路径（绝对或相对工作区根）。
    fn copy_file_path(&mut self, p: &Path, relative: bool, ctx: &egui::Context) {
        let text = if relative {
            self.root
                .as_ref()
                .and_then(|r| p.strip_prefix(r).ok())
                .map(|s| s.display().to_string())
                .unwrap_or_else(|| p.display().to_string())
        } else {
            p.display().to_string()
        };
        ctx.copy_text(text.clone());
        self.notify(ctx, format!("已复制：{text}"));
    }

    /// 在系统文件管理器中定位文件（Finder / 资源管理器）。
    fn reveal_in_system(&self, p: &Path) {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", p.display()))
                .spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let dir = p.parent().unwrap_or(p);
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }

    /// 关闭所有指向该路径的标签（从后往前，避免下标漂移）。
    fn close_tabs_with_path(&mut self, p: &Path) {
        for i in (0..self.tabs.len()).rev() {
            if self.tabs[i].path.as_deref() == Some(p) {
                self.close_tab(i);
            }
        }
    }

    fn fs_delete(&mut self, p: &Path) {
        let result = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        match result {
            Ok(_) => {
                self.close_tabs_with_path(p);
                self.file_clipboard = None;
                self.refresh_tree();
                self.status = format!("已删除 {}", p.display());
            }
            Err(e) => self.status = format!("删除失败：{e}"),
        }
    }

    fn fs_rename(&mut self, p: &Path, new_name: &str) {
        let name = new_name.trim();
        if name.is_empty() {
            return;
        }
        let Some(parent) = p.parent() else {
            return;
        };
        let target = parent.join(name);
        if target.exists() {
            self.status = format!("已存在同名文件：{}", target.display());
            return;
        }
        if std::fs::rename(p, &target).is_err() {
            self.status = "重命名失败".to_string();
            return;
        }
        // 打开的标签与最近打开列表同步改路径
        for t in self.tabs.iter_mut() {
            if t.path.as_deref() == Some(p) {
                t.path = Some(target.clone());
            }
        }
        if self.doc.path.as_deref() == Some(p) {
            self.doc.path = Some(target.clone());
        }
        let old = p.display().to_string();
        let new = target.display().to_string();
        if let Some(r) = self.config.recent.iter_mut().find(|x| x.as_str() == old) {
            *r = new;
        }
        self.config.save();
        self.refresh_tree();
        self.status = format!("已重命名为 {}", name);
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            let to = dst.join(e.file_name());
            if e.file_type()?.is_dir() {
                Self::copy_dir_recursive(&e.path(), &to)?;
            } else {
                std::fs::copy(e.path(), to)?;
            }
        }
        Ok(())
    }

    /// 粘贴剪贴板里的文件/文件夹到目标目录；同名自动追加 (2) (3)…
    /// 剪切的文件若正被打开，标签路径同步改到新位置（内容不丢）。
    fn fs_paste(&mut self, dest_dir: &Path) {
        let Some((src, cut)) = self.file_clipboard.take() else {
            return;
        };
        let Some(name) = src.file_name().map(|n| n.to_os_string()) else {
            return;
        };
        let mut dest = dest_dir.join(name);
        if dest.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let ext = src
                .extension()
                .map(|s| format!(".{}", s.to_string_lossy()))
                .unwrap_or_default();
            let mut n = 2;
            loop {
                dest = dest_dir.join(format!("{stem} ({n}){ext}"));
                if !dest.exists() {
                    break;
                }
                n += 1;
            }
        }
        let result = if src.is_dir() {
            Self::copy_dir_recursive(&src, &dest)
        } else {
            std::fs::copy(&src, &dest).map(|_| ())
        };
        match result {
            Ok(_) => {
                if cut {
                    let _ = std::fs::remove_dir_all(&src).or_else(|_| std::fs::remove_file(&src));
                    for t in self.tabs.iter_mut() {
                        if t.path.as_deref() == Some(src.as_path()) {
                            t.path = Some(dest.clone());
                        }
                    }
                    if self.doc.path.as_deref() == Some(src.as_path()) {
                        self.doc.path = Some(dest.clone());
                    }
                }
                self.refresh_tree();
                self.status = format!("已粘贴到 {}", dest.display());
            }
            Err(e) => self.status = format!("粘贴失败：{e}"),
        }
    }

    /// 新建文件（并打开）/ 新建文件夹。
    fn fs_new(&mut self, dir: &Path, is_dir: bool, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let target = dir.join(name);
        let result = if is_dir {
            std::fs::create_dir_all(&target)
        } else {
            std::fs::File::create(&target).map(|_| ())
        };
        match result {
            Ok(_) => {
                self.refresh_tree();
                self.status = format!("已创建 {}", target.display());
                if !is_dir {
                    self.open_file(target);
                }
            }
            Err(e) => self.status = format!("创建失败：{e}"),
        }
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
                let mut editor = EditorState {
                    focus_req: Some(0),
                    ..Default::default()
                };
                // 还原上次保存的折叠状态。
                editor.folded = editor::load_folds(&path, &doc);
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
                // 折叠状态随文档路径一起落盘（覆盖「首次另存为」后才确定路径的情况）。
                editor::save_folds(&path, &self.doc, &self.editor.folded);
                self.status = "已保存".to_string();
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
            Ok(_) => self.status = "已导出 HTML".to_string(),
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

    /// 打开「插入链接」弹窗：先把当前选区取出来存好，确认时再写回 Markdown。
    fn open_link_dialog(&mut self) {
        let sel = editor::selection_text(&self.doc, &self.editor);
        let (text, target) = match sel {
            Some((idx, a, z, text)) => (text, Some((idx, a, z))),
            None => (String::new(), None),
        };
        self.link_dialog = LinkDialog {
            open: true,
            url: String::new(),
            text,
            target,
            focus_url: true,
        };
    }

    /// 链接弹窗点「插入」：按记住的选区把 [文字](URL) 写回文档。
    fn confirm_link_dialog(&mut self) {
        let url = self.link_dialog.url.trim().to_string();
        if url.is_empty() {
            self.status = "请先粘贴或输入链接地址".to_string();
            self.link_dialog.focus_url = true;
            return;
        }
        let label = self.link_dialog.text.clone();
        let target = self.link_dialog.target;
        if editor::insert_link(&mut self.doc, &mut self.editor, target, &label, &url) {
            self.on_changed();
            self.status = format!("已插入链接 {}", url);
            self.link_dialog.open = false;
        }
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
            Command::QuickOpen => self.open_quick_palette(),
            Command::CommandPalette => self.palette.open_with(PaletteMode::Commands),
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
            Command::ToggleLineNumbers => {
                self.config.show_line_numbers = !self.config.show_line_numbers;
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
            // 加粗用标准 Markdown 的 **：即时渲染态由 inline_job 隐藏标记、显示粗体，
            // 源码态则原样显示 **加粗的部分**（WYSIWYG 与源码两态一致）。
            Command::Bold => editor::wrap(&mut self.doc, &mut self.editor, "**", "**", true),
            Command::Italic => editor::wrap(&mut self.doc, &mut self.editor, "<em>", "</em>", true),
            Command::Strike => editor::wrap(&mut self.doc, &mut self.editor, "~~", "~~", true),
            Command::Underline => {
                editor::wrap(&mut self.doc, &mut self.editor, "<u>", "</u>", true)
            }
            Command::Code => editor::wrap(&mut self.doc, &mut self.editor, "`", "`", true),
            Command::Link => self.open_link_dialog(),
            Command::Image => {
                // 从磁盘选图片：按设置复制到文档旁的资源目录（默认 imgs/），
                // 文档里写相对路径；未保存的文档回退到当前工作目录。
                let base = crate::image_insert::doc_dir(self.doc.path.as_deref());
                let mut dialog = rfd::FileDialog::new()
                    .set_title("插入图片")
                    .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]);
                if base.is_dir() {
                    dialog = dialog.set_directory(&base);
                }
                if let Some(p) = dialog.pick_file() {
                    self.insert_image_path(&p);
                }
            }
            Command::ClearFormat => {
                // 与工具条橡皮擦同一动作：当前块去掉块级/行内标记变回普通段落。
                if editor::clear_format(&mut self.doc, &mut self.editor) {
                    self.on_changed();
                    self.status = "已清除格式".to_string();
                } else {
                    self.status = "这一块不是文本段落，没有可清除的格式".to_string();
                }
            }
            Command::Quote => editor::apply_prefix(&mut self.doc, &mut self.editor, "> "),
            Command::CodeBlock => editor::insert_code_block(&mut self.doc, &mut self.editor),
            Command::Bullet => editor::apply_prefix(&mut self.doc, &mut self.editor, "- "),
            Command::Ordered => editor::apply_prefix(&mut self.doc, &mut self.editor, "1. "),
            // 任务列表：把当前块/选中文字转成任务项（保留正文、不新增块），
            // 与标题/列表按钮一致走 apply_prefix；这样回车继承逻辑才能衔接。
            Command::Task => editor::apply_prefix(&mut self.doc, &mut self.editor, "- [ ] "),
            Command::Rule => editor::insert_template(&mut self.doc, &mut self.editor, "---"),
            // 和 Typora 一样：先问几行几列，再插入
            Command::Table => self.table_dialog = true,
            Command::Math => {
                editor::insert_template(&mut self.doc, &mut self.editor, "$$\nE = mc^2\n$$")
            }
            Command::InlineMath => {
                editor::insert_template(&mut self.doc, &mut self.editor, "$E = mc^2$")
            }
            Command::Flowchart => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "\u{60}\u{60}\u{60}mermaid\nflowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]\n\u{60}\u{60}\u{60}",
            ),
            Command::Mermaid => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "```mermaid\npie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31\n  \"分类三\" : 27\n```",
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
            Command::Find => self.find.open_for(false),
            Command::Graph => {
                self.spawn_link_graph(ctx);
                self.graph.open = true;
            }
            Command::Stats => self.stats.open = true,
            Command::Shortcuts => self.shortcuts = true,
            Command::Settings => self.settings.open(),
            Command::About => self.about = true,
            Command::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            Command::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
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
        #[cfg(test)]
        eprintln!(
            "[shortcuts] cmd={cmd} ctrl={ctrl} alt={alt} shift={shift} C_pressed={} terminal_focus={}",
            key(egui::Key::C),
            self.terminal.has_focus()
        );

        // ---- 剪贴板图片 ----
        // Cmd/Ctrl+V 时若剪贴板里只有图片（截图、从画图/Finder 复制的图片），
        // egui-winit 发的是 `Event::PasteImage` 而不是 `Paste(文本)`，
        // 这里把它落盘到文档旁的资源目录并插入 Markdown 图片链接。
        if !self.terminal.has_focus() {
            let pasted = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::PasteImage(img) => Some(img.clone()),
                    _ => None,
                })
            });
            if let Some(img) = pasted {
                self.insert_clipboard_image(&img);
                return;
            }
        }

        // ---- 跨块选区的复制 / 剪切 ----
        // egui-winit 会把 Cmd/Ctrl+C、Cmd/Ctrl+X 直接转成虚拟事件
        // `Event::Copy` / `Event::Cut`，不再产生 Key 事件——只按 Key 判断
        // 永远收不到（这正是之前拖选后复制/剪切没反应的原因）。
        // 终端有焦点时让给终端（它有自己的一套复制粘贴）。
        if !self.terminal.has_focus() {
            let (copy, cut) = ctx.input(|i| {
                (
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                )
            });
            if copy && self.copy_cross_sel(ctx) {
                return;
            }
            if cut && self.cut_cross_sel(ctx) {
                return;
            }
        }

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
        // Ctrl+C / Ctrl+X 也认（与 Ctrl+S 同理）。终端有焦点时不抢：
        // Ctrl+C 在终端里是「中断正在运行的程序」，抢走会打断命令。
        if mac && ctrl && !shift && !alt && !self.terminal.has_focus() {
            if key(egui::Key::C) && self.copy_cross_sel(ctx) {
                return;
            }
            if key(egui::Key::X) && self.cut_cross_sel(ctx) {
                return;
            }
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
        if key(egui::Key::C) && cmd {
            // 跨块选区（渲染态拖选）的复制：无编辑态焦点，不会与 TextEdit 冲突。
            // 没有跨块选区时不拦截，单块选区交给 TextEdit 原生复制。
            if self.copy_cross_sel(ctx) {
                return;
            }
        } else if key(egui::Key::X) && cmd {
            // 剪切跨块选区：复制到剪贴板后删除选中内容。
            if self.cut_cross_sel(ctx) {
                return;
            }
        } else if key(egui::Key::S) && shift {
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
        } else if key(egui::Key::C) {
            // 跨块选区（渲染态拖选）的复制：无编辑态焦点，不会与 TextEdit 冲突。
            // 没有跨块选区时不拦截，单块选区交给 TextEdit 原生复制。
            if let Some(text) = editor::cross_sel_text(&self.doc, self.editor.cross_sel) {
                ctx.copy_text(text);
                self.notify(ctx, "已复制".to_string());
                return;
            }
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
                    ("段落", MenuKind::Paragraph),
                    ("格式", MenuKind::Format),
                    ("视图", MenuKind::View),
                    ("主题", MenuKind::Theme),
                    ("窗口", MenuKind::Window),
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
        let mut chosen_tpl: Option<&'static str> = None;
        // 按钮较多（13 个）：窗口变窄时换行，而不是把最右边的「搜索」挤到看不见。
        ui.horizontal_wrapped(|ui| {
            ui.add_space(6.0);
            // 「命令面板 / 快速打开」按钮已移除：命令面板的全部功能收进菜单栏「文件」，
            // 打开面板仍走 Cmd/Ctrl+P（命令面板）与 Cmd/Ctrl+Shift+O（快速打开）。
            // 撤销 / 重做放在「标题」左边：改错了能立刻回退一步。
            if icon_button(ui, "撤销 (Cmd/Ctrl+Z)", painter_undo_icon) {
                chosen = Some(Command::Undo);
            }
            if icon_button(ui, "重做 (Cmd/Ctrl+Shift+Z)", painter_redo_icon) {
                chosen = Some(Command::Redo);
            }
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
            if ui
                .add(egui::Button::new(RichText::new("B").strong()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("加粗：作用于选中文字；无选区时作用于整行")
                .clicked()
            {
                chosen = Some(Command::Bold);
            }
            if ui
                .add(egui::Button::new(RichText::new("U").underline()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("下划线")
                .clicked()
            {
                chosen = Some(Command::Underline);
            }
            if ui
                .add(egui::Button::new(RichText::new("I").italics()))
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("斜体 (Cmd/Ctrl+I)")
                .clicked()
            {
                chosen = Some(Command::Italic);
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
            // 三种列表收进一个下拉，节省工具条空间。
            ui.menu_button("列表", |ui| {
                for (label, cmd) in [
                    ("无序列表", Command::Bullet),
                    ("有序列表", Command::Ordered),
                    ("任务列表", Command::Task),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            // 引用从「插入」下拉里提出来，高频使用，放在「插入」左侧。
            if ui
                .button("引用")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("引用块 (Cmd/Ctrl+Option+Q)")
                .clicked()
            {
                chosen = Some(Command::Quote);
            }
            // 图片从「插入」下拉里提出来：高频使用，放在「插入」左侧。
            // 点它从磁盘选图；Cmd/Ctrl+V 则直接插入剪贴板里的图片。
            if ui
                .button("图片")
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text("插入图片（也可 Cmd/Ctrl+V 粘贴剪贴板图片）")
                .clicked()
            {
                chosen = Some(Command::Image);
            }
            // 插入类动作收进一个下拉：表格、代码、链接、分隔线、公式、图表…
            // 全塞在工具条上会挤成一排小按钮，下拉更好找。
            ui.menu_button("插入", |ui| {
                for (label, cmd) in [
                    ("表格", Command::Table),
                    ("代码块", Command::CodeBlock),
                    ("行内代码", Command::Code),
                    ("超链接", Command::Link),
                    ("分隔线", Command::Rule),
                    ("行内公式", Command::InlineMath),
                    ("公式块", Command::Math),
                    ("图表", Command::Mermaid),
                    ("流程图", Command::Flowchart),
                    ("脚注", Command::Footnote),
                    ("目录 [TOC]", Command::Toc),
                ] {
                    if ui.button(label).clicked() {
                        chosen = Some(cmd);
                        ui.close();
                    }
                }
            });
            // Mermaid 下拉：插入各类图表的标准模板，与「插入」按钮并排。
            // 渲染器支持流程图/时序图/饼图；其余类型以代码块形式保真显示。
            ui.menu_button("Mermaid", |ui| {
                for (label, template) in MERMAID_TEMPLATES {
                    if ui.button(*label).clicked() {
                        chosen_tpl = Some(*template);
                        ui.close();
                    }
                }
            });
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
        if let Some(tpl) = chosen_tpl {
            editor::insert_template(&mut self.doc, &mut self.editor, tpl);
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
                // 关闭按钮：加大字号与点击热区（20×20），悬停时亮色反馈。
                let x = ui.add(
                    egui::Button::new(RichText::new("×").size(15.0).color(self.theme.dim))
                        .min_size(egui::vec2(22.0, 20.0))
                        .frame(false),
                );
                if x.hovered() {
                    ui.painter()
                        .rect_filled(x.rect, 4.0, self.theme.dim.gamma_multiply(0.25));
                }
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
                ui.label(RichText::new(name).small().strong().color(self.theme.text));
                // 大纲页签：一键展开 / 折叠全部
                if self.side_tab == SideTab::Outline {
                    if ui.small_button("展开").clicked() {
                        self.outline_folded.clear();
                    }
                    if ui
                        .small_button("折叠")
                        .on_hover_text("折叠所有含小节的标题")
                        .clicked()
                    {
                        for (i, item) in self.outline.iter().enumerate() {
                            if self
                                .outline
                                .get(i + 1)
                                .is_some_and(|n| n.level > item.level)
                            {
                                self.outline_folded.insert(i);
                            }
                        }
                    }
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
                &mut self.icons,
                &mut actions,
            ),
            SideTab::Outline => {
                if self.config.show_outline {
                    let mut fold_toggles: Vec<usize> = Vec::new();
                    sidebar::outline_panel(
                        ui,
                        &self.outline,
                        &self.theme,
                        &mut actions,
                        &self.outline_folded,
                        &mut fold_toggles,
                    );
                    for i in fold_toggles {
                        if !self.outline_folded.insert(i) {
                            self.outline_folded.remove(&i);
                        }
                    }
                } else {
                    // 隐藏大纲时回退到文件树，避免侧栏出现空白或提示文字。
                    sidebar::file_tree(
                        ui,
                        &mut self.tree,
                        self.doc.path.as_deref(),
                        &self.theme,
                        &mut self.icons,
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
                SideAction::Context(p) => {
                    // 右键：确保侧栏处于文件列表状态，并在指针处弹出菜单。
                    self.side_tab = SideTab::Files;
                    self.config.show_sidebar = true;
                    let pos = ui
                        .ctx()
                        .input(|i| i.pointer.interact_pos())
                        .unwrap_or(egui::Pos2::ZERO);
                    self.ctx_menu = Some((p, pos));
                    ui.ctx().request_repaint();
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
                MenuKind::Paragraph => "段落",
                MenuKind::Format => "格式",
                MenuKind::View => "视图",
                MenuKind::Theme => "主题",
                MenuKind::Window => "窗口",
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
                    if matches!(kind, MenuKind::File | MenuKind::Paragraph) {
                        // 文件 / 段落条目较多，超出高度时内部滚动。
                        egui::ScrollArea::vertical()
                            .max_height(480.0)
                            .show(ui, |ui| {
                                for c in menu_commands(kind) {
                                    if ui.button(c.label()).clicked() {
                                        chosen = Some(c);
                                    }
                                }
                            });
                    } else {
                        for c in menu_commands(kind) {
                            if ui.button(c.label()).clicked() {
                                chosen = Some(c);
                            }
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
                        ("单击链接", "打开链接"),
                    ];
                    for (k, v) in rows {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(k).monospace().color(self.theme.accent));
                            ui.label(v);
                        });
                    }
                });
        }

        // ---------------- 文件树右键菜单 ----------------
        if let Some((target, pos)) = self.ctx_menu.clone() {
            #[derive(PartialEq)]
            enum FileMenuAction {
                Open,
                CopyPath,
                CopyRelative,
                CopyFile,
                CutFile,
                Paste,
                Rename,
                Delete,
                Reveal,
                NewFile,
                NewDir,
            }
            let mut chosen: Option<FileMenuAction> = None;
            let is_dir = target.is_dir();
            let reveal_label = if cfg!(target_os = "macos") {
                "在 Finder 中显示"
            } else {
                "在文件管理器中显示"
            };
            let area = egui::Area::new(egui::Id::new("file_ctx_menu"))
                .fixed_pos(pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .stroke(egui::Stroke::new(1.0, self.theme.border))
                        .corner_radius(6.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.set_min_width(170.0);
                            ui.style_mut().interaction.selectable_labels = false;
                            ui.style_mut().spacing.item_spacing.y = 2.0;
                            if !is_dir
                                && ui
                                    .add(
                                        egui::Button::new("打开")
                                            .frame(false)
                                            .min_size(egui::vec2(158.0, 20.0)),
                                    )
                                    .clicked()
                            {
                                chosen = Some(FileMenuAction::Open);
                            }
                            if ui
                                .add(
                                    egui::Button::new("复制路径")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyPath);
                            }
                            if ui
                                .add(
                                    egui::Button::new("复制相对路径")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyRelative);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("复制")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CopyFile);
                            }
                            if ui
                                .add(
                                    egui::Button::new("剪切")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::CutFile);
                            }
                            let paste_enabled = self.file_clipboard.is_some();
                            if ui
                                .add_enabled(
                                    paste_enabled,
                                    egui::Button::new("粘贴")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Paste);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("重命名")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Rename);
                            }
                            if ui
                                .add(
                                    egui::Button::new("删除")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Delete);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new("新建文件")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::NewFile);
                            }
                            if ui
                                .add(
                                    egui::Button::new("新建文件夹")
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::NewDir);
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new(reveal_label)
                                        .frame(false)
                                        .min_size(egui::vec2(158.0, 20.0)),
                                )
                                .clicked()
                            {
                                chosen = Some(FileMenuAction::Reveal);
                            }
                        });
                });
            // 点在菜单外面：收起菜单。
            let clicked_outside = ctx.input(|i| i.pointer.primary_clicked())
                && !area
                    .response
                    .rect
                    .contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or(egui::Pos2::ZERO)));
            if chosen.is_some() || clicked_outside {
                self.ctx_menu = None;
            }
            if let Some(act) = chosen {
                match act {
                    FileMenuAction::Open => {
                        if target.is_file() {
                            self.open_file(target.clone());
                        }
                    }
                    FileMenuAction::CopyPath => self.copy_file_path(&target, false, ctx),
                    FileMenuAction::CopyRelative => self.copy_file_path(&target, true, ctx),
                    FileMenuAction::CopyFile => {
                        self.file_clipboard = Some((target.clone(), false));
                        self.status = "已复制文件".to_string();
                    }
                    FileMenuAction::CutFile => {
                        self.file_clipboard = Some((target.clone(), true));
                        self.status = "已剪切文件".to_string();
                    }
                    FileMenuAction::Paste => {
                        let dest = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.fs_paste(&dest);
                    }
                    FileMenuAction::Rename => {
                        let name = target
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        self.rename_dialog = Some((target.clone(), name));
                    }
                    FileMenuAction::Delete => {
                        self.delete_confirm = Some(target.clone());
                    }
                    FileMenuAction::Reveal => self.reveal_in_system(&target),
                    FileMenuAction::NewFile => {
                        let dir = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.new_dialog = Some((dir, false, String::new()));
                    }
                    FileMenuAction::NewDir => {
                        let dir = if is_dir {
                            target.clone()
                        } else {
                            target
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| target.clone())
                        };
                        self.new_dialog = Some((dir, true, String::new()));
                    }
                }
            }
        }

        // ---------------- 重命名 / 删除确认 / 新建 对话框 ----------------
        if let Some((target, _)) = self.rename_dialog.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            let mut input = std::mem::take(&mut self.rename_dialog)
                .map(|(_, b)| b)
                .unwrap_or_default();
            egui::Window::new("重命名")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("原路径：{}", target.display()));
                    ui.text_edit_singleline(&mut input);
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_rename(&target, &input);
                self.rename_dialog = None;
            } else if open {
                self.rename_dialog = Some((target, input));
            } else {
                self.rename_dialog = None;
            }
        }

        if let Some(target) = self.delete_confirm.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("删除")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("确定要删除 {} 吗？", target.display()));
                    ui.label(
                        RichText::new("此操作不可撤销。")
                            .small()
                            .color(self.theme.dim),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("删除").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_delete(&target);
                self.delete_confirm = None;
            } else if !open {
                self.delete_confirm = None;
            }
        }

        if let Some((dir, is_dir, _)) = self.new_dialog.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            let mut input = std::mem::take(&mut self.new_dialog)
                .map(|(_, _, b)| b)
                .unwrap_or_default();
            let win_title = if is_dir {
                "新建文件夹"
            } else {
                "新建文件"
            };
            egui::Window::new(win_title)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("位置：{}", dir.display()));
                    ui.text_edit_singleline(&mut input);
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            confirm = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if confirm {
                self.fs_new(&dir, is_dir, &input);
                self.new_dialog = None;
            } else if open {
                self.new_dialog = Some((dir, is_dir, input));
            } else {
                self.new_dialog = None;
            }
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

        if self.link_dialog.open {
            let mut open = true;
            let mut insert = false;
            let mut cancel = false;
            egui::Window::new("插入链接")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("链接地址");
                    let url_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.link_dialog.url)
                            .hint_text("粘贴链接，如 https://example.com")
                            .desired_width(380.0),
                    );
                    if self.link_dialog.focus_url {
                        url_resp.request_focus();
                        self.link_dialog.focus_url = false;
                    }
                    if url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        insert = true;
                    }
                    ui.add_space(6.0);
                    ui.label("显示文字（留空则直接显示链接地址）");
                    let text_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.link_dialog.text)
                            .hint_text("默认使用选中的文字")
                            .desired_width(380.0),
                    );
                    if text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        insert = true;
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("插入").clicked() {
                            insert = true;
                        }
                        if ui.button("取消").clicked() {
                            cancel = true;
                        }
                    });
                });
            if insert {
                self.confirm_link_dialog();
            }
            if cancel || !open {
                self.link_dialog.open = false;
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

    /// 复制渲染态拖选出的跨块选区；没有选区时返回 false（交给 TextEdit 原生复制）。
    fn copy_cross_sel(&mut self, ctx: &egui::Context) -> bool {
        let Some(text) = editor::cross_sel_text(&self.doc, self.editor.cross_sel) else {
            return false;
        };
        ctx.copy_text(text);
        self.notify(ctx, "已复制".to_string());
        true
    }

    /// 剪切跨块选区：先复制到剪贴板，再删除选中的内容。
    /// 没有选区时返回 false（交给 TextEdit 原生剪切）。
    fn cut_cross_sel(&mut self, ctx: &egui::Context) -> bool {
        let Some(text) = editor::cut_cross_sel(&mut self.doc, &mut self.editor) else {
            return false;
        };
        ctx.copy_text(text);
        self.notify(ctx, "已剪切".to_string());
        self.on_changed();
        ctx.request_repaint();
        true
    }

    /// 更新状态栏文字并弹出轻提示（底部居中浮层，自动淡出），同时播放一声轻音效。
    fn notify(&mut self, ctx: &egui::Context, msg: String) {
        self.status = msg.clone();
        self.toast = Some((msg, ctx.input(|i| i.time)));
        Self::play_toast_sound();
    }

    /// 轻提示音效：不引音频库，直接调系统自带播放器放一声短促的提示音，
    /// 后台线程 wait 防僵尸进程；系统/音效文件不存在时静默跳过，不影响任何功能。
    fn play_toast_sound() {
        #[cfg(target_os = "macos")]
        let mut cmd = {
            let mut c = std::process::Command::new("afplay");
            // Tink 是系统音效里最轻脆短促的一声，-v 0.4 压低音量做轻提示
            c.args(["-v", "0.4", "/System/Library/Sounds/Tink.aiff"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("powershell");
            c.args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "[System.Media.SystemSounds]::Asterisk.Play()",
            ]);
            c
        };
        #[cfg(target_os = "linux")]
        let mut cmd = {
            let mut c = std::process::Command::new("paplay");
            c.arg("/usr/share/sounds/freedesktop/stereo/complete.oga");
            c
        };
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        if let Ok(mut child) = cmd.spawn() {
            // 后台线程回收子进程，避免反复复制后留下僵尸进程
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    /// 轻提示渲染：底部居中浮层（白底浅绿描边 + 绿色对勾图标），停留 1.8s 后
    /// 用 0.4s 淡出。只读不改 UI 状态，不挡鼠标（interactable(false)）。
    fn show_toast(&mut self, ctx: &egui::Context) {
        const HOLD: f32 = 1.8;
        const FADE: f32 = 0.4;
        let Some((msg, at)) = self.toast.clone() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let elapsed = (now - at) as f32;
        if elapsed > HOLD + FADE {
            self.toast = None;
            return;
        }
        // 淡出期间持续重绘，否则动画会卡住
        ctx.request_repaint();
        let alpha = ((HOLD + FADE - elapsed) / FADE).clamp(0.0, 1.0);
        let mix = |c: egui::Color32| -> egui::Color32 {
            let a = (c.a() as f32 * alpha) as u8;
            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
        };
        // 配色固定为参考样式：白底 / 浅绿描边 / 绿色对勾 / 深色文字，不随主题变化
        const BG: egui::Color32 = egui::Color32::WHITE;
        const BORDER: egui::Color32 = egui::Color32::from_rgb(0x9B, 0xD8, 0x76);
        const ICON: egui::Color32 = egui::Color32::from_rgb(0x52, 0xC4, 0x1A);
        const TEXT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
        // 不用 anchor：Area 会记住上次位置，anchor 在状态残留时可能被顶到窗口中部；
        // 直接每帧按屏幕矩形算出定位点，用 fixed_pos + pivot 强制定位。
        // 气泡下缘落在窗口高度的 40% 处（中上部）：醒目且不与顶部工具条打架。
        let screen = ctx.viewport_rect();
        let pos = egui::pos2(screen.center().x, screen.top() + screen.height() * 0.40);
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(mix(BG))
                    .stroke(egui::Stroke::new(1.0, mix(BORDER)))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 10,
                        spread: 0,
                        color: mix(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 45)),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // 自绘绿色圆 + 白色对勾图标，不依赖 emoji 字体
                            let (ir, _) = ui
                                .allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            let p = ui.painter();
                            let c = ir.center();
                            p.circle_filled(c, 8.0, mix(ICON));
                            p.add(egui::Shape::line(
                                vec![
                                    egui::pos2(c.x - 3.5, c.y + 0.5),
                                    egui::pos2(c.x - 1.0, c.y + 3.0),
                                    egui::pos2(c.x + 3.5, c.y - 2.5),
                                ],
                                egui::Stroke::new(1.8, mix(egui::Color32::WHITE)),
                            ));
                            ui.label(egui::RichText::new(&msg).size(13.0).color(mix(TEXT)));
                        });
                    });
            });
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
        // CentralPanel 默认填充 panel_fill（浅色 #F3F3F3），编辑区应是正文色：
        // 复现 egui 0.37 Frame::central_panel（inner_margin 8），仅把底色换成 theme.bg（浅色 #FFFFFF）。
        let central_frame = egui::Frame::new().inner_margin(8).fill(self.theme.bg);
        let (event, actions, home_act) = egui::CentralPanel::default()
            .frame(central_frame)
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
                    self.config.show_line_numbers,
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
                Action::Notify(msg) => self.notify(&ctx, msg),
                Action::Caret(i, off) => {
                    self.editor.active = i;
                    self.editor.focus_req = Some(i);
                    self.editor.pending_caret = Some(off);
                }
                Action::TableCellClick(bi, ri, ci, end) => {
                    // 渲染态点了表格某个单元格：进入编辑态时直接聚焦那一格，
                    // 光标按点击位置落到文字开头或末尾。
                    self.editor.table_focus = Some((ri, ci));
                    self.editor.table_caret_end = end;
                    // 记录最后命中的单元格：表格工具栏的行/列操作以此为基准。
                    ctx.memory_mut(|m| {
                        m.data
                            .insert_temp(egui::Id::new(("table_cell_ref", bi)), (ri, ci));
                    });
                }
                Action::TableOp(i, op) => {
                    editor::apply_table_op(&mut self.doc, i, op);
                    self.on_changed();
                }
                Action::AskDeleteBlock(i) => {
                    // 请求删除代码块：弹确认窗，确定才真正删除（取消不删）。
                    self.code_delete_confirm = Some(i);
                }
                // 跨块拖选已在 show_editor 帧末聚合进 cross_sel，不会到达这里
                Action::CrossDrag(..) => {}
                // 双击选词 / 三击选整段 / Shift+点击扩选：同样在 show_editor 聚合
                Action::WordSel(..) | Action::BlockSel(..) | Action::ShiftClick(..) => {}
                Action::FoldToggle(i) => {
                    // 折叠/展开代码块：只改视图状态，不动文档。
                    if !self.editor.folded.insert(i) {
                        self.editor.folded.remove(&i);
                    }
                    // 有路径的文档立即把折叠状态写盘，关闭/重开不丢。
                    if let Some(p) = &self.doc.path {
                        editor::save_folds(p, &self.doc, &self.editor.folded);
                    }
                }
                Action::SetLang(i, lang) => {
                    let rebuilt = match self.doc.blocks.get(i) {
                        Some(crate::ast::Block::Code {
                            code,
                            source,
                            quote,
                            ..
                        }) => {
                            let fence = if source.contains("~~~") { "~~~" } else { "```" };
                            if *quote == 0 {
                                Some(format!("{}{}\n{}\n{}", fence, lang, code, fence))
                            } else {
                                // 引用内的代码块：换语言时保留每行的 `>` 前缀。
                                let p = ">".repeat(*quote);
                                let body: Vec<String> = code
                                    .split('\n')
                                    .map(|l| {
                                        if l.is_empty() {
                                            p.clone()
                                        } else {
                                            format!("{p} {l}")
                                        }
                                    })
                                    .collect();
                                Some(format!(
                                    "{p} {fence}{lang}\n{}\n{p} {fence}",
                                    body.join("\n")
                                ))
                            }
                        }
                        // 图表块：选 mermaid 保持不变，选其它语言即转为普通代码块
                        Some(crate::ast::Block::Diagram { code, .. }) => {
                            if lang == "mermaid" {
                                None
                            } else {
                                Some(format!("```{}\n{}\n```", lang, code))
                            }
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
                Action::ResizeImage(bi, no, width) => {
                    // 拖动图片右下角缩放：把新宽度写成 `{width=N}` 属性。
                    // 拖动过程中每帧都会来，所以不记撤销栈（否则一次拖动几十步）。
                    if let Some(src) = resize_image_source(&self.doc, bi, no, width) {
                        self.doc.edit_block(bi, &src);
                        self.on_changed();
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
        // 代码块删除确认弹窗：确定才删除，取消（或点 × 关闭）不删。
        if let Some(idx) = self.code_delete_confirm {
            let mut open = true;
            egui::Window::new("删除代码块")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.label("确定要删除这个代码块吗？删除后可以用撤销找回。");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("确定删除").clicked() {
                            self.doc.record();
                            // 只剩这一块时删成空段落，保证文档至少有一个块
                            if self.doc.blocks.len() <= 1 {
                                self.doc.edit_block(idx, "");
                            } else {
                                self.doc.remove_block(idx);
                            }
                            let n = self.doc.blocks.len();
                            self.editor.active = self.editor.active.min(n - 1);
                            self.on_changed();
                            self.code_delete_confirm = None;
                        }
                        if ui.button("取消").clicked() {
                            self.code_delete_confirm = None;
                        }
                    });
                });
            if !open {
                self.code_delete_confirm = None;
            }
        }
        self.show_toast(&ctx);
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

    /// 拖动缩放：只给目标图片写入 `{width=N}`，其余图片属性不动；
    /// 混着正文的段落不处理（重建会把文字弄丢）。
    #[test]
    fn resize_image_source_rewrites_width_attr() {
        let doc = Document::new("![a](imgs/a.png)\n![b](imgs/b.png){width=120}", None);
        let src = resize_image_source(&doc, 0, 1, 300).expect("纯图片段落应可重建");
        assert_eq!(src, "![a](imgs/a.png)\n![b](imgs/b.png){width=300}");
        assert!(
            resize_image_source(&doc, 0, 9, 300).is_none(),
            "序号越界不处理"
        );
        let mixed = Document::new("文字 ![a](a.png)", None);
        assert!(
            resize_image_source(&mixed, 0, 0, 300).is_none(),
            "混着正文的段落不应重建源码"
        );
    }

    /// 加粗按钮：源码用标准 Markdown 的 ** 标记（即时渲染态由 inline_job
    /// 隐藏标记、显示粗体），不再写 <strong>。
    #[test]
    fn bold_button_writes_markdown_stars() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("加粗这段文字", None);
        app.editor.last_sel = Some((0, 2, 6)); // 选中「这段文字」
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.source(), "加粗**这段文字**");
    }

    /// 跨块拖选：鼠标选中某块的文字后点加粗，只加粗选中的文字，
    /// 不碰光标所在的活动块（两者往往不是同一行）。
    #[test]
    fn bold_applies_to_cross_sel_not_active_block() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("光标行\n\n第二行选中文字结尾\n\n第三行", None);
        app.editor.active = 0; // 光标在第 0 块
        // 鼠标拖选的是第 2 块的「选中文字」（字符 3..7）
        app.editor.cross_sel = Some((2, 3, 2, 7));
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.blocks[0].source(), "光标行", "光标所在行不应被加粗");
        assert_eq!(app.doc.blocks[2].source(), "第二行**选中文字**结尾");
    }

    /// 跨多块选区：首块取选中片段、中间块整块、尾块取片段。
    #[test]
    fn bold_cross_sel_spans_multiple_blocks() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("甲甲乙丙丁\n\n第二块全文\n\n戊己庚辛", None);
        app.editor.active = 0;
        // 选区：块0 的 2..5（乙丙丁） → 块4 的 0..2（戊己）；0/4 是块下标
        app.editor.cross_sel = Some((0, 2, 4, 2));
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.blocks[0].source(), "甲甲**乙丙丁**");
        assert_eq!(app.doc.blocks[4].source(), "**戊己**庚辛");
    }

    /// 再次对已加粗的选区点加粗 → 取消加粗。
    #[test]
    fn bold_cross_sel_toggles_off() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("前缀粗体后缀", None);
        app.editor.cross_sel = Some((0, 2, 0, 4));
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.source(), "前缀**粗体**后缀");
        // 渲染态仍显示纯文本「前缀粗体后缀」，再次选中「粗体」应取消加粗
        app.editor.cross_sel = Some((0, 2, 0, 4));
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.source(), "前缀粗体后缀");
    }

    /// 无跨块选区时，加粗仍作用于活动块（原有行为不变）。
    #[test]
    fn bold_without_cross_sel_uses_active_block() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("正文一段", None);
        app.editor.last_sel = Some((0, 2, 4));
        app.execute(Command::Bold, &ctx);
        assert_eq!(app.doc.source(), "正文**一段**");
    }

    /// 行内代码按钮：跨块拖选一段文字后点「行内代码」，应只包裹选中文字，
    /// 光标所在的活动块不受影响（与加粗共用 wrap 逻辑）。
    #[test]
    fn inline_code_applies_to_cross_sel() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("光标行\n\n第二行选中文字结尾", None);
        app.editor.active = 0;
        app.editor.cross_sel = Some((2, 3, 2, 7)); // 选中块2的「选中文字」
        app.execute(Command::Code, &ctx);
        assert_eq!(app.doc.blocks[0].source(), "光标行", "光标行不应被改动");
        assert_eq!(app.doc.blocks[2].source(), "第二行`选中文字`结尾");
    }

    /// 点「有序列表」「无序列表」按钮：把当前段落转成列表项、不新增块。
    #[test]
    fn list_buttons_convert_current_block() {
        let ctx = egui::Context::default();
        for (cmd, want_marker) in [(Command::Ordered, "1."), (Command::Bullet, "-")] {
            let mut app = test_app();
            app.doc = Document::new("列表内容", None);
            app.execute(cmd.clone(), &ctx);
            assert_eq!(app.doc.blocks.len(), 1, "不应新增块");
            assert!(
                matches!(&app.doc.blocks[0], crate::ast::Block::ListItem { item, .. } if item.marker == want_marker),
                "{:?} 应转成标记 {want_marker} 的列表项: {:?}",
                &cmd,
                app.doc.blocks[0]
            );
        }
    }

    /// 点「任务列表」按钮：把光标所在段落转成任务项，保留文字、不新增块。
    #[test]
    fn task_button_converts_current_block() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.doc = Document::new("写文档", None);
        app.execute(Command::Task, &ctx);
        assert_eq!(app.doc.blocks.len(), 1, "不应新增块，只转换当前块");
        assert!(
            matches!(&app.doc.blocks[0], crate::ast::Block::ListItem { item, .. } if item.checked == Some(false)),
            "当前段应转成任务项: {:?}",
            app.doc.blocks[0]
        );
        assert_eq!(app.doc.source(), "- [ ] 写文档");
    }

    /// 即时渲染态：inline_job 把 **...** 的内容渲染为粗体、标记隐藏。
    #[test]
    fn inline_job_renders_stars_as_bold() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let job =
            crate::render::inline_job("**粗体**", &theme, 16.0, theme.text, 400.0, None, None);
        assert!(
            job.sections.iter().any(|sec| {
                matches!(&sec.format.font_id.family, egui::FontFamily::Name(n) if n.as_ref() == "markd-bold")
            }),
            "双星号内容应以粗体字体渲染: {job:?}"
        );
    }

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

    /// 构造一个最小化的真实 MarkdApp：不装 macOS 原生菜单、不读用户配置和
    /// 快照目录，其余字段与 MarkdApp::new 相同，用来在测试里驱动
    /// `eframe::App::ui` 这条真实的完整帧路径（含终端面板 / read_only 逻辑）。
    fn test_app() -> MarkdApp {
        let config = Config::default();
        let mut theme = Theme::get(config.theme);
        theme.body = config.font_size;
        theme.mono = (config.font_size * 0.84).max(11.0);
        let doc = Document::new("", None);
        let stats_cache = doc.stats();
        MarkdApp {
            stats: WritingStats::new(&doc),
            stats_cache,
            stats_at: std::time::Instant::now(),
            doc,
            config,
            theme,
            editor: EditorState::default(),
            images: crate::render::ImageCache::new(),
            icons: crate::icons::IconCache::default(),
            root: None,
            tree: Vec::new(),
            expanded: HashSet::new(),
            outline: Vec::new(),
            side_tab: SideTab::Files,
            outline_folded: HashSet::new(),
            menu: None,
            workspace_input: String::new(),
            palette: Palette::default(),
            search: SearchPanel::default(),
            find: FindPanel::default(),
            graph: GraphPanel::default(),
            snapshots: SnapshotStore {
                items: Vec::new(),
                open: false,
            },
            status: String::new(),
            toast: None,
            about: false,
            shortcuts: false,
            table_dialog: false,
            table_rows: 3,
            table_cols: 3,
            link_dialog: Default::default(),
            link_docs: HashMap::new(),
            link_rx: None,
            last_autosave: now_secs(),
            exit_dialog: false,
            code_delete_confirm: None,
            force_quit: false,
            theme_dirty: false,
            menu_guard: false,
            workspace_dialog: false,
            quick_cache_dir: String::new(),
            quick_cache: Vec::new(),
            menu_queue: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::VecDeque::new(),
            )),
            terminal: Terminal::new(std::env::temp_dir()),
            settings: Default::default(),
            home: true,
            tabs: Vec::new(),
            active: 0,
            untitled_seq: 0,
            next_uid: 1,
            frame_count: 0,
            ctx_menu: None,
            file_clipboard: None,
            rename_dialog: None,
            delete_confirm: None,
            new_dialog: None,
        }
    }

    /// 跑一帧真实 app：1280×800 窗口，所有面板 + 编辑区完整渲染。
    fn run_real_frame(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        events: Vec<egui::Event>,
    ) {
        let _ = run_real_frame_ext(ctx, app, frame, events);
    }

    /// 同上，但额外返回 (全部绘制形状, 系统剪贴板收到的文本)：
    /// 跨块选区的手动链路验证需要抓块坐标和剪贴板内容。
    fn run_real_frame_ext(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        events: Vec<egui::Event>,
    ) -> (Vec<egui::epaint::ClippedShape>, String) {
        use eframe::App as _;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            let mut root = egui::Ui::new(
                root.ctx().clone(),
                egui::Id::unique("markd_test_root"),
                egui::UiBuilder::new(),
            );
            app.ui(&mut root, frame);
        });
        out.textures_delta.clear();
        // 0.37 的剪贴板写入走 platform_output.commands 的 CopyText 命令
        let copied = out
            .platform_output
            .commands
            .iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        (out.shapes, copied)
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    /// 手动链路验证：拖选跨块 → Cmd+C 复制剪贴板 → 退格合并 → Cmd+Z 撤销。
    #[test]
    fn cross_block_manual_flow() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();
        app.doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        app.home = false;
        app.editor.active = 0;
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }

        // 抓"第二段"/"第三段"的文本锚点（块文字左上角）
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> egui::Pos2 {
            for cs in shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    if t.galley.text() == name {
                        return t.pos;
                    }
                }
            }
            panic!("{name} 未渲染");
        };
        let p2 = find(&shapes, "第二段");
        let p4 = find(&shapes, "第三段");
        println!(
            "[坐标] 第二段@({:.1},{:.1}) 第三段@({:.1},{:.1})",
            p2.x, p2.y, p4.x, p4.y
        );
        // 拖选点：起点在块 2 文字开头（字符 0），终点在块 4 行尾空白（钳到末尾）
        let start = egui::pos2(p2.x + 3.0, p2.y + 9.0);
        let end = egui::pos2(p4.x + 300.0, p4.y + 9.0);

        // 拖选：按下 → 移动 → 多帧保持（drag_started 晚一帧生效）→ 抬起
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::PointerMoved(end)],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::PointerMoved(end)],
        );
        run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(end, false)]);
        for _ in 0..2 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let sel = app.editor.cross_sel.expect("拖选后应建立跨块选区");
        let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!((lo, hi), (2, 4), "选区应覆盖第二段到第三段，实际 {sel:?}");

        // Cmd+C：egui-winit 会把它转成 Event::Copy（不产生 Key 事件），
        // 剪贴板应收到两段文本。
        let (_, copied) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Copy]);
        assert_eq!(copied, "第二段\n\n第三段", "Cmd+C 应把跨块选区复制进剪贴板");

        // 退格：删除选区（跨块合并）
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::Key {
                key: egui::Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let src = app.doc.source();
        assert!(
            !src.contains("第二段") && !src.contains("第三段"),
            "退格应删除选中的两段，实际 {src:?}"
        );
        assert!(src.contains("第一段"), "未选中的第一段必须保留：{src:?}");

        // Cmd+Z：撤销恢复
        let cmd_z = egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
        };
        let mods_cmd = egui::Event::ModifiersChanged(egui::Modifiers {
            command: true,
            mac_cmd: true,
            ..Default::default()
        });
        run_real_frame(&ctx, &mut app, &mut frame, vec![mods_cmd, cmd_z]);
        assert_eq!(
            app.doc.source(),
            "第一段\n\n第二段\n\n第三段",
            "Cmd+Z 应恢复被删除的两段"
        );
    }

    /// Cmd+X：跨块选区应先复制进剪贴板，再从文档里删除。
    #[test]
    fn cross_block_cut_shortcut_copies_and_deletes() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();
        app.doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        app.home = false;
        app.editor.active = 0;
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        // 直接给一个跨块选区：第二段(2) 到第三段(4) 全选。
        app.editor.cross_sel = Some((2, 0, 4, 3));

        // egui-winit 把 Cmd+X 转成 Event::Cut（不产生 Key 事件）。
        let (_, copied) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Cut]);
        assert_eq!(copied, "第二段\n\n第三段", "Cmd+X 应把跨块选区复制进剪贴板");
        assert!(app.editor.cross_sel.is_none(), "剪切后选区应清除");
        let src = app.doc.source();
        assert!(
            !src.contains("第二段") && !src.contains("第三段"),
            "Cmd+X 应删除选中的两段，实际 {src:?}"
        );
        assert!(src.contains("第一段"), "未选中的第一段必须保留：{src:?}");
    }

    /// 真实 app 完整帧路径：新建文档后点编辑区空白，光标必须一直在第一行。
    /// 这是用户报告 bug 的原始场景；驱动的是 `MarkdApp::ui`，不是仿真面板。
    #[test]
    fn real_app_blank_click_after_new_document() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();

        // 首页一帧，然后走「+ 新建」的真实路径
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        app.new_document();
        // 让 focus_req=Some(0) 落地：此时光标应已在第一行（用户确认的初始状态）
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let tab_key = app.tabs[app.active].uid;
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::unique(("block_edit", tab_key, 0))),
            "新建文档后光标应默认在第一行"
        );

        // 反复点编辑区空白，每次点完光标都必须还在第一行：
        // 正文下方 / 左侧页边距 / 正文上方贴着的空白，全部覆盖。
        for (round, blank) in [
            (1, egui::pos2(600.0, 400.0)),
            (2, egui::pos2(240.0, 300.0)),
            (3, egui::pos2(600.0, 130.0)),
        ] {
            run_real_frame(
                &ctx,
                &mut app,
                &mut frame,
                vec![
                    egui::Event::PointerMoved(blank),
                    pointer_button(blank, true),
                ],
            );
            run_real_frame(
                &ctx,
                &mut app,
                &mut frame,
                vec![pointer_button(blank, false)],
            );
            for _ in 0..3 {
                run_real_frame(&ctx, &mut app, &mut frame, vec![]);
            }
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(egui::Id::unique(("block_edit", tab_key, 0))),
                "第 {round} 次点空白后光标必须仍在第一行"
            );
            assert_eq!(
                app.editor.active, 0,
                "第 {round} 次点空白后活动块应为第一块"
            );
            assert_eq!(
                app.editor.last_cursor, 0,
                "第 {round} 次点空白后光标应在行首"
            );
        }
    }

    /// 代码块编辑态换语言必须一次点击完成：
    /// 光标在代码块内（TextEdit 持有焦点）→ 点一次语言下拉（弹层打开）→
    /// 点一次语言项（语言改掉、弹层收起）。这是用户报告的「需要点两次」bug 的原始场景。
    #[test]
    fn real_app_code_lang_combo_one_click() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();

        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        app.new_document();
        // 把第一块做成代码块并聚焦，让 TextEdit 持有焦点（= 光标在代码块里）
        app.doc.edit_block(0, "```rust\nfn main() {}\n```");
        app.editor.active = 0;
        app.editor.focus_req = Some(0);
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let tab_key = app.tabs[app.active].uid;
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::unique(("block_edit", tab_key, 0))),
            "前置条件：光标应在代码块的编辑框里"
        );

        // 从测试钩子里拿到语言下拉按钮的矩形与弹层 id。
        // 只取本测试线程写入的记录：COMBO_TRACE 是全局的，并行跑别的
        // 渲染测试时也会往里 push，取 last() 会偶尔拿到别人的矩形（偶发失败）。
        let me = std::thread::current().id();
        let (rect, popup_id, ..) = *crate::render::COMBO_TRACE
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|e| e.4 == me)
            .expect("代码块编辑态应渲染语言下拉");
        crate::render::COMBO_TRACE.lock().unwrap().clear();

        // 单击一次语言下拉（按下/松开分两帧），弹层必须打开
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![
                egui::Event::PointerMoved(rect.center()),
                pointer_button(rect.center(), true),
            ],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![pointer_button(rect.center(), false)],
        );
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        assert!(
            egui::Popup::is_id_open(&ctx, popup_id),
            "一次点击后语言下拉弹层应打开"
        );

        // 弹层里点第一个语言项（列表 A→Z，「纯文本」排最前）
        let popup_rect = ctx
            .read_response(popup_id)
            .map(|r| r.rect)
            .expect("弹层应已渲染出响应矩形");
        let item_pos = egui::pos2(popup_rect.left() + 30.0, popup_rect.top() + 45.0);
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![
                egui::Event::PointerMoved(item_pos),
                pointer_button(item_pos, true),
            ],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![pointer_button(item_pos, false)],
        );
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }

        // 语言必须已改掉（rust → 其它），且一次点击就生效
        let lang = match app.doc.blocks.first() {
            Some(crate::ast::Block::Code { lang, .. }) => lang.clone(),
            other => panic!("第一块应仍是代码块，实际 {other:?}"),
        };
        assert_ne!(lang, "rust", "点一次语言项后语言应已改变");
        assert!(
            !egui::Popup::is_id_open(&ctx, popup_id),
            "选完语言后弹层应收起"
        );
    }
}
