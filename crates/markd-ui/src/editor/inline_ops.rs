//! 行内样式包裹与链接插入。

use super::*;

/// 用标记包裹内容（加粗 / 斜体 / 删除线 / 行内代码 / 链接共用）：
/// - 有选区（含工具条点击时记住的选区）→ 包裹选中的文字；
/// - 无选区且 `whole_block` → 整个块（光标所在行）加标记；
/// - 无选区且 !`whole_block` → 在光标处插入一对空标记（链接的用法）。
/// 选区或整块已经带着同样标记时再点一次 = 取消（WPS 式开关）。
/// 始终基于「可编辑文本」操作（标题不含 #、代码块不含围栏），再还原成源码写回，
/// 这样标记的字节偏移和屏幕上的字符位置才能对得上。
pub fn wrap(
    doc: &mut Document,
    state: &mut EditorState,
    open: &str,
    close: &str,
    whole_block: bool,
) {
    // 鼠标拖选的跨块选区优先：只包裹选中的文字，绝不动光标所在的活动块
    // （两者往往不是同一行）。
    if state.cross_sel.is_some() {
        wrap_cross_sel(doc, state, open, close);
        return;
    }
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
    // 代码组编辑的是单个标签的代码，选区下标与整份源码不对应；
    // 行内标记对代码也没有意义，直接忽略。
    if matches!(b, Block::CodeGroup { .. }) {
        return;
    }
    let text = block_buffer(b);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len == 0 && whole_block {
        return; // 空行没有可加粗的内容
    }

    let open_c = open.chars().count();
    let close_c = close.chars().count();
    let sel_opt = state.sel.or_else(|| {
        state
            .last_sel
            .filter(|(b, _, _)| *b == idx)
            .map(|(_, a, z)| (a, z))
    });
    let (a, z) = sel_opt.unwrap_or_else(|| {
        if whole_block {
            (0, len)
        } else {
            let p = state.last_cursor.min(len);
            (p, p)
        }
    });
    let a = a.min(len);
    let z = z.min(len);

    // 已经带着同样标记 → 这次点击是取消
    let already = z > a
        && chars[a..z].starts_with(&open.chars().collect::<Vec<_>>())
        && chars[a..z].ends_with(&close.chars().collect::<Vec<_>>());
    let whole_already = a == 0
        && z == len
        && chars.starts_with(&open.chars().collect::<Vec<_>>())
        && chars.ends_with(&close.chars().collect::<Vec<_>>());

    let out = if (sel_opt.is_some() && already) || (sel_opt.is_none() && whole_already) {
        let mut t: String = chars[..a].iter().collect();
        t.extend(&chars[a + open_c..z - close_c]);
        t.extend(&chars[z..]);
        t
    } else {
        let mut t: String = chars[..a].iter().collect();
        t.push_str(open);
        t.extend(&chars[a..z]);
        t.push_str(close);
        t.extend(&chars[z..]);
        t
    };
    // 标题等块的可编辑文本不含块级标记：还原成源码时由 rebuild_source 补回，
    // 否则加粗标题会把 # 一起丢掉。
    let src_out = rebuild_source(b, &out);
    if out == text {
        return;
    }
    doc.record();
    doc.edit_block(idx, &src_out);
    // 选区已经用掉：清掉记录，并要求把光标收敛成单点（见 caret_req），
    // 不然文本框里存的旧选区会让刚加粗/斜体过的文字一直保持选中。
    state.sel = None;
    state.last_sel = None;
    state.last_cursor = (a + open_c).min(out.chars().count());
    state.caret_req = Some((idx, state.last_cursor));
}

/// 给「鼠标拖选的跨块选区」包裹行内标记：只对选中的文字加标记，
/// 覆盖的中间块整块加，光标所在的活动块一律不碰。
///
/// 选区下标基于所见文本（plain），用它切出选中片段的典型文本，再在
/// 可编辑文本（buf，含 **、` 等标记）中按内容定位——这样已加粗的文字
/// 再次点击时能识别外围已有的标记并正确取消。
pub(super) fn wrap_cross_sel(doc: &mut Document, state: &mut EditorState, open: &str, close: &str) {
    let Some(sel) = state.cross_sel.take() else {
        return;
    };
    let (s, sc, e, ec) = cross_sel_range(sel);
    if s >= doc.blocks.len() || e >= doc.blocks.len() {
        return;
    }
    let open_c = open.chars().count();
    let close_c = close.chars().count();

    let mut edits: Vec<(usize, String)> = Vec::new();
    let mut first_caret: Option<usize> = None;
    for i in s..=e {
        let Some(b) = doc.blocks.get(i) else { continue };
        let buf = block_buffer(b);
        let plain = b.plain();
        let plain_chars: Vec<char> = plain.chars().collect();
        let plen = plain_chars.len();
        let pt0 = if i == s { sc.min(plen) } else { 0 };
        let pt1 = if i == e { ec.min(plen) } else { plen };
        if pt0 >= pt1 {
            continue;
        }
        // 用选中文字的内容在 buf 中定位（标记夹在文字之间，按下标映射会错位）
        let seg: String = plain_chars[pt0..pt1].iter().collect();
        let seg_chars: Vec<char> = seg.chars().collect();
        let buf_chars: Vec<char> = buf.chars().collect();
        let b0 = (0..=buf_chars.len().saturating_sub(seg_chars.len()))
            .find(|&k| buf_chars[k..k + seg_chars.len()] == seg_chars[..]);
        let Some(b0) = b0 else { continue };
        let b1 = b0 + seg_chars.len();
        // 选中片段外围已带同样标记 → 这次是取消
        let outer_open =
            b0 >= open_c && buf_chars[b0 - open_c..b0].iter().collect::<String>() == open;
        let outer_close = b1 + close_c <= buf_chars.len()
            && buf_chars[b1..b1 + close_c].iter().collect::<String>() == close;
        let out = if outer_open && outer_close {
            let mut t: String = buf_chars[..b0 - open_c].iter().collect();
            t.extend(&buf_chars[b0..b1]);
            t.extend(&buf_chars[b1 + close_c..]);
            t
        } else {
            let mut t: String = buf_chars[..b0].iter().collect();
            t.push_str(open);
            t.extend(&buf_chars[b0..b1]);
            t.push_str(close);
            t.extend(&buf_chars[b1..]);
            t
        };
        let src_out = rebuild_source(b, &out);
        if first_caret.is_none() {
            first_caret = Some(b0 + open_c);
        }
        edits.push((i, src_out));
    }
    if edits.is_empty() {
        return;
    }
    doc.record();
    for (i, src) in edits {
        doc.edit_block(i, &src);
    }
    state.last_sel = None;
    if let Some(c) = first_caret {
        state.caret_req = Some((s, c));
    }
}

