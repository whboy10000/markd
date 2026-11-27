//! Author: aurelianus <aurelianus@126.com>

//! 渲染层：把 AST 画成富文本 / 表格 / 代码块 / 流程图 / 图片。
//!
//! 非活动块走这里；活动块由 editor 用 TextEdit 显示源码，形成 Hybrid View。
//!
//! 子模块：`inline` 行内排版，`block` 块级分派，`syntax` 代码着色，
//! `diagram` 各类 Mermaid 图表，`media` 图片，`video` 视频卡片，`attachment` 附件卡片，
//! `language` 语言下拉，`frames` 外框样式。

use crate::ast::*;
use crate::document::OutlineItem;
use crate::theme::{
    LangFamily, Theme, block_comment_for, lang_family, lang_key, line_comments_for,
};
use crate::workspace;
use egui::text::{LayoutJob, TextFormat};
use egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod attachment;
mod block;
mod diagram;
mod frames;
mod inline;
mod language;
mod lists;
mod math;
mod media;
mod syntax;
#[cfg(test)]
mod tests;
mod video;

pub use self::{block::*, inline::*, language::*, lists::*, math::*, media::*, syntax::*};
pub(crate) use self::{attachment::*, diagram::*, frames::*, video::*};

#[derive(Clone, Debug)]
pub enum Action {
    ToggleTask(usize),
    OpenLink(String),
    Jump(usize),
    Notify(String),
    /// 修改代码块的语言（info string）。
    SetLang(usize, String),
    /// 切换代码组里显示的标签：(块下标, 标签下标)。
    CodeGroupTab(usize, usize),
    /// 修改代码组当前标签的语言（下拉选择）。
    CodeGroupLang(usize, String),
    /// 给代码组新增一个语言标签。
    CodeGroupAddTab(usize),
    /// 删除代码组当前的语言标签（至少保留一个）。
    CodeGroupRemoveTab(usize),
    /// 点击了某个块：块下标 + 点击位置（相对块左上角），用于把光标放到点击处。
    Caret(usize, egui::Vec2),
    /// 点击了表格单元格：(块下标, 行, 列, 光标是否在文字末尾)。
    /// 进入编辑态时聚焦该单元格，光标落在点击位置附近而不是固定跳到第一格。
    TableCellClick(usize, usize, usize, bool),
    /// 表格内按住拖动：`Some((起格, 止格))` = 只选中经过的单元格；
    /// `None` = 指针已拖出表格，交回「整张表」的跨块选区。
    TableDrag(usize, Option<((usize, usize), (usize, usize))>),
    /// 表格工具栏的结构操作：(块下标, 操作)。
    TableOp(usize, crate::editor::TableOp),
    /// 请求删除某个代码块：app 层弹确认窗，确定后整块删除。
    AskDeleteBlock(usize),
    /// 请求删除整张表格：app 层弹确认窗，确定后才删除。
    AskDeleteTable(usize),
    /// 折叠/展开某个代码块（渲染态右上角的折叠按钮）。
    FoldToggle(usize),
    /// 跨块拖选进展：(块下标, 字符下标, 是否拖动开始)。
    /// 拖动开始时锚定选区起点，之后每次移动更新选区终点。
    CrossDrag(usize, usize, bool),
    /// 双击选词：块下标 + 词的字符区间 [起, 止)。
    WordSel(usize, usize, usize),
    /// 三击选中整块（一行/一段）。
    BlockSel(usize),
    /// Shift+点击：从光标/选区锚点到点击位置全部选中。
    ShiftClick(usize, usize),
    /// 拖动图片右下角缩放：(块下标, 图片序号, 新宽度像素)。
    ResizeImage(usize, usize, u32),
    /// 点开附件卡片：本地文件直接交给系统程序，远程文件先下到本机再打开。
    PreviewFile(String),
}

pub struct RenderEnv<'a> {
    pub theme: &'a Theme,
    pub ctx: &'a egui::Context,
    pub images: &'a mut ImageCache,
    pub base_dir: Option<&'a Path>,
    pub outline: &'a [OutlineItem],
    pub actions: &'a mut Vec<Action>,
    /// 正文排版宽度。显式传给每个文本块，避免在横向布局里
    /// Label 退回 `Extend`（无限宽）后长段落永远不换行。
    pub max_width: f32,
    /// 当前跨块选区 (锚块, 锚字符, 头块, 头字符)：渲染态块用它画选中底色。
    pub cross_sel: Option<(usize, usize, usize, usize)>,
    /// 已折叠的代码块下标集合：折叠态的代码块只画一个「展开」按钮和提示行。
    pub folded: &'a HashSet<usize>,
    /// 代码组当前显示的标签下标（由编辑器状态传入，越界时钳到最后一个）。
    pub group_tab: usize,
}
