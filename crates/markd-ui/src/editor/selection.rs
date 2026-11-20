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
/// 其余块（代码块、图表、表格、公式、纯图片段落…）没有可映射的字符位置，
/// 拖选时按「整块」处理。
pub(super) fn text_selection_block(b: &Block) -> bool {
    match b {
        // 纯图片段落走 image_preview，没有文字可映射，按原子块处理；
        // 图文混排的段落仍然走 text_response。
        Block::Paragraph { content, .. } => content.iter().any(|i| {
            !matches!(
                i,
                crate::ast::Inline::Image { .. } | crate::ast::Inline::Break
            )
        }),
        Block::Heading { .. }
        | Block::Quote { .. }
        | Block::ListItem { .. }
        | Block::Footnote { .. } => true,
        _ => false,
    }
}

/// 渲染态就把正文按字符排版出来的块：代码块。
///
/// 它不走 `text_selection_block`（拖选跨过它时仍按整块取端点，保证「整块选走」
/// 的老行为），但面板里的代码 galley 与 `plain()` 逐字符一致，所以**按下点落在
/// 代码区里**时可以按字符精确锚定，选中一两行而不是一整块。
pub(crate) fn line_selectable_block(b: &Block) -> bool {
    matches!(b, Block::Code { .. })
}

/// 非文本块是否被跨块选区整块覆盖：在两端之间，或作为端点。
/// 这类块（图表/表格/公式…）是原子的：只要选区碰到它，就整块计入。
/// 展开的代码块不走这层整块底色（调用方 `view.rs` 会跳过），它自己逐行画高亮。
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
        // 图表/表格/公式这类非文本块是原子的：作为两端时整块计入，
        // 不按字符位置截取（拖选时它们的端点字符只是「块首/块尾」的标记）。
        // 代码块例外：正文逐字符可映射，端点字符就是真正选到的位置，
        // 这样在代码块里拖两行就只复制那两行（跨过它时仍整块，见 view.rs）。
        let atomic = (i == s || i == e) && !text_selection_block(b) && !line_selectable_block(b);
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

// ---------------------------------------------------------------- 表格单元格选区

/// 表格选中的单元格矩形：`(块下标, 起点(行, 列), 终点(行, 列))`。
/// 行列下标含表头，与编辑态草稿一致：0 = 表头行。
pub type TableSel = (usize, (usize, usize), (usize, usize));

/// 单元格矩形归一化成「左上 — 右下」。
pub fn table_sel_rect(sel: TableSel) -> ((usize, usize), (usize, usize)) {
    let (_, (r0, c0), (r1, c1)) = sel;
    ((r0.min(r1), c0.min(c1)), (r0.max(r1), c0.max(c1)))
}

/// 表格的单元格文字网格：第 0 行是表头。非表格块返回 None。
fn table_grid(b: &Block) -> Option<Vec<Vec<String>>> {
    let Block::Table { head, rows, .. } = b else {
        return None;
    };
    let mut grid: Vec<Vec<String>> =
        vec![head.iter().map(|c| crate::ast::inline_plain(c)).collect()];
    grid.extend(
        rows.iter()
            .map(|r| r.iter().map(|c| crate::ast::inline_plain(c)).collect()),
    );
    Some(grid)
}

/// 选出矩形内的单元格文字（行列越界时钳到表内）。非表格块返回 None。
fn table_sel_rows(doc: &Document, sel: TableSel) -> Option<Vec<Vec<String>>> {
    let b = doc.blocks.get(sel.0)?;
    let grid = table_grid(b)?;
    if grid.is_empty() {
        return None;
    }
    let ((r0, c0), (r1, c1)) = table_sel_rect(sel);
    let rows = grid.len() - 1;
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let (r0, r1) = (r0.min(rows), r1.min(rows));
    let (c0, c1) = (
        c0.min(cols.saturating_sub(1)),
        c1.min(cols.saturating_sub(1)),
    );
    let out = grid[r0..=r1]
        .iter()
        .map(|row| {
            (c0..=c1)
                .map(|c| row.get(c).cloned().unwrap_or_default())
                .collect()
        })
        .collect();
    Some(out)
}

/// 单元格选区的可见文本（Cmd+C）：列之间用制表符、行之间用换行，
/// 粘到表格软件里正好是一格一个单元格。
pub fn table_sel_text(doc: &Document, state: &EditorState) -> Option<String> {
    let rows = table_sel_rows(doc, state.table_sel?)?;
    Some(
        rows.iter()
            .map(|r| r.join("\t"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 单元格选区的 Markdown 源码（右键「复制为 Markdown」）：每行还原成 `| a | b |`。
pub fn table_sel_markdown(doc: &Document, state: &EditorState) -> Option<String> {
    let rows = table_sel_rows(doc, state.table_sel?)?;
    Some(
        rows.iter()
            .map(|r| format!("| {} |", r.join(" | ")))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// 单元格选区的 HTML 表格（带格式复制，粘到富文本软件仍是表格）。
pub fn table_sel_html(doc: &Document, state: &EditorState) -> Option<String> {
    let rows = table_sel_rows(doc, state.table_sel?)?;
    let ((r0, _), _) = table_sel_rect(state.table_sel?);
    let body = rows
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let tag = if r0 == 0 && k == 0 { "th" } else { "td" };
            let cells = r
                .iter()
                .map(|c| format!("<{tag}>{}</{tag}>", crate::export::inline_markdown_html(c)))
                .collect::<Vec<_>>()
                .join("");
            format!("<tr>{cells}</tr>")
        })
        .collect::<Vec<_>>()
        .join("");
    Some(format!("<table>{body}</table>"))
}