/// 当前选区（块下标, 起始, 结束, 选中文字）。
///
/// 点工具条按钮或打开弹窗时编辑框已经失焦，实时选区 `sel` 可能被清空，
/// 所以要回退到 `last_sel`（最近一次有效选区）。选区为空或块已被删除时返回 None。
pub fn selection_text(
    doc: &Document,
    state: &EditorState,
) -> Option<(usize, usize, usize, String)> {
    // 表格里拖选了单元格时，活动块的编辑框已经让位（焦点交出、`sel` 可能是
    // 上一次留下的旧选区）：此时不能把那段旧选区当作用户的选区来剪切/加粗，
    // 否则会改到用户根本没碰的文字。
    if state.table_sel.is_some() {
        return None;
    }
    let (idx, a, z) = match state.sel {
        Some((a, z)) => (state.active, a, z),
        None => state
            .last_sel
            .filter(|(b, _, _)| *b == state.active)
            .map(|(b, a, z)| (b, a, z))?,
    };
    let b = doc.blocks.get(idx)?;
    let chars: Vec<char> = block_buffer(b).chars().collect();
    let (a, z) = (a.min(chars.len()), z.min(chars.len()));
    if a >= z {
        return None;
    }
    Some((idx, a, z, chars[a..z].iter().collect()))
}

/// 把一个链接插进指定块：`[label](url)`。
///
/// - `target` 是打开弹窗前记住的选区；确认时直接按它写回，不依赖编辑框还持有焦点。
/// - 没有选区时插入到光标处；label 为空则用 URL 当显示文字，避免出现看不见的链接。
pub fn insert_link(
    doc: &mut Document,
    state: &mut EditorState,
    target: Option<(usize, usize, usize)>,
    label: &str,
    url: &str,
) -> bool {
    let url = url.trim();
    if url.is_empty() {
        return false;
    }
    let idx = target.map(|(i, _, _)| i).unwrap_or(state.active);
    let Some(b) = doc.blocks.get(idx) else {
        return false;
    };
    // 代码块的链接没有意义，且代码组会破坏围栏结构。
    if matches!(b, Block::CodeGroup { .. }) {
        return false;
    }
    let text = block_buffer(b);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let (a, z) = target
        .map(|(_, a, z)| (a.min(len), z.min(len)))
        .unwrap_or_else(|| {
            let p = state.last_cursor.min(len);
            (p, p)
        });
    let (a, z) = if a <= z { (a, z) } else { (z, a) };

    // 标签里的方括号会截断 Markdown 链接语法；换行也会破坏单行链接。
    let mut label = label.trim().replace('\n', " ").replace('\r', " ");
    if label.is_empty() {
        label = url.to_string();
    }
    label = label.replace('[', "［").replace(']', "］");
    // 空格和右括号会提前结束 `(url)`，编码掉而不是让解析器截断链接。
    let url = url.replace(' ', "%20").replace(')', "%29");

    let mut out: String = chars[..a].iter().collect();
    out.push('[');
    out.push_str(&label);
    out.push_str("](");
    out.push_str(&url);
    out.push(')');
    out.extend(&chars[z..]);

    let src_out = rebuild_source(b, &out);
    doc.record();
    doc.edit_block(idx, &src_out);
    state.sel = None;
    state.last_sel = None;
    state.last_cursor =
        (a + label.chars().count() + url.chars().count() + 4).min(out.chars().count());
    state.caret_req = Some((idx, state.last_cursor));
    state.focus_req = Some(idx);
    true
}

/// 把文档里旧图片地址替换成新的公网地址（图床上传完成后用）。
/// 只做一次替换：找到第一个包含旧地址的块，写完返回 true。
pub fn replace_image_url(doc: &mut Document, old: &str, new: &str) -> bool {
    if old == new {
        return false;
    }
    for i in 0..doc.blocks.len() {
        let src = doc.blocks[i].source().to_string();
        if src.contains(old) {
            doc.record();
            doc.edit_block(i, &src.replace(old, new));
            return true;
        }
    }
    false
}
