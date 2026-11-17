//! 块级格式命令：模板插入、前缀转换（标题/列表/引用）、代码块、清除格式。

use super::*;

/// 在活动块后插入一个模板块。
pub fn insert_template(doc: &mut Document, state: &mut EditorState, template: &str) {
    doc.record();
    let idx = state.active;
    doc.insert_block_after(idx, template);
    state.active = idx + 1;
    state.focus_req = Some(state.active);
    state.scroll_req = Some(state.active);
    state.scroll_top = false;
    state.last_cursor = template.chars().count();
}

/// 插入一张图片：图片块本身不留在编辑态（否则预览里显示的是 `![](...)` 源码），
/// 而是在图片后面补一个空段落并把光标移过去——图片立刻以渲染态显示。
pub fn insert_image(doc: &mut Document, state: &mut EditorState, markdown: &str) {
    doc.record();
    let idx = state.active;
    let n = crate::ast::parse(markdown).len().max(1);
    doc.insert_block_after(idx, markdown);
    doc.insert_block_after(idx + n, "");
    state.active = idx + n + 1;
    state.focus_req = Some(state.active);
    state.scroll_req = Some(state.active);
    state.scroll_top = false;
    state.last_cursor = 0;
    state.sel = None;
    state.last_sel = None;
}

/// 给活动块设置/替换行首前缀（标题、引用、列表等）。
pub fn apply_prefix(doc: &mut Document, state: &mut EditorState, prefix: &str) {
    // 引用按钮是开关：目标块已经是引用时再点一次，去掉 `>` 还原成普通段落。
    if prefix == "> " && unquote_blocks(doc, state) {
        return;
    }
    // 鼠标拖选（可跨块）：把选区覆盖的每一段都转成同一种块级格式，
    // 而不是只改「上一个活动块」——否则选好文字再点列表按钮会看起来没反应。
    if let Some(sel) = state.cross_sel.take() {
        let (s, _, e, _) = cross_sel_range(sel);
        let n = doc.blocks.len();
        if s >= n || e >= n {
            return;
        }
        let ordered = ordered_prefix(prefix);
        let mut num = ordered.map(|(n, _)| n);
        let mut first_edited: Option<usize> = None;
        let mut edits: Vec<(usize, String)> = Vec::new();
        for i in s..=e {
            let Some(b) = doc.blocks.get(i) else { continue };
            let cleaned = strip_block_prefix(b.source());
            // 选区里的空行保持原样，避免生成一串空列表项。
            if cleaned.trim().is_empty() {
                continue;
            }
            let p = match (ordered, &mut num) {
                (Some((_, tail)), Some(k)) => {
                    let out = format!("{k}{tail} ");
                    *k += 1;
                    out
                }
                _ => prefix.to_string(),
            };
            if first_edited.is_none() {
                first_edited = Some(i);
            }
            edits.push((i, prefix_block(b, &p, &cleaned)));
        }
        if edits.is_empty() {
            return;
        }
        doc.record();
        for (i, src) in edits {
            doc.edit_block(i, &src);
        }
        // 转换后把光标放到第一段末尾，后续输入/回车延续该列表。
        let first = first_edited.unwrap_or(s);
        let cursor = doc
            .blocks
            .get(first)
            .map(|b| block_buffer(b).chars().count())
            .unwrap_or(0);
        state.active = first;
        state.focus_req = Some(first);
        state.caret_req = Some((first, cursor));
        state.last_cursor = cursor;
        state.sel = None;
        state.last_sel = None;
        return;
    }

    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
    let cleaned = strip_block_prefix(b.source());
    let src_out = prefix_block(b, prefix, &cleaned);
    doc.record();
    doc.edit_block(idx, &src_out);
    // 点工具条会让编辑框失焦；改完把焦点和光标还回去，用户可以继续输入。
    let cursor = doc
        .blocks
        .get(idx)
        .map(|b| block_buffer(b).chars().count())
        .unwrap_or(0);
    state.focus_req = Some(idx);
    state.caret_req = Some((idx, cursor));
    state.sel = None;
    state.last_sel = None;
    state.last_cursor = cursor;
}

