//! Author: aurelianus <aurelianus@126.com>

//! 应用外壳：窗口布局、菜单、快捷键、状态栏、各功能面板的整合。
//!
//! `MarkdApp` 本身与共享状态结构定义在这里，具体行为按主题拆分到子模块：
//! `lifecycle` 生命周期、`tabs` 标签页、`files` 文件树、`actions` 命令与快捷键、
//! `ui` 顶层面板、`dialogs` 弹窗、`feedback` 剪贴板与轻提示。

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

mod actions;
mod dialogs;
mod feedback;
mod files;
mod graph;
mod home;
mod lifecycle;
pub(crate) mod menu;
mod paint;
mod tabs;
#[cfg(test)]
mod tests;
mod ui;
mod update;
mod util;

use self::{graph::*, menu::*, paint::*, util::*};

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
    /// 待确认删除的表格块下标：点「删除表格」后弹窗，确定才真正删除。
    table_delete_confirm: Option<usize>,
    /// 后台图床上传任务；完成后自动把本地链接替换成远程 URL。
    image_uploads: Vec<std::sync::mpsc::Receiver<ImageUpload>>,
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

/// 后台上传图片的结果：完成时把文档里的旧链接替换成图床 URL。
struct ImageUpload {
    /// 插入图片时所在标签页的 uid：切换标签后不要把别的文档改掉。
    tab_uid: u64,
    /// 插入时写进文档的本地 URL（替换目标）。
    old_url: String,
    /// 图片名（用于提示）。
    name: String,
    result: Result<String, String>,
}
