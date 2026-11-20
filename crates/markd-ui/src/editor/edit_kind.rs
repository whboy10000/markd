//! 编辑态块的源码重建：把编辑缓冲还原成 Markdown 源码。

use super::*;

/// 活动块的「编辑形态」：标题只编辑文字、代码块只编辑代码（不含围栏），
/// 像 Typora 那样直接在渲染结果上改，而不是切到 Markdown 源码。
#[derive(Clone)]
pub(super) enum EditKind {
    Source,
    Heading(u8),
    /// 代码块：`quote` 表示它写在引用内部（源码每行带 `>` 前缀），
    /// 写回时要逐行补回前缀，编辑时左侧照渲染态显示引用竖线。
    Code {
        lang: String,
        quote: usize,
    },
    /// 图表块（mermaid 等）：编辑时隐藏 ``` 围栏，只编辑图形描述代码。
    Diagram(String),
    /// 代码组：编辑当前标签的代码（不含外层 ````code-group 与内层围栏），
    /// 切回渲染态时按标签列表重建整份源码。
    CodeGroup {
        items: Vec<crate::ast::CodeGroupItem>,
        tab: usize,
    },
    /// 列表项（含任务列表）：编辑时隐藏行首标记（- / 1. / [ ]），只编辑正文，
    /// 复选框/圆点/编号照渲染态显示，所见即所得。
    List {
        indent: usize,
        marker: String,
        checked: Option<bool>,
    },
    /// 引用块：编辑时隐藏 `>` 标记，只编辑正文，左侧引用竖线照常显示。
    Quote {
        level: usize,
    },
}

impl EditKind {
    /// 围栏本身就是块语义的编辑形态：代码块（含引用内）、图表块、代码组。
    /// 它们的 ` ``` ` 一旦丢掉，正文就掉成普通段落，所以：
    /// 1) 行首退格不把这类块并回上一块（见 `view` 的「行首退格」分支）；
    /// 2) 正文删空仍写回空围栏，不写空源码（见 `view` 的「普通输入写回」分支）。
    /// 退出办法：空块连按两次回车、块头「删除」按钮，或拖选后删除。
    pub(super) fn is_fenced(&self) -> bool {
        matches!(
            self,
            EditKind::Code { .. } | EditKind::Diagram(_) | EditKind::CodeGroup { .. }
        )
    }

    pub(super) fn of(b: &Block) -> Self {
        match b {
            Block::Heading { level, .. } => EditKind::Heading(*level),
            Block::Code { lang, quote, .. } => EditKind::Code {
                lang: lang.clone(),
                quote: *quote,
            },
            Block::Diagram { .. } => EditKind::Diagram("mermaid".to_string()),
            Block::Quote { level, .. } => EditKind::Quote { level: *level },
            Block::ListItem { item, .. } => EditKind::List {
                indent: item.indent,
                marker: item.marker.clone(),
                checked: item.checked,
            },
            _ => EditKind::Source,
        }
    }

