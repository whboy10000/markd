//! Author: aurelianus <aurelianus@126.com>

//! Hybrid View 编辑器：活动块显示源码，其余块实时渲染成富文本。
//!
//! 子模块按职责拆分：
//! - `folds`：代码块折叠状态的持久化；
//! - `text`：字符位置与行首标记工具；
//! - `commands`：块级格式命令；
//! - `inline_ops`：行内样式与链接；
//! - `layout`：块高/行号排版表（视口裁剪的几何基础）；
//! - `table`：表格结构操作；
//! - `edit_kind`：编辑态块的源码重建；
//! - `selection`：跨块选区；
//! - `source_mode` / `view`：源码模式与混合模式的主绘制入口。

use crate::ast::Block;
use crate::document::{Document, OutlineItem};
use crate::render::{self, Action, ImageCache, RenderEnv};
use crate::theme::Theme;
use egui::{self, Id, Key, Sense, TextEdit};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod commands;
mod edit_kind;
mod folds;
mod inline_ops;
mod layout;
mod selection;
mod source_mode;
mod table;
#[cfg(test)]
mod tests;
mod text;
mod view;

pub(crate) use self::edit_kind::*;
pub use self::{
    commands::*, folds::*, inline_ops::*, layout::BlockLayout, selection::*,
    source_mode::SourceCache, table::*, view::*,
};
use self::{source_mode::*, text::*};

#[derive(Default)]
pub struct EditorState {
    pub active: usize,
    pub focus_req: Option<usize>,
    pub scroll_req: Option<usize>,
    /// true = 把目标块对齐到视口顶部（大纲跳转）；false = 只做最小滚动（新建/换行）。
    pub scroll_top: bool,
    pub last_cursor: usize,
    pub sel: Option<(usize, usize)>,
    /// 表格刚成为活动块时要聚焦的单元格：(行, 列)，用一次就清。
    pub table_focus: Option<(usize, usize)>,
    /// 聚焦表格单元格时光标是否落到文字末尾（点击文字右半边进入编辑时为 true）。
    pub table_caret_end: bool,
    /// 最近一次有效选区：(块下标, 起, 止)。点工具条按钮时编辑框已经失焦、`sel` 被清空，
    /// 加粗/斜体/行内代码这些按钮靠它才能作用在刚选中的文字上。
    pub last_sel: Option<(usize, usize, usize)>,
    /// 终端持有键盘焦点时置 true：编辑区不渲染可编辑控件，避免把焦点抢回来。
    pub read_only: bool,
    /// 本帧是否点击了某个块（app 用它把焦点从终端交还给编辑区）。
    pub clicked: bool,
    /// 正在原地编辑的表格草稿：(块下标, 单元格文本, 对齐)。
    pub table_draft: Option<(usize, Vec<Vec<String>>, Vec<crate::ast::Align>)>,
    /// 表格里按下鼠标的起点：(块下标, 行, 列)。从该单元格拖到别的单元格或
    /// 表格外时，整张表格进入跨块选区，可直接复制/剪切。
    pub table_drag_from: Option<(usize, usize, usize)>,
    /// 点击进入编辑时，光标应落在的位置（相对块左上角）。
    pub pending_caret: Option<egui::Vec2>,
    /// 查找/替换跳转要求选中的文字：(块下标, 起始字符, 结束字符)，用后清零。
    pub select_req: Option<(usize, usize, usize)>,
    /// 跨块选区 (锚块, 锚字符, 头块, 头字符)：在渲染态块上按住拖动产生，
    /// 覆盖首尾块之间的所有内容。Cmd+C 复制、输入/退格替换、单击清除。
    /// 字符按各块所见文本（plain）计，与渲染态 galley 光标同体系。
    pub cross_sel: Option<(usize, usize, usize, usize)>,
    /// 在编辑态块里按下鼠标时的位置：(块下标, 字符下标)。
    /// 从编辑态段落往下/上拖到别的块时，跨块选区以这个按下点为锚点，
    /// 否则锚点会变成"拖出去时已经跑到行尾的光标"，起始那一段就选不上。
    pub press_caret: Option<(usize, usize)>,
    /// 格式按钮用完选区后把光标收敛成单点：(块下标, 字符下标)，用后清零。
    /// 不然文本框里存的旧选区会让刚处理过的文字一直保持选中状态。
    pub caret_req: Option<(usize, usize)>,
    /// 缩进空格数（设置里可调）。
    pub tab_size: usize,
    /// 是否自动补全括号/引号。
    pub auto_pair: bool,
    /// 编辑区空白处右键菜单的弹出位置；None 表示菜单关闭。
    pub context_menu: Option<egui::Pos2>,
    /// 已折叠的代码块下标集合（渲染态折叠，切换标签/文档时随 EditorState 一起保存）。
    pub folded: HashSet<usize>,
    /// 代码组当前显示的标签：(块下标 → 标签下标)。
    pub code_group_tab: HashMap<usize, usize>,
    /// 块高/行号排版表：主绘制循环靠它只排版视口内的块。
    pub layout: BlockLayout,
    /// 源码模式的整篇派生数据（源码串、行首表、最长行宽度），按文档修订号失效。
    pub source: SourceCache,
}

impl EditorState {
    /// 代码组块当前显示的标签下标（没有记录时为 0）。
    pub fn group_tab(&self, idx: usize) -> usize {
        self.code_group_tab.get(&idx).copied().unwrap_or(0)
    }
}

pub enum EditorEvent {
    None,
    Changed,
}

// ---------------------------------------------------------------- 折叠状态持久化
//
// 折叠状态按文档绝对路径存到 ~/.markd/folds.json，内容是
// `{ 文档路径: [折叠代码块的 source 字符串] }`。
//
// 用块的 source（含围栏的完整 Markdown 源码）而不是块下标做键：文档在折叠块
// 之前增删块会导致下标整体错位，source 则不受影响。只有当折叠块自身的源码被
// 修改时（例如点开它编辑再离开）才会随保存更新到新 source。两个内容完全相同的
// 代码块会共享同一键，展开/折叠会同步，属可接受的边缘情况。