/// 给一块加上行首前缀。代码块 / 图表块用引用按钮时，围栏和正文每一行都要
/// 补 `>`：只给第一行加前缀会被解析成「空代码块 + 正文段落」，代码被拆散。
fn prefix_block(b: &Block, prefix: &str, cleaned: &str) -> String {
    // 代码组不支持塞进引用：解析器按行首 `> ``` 识别引用代码块，
    // 外层 4 个反引号会被当成普通代码块，整组被拆散。保持原样。
    if matches!(b, Block::CodeGroup { .. }) && prefix.starts_with('>') {
        return b.source().to_string();
    }
    if prefix.starts_with('>') && matches!(b, Block::Code { .. } | Block::Diagram { .. }) {
        let level = prefix.chars().take_while(|c| *c == '>').count().max(1);
        let p = ">".repeat(level);
        return b
            .source()
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    p.clone()
                } else {
                    format!("{p} {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    format!("{prefix}{cleaned}")
}

/// 插入代码块。活动块是引用时，围栏连同 `>` 前缀一起插到引用内部，
/// 代码块继续显示引用竖线，而不是把引用拆成两段。
pub fn insert_code_block(doc: &mut Document, state: &mut EditorState) {
    let level = match doc.blocks.get(state.active) {
        Some(Block::Quote { level, .. }) => *level,
        Some(Block::Code { quote, .. }) if *quote > 0 => *quote,
        _ => 0,
    };
    if level == 0 {
        insert_template(doc, state, "```\n\n```");
        // 代码块的可编辑缓冲只有正文（不含围栏），光标应落在正文开头，
        // 而不是模板源码的长度（否则光标会被钳到缓冲外）。
        state.last_cursor = 0;
        state.caret_req = Some((state.active, 0));
        state.sel = None;
        state.last_sel = None;
        return;
    }
    let p = ">".repeat(level);
    let tpl = format!("{p} ```\n{p} \n{p} ```");
    doc.record();
    let idx = state.active;
    doc.insert_block_after(idx, &tpl);
    state.active = idx + 1;
    state.focus_req = Some(state.active);
    state.caret_req = Some((state.active, 0));
    state.scroll_req = Some(state.active);
    state.scroll_top = false;
    state.last_cursor = 0;
    state.sel = None;
    state.last_sel = None;
}

/// 插入代码组：同一个代码区用 Tab 切换多种语言，默认给出 Java / Python。
pub fn insert_code_group(doc: &mut Document, state: &mut EditorState) {
    let items = vec![
        crate::ast::CodeGroupItem {
            lang: "java".to_string(),
            code: String::new(),
        },
        crate::ast::CodeGroupItem {
            lang: "python".to_string(),
            code: String::new(),
        },
    ];
    let src = crate::ast::code_group_source(&items);
    insert_template(doc, state, &src);
    // 编辑缓冲是「当前标签的代码」（不含围栏），光标应落在空代码开头。
    state.code_group_tab.insert(state.active, 0);
    state.last_cursor = 0;
    state.caret_req = Some((state.active, 0));
    state.sel = None;
    state.last_sel = None;
}

/// 引用按钮的「再点一次取消」：目标块（跨块选区覆盖的块，或当前活动块）
/// 全部是引用时，去掉行首所有 `>` 还原成普通段落，返回 true。
pub(super) fn unquote_blocks(doc: &mut Document, state: &mut EditorState) -> bool {
    let targets: Vec<usize> = match state.cross_sel {
        Some(sel) => {
            let (s, _, e, _) = cross_sel_range(sel);
            (s..=e).collect()
        }
        None => vec![state.active],
    };
    // 选区里的空行（块间空档）不参与判断，也不改。
    let targets: Vec<usize> = targets
        .into_iter()
        .filter(|&i| {
            doc.blocks
                .get(i)
                .is_some_and(|b| !b.source().trim().is_empty())
        })
        .collect();
    if targets.is_empty()
        || !targets.iter().all(|&i| {
            doc.blocks
                .get(i)
                .is_some_and(|b| quote_level_of(b).is_some())
        })
    {
        return false;
    }
    doc.record();
    for &i in &targets {
        let level = doc.blocks.get(i).and_then(quote_level_of).unwrap_or(1);
        let src = strip_quote_lines(doc.blocks[i].source(), level);
        doc.edit_block(i, &src);
    }
    let first = targets[0];
    state.cross_sel = None;
    state.active = first;
    let cursor = doc
        .blocks
        .get(first)
        .map(|b| block_buffer(b).chars().count())
        .unwrap_or(0);
    state.focus_req = Some(first);
    state.caret_req = Some((first, cursor));
    state.last_cursor = cursor;
    state.sel = None;
    state.last_sel = None;
    true
}

/// 去掉每一行开头的 `>` 前缀与紧随的一个空格（正文自身的缩进要保留）。
pub(super) fn strip_quote_lines(src: &str, level: usize) -> String {
    src.split('\n')
        .map(|line| {
            let mut s = line;
            for _ in 0..level.max(1) {
                s = s.strip_prefix('>').unwrap_or(s);
            }
            s.strip_prefix(' ').unwrap_or(s).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 块是否带引用竖线：引用段落，或写在引用里的代码块（`> ```…`）。
pub(super) fn quote_level_of(b: &Block) -> Option<usize> {
    match b {
        Block::Quote { level, .. } => Some(*level),
        Block::Code { quote, .. } if *quote > 0 => Some(*quote),
        _ => None,
    }
}

/// 去掉块级标记：#、>、-、*、+、有序列表（1. / 1)）、[ ] 任务标记、前导空格。
///
/// 关键：有序列表必须是「数字 + `.`/`)` + 空格」才算标记。正文里的章节号
/// （如「1.2 核心架构」的 1.2）点号后面是数字而非空格，必须原样保留，
/// 否则加标题会把编号剥掉。
pub(super) fn strip_block_prefix(src: &str) -> String {
    let mut s = src;
    // 循环剥离叠加的标记（如 "# > - 1. "）
    loop {
        let before = s;
        s = s.trim_start_matches(' ');
        // 标题 / 引用标记，可连续叠加
        while let Some(rest) = s.strip_prefix('#').or_else(|| s.strip_prefix('>')) {
            s = rest.trim_start_matches(' ');
        }
        // 无序列表标记：- / * / + 后跟空格
        if let Some(rest) = s
            .strip_prefix("- ")
            .or_else(|| s.strip_prefix("* "))
            .or_else(|| s.strip_prefix("+ "))
        {
            s = rest;
        }
        // 有序列表标记：数字 + '.' 或 ')' + 空格
        let bytes = s.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i > 0
            && i + 1 < bytes.len()
            && (bytes[i] == b'.' || bytes[i] == b')')
            && bytes[i + 1] == b' '
        {
            s = &s[i + 2..];
        }
        // 任务列表标记
        if let Some(rest) = s
            .strip_prefix("[ ] ")
            .or_else(|| s.strip_prefix("[x] "))
            .or_else(|| s.strip_prefix("[X] "))
        {
            s = rest;
        }
        if s == before {
            break;
        }
    }
    s.to_string()
}

/// 去掉行内强调标记：反引号全部去掉，成对的星号/下划线/波浪号去掉。
/// 单个 * 和 _ 保留，免得把 snake_case、2*3 这种正常文字也擦掉。
pub(super) fn strip_inline_markers(s: &str) -> String {
    let s = s
        .replace("<u>", "")
        .replace("</u>", "")
        .replace("<strong>", "")
        .replace("</strong>", "")
        .replace("<em>", "")
        .replace("</em>", "");
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{60}' => {}
            '*' | '_' | '~' if chars.peek() == Some(&c) => {
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// 擦除格式（WPS 橡皮擦那种用法）：把光标所在块变回普通段落，
/// 同时去掉行内强调标记。表格 / 公式 / 图表 / 分隔线 / 空白这些结构块不动，
/// 返回 false 由调用方提示。
pub fn clear_format(doc: &mut Document, state: &mut EditorState) -> bool {
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return false;
    };
    let raw = match b {
        Block::Heading { .. }
        | Block::Paragraph { .. }
        | Block::Quote { .. }
        | Block::ListItem { .. }
        | Block::Footnote { .. } => strip_block_prefix(b.source()),
        // 代码块擦成段落时保留代码文字，但不再带围栏
        Block::Code { code, .. } => code.trim_end().to_string(),
        _ => return false,
    };
    let plain = strip_inline_markers(&raw);
    doc.record();
    doc.edit_block(idx, plain.trim());
    true
}

/// 把当前块还原成普通段落：apply_prefix 的空前缀就是「去掉所有块级标记」。
pub fn make_paragraph(doc: &mut Document, state: &mut EditorState) {
    apply_prefix(doc, state, "");
}

pub fn toggle_task(doc: &mut Document, idx: usize) {
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
    let s = b.source().to_string();
    let new = if s.contains("[ ]") {
        s.replacen("[ ]", "[x]", 1)
    } else {
        s.replacen("[x]", "[ ]", 1).replacen("[X]", "[ ]", 1)
    };
    if new != s {
        doc.record();
        doc.edit_block(idx, &new);
    }
}