    /// 把编辑文本还原成 Markdown 源码。
    pub(super) fn rebuild(&self, text: &str) -> String {
        match self {
            EditKind::Source => text.to_string(),
            EditKind::Heading(level) => {
                format!(
                    "{} {}",
                    "#".repeat((*level).max(1) as usize),
                    text.trim_end()
                )
            }
            // 图表块与代码块同构：都补回 ```lang 围栏，往返一致。
            EditKind::Diagram(lang) => {
                let mut s = String::with_capacity(text.len() + lang.len() + 8);
                s.push_str("```");
                s.push_str(lang);
                s.push('\n');
                s.push_str(text);
                // 闭合围栏前总是补一个换行。解析器把 body 行用 \n 拼成 code
                //（不带末尾换行），所以缓冲末尾的 \n 表示「末尾有一个空行」，
                // 必须多补一个 \n 才能往返：缓冲 "abc\n" → 源码 "```lang\nabc\n\n```"
                // → 解析回 code "abc\n"。若只在不以 \n 结尾时补，回车产生的末尾
                // 空行会在写回后丢失。
                s.push('\n');
                s.push_str("```");
                s
            }
            EditKind::CodeGroup { items, tab } => {
                // 只把当前标签的代码替换进标签列表，其余语言的代码原样保留。
                let mut items = items.clone();
                if let Some(it) = items.get_mut(*tab) {
                    it.code = text.to_string();
                }
                crate::ast::code_group_source(&items)
            }
            EditKind::Code { lang, quote } => {
                let mut s = String::with_capacity(text.len() + lang.len() + 16);
                if *quote == 0 {
                    s.push_str("```");
                    s.push_str(lang);
                    s.push('\n');
                    s.push_str(text);
                    s.push('\n');
                    s.push_str("```");
                    return s;
                }
                // 引用内的代码块：围栏和每一行正文都要带引用前缀，
                // 否则写回后代码块会「跑出」引用（解析器按行首 `>` 判断归属）。
                let p = ">".repeat(*quote);
                s.push_str(&p);
                s.push_str(" ```");
                s.push_str(lang);
                s.push('\n');
                for line in text.split('\n') {
                    s.push_str(&p);
                    if !line.is_empty() {
                        s.push(' ');
                        s.push_str(line);
                    }
                    s.push('\n');
                }
                s.push_str(&p);
                s.push_str(" ```");
                s
            }
            EditKind::List { indent, marker, .. } => {
                // 任务项的 marker 本身就是 "- [ ]"（解析器把勾选框并入 marker），
                // 普通项是 "-" / "1."，勾选状态变化由 ToggleTask 直接改源码，
                // 这里只补回缩进、标记与一个空格。
                let mut s = " ".repeat(*indent);
                s.push_str(marker);
                s.push(' ');
                s.push_str(text);
                s
            }
            EditKind::Quote { level } => {
                // 引用层级用连续 `>` 表示，解析器只认行首连续 `>`。
                format!("{} {}", ">".repeat((*level).max(1)), text.trim_end())
            }
        }
    }
}

/// 把当前块并回上一块：内容接在上一块末尾，光标停在拼接处。
pub(super) fn merge_into_previous_block(doc: &mut Document, state: &mut EditorState, cur: &str) {
    let active = state.active;
    if active == 0 {
        return;
    }
    // 围栏块（代码块/图表/代码组）由调用方用 `EditKind::is_fenced` 拦下，
    // 不会走到这里；代码组尤其危险：并块会把整组围栏当正文塞进上一段，
    // 其它标签的代码也跟着变成普通文字。
    let prev_block = doc.blocks.get(active - 1);
    // 光标落点要按「上一块的可编辑文本」长度算，不是源码长度：
    // 代码块的源码含 ```lang 与闭合围栏，比代码正文长。用源码长度会把
    // 光标放到超出正文的位置，egui 钳制后就跑到代码中间行。
    let prev_buf_len = prev_block
        .map(|b| block_buffer(b).chars().count())
        .unwrap_or(0);
    let mut prev = prev_block
        .map(|b| b.source().to_string())
        .unwrap_or_default();
    prev.push_str(cur);
    doc.record();
    doc.edit_block(active - 1, &prev);
    doc.remove_block(active);
    state.active = active - 1;
    state.focus_req = Some(state.active);
    state.last_cursor = prev_buf_len + cur.chars().count();
    state.sel = None;
}

/// 引用块的编辑缓冲：去掉行首连续 `>` 与紧随的空格，保留行内 Markdown 标记，
/// 这样编辑时看不到源码标记、写回时又不会丢掉加粗/链接等格式。
pub(super) fn quote_editable_text(src: &str) -> String {
    src.trim_end()
        .trim_start_matches('>')
        .trim_start_matches(' ')
        .to_string()
}

/// 块的可编辑文本：引用只隐藏 `>`、标题/列表等只保留文字，其余块就是 Markdown 源码。
pub(super) fn block_editable_text(b: &Block) -> String {
    match EditKind::of(b) {
        EditKind::Source => b.source().to_string(),
        EditKind::Quote { .. } => quote_editable_text(b.source()),
        _ => b.plain(),
    }
}

/// 块的「可编辑文本」：标题只含文字、代码块只含代码，其余块就是 Markdown 源码。
/// 查找/替换都基于这份文本，替换后再用 rebuild_source 还原成源码写回。
pub(crate) fn block_buffer(b: &Block) -> String {
    block_editable_text(b)
}

/// 把改过的可编辑文本还原成块的 Markdown 源码（标题补 #、代码块补围栏）。
pub(crate) fn rebuild_source(b: &Block, text: &str) -> String {
    EditKind::of(b).rebuild(text)
}
