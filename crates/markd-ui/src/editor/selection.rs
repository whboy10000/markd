//! 跨块选区：范围计算、复制/剪切与替换编辑。

use super::*;

/// 规范化跨块选区顺序：(起块, 起字符, 止块, 止字符)。
pub(super) fn cross_sel_range(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (a, ac, b, bc) = sel;
    if (a, ac) <= (b, bc) {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    }
}

/// 渲染态用 text_response 自己画文字选区的块。
/// 其余块（代码块、图表、表格、公式…）没有可映射的字符位置，拖选时按「整块」处理。
pub(super) fn text_selection_block(b: &Block) -> bool {
    matches!(
        b,
        Block::Heading { .. }
            | Block::Paragraph { .. }
            | Block::Quote { .. }
            | Block::ListItem { .. }
            | Block::Footnote { .. }
    )
}

/// 非文本块是否被跨块选区整块覆盖：在两端之间，或作为端点。
/// 这类块（代码块/图表/表格…）是原子的：只要选区碰到它，就整块计入。
pub(super) fn atomic_block_selected(sel: Option<(usize, usize, usize, usize)>, idx: usize) -> bool {
    let Some(sel) = sel else { return false };
    let (s, sc, e, ec) = cross_sel_range(sel);
    // 只有按下、还没拖出范围：不画整块底色。
    if (s, sc) == (e, ec) {
        return false;
    }
    s <= idx && idx <= e
}

/// 跨块选区的文本（Cmd+C 复制用）：首尾块取选中片段，中间块取全文，
/// 块与块之间用空行连接（所见文本）。
pub fn cross_sel_text(doc: &Document, sel: Option<(usize, usize, usize, usize)>) -> Option<String> {
    let (s, sc, e, ec) = cross_sel_range(sel?);
    // 只有按下、还没拖出范围：视为没有选区。
    if (s, sc) == (e, ec) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for i in s..=e {
        let Some(b) = doc.blocks.get(i) else { continue };
        let plain = b.plain();
        let len = plain.chars().count();
        // 代码块/图表/表格这类非文本块是原子的：作为两端时整块计入，
        // 不按字符位置截取（拖选时它们的端点字符只是「块首/块尾」的标记）。
        let atomic = (i == s || i == e) && !text_selection_block(b);
        let t0 = if i == s && !atomic { sc.min(len) } else { 0 };
        let t1 = if i == e && !atomic { ec.min(len) } else { len };
        if t0 < t1 {
            out.push(plain.chars().skip(t0).take(t1 - t0).collect());
        }
    }
    (!out.is_empty()).then(|| out.join("\n\n"))
}

/// 剪切跨块选区：返回应写入剪贴板的文本，并把选中内容从文档里删掉。
/// 返回 None 表示当前没有跨块选区（交给 TextEdit 原生剪切）。
pub fn cut_cross_sel(doc: &mut Document, state: &mut EditorState) -> Option<String> {
    let text = cross_sel_text(doc, state.cross_sel)?;
    apply_cross_sel_edit(doc, state, None).then_some(text)
}

/// 把跨块选区替换为 `insert`（None = 纯删除）：
/// 首块保留前缀 + 插入文本 + 尾块保留后缀合并成一块（按首块类型补标记），
/// 中间块与尾块移除。返回是否发生了修改。
pub(super) fn apply_cross_sel_edit(
    doc: &mut Document,
    state: &mut EditorState,
    insert: Option<&str>,
) -> bool {
    let Some(sel) = state.cross_sel.take() else {
        return false;
    };
    let n = doc.blocks.len();
    let (s, sc, e, ec) = cross_sel_range(sel);
    if s >= n || e >= n {
        return false;
    }
    let head_plain = doc.blocks[s].plain();
    let tail_plain = doc.blocks[e].plain();
    let sc = sc.min(head_plain.chars().count());
    let ec = ec.min(tail_plain.chars().count());
    let head_keep: String = head_plain.chars().take(sc).collect();
    let tail_keep: String = tail_plain.chars().skip(ec).collect();
    let merged = format!("{head_keep}{}{tail_keep}", insert.unwrap_or(""));

    doc.record();
    // 按首块的编辑类型补回标记（标题的 #、列表的 - 等）
    let ek = EditKind::of(&doc.blocks[s]);
    doc.edit_block(s, &ek.rebuild(&merged));
    // 中间块与尾块倒序移除（索引不失效）
    for i in (s + 1..=e.min(n - 1)).rev() {
        if i < doc.blocks.len() {
            doc.remove_block(i);
        }
    }
    // 光标落到插入文本之后
    state.active = s;
    state.focus_req = Some(s);
    state.last_cursor = head_keep.chars().count() + insert.map(|t| t.chars().count()).unwrap_or(0);
    state.sel = None;
    true
}
