//! Author: aurelianus <aurelianus@126.com>

//! Hybrid View 编辑器：活动块显示源码，其余块实时渲染成富文本。

use crate::ast::Block;
use crate::document::{Document, OutlineItem};
use crate::render::{self, Action, ImageCache, RenderEnv};
use crate::theme::Theme;
use egui::{self, Id, Key, Sense, TextEdit};
use std::path::Path;

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
    /// 点击进入编辑时，光标应落在的位置（相对块左上角）。
    pub pending_caret: Option<egui::Vec2>,
    /// 查找/替换跳转要求选中的文字：(块下标, 起始字符, 结束字符)，用后清零。
    pub select_req: Option<(usize, usize, usize)>,
    /// 格式按钮用完选区后把光标收敛成单点：(块下标, 字符下标)，用后清零。
    /// 不然文本框里存的旧选区会让刚处理过的文字一直保持选中状态。
    pub caret_req: Option<(usize, usize)>,
    /// 缩进空格数（设置里可调）。
    pub tab_size: usize,
    /// 是否自动补全括号/引号。
    pub auto_pair: bool,
}

pub enum EditorEvent {
    None,
    Changed,
}

fn split_at_char(s: &str, pos: usize) -> (String, String) {
    let mut a = String::new();
    let mut b = String::new();
    for (i, c) in s.chars().enumerate() {
        if i < pos {
            a.push(c);
        } else {
            b.push(c);
        }
    }
    (a, b)
}

fn insert_at_char(s: &str, pos: usize, ins: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == pos {
            out.push_str(ins);
        }
        out.push(c);
    }
    if pos >= s.chars().count() {
        out.push_str(ins);
    }
    out
}

fn inherited_marker(s: &str) -> String {
    let indent: String = s.chars().take_while(|c| *c == ' ').collect();
    let rest = &s[indent.len()..];
    let b = rest.as_bytes();
    if b.is_empty() {
        return String::new();
    }
    if (b[0] == b'-' || b[0] == b'*' || b[0] == b'+') && b.get(1) == Some(&b' ') {
        let after = &rest[2..];
        if after.starts_with("[ ] ") || after.starts_with("[x] ") || after.starts_with("[X] ") {
            return format!("{}- [ ] ", indent);
        }
        return format!("{}- ", indent);
    }
    let mut k = 0usize;
    while k < b.len() && b[k].is_ascii_digit() {
        k += 1;
    }
    if k > 0 && k + 1 < b.len() && (b[k] == b'.' || b[k] == b')') && b[k + 1] == b' ' {
        return format!("{}1. ", indent);
    }
    String::new()
}

fn pair_close(t: &str) -> Option<&'static str> {
    match t {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        "\"" => Some("\""),
        "'" => Some("'"),
        _ => None,
    }
}

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

/// 给活动块设置/替换行首前缀（标题、引用、列表等）。
pub fn apply_prefix(doc: &mut Document, state: &mut EditorState, prefix: &str) {
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
    let cleaned = strip_block_prefix(b.source());
    doc.record();
    doc.edit_block(idx, &format!("{}{}", prefix, cleaned));
    state.last_cursor = (prefix.chars().count() + cleaned.chars().count()).min(usize::MAX);
}

/// 去掉块级标记：#、>、-、*、1.、[ ] 以及前导空格。
fn strip_block_prefix(src: &str) -> String {
    src.trim_start_matches('#')
        .trim_start_matches('>')
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
        .trim_start_matches("[ ] ")
        .trim_start_matches("[x] ")
        .trim_start_matches("[X] ")
        .to_string()
}

/// 去掉行内强调标记：反引号全部去掉，成对的星号/下划线/波浪号去掉。
/// 单个 * 和 _ 保留，免得把 snake_case、2*3 这种正常文字也擦掉。
fn strip_inline_markers(s: &str) -> String {
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
    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
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

/// 当前选区（块下标, 起始, 结束, 选中文字）。
///
/// 点工具条按钮或打开弹窗时编辑框已经失焦，实时选区 `sel` 可能被清空，
/// 所以要回退到 `last_sel`（最近一次有效选区）。选区为空或块已被删除时返回 None。
pub fn selection_text(
    doc: &Document,
    state: &EditorState,
) -> Option<(usize, usize, usize, String)> {
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

/// 由单元格内容还原出 Markdown 表格源码。
fn table_source(grid: &[Vec<String>], align: &[crate::ast::Align]) -> String {
    use crate::ast::Align;
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut s = String::new();
    let row = |cells: &Vec<String>| {
        let mut line = String::from("|");
        for c in 0..cols {
            line.push(' ');
            line.push_str(cells.get(c).map(|x| x.as_str()).unwrap_or(""));
            line.push_str(" |");
        }
        line
    };
    if let Some(head) = grid.first() {
        s.push_str(&row(head));
        s.push('\n');
    }
    s.push('|');
    for c in 0..cols {
        s.push_str(match align.get(c).copied().unwrap_or(Align::Left) {
            Align::Left => " :--- |",
            Align::Center => " :---: |",
            Align::Right => " ---: |",
        });
    }
    for r in grid.iter().skip(1) {
        s.push('\n');
        s.push_str(&row(r));
    }
    s
}

/// 活动块的「编辑形态」：标题只编辑文字、代码块只编辑代码（不含围栏），
/// 像 Typora 那样直接在渲染结果上改，而不是切到 Markdown 源码。
#[derive(Clone)]
enum EditKind {
    Source,
    Heading(u8),
    Code(String),
    /// 图表块（mermaid 等）：编辑时隐藏 ``` 围栏，只编辑图形描述代码。
    Diagram(String),
}

impl EditKind {
    fn of(b: &Block) -> Self {
        match b {
            Block::Heading { level, .. } => EditKind::Heading(*level),
            Block::Code { lang, .. } => EditKind::Code(lang.clone()),
            Block::Diagram { .. } => EditKind::Diagram("mermaid".to_string()),
            _ => EditKind::Source,
        }
    }

    /// 把编辑文本还原成 Markdown 源码。
    fn rebuild(&self, text: &str) -> String {
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
            EditKind::Code(lang) | EditKind::Diagram(lang) => {
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
        }
    }
}

/// 把当前块并回上一块：内容接在上一块末尾，光标停在拼接处。
fn merge_into_previous_block(doc: &mut Document, state: &mut EditorState, cur: &str) {
    let active = state.active;
    if active == 0 {
        return;
    }
    let mut prev = doc
        .blocks
        .get(active - 1)
        .map(|b| b.source().to_string())
        .unwrap_or_default();
    prev.push_str(cur);
    doc.record();
    doc.edit_block(active - 1, &prev);
    doc.remove_block(active);
    state.active = active - 1;
    state.focus_req = Some(state.active);
    state.last_cursor = prev.chars().count().saturating_sub(cur.chars().count());
    state.sel = None;
}

/// 块的「可编辑文本」：标题只含文字、代码块只含代码，其余块就是 Markdown 源码。
/// 查找/替换都基于这份文本，替换后再用 rebuild_source 还原成源码写回。
pub(crate) fn block_buffer(b: &Block) -> String {
    match EditKind::of(b) {
        EditKind::Source => b.source().to_string(),
        _ => b.plain(),
    }
}

/// 把改过的可编辑文本还原成块的 Markdown 源码（标题补 #、代码块补围栏）。
pub(crate) fn rebuild_source(b: &Block, text: &str) -> String {
    EditKind::of(b).rebuild(text)
}

/// 滚动条所在的那条窄带：点这里应该交给滚动条，不能当成空白点击。
fn scroll_bar_zone(ui: &egui::Ui, inner_rect: egui::Rect, content_height: f32) -> egui::Rect {
    if content_height > inner_rect.height() {
        let w = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
        egui::Rect::from_min_max(
            egui::pos2(inner_rect.right() - w - 2.0, inner_rect.top()),
            egui::pos2(inner_rect.right() + 8.0, inner_rect.bottom()),
        )
    } else {
        egui::Rect::NOTHING
    }
}

/// 空白点击调试：`MARKD_TRACE_BLANK=1 cargo run` 时打印判定结果。
fn trace_blank(pos: &Option<egui::Pos2>, msg: &str) {
    if std::env::var("MARKD_TRACE_BLANK").is_ok() {
        eprintln!("[blank] pointer={pos:?} {msg}");
    }
}

#[allow(clippy::too_many_arguments, deprecated)]
pub fn show_editor(
    ui: &mut egui::Ui,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    ctx: &egui::Context,
    images: &mut ImageCache,
    base_dir: Option<&Path>,
    outline: &[OutlineItem],
    source_mode: bool,
    focus_mode: bool,
    typewriter: bool,
    // 标签唯一编号：拼进 TextEdit 的 Id，光标/选中状态才不会被别的标签复用。
    tab_key: u64,
    actions: &mut Vec<Action>,
) -> EditorEvent {
    // ---------------- 源码模式：整篇文档一个多行编辑器 ----------------
    if source_mode {
        let mut src = doc.source();
        // TextEdit 会随内容自动撑高，直接放在父 Ui 上会被裁剪、滚轮无处生效，
        // 表现就是源码模式下鼠标滚不动；必须像下方混合模式一样包一层 ScrollArea。
        let mut changed = false;
        let mut editor_rect = egui::Rect::NOTHING;
        let scroll = egui::ScrollArea::vertical()
            .id_salt("source_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let out = TextEdit::multiline(&mut src)
                    .id(Id::new(("source_editor", tab_key)))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(ui.available_width())
                    .frame(egui::Frame::NONE)
                    .code_editor()
                    .show(ui);
                changed = out.response.changed();
                editor_rect = out.response.rect;
            });
        // 源码模式同样支持「点空白落光标」：点在编辑框外（页边距/正文下方）时，
        // 把光标移到文档末尾（最后一行）并立刻聚焦，行为与混合模式一致。
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let clicked = ctx.input(|i| i.pointer.primary_clicked());
        let bar_zone = scroll_bar_zone(ui, scroll.inner_rect, scroll.content_size.y);
        let blank = pointer.filter(|p| {
            clicked
                && ui.max_rect().contains(*p)
                && !bar_zone.contains(*p)
                && !editor_rect.contains(*p)
                && ctx
                    .layer_id_at(*p)
                    .is_none_or(|layer| layer == ui.layer_id())
        });
        if let Some(p) = blank {
            let id = Id::new(("source_editor", tab_key));
            if let Some(mut ts) = egui::TextEdit::load_state(ui.ctx(), id) {
                ts.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(src.chars().count()),
                )));
                ts.store(ui.ctx(), id);
            }
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
            trace_blank(&pointer, &format!("source -> 文档末尾 (点在 {p:?})"));
        } else {
            trace_blank(&pointer, "source miss");
        }
        if changed {
            doc.maybe_record();
            doc.set_source(&src);
            return EditorEvent::Changed;
        }
        return EditorEvent::None;
    }

    // 编辑区空白处也要能落光标。先登记一个底层热区作为兜底；真正判定用
    // 本帧指针位置 + 块矩形（见 ScrollArea 之后）——不依赖哪个控件抢到这次
    // 点击，所以即使 ScrollArea/滚动条吃掉了事件，空白点击也不会丢。
    let bg = ui.interact(
        ui.max_rect(),
        Id::new(("editor_blank", tab_key)),
        Sense::click(),
    );

    let active = state.active.min(doc.blocks.len().saturating_sub(1));
    state.active = active;
    let active_multiline = doc
        .blocks
        .get(active)
        .map(|b| b.is_multiline())
        .unwrap_or(false);

    let key_state = ctx.input(|i| {
        (
            i.key_pressed(Key::Enter) && !i.modifiers.command && !i.modifiers.ctrl,
            i.key_pressed(Key::Backspace),
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Tab),
            i.modifiers.shift,
        )
    });
    let (enter, backspace, up, down, tab, shift) = key_state;
    let typed_pair: Option<String> = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Text(t) => pair_close(t).map(|s| s.to_string()),
            _ => None,
        })
    });

    // 只维护「活动块」的可编辑缓冲；其它块直接借用 doc.blocks。
    // 之前每帧把所有块的源码 clone 一遍，10 万字时要分配上千个 String。
    let edit_kind = doc
        .blocks
        .get(active)
        .map(EditKind::of)
        .unwrap_or(EditKind::Source);
    let mut active_buf = match doc.blocks.get(active) {
        Some(b) if !matches!(edit_kind, EditKind::Source) => b.plain(),
        Some(b) => b.source().to_string(),
        None => String::new(),
    };
    // 空块行首退格要并回上一块：这个判断必须基于「按键之前」的缓冲区，
    // 否则删掉最后一个字符的那一帧就会被误判成空块。
    let buf_was_empty = active_buf.is_empty();
    // 按键前的完整缓冲快照：TextEdit 会原地改写 active_buf，「末尾空行再按回车
    // 退出代码块」需要比对「本帧的改动是否恰好是在末尾追加了一个换行」，
    // 以排除同帧既打字又回车的误判。
    let pre_buf = active_buf.clone();
    let buf_ended_nl = pre_buf.ends_with('\n');
    let mut changed = false;
    let mut new_active: Option<usize> = None;
    let mut active_rect: Option<egui::Rect> = None;
    let mut cursor_idx = active_buf.chars().count();
    let mut selection: Option<(usize, usize)> = None;
    let mut pair_insert: Option<(usize, String)> = None;
    let mut table_edit_src: Option<String> = None;
    // 代码块编辑态里更换语言时置位，帧末统一处理
    let mut lang_change: Option<String> = None;
    // 本帧每个块实际占用的矩形：(块下标, 矩形)。空白点击靠它区分「点在块上」和「点在留白」。
    let mut block_rects: Vec<(usize, egui::Rect)> = Vec::new();

    let scroll_out = egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let blocks = &doc.blocks;
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            ui.horizontal_top(|ui| {
                let pad = ((outer_w - page_w) / 2.0).max(0.0);
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(page_w);
                    for idx in 0..blocks.len() {
                        let block = &blocks[idx];
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        let is_active = idx == active;
                        let editing = is_active && !state.read_only;

                        let resp = if editing && matches!(block, Block::Table { .. }) {
                            // 表格：像 Typora 一样直接在表格单元格里编辑，不看管道源码。
                            if let Block::Table {
                                head, rows, align, ..
                            } = block
                            {
                                let need =
                                    !matches!(&state.table_draft, Some((i, _, _)) if *i == idx);
                                if need {
                                    let mut grid: Vec<Vec<String>> = Vec::new();
                                    grid.push(
                                        head.iter().map(|c| crate::ast::inline_plain(c)).collect(),
                                    );
                                    for r in rows.iter() {
                                        grid.push(
                                            r.iter().map(|c| crate::ast::inline_plain(c)).collect(),
                                        );
                                    }
                                    state.table_draft = Some((idx, grid, align.clone()));
                                    // 首次进入编辑：默认聚焦第一个单元格。若渲染态刚
                                    // 报上来 TableCellClick，app 已把 table_focus 设为
                                    // 点击的格子，这里不要覆盖它。
                                    if state.table_focus.is_none() {
                                        state.table_focus = Some((0, 0));
                                    }
                                }
                            }
                            let mut edited = false;
                            let frame = egui::Frame::NONE
                                .fill(theme.bg)
                                .stroke(egui::Stroke::new(1.0, theme.table_line))
                                .corner_radius(4.0)
                                .inner_margin(6.0);
                            let inner = frame.show(ui, |ui| {
                                if let Some((_, grid, _)) = state.table_draft.as_mut() {
                                    // 列宽与渲染态保持一致：平分可用宽度。
                                    // 以前单元格固定 140px，一点进编辑整张表就缩水成一小块。
                                    let cols = grid.first().map(|r| r.len()).unwrap_or(1).max(1);
                                    let gap =
                                        ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                                    let col_w = ((ui.available_width() - gap).max(120.0)
                                        / cols as f32)
                                        .max(56.0);
                                    ui.set_min_width(col_w * cols as f32 + gap);
                                    // 表头行各单元格的矩形：Grid 之后用标题色把间隙补满
                                    let mut head_rects: Vec<egui::Rect> = Vec::new();
                                    egui::Grid::new(("table_edit", idx))
                                        .min_col_width(col_w)
                                        .max_col_width(col_w)
                                        .show(ui, |ui| {
                                            for (ri, row) in grid.iter_mut().enumerate() {
                                                for (ci, cell) in row.iter_mut().enumerate() {
                                                    // 编辑态与渲染态同一套配色与字号（body*0.95）：
                                                    // 行高一致，点击进入编辑时表格才不会跳。
                                                    let (fill, color) = if ri == 0 {
                                                        (theme.table_head, theme.heading)
                                                    } else {
                                                        (theme.bg, theme.text)
                                                    };
                                                    let r = ui.add(
                                                        egui::TextEdit::singleline(cell)
                                                            .desired_width(col_w)
                                                            .font(egui::FontId::new(
                                                                theme.body * 0.95,
                                                                egui::FontFamily::Proportional,
                                                            ))
                                                            .text_color(color)
                                                            .frame(
                                                                egui::Frame::NONE
                                                                    .fill(fill)
                                                                    .inner_margin(3.0),
                                                            ),
                                                    );
                                                    if ri == 0 {
                                                        head_rects.push(r.rect);
                                                    }
                                                    // 渲染态点击了哪个单元格，这里就聚焦哪一格，
                                                    // 光标落在点击位置附近（右半边 → 文字末尾）。
                                                    if state.table_focus == Some((ri, ci)) {
                                                        state.table_focus = None;
                                                        let mut ts = egui::TextEdit::load_state(
                                                            ui.ctx(),
                                                            r.id,
                                                        )
                                                        .unwrap_or_default();
                                                        let pos = if state.table_caret_end {
                                                            cell.chars().count()
                                                        } else {
                                                            0
                                                        };
                                                        ts.cursor.set_char_range(Some(
                                                            egui::text::CCursorRange::one(
                                                                egui::text::CCursor::new(pos),
                                                            ),
                                                        ));
                                                        ts.store(ui.ctx(), r.id);
                                                        r.request_focus();
                                                    }
                                                    if r.changed() {
                                                        edited = true;
                                                    }
                                                }
                                                ui.end_row();
                                            }
                                        });
                                    // 与渲染态一致：标题行底色铺满整行，盖住单元格之间的缝隙
                                    if head_rects.len() == cols {
                                        let top = head_rects
                                            .iter()
                                            .map(|x| x.top())
                                            .fold(f32::MAX, f32::min);
                                        let bottom = head_rects
                                            .iter()
                                            .map(|x| x.bottom())
                                            .fold(f32::MIN, f32::max);
                                        let p = ui.painter();
                                        for k in 1..cols {
                                            let left = head_rects[k - 1].right();
                                            let right = head_rects[k].left();
                                            if right > left {
                                                p.rect_filled(
                                                    egui::Rect::from_min_max(
                                                        egui::Pos2::new(left, top),
                                                        egui::Pos2::new(right, bottom),
                                                    ),
                                                    0.0,
                                                    theme.table_head,
                                                );
                                            }
                                        }
                                    }
                                }
                            });
                            if edited {
                                if let Some((_, grid, aligns)) = state.table_draft.as_ref() {
                                    table_edit_src = Some(table_source(grid, aligns));
                                }
                                changed = true;
                            }
                            // 编辑态不要给整块再盖一层点击感应：那层会挡住单元格 TextEdit
                            // 的点击聚焦，表现为「点了没反应」。单元格自己处理点击。
                            inner.response
                        } else if editing {
                            // 代码块编辑态也保留工具行：复制 + 语言选择（可搜索、A→Z），
                            // 和渲染态完全一致，点进去不会失去这两样能力。
                            if let EditKind::Code(lang) = &edit_kind {
                                let mut copied = false;
                                ui.horizontal(|ui| {
                                    // 左上角语言文字标签已移除：右上角语言下拉已显示当前语言。
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .button(egui::RichText::new("复制").small())
                                                .clicked()
                                            {
                                                copied = true;
                                            }
                                            if let Some(l) = render::language_combo(
                                                ui,
                                                ("code_lang_edit", tab_key, idx),
                                                lang,
                                            ) {
                                                lang_change = Some(l);
                                            }
                                        },
                                    );
                                });
                                if copied {
                                    ctx.copy_text(active_buf.clone());
                                    actions.push(Action::Notify("复制成功".to_string()));
                                }
                                // 与渲染态对齐：工具行和代码区之间留同样的 3px 空隙，
                                // 否则点击进入编辑时这一行会差 3px，整块往下跳一下。
                                ui.add_space(3.0);
                            }
                            let id = Id::new(("block_edit", tab_key, idx));
                            // 标题只编辑文字、代码块只编辑代码（不含围栏），像 Typora 一样直接改。
                            // 代码块的行数 + 1 对齐渲染态：渲染态的 code_job 会在末尾
                            // 补一个换行（多一行视觉空行），编辑态用 desired_rows 预留
                            // 同样高度，点击进入/退出编辑时内容不跳。
                            let rows = if matches!(edit_kind, EditKind::Code(_)) {
                                active_buf.lines().count().max(1) + 1
                            } else if block.is_multiline() {
                                2
                            } else {
                                1
                            };
                            // 编辑态行内排版：缓冲区还是原始 Markdown，但标记被隐藏，
                            // 看起来就是渲染后的富文本，而不是源码。
                            // 光标位置传给排版器：紧挨光标的标记要显示出来，
                            // 不然在加粗文字旁边按 Shift+8 打的星号会看不见。
                            let caret = state.last_cursor;
                            let mut layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let job = render::inline_job(
                                        buf.as_str(),
                                        theme,
                                        theme.body,
                                        theme.text,
                                        wrap,
                                        Some(caret),
                                    );
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            // 代码块编辑时也走高亮排版：缓冲区是纯代码，用同一套词法着色，
                            // 边打字边能看到关键字/字符串的颜色，而不是退化成一片纯文本。
                            let code_lang = match &edit_kind {
                                EditKind::Code(l) | EditKind::Diagram(l) => l.as_str(),
                                _ => "",
                            };
                            let mut code_layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let mut job =
                                        render::code_job_edit(buf.as_str(), code_lang, theme);
                                    job.wrap.max_width = wrap;
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            let mut te = TextEdit::multiline(&mut active_buf)
                                .id(id)
                                .desired_rows(rows)
                                .desired_width(page_w);
                            te = match &edit_kind {
                                EditKind::Heading(level) => te
                                    .font(egui::FontId::new(
                                        theme.heading_size(*level),
                                        egui::FontFamily::Proportional,
                                    ))
                                    .text_color(theme.heading)
                                    .frame(egui::Frame::NONE)
                                    .return_key(Option::<egui::KeyboardShortcut>::None),
                                EditKind::Code(_) | EditKind::Diagram(_) => te
                                    .font(egui::FontId::new(
                                        theme.mono,
                                        egui::FontFamily::Monospace,
                                    ))
                                    .text_color(theme.text)
                                    .layouter(&mut code_layouter)
                                    .frame(
                                        egui::Frame::NONE
                                            .fill(theme.code_bg)
                                            .inner_margin(12.0)
                                            .corner_radius(6.0),
                                    ),
                                EditKind::Source => te
                                    .font(egui::FontId::new(
                                        theme.body,
                                        egui::FontFamily::Proportional,
                                    ))
                                    .frame(egui::Frame::NONE)
                                    .return_key(Option::<egui::KeyboardShortcut>::None)
                                    .layouter(&mut layouter),
                            };
                            let out = te.show(ui);
                            // 编辑态里单击链接也直接打开：用户刚插好链接就想点一下验证，
                            // 这时块还处于编辑态，不能只把光标放进链接文字里。
                            // 仍可用方向键把光标移进链接标签修改文字。
                            if out.response.response.clicked() {
                                if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                                    let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                    if let Some(url) = render::link_at_markdown(&active_buf, ci) {
                                        actions.push(Action::OpenLink(url));
                                    }
                                }
                            }
                            // 点击进入编辑：把光标放到刚才点击的位置，而不是跳到末尾/居中
                            if let Some(off) = state.pending_caret.take() {
                                let ccursor = out.galley.cursor_from_pos(off);
                                let mut ts = out.state.clone();
                                ts.cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                                ts.store(ui.ctx(), id);
                                cursor_idx = ccursor.index.0.min(active_buf.chars().count());
                            }
                            // 查找/替换跳转：把命中的文字选中（字符下标按当前缓冲算）。
                            if let Some((bi, a, b)) = state.select_req
                                && bi == idx
                            {
                                state.select_req = None;
                                let len = active_buf.chars().count();
                                let (a, b) = (a.min(len), b.min(len));
                                let mut ts = out.state.clone();
                                ts.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(a),
                                    egui::text::CCursor::new(b),
                                )));
                                ts.store(ui.ctx(), id);
                                out.response.request_focus();
                                cursor_idx = a;
                            }
                            let cr = out.cursor_range;
                            let resp = out.response;
                            if resp.changed() {
                                changed = true;
                            }
                            let len = active_buf.chars().count();
                            if let Some(r) = cr {
                                let a = r.primary.index.0;
                                let b = r.secondary.index.0;
                                cursor_idx = a.min(len);
                                selection = if a == b {
                                    None
                                } else {
                                    Some((a.min(b).min(len), a.max(b).min(len)))
                                };
                            }
                            // 格式按钮（加粗/斜体等）用完选区后，把文本框里存的
                            // 旧选区收敛成单点光标，不然那段文字会一直保持选中。
                            if let Some((bi, pos)) = state.caret_req
                                && bi == idx
                            {
                                state.caret_req = None;
                                let pos = pos.min(len);
                                let mut ts = out.state.clone();
                                ts.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(pos),
                                )));
                                ts.store(ui.ctx(), id);
                                cursor_idx = pos;
                                selection = None;
                                resp.request_focus();
                            }
                            if let Some(f) = state.focus_req {
                                if f == idx {
                                    resp.request_focus();
                                }
                            }
                            resp.response
                        } else {
                            let mut env = RenderEnv {
                                theme,
                                ctx,
                                images,
                                base_dir,
                                outline,
                                actions,
                                max_width: page_w,
                            };
                            render::render_block(ui, idx, block, &mut env, focus_mode)
                        };
                        if resp.rect.is_positive() {
                            block_rects.push((idx, resp.rect));
                        }
                        if resp.clicked() {
                            new_active = Some(idx);
                            state.clicked = true;
                        }
                        if is_active {
                            active_rect = Some(resp.rect);
                            if !editing {
                                state.sel = None;
                            }
                        }
                        ui.add_space(if compact { 1.0 } else { 6.0 });
                    }
                });
            });
            // 只在「本帧真的在输入」时跟随滚动。若每帧都检查，用户用滚轮滚开
            // 会被立刻拉回活动块，表现就是滚轮失效、来回跳。
            if !state.read_only && changed {
                if let Some(r) = active_rect {
                    if !ui.clip_rect().contains_rect(r) {
                        ui.scroll_to_rect(r, None);
                    }
                }
            }
            // 打字机模式只在光标移动时居中。以前每帧都强制 scroll_to_rect，
            // 会把用户用滚轮翻到的位置立刻拉回去，表现就是「滚轮滚不动」。
            if typewriter && !state.read_only && cursor_idx != state.last_cursor {
                if let Some(r) = active_rect {
                    ui.scroll_to_rect(r, Some(egui::Align::Center));
                }
            }
            // 大纲 / TOC / 新建块等显式跳转请求。必须 take()：
            // 之前只写不读，所以点大纲标题完全不会滚动。
            if state.scroll_req.take().is_some() {
                if let Some(r) = active_rect {
                    // 换行/新建块只做「最小滚动」：上面的文字保持不动，只有超出视口才滚。
                    let align = if state.scroll_top {
                        Some(egui::Align::TOP)
                    } else {
                        None
                    };
                    ui.scroll_to_rect(r, align);
                }
                state.scroll_top = false;
            }
        });

    // 空白点击判定：不依赖哪个控件抢到这次点击，直接看指针落点。
    // 条件：在编辑区范围内、不在任何块（含块里的链接/按钮）上、且事件归属
    // 编辑区这一层（弹窗/对话框盖在上面时不算）。滚动条那一条也要排除。
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let primary_clicked = ctx.input(|i| i.pointer.primary_clicked());
    let bar_zone = scroll_bar_zone(ui, scroll_out.inner_rect, scroll_out.content_size.y);
    let blank_pos = pointer.filter(|p| {
        ui.max_rect().contains(*p)
            && !bar_zone.contains(*p)
            && !block_rects.iter().any(|(_, r)| r.contains(*p))
            && ctx
                .layer_id_at(*p)
                .is_none_or(|layer| layer == ui.layer_id())
    });
    // bg 是兜底：极端情况下拿不到指针坐标，但底层热区仍然命中了。
    let blank_clicked = (primary_clicked && blank_pos.is_some()) || bg.clicked();

    if !state.read_only {
        state.last_cursor = cursor_idx;
        state.sel = selection;
        if let Some((a, z)) = selection {
            state.last_sel = Some((active, a, z));
        }
    }
    if std::env::var("MARKD_TRACE_FOCUS").is_ok() {
        eprintln!(
            "[focus] req={:?} focused={:?} editing_block={} active={}",
            state.focus_req,
            ctx.memory(|m| m.focused()),
            !state.read_only,
            active
        );
    }
    state.focus_req = None;

    // 点在编辑区空白处：重新聚焦并把光标放回去。
    // - 新建文档只有一个块：无论点多少次，光标都回到第一行闪烁（caret 0）。
    // - 多块文档：聚焦离落点最近的块；点在正文下方时放到最后一块末尾，方便继续写。
    // 这一步要放在 `focus_req = None` 之后，否则请求会被同一帧清掉，
    // 下一帧 TextEdit 拿不到焦点，光标依旧不闪。
    if blank_clicked {
        // 空白点击统一落到「可以继续输入的最后一行」：最后一块的末尾。
        // 新建空文档只有一块，末尾就是第一行行首——所以无论点多少次、点哪里，
        // 光标都在第一行闪烁。
        let target = doc.blocks.len().saturating_sub(1);
        let caret = doc
            .blocks
            .get(target)
            .map(|b| block_buffer(b).chars().count())
            .unwrap_or(0);
        state.active = target;
        state.focus_req = Some(target);
        state.caret_req = Some((target, caret));
        state.last_cursor = caret;
        state.sel = None;
        state.last_sel = None;
        state.pending_caret = None;
        // 立即把焦点设过去，不等下一帧的 TextEdit 重新请求：
        // 中间任何一帧都不会出现「没有光标」的空窗。
        ctx.memory_mut(|m| m.request_focus(Id::new(("block_edit", tab_key, target))));
        state.clicked = true;
        ctx.request_repaint();
        trace_blank(
            &blank_pos,
            &format!("block {target} caret {caret} (blocks={})", doc.blocks.len()),
        );
    } else {
        trace_blank(
            &blank_pos,
            &format!(
                "miss: primary_clicked={primary_clicked} blocks={} bg_clicked={}",
                block_rects.len(),
                bg.clicked()
            ),
        );
    }

    // ---------------- 结构性按键（回车 / 退格 / Tab）----------------
    // 终端持有键盘焦点时（read_only = true），按键全部归终端：
    // 编辑区不再处理回车/退格/Tab，否则在终端里按回车，编辑区的段落也跟着换行。
    if state.read_only {
        return EditorEvent::None;
    }

    // ---------------- 代码块换语言 ----------------
    // 先把本帧可能发生的文本修改按旧语言写回，再整体重建源码换语言，
    // 避免旧的 edit_kind 把新语言覆盖回去。
    if let Some(new_lang) = lang_change.take() {
        if changed {
            doc.maybe_record();
            let src = edit_kind.rebuild(&active_buf);
            doc.edit_block(active, &src);
        }
        let mut code = active_buf.trim_end_matches('\n').to_string();
        code.push('\n');
        doc.record();
        doc.edit_block(active, &format!("```{new_lang}\n{code}```"));
        return EditorEvent::Changed;
    }

    // ---------------- 行首退格：并回上一块 ----------------
    // 放在写回之前：以前这段在 `if changed { return }` 之后，删空那块文本的那一帧
    // 会直接提前返回，导致「行首退格」永远执行不到。
    if backspace && active > 0 && state.sel.is_none() && (buf_was_empty || state.last_cursor == 0) {
        merge_into_previous_block(doc, state, &active_buf);
        return EditorEvent::Changed;
    }

    // ---------------- 代码块末尾空行回车：退出代码块 ----------------
    // 光标在末尾空行上再按一次回车，退出代码块编辑（Typora 行为）：
    // 去掉末尾空行写回，在下方插入普通段落块并聚焦。判定条件：
    // 1) 本帧按了回车且 TextEdit 真的处理了它（changed 为真要求持有焦点）；
    // 2) 按键前缓冲已以换行结尾（末行是空行）——第一次回车只是新建行，不会命中；
    // 3) 本帧的改动恰好是在末尾追加一个换行且光标仍在缓冲末尾，
    //    排除「同帧打字 + 回车」「行中间回车」的误判。
    if enter
        && changed
        && matches!(edit_kind, EditKind::Code(_))
        && buf_ended_nl
        && state.sel.is_none()
        && cursor_idx == active_buf.chars().count()
        && active_buf == format!("{pre_buf}\n")
    {
        let code = active_buf.trim_end_matches('\n').to_string();
        doc.record();
        if code.is_empty() {
            // 空代码块退出：整块直接变成普通段落，不再插入新块
            doc.edit_block(active, "");
        } else {
            doc.edit_block(active, &edit_kind.rebuild(&code));
            doc.insert_block_after(active, "");
            state.active = active + 1;
        }
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.scroll_top = false;
        state.last_cursor = 0;
        state.sel = None;
        return EditorEvent::Changed;
    }

    // ---------------- 自动配对（括号与引号） ----------------
    if changed && state.auto_pair {
        if let Some(close) = &typed_pair {
            let pos = cursor_idx.min(active_buf.chars().count());
            pair_insert = Some((pos, close.clone()));
        }
    }
    if let Some((pos, close)) = pair_insert {
        active_buf = insert_at_char(&active_buf, pos, &close);
        changed = true;
    }

    // ---------------- 普通输入写回 ----------------
    if changed {
        doc.maybe_record();
        let src = table_edit_src.clone().unwrap_or_else(|| {
            // 删空了就落成空段落：不要把标题的 #、代码块的围栏这些残留标记
            // 当正文写回去，否则下一次退格只会在标记里删字符，永远合并不回上一行。
            if active_buf.trim().is_empty() {
                String::new()
            } else {
                edit_kind.rebuild(&active_buf)
            }
        });
        doc.edit_block(active, &src);
        return EditorEvent::Changed;
    }

    // ---------------- 结构性按键 ----------------
    if let Some(i) = new_active {
        if i != active {
            state.active = i;
            state.focus_req = Some(i);
            state.last_cursor = 0;
            ctx.request_repaint();
        }
    }

    if enter && !active_multiline {
        let buf = active_buf.clone();
        let pos = state.last_cursor.min(buf.chars().count());
        let (left, right) = split_at_char(&buf, pos);
        let marker = if matches!(edit_kind, EditKind::Source) {
            inherited_marker(&buf)
        } else {
            String::new()
        };
        doc.record();
        doc.edit_block(active, &edit_kind.rebuild(&left));
        doc.insert_block_after(active, &format!("{}{}", marker, right));
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.scroll_req = Some(state.active);
        state.scroll_top = false;
        state.last_cursor = marker.chars().count();
        return EditorEvent::Changed;
    }

    if up && state.last_cursor == 0 && active > 0 {
        state.active = active - 1;
        state.focus_req = Some(state.active);
        state.last_cursor = 0;
        ctx.request_repaint();
    }
    if down && state.active + 1 < doc.blocks.len() {
        state.active = active + 1;
        state.focus_req = Some(state.active);
        state.last_cursor = 0;
        ctx.request_repaint();
    }
    if tab {
        let cur = active_buf.clone();
        let indent = " ".repeat(state.tab_size.max(1));
        let updated = if shift {
            cur.strip_prefix(indent.as_str())
                .unwrap_or(&cur)
                .to_string()
        } else {
            format!("{indent}{cur}")
        };
        if updated != cur {
            doc.record();
            doc.edit_block(active, &updated);
            return EditorEvent::Changed;
        }
    }

    EditorEvent::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 图表块编辑往返：编辑缓冲只含图形代码（无围栏），改完能补回围栏写回。
    /// 插入流程图后必须还能再次编辑——回归「点击图形无法重新激活块」的修复。
    #[test]
    fn diagram_edit_roundtrip() {
        let src = "```mermaid\nflowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]\n```";
        let doc = Document::new(src, None);
        let b = &doc.blocks[0];
        assert!(
            matches!(b, Block::Diagram { .. }),
            "应解析出图表块，实际是别的块类型"
        );
        let ek = EditKind::of(b);
        assert!(
            matches!(ek, EditKind::Diagram(_)),
            "图表块应识别为 Diagram 编辑类型"
        );
        assert_eq!(
            block_buffer(b),
            "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]",
            "编辑缓冲不应包含 ``` 围栏"
        );
        // 改代码后重建源码：补回围栏，图形随之更新
        let rebuilt = rebuild_source(b, "flowchart TD\n  X[起点] --> Y[终点]");
        assert_eq!(
            rebuilt,
            "```mermaid\nflowchart TD\n  X[起点] --> Y[终点]\n```"
        );
    }

    fn frame(width: f32, f: impl FnMut(&mut egui::Ui)) {
        frame_sized(width, 900.0, f);
    }

    /// 复用同一个 Context 跑一帧：模拟真实运行时（galley / 纹理缓存都是热的）。
    #[allow(clippy::too_many_arguments)]
    fn frame_warm(
        ctx: &egui::Context,
        width: f32,
        height: f32,
        doc: &mut Document,
        state: &mut EditorState,
        theme: &Theme,
        images: &mut ImageCache,
        outline: &[crate::document::OutlineItem],
        actions: &mut Vec<crate::render::Action>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let _ = show_editor(
                    ui, doc, state, theme, &c, images, None, outline, false, false, false, 1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
    }

    fn frame_sized(width: f32, height: f32, mut f: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| f(ui));
        });
        // 无头环境下没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
        out.textures_delta.clear();
    }

    /// 回归：编辑区正文（段落 / 活动块 / 引用）都必须按窗口宽度软换行。
    #[test]
    fn long_paragraph_wraps_in_editor_column() {
        let text = "word ".repeat(300);
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut label_size = egui::Vec2::ZERO;
        let mut edit_size = egui::Vec2::ZERO;
        let mut quote_size = egui::Vec2::ZERO;
        frame(700.0, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("t")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let outer_w = ui.available_width();
                    let page_w = (outer_w * 0.9).max(200.0);
                    ui.horizontal_top(|ui| {
                        let pad = ((outer_w - page_w) / 2.0).max(0.0);
                        ui.add_space(pad);
                        ui.vertical(|ui| {
                            ui.set_width(page_w);
                            label_size = ui.add(egui::Label::new(text.clone())).rect.size();
                            let mut buf = text.clone();
                            let out = TextEdit::multiline(&mut buf)
                                .desired_rows(1)
                                .desired_width(page_w)
                                .frame(egui::Frame::NONE)
                                .return_key(Option::<egui::KeyboardShortcut>::None)
                                .show(ui);
                            edit_size = out.response.response.rect.size();

                            // 引用块内部是横向布局，最容易退化成不换行。
                            let quote = Block::Quote {
                                level: 1,
                                content: vec![crate::ast::Inline::Text(text.clone())],
                                source: text.clone(),
                            };
                            let ctx_q = ui.ctx().clone();
                            let mut env = RenderEnv {
                                theme: &theme,
                                ctx: &ctx_q,
                                images: &mut images,
                                base_dir: None,
                                outline: &[],
                                actions: &mut actions,
                                max_width: page_w,
                            };
                            quote_size = render::render_block(ui, 0, &quote, &mut env, false)
                                .rect
                                .size();
                        });
                    });
                });
        });
        assert!(label_size.y > 40.0, "Label 没有换行：size={label_size:?}");
        assert!(label_size.x <= 700.0, "Label 超出窗口：size={label_size:?}");
        assert!(edit_size.y > 40.0, "TextEdit 没有换行：size={edit_size:?}");
        assert!(quote_size.y > 40.0, "引用块没有换行：size={quote_size:?}");
        assert!(quote_size.x <= 700.0, "引用块超出窗口：size={quote_size:?}");
    }

    /// 原地编辑：标题只编辑文字、代码块只编辑代码，写回时再补回 Markdown 标记。
    #[test]
    fn edit_kind_rebuilds_markdown() {
        assert_eq!(EditKind::Heading(2).rebuild("标题"), "## 标题");

        let src = EditKind::Code("rust".to_string()).rebuild("fn main() {}");
        assert_eq!(src, "```rust\nfn main() {}\n```");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { lang, code, .. } if lang == "rust" && code == "fn main() {}"),
            "代码块写回后应能还原成同一段代码"
        );

        // 多行代码同样可往返
        let src = EditKind::Code(String::new()).rebuild("a\nb");
        let blocks = crate::ast::parse(&src);
        assert!(matches!(&blocks[0], Block::Code { code, .. } if code == "a\nb"));

        // 末尾空行（缓冲以 \n 结尾）也要能往返：在末行回车产生的空行不丢失
        let src = EditKind::Code("rust".to_string()).rebuild("a\n");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { code, .. } if code == "a\n"),
            "{src}"
        );
        let src = EditKind::Code("rust".to_string()).rebuild("\n");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { code, .. } if code == "\n"),
            "{src}"
        );
    }

    /// 清除格式：标题 / 引用 / 列表 / 任务列表都变回普通段落，行内加粗也去掉。
    #[test]
    fn clear_format_turns_blocks_into_paragraph() {
        for (src, want) in [
            ("## **粗体**标题", "粗体标题"),
            ("> 引用文字", "引用文字"),
            ("- 列表项", "列表项"),
            ("1. 有序项", "有序项"),
            ("- [ ] 待办", "待办"),
            ("正文里有 ~~删除线~~ 和 `代码`", "正文里有 删除线 和 代码"),
        ] {
            let mut doc = Document::new(src, None);
            let mut st = EditorState::default();
            assert!(clear_format(&mut doc, &mut st), "{src}");
            assert_eq!(doc.source(), want);
        }
    }

    /// 代码块擦成段落时保留代码正文，不带围栏；表格等结构块不做处理。
    #[test]
    fn clear_format_handles_code_and_refuses_structures() {
        let mut doc = Document::new(
            "\u{60}\u{60}\u{60}rust\nfn main() {}\n\u{60}\u{60}\u{60}",
            None,
        );
        let mut st = EditorState::default();
        assert!(clear_format(&mut doc, &mut st));
        assert_eq!(doc.source(), "fn main() {}");

        let mut table = Document::new("| a | b |\n| - | - |\n| 1 | 2 |", None);
        assert!(!clear_format(&mut table, &mut st));
    }

    /// 点工具条按钮时编辑框已经失焦、`sel` 被清空，要用 last_sel 记住的选区
    /// 才能把行内代码/加粗作用在刚选中的文字上。
    #[test]
    fn wrap_uses_last_selection_after_focus_loss() {
        let mut doc = Document::new("选中这段文字", None);
        let mut st = EditorState::default();
        st.sel = None;
        st.last_sel = Some((0, 0, 2));
        wrap(&mut doc, &mut st, "`", "`", true);
        assert_eq!(doc.source(), "`选中`这段文字");
        assert!(st.last_sel.is_none(), "用过的选区要清掉，避免重复包裹");
    }

    /// 加粗（工具条按钮）：没有选区时，整行（光标所在块）整体加粗；
    /// 再点一次取消——整行已经带着 ** 时，点击就是去掉标记。
    #[test]
    fn bold_whole_line_toggles() {
        let mut doc = Document::new("正文一段", None);
        let mut st = EditorState::default();
        wrap(&mut doc, &mut st, "**", "**", true);
        assert_eq!(doc.source(), "**正文一段**");
        wrap(&mut doc, &mut st, "**", "**", true);
        assert_eq!(doc.source(), "正文一段");
    }

    /// 加粗作用在标题块时，偏移按「去掉 # 的可编辑文本」计算，# 不能被裹进标记里。
    #[test]
    fn bold_on_heading_keeps_marker_outside() {
        let mut doc = Document::new("# 标题内容", None);
        let mut st = EditorState::default();
        wrap(&mut doc, &mut st, "**", "**", true);
        assert_eq!(doc.source(), "# **标题内容**");
    }

    /// 链接保持旧行为：没有选区时只在光标处插入一对空标记，不裹整行。
    #[test]
    fn link_inserts_at_caret_without_selection() {
        let mut doc = Document::new("正文一段", None);
        let mut st = EditorState::default();
        st.last_cursor = 2;
        wrap(&mut doc, &mut st, "[", "](https://)", false);
        assert_eq!(doc.source(), "正文[](https://)一段");
    }

    /// 选中文字后打开链接弹窗：确认时用记住的选区写回，选中文字必须保留。
    #[test]
    fn insert_link_keeps_selected_text() {
        let mut doc = Document::new("看这段文字", None);
        let mut st = EditorState::default();
        assert!(insert_link(
            &mut doc,
            &mut st,
            Some((0, 1, 3)),
            "这段",
            "https://example.com",
        ));
        assert_eq!(doc.source(), "看[这段](https://example.com)文字");
        assert_eq!(st.focus_req, Some(0));
    }

    /// 没有选区时用 URL 当显示文字，链接可见、可点。
    #[test]
    fn insert_link_without_selection_uses_url_label() {
        let mut doc = Document::new("正文", None);
        let mut st = EditorState::default();
        st.last_cursor = 2;
        assert!(insert_link(&mut doc, &mut st, None, "", "https://e.com"));
        assert_eq!(doc.source(), "正文[https://e.com](https://e.com)");
    }

    /// selection_text 在失焦后会回退到 last_sel，供链接弹窗取选中文字。
    #[test]
    fn selection_text_falls_back_to_last_sel() {
        let doc = Document::new("选中这段文字", None);
        let mut st = EditorState::default();
        st.last_sel = Some((0, 2, 4));
        assert_eq!(
            selection_text(&doc, &st),
            Some((0, 2, 4, "这段".to_string()))
        );
    }

    /// 删除线（工具条按钮）：选中文字后两侧各补一个 ~~，渲染成一条横线。
    #[test]
    fn strike_wraps_selection() {
        let mut doc = Document::new("普通文字", None);
        let mut st = EditorState {
            sel: Some((0, 2)),
            ..Default::default()
        };
        wrap(&mut doc, &mut st, "~~", "~~", true);
        assert_eq!(doc.source(), "~~普通~~文字");
        assert!(doc.dirty);
    }

    /// 点一下编辑区空白处（按下/松开分两帧），再跑两帧让焦点落地。
    #[allow(deprecated)]
    fn click_blank(
        ctx: &egui::Context,
        doc: &mut Document,
        state: &mut EditorState,
        theme: &Theme,
        images: &mut ImageCache,
        actions: &mut Vec<crate::render::Action>,
        pos: egui::Pos2,
    ) {
        run_editor_frame(
            ctx,
            doc,
            state,
            theme,
            images,
            actions,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run_editor_frame(
            ctx,
            doc,
            state,
            theme,
            images,
            actions,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        // 给「请求焦点 + 设置光标」一帧的落地时间
        for _ in 0..2 {
            run_editor_frame(ctx, doc, state, theme, images, actions, vec![]);
        }
    }

    /// 新建空白文档后，无论点多少次、点哪块空白，光标都应回到第一行并保持闪烁。
    /// 之前只在块自身矩形内处理点击，点正文下方没有任何控件命中，焦点就丢了。
    #[allow(deprecated)]
    #[test]
    fn blank_click_refocuses_first_line() {
        let mut doc = Document::new("", None);
        let mut state = EditorState::default();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        // 第一帧建立布局
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        // 正文下方、左右页边距、页面中部：反复点，每次都必须落回第一行。
        for (i, pos) in [
            egui::pos2(400.0, 800.0),
            egui::pos2(20.0, 400.0),
            egui::pos2(780.0, 400.0),
            egui::pos2(400.0, 300.0),
            egui::pos2(400.0, 800.0),
        ]
        .into_iter()
        .enumerate()
        {
            click_blank(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                pos,
            );
            assert_eq!(state.active, 0, "第 {} 次点空白后应回到第一个块", i + 1);
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(egui::Id::new(("block_edit", 1, 0))),
                "第 {} 次点空白后第一行的 TextEdit 必须持有键盘焦点",
                i + 1
            );
            assert_eq!(
                state.last_cursor,
                0,
                "第 {} 次点空白后光标应停在第一行行首",
                i + 1
            );
        }
    }

    /// 模拟真实 app 的面板结构（menubar/toolbar/tabbar/sidebar + CentralPanel）跑一帧。
    #[allow(clippy::too_many_arguments)]
    fn run_app_like_frame(
        ctx: &egui::Context,
        doc: &mut Document,
        state: &mut EditorState,
        theme: &Theme,
        images: &mut ImageCache,
        actions: &mut Vec<crate::render::Action>,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let outline = doc.outline();
        let mut out = ctx.run_ui(input, |root| {
            let mut root = egui::Ui::new(
                root.clone(),
                egui::Id::unique("app_root"),
                egui::UiBuilder::new(),
            );
            egui::Panel::top("menubar").show(&mut root, |ui| {
                ui.label("menu");
            });
            egui::Panel::top("toolbar").show(&mut root, |ui| {
                ui.label("toolbar");
            });
            egui::Panel::bottom("statusbar").show(&mut root, |ui| {
                ui.label("status");
            });
            egui::Panel::left("sidebar")
                .exact_size(220.0)
                .show(&mut root, |ui| {
                    ui.label("sidebar");
                });
            egui::Panel::top("tabbar").show(&mut root, |ui| {
                ui.label("tabs");
            });
            egui::CentralPanel::default()
                .frame(egui::Frame::new().inner_margin(8))
                .show(&mut root, |ui| {
                    let _ = show_editor(
                        ui, doc, state, theme, ctx, images, None, &outline, false, false, false, 1,
                        actions,
                    );
                });
        });
        out.textures_delta.clear();
    }

    /// 真实面板结构下点空白：焦点必须回到第一行（复现用户报告的 bug）。
    #[test]
    fn blank_click_refocuses_in_app_like_layout() {
        let mut doc = Document::new("", None);
        let mut state = EditorState::default();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        run_app_like_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        let pos = egui::pos2(600.0, 500.0);
        run_app_like_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run_app_like_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        // 给「请求焦点 + 设置光标」两帧的落地时间
        for _ in 0..2 {
            run_app_like_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![],
            );
        }
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new(("block_edit", 1, 0))),
            "真实面板结构下点空白也必须把焦点还给第一行"
        );
    }

    /// 多块文档：无论点正文下方还是页边空白，光标都落到最后一行（最后一块末尾）。
    #[allow(deprecated)]
    #[test]
    fn blank_click_below_content_focuses_last_block_end() {
        let mut doc = Document::new("a\n\nb", None);
        let mut state = EditorState::default();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        // 点正文下方、点正文左侧的空白，结果都一样：光标落在最后一行（最后一块末尾）。
        let last = doc.blocks.len() - 1;
        for pos in [egui::pos2(400.0, 800.0), egui::pos2(20.0, 300.0)] {
            click_blank(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                pos,
            );
            assert_eq!(state.active, last, "点空白 {pos:?} 应回到最后一块");
            assert_eq!(
                ctx.memory(|m| m.focused()),
                Some(egui::Id::new(("block_edit", 1, last))),
                "点空白 {pos:?} 后最后一块的 TextEdit 必须持有键盘焦点"
            );
            assert_eq!(
                state.last_cursor, 1,
                "点空白 {pos:?} 光标应停在最后一块末尾"
            );
        }
    }

    /// 源码模式：点编辑框外的空白，光标跳到文档末尾（最后一行）并重新聚焦。
    #[test]
    fn source_mode_blank_click_focuses_document_end() {
        let mut doc = Document::new("a\nb", None);
        let mut state = EditorState::default();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let outline = doc.outline();
        let mut src_frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let outline = &outline;
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = show_editor(
                        ui,
                        &mut doc,
                        &mut state,
                        &theme,
                        &ctx,
                        &mut images,
                        None,
                        outline,
                        true,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
        };
        src_frame(vec![]);
        let pos = egui::pos2(400.0, 800.0);
        src_frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        src_frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        for _ in 0..2 {
            src_frame(vec![]);
        }
        let id = egui::Id::new(("source_editor", 1));
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id),
            "源码模式点空白后源码编辑器必须持有键盘焦点"
        );
        let caret = egui::TextEdit::load_state(&ctx, id)
            .and_then(|ts| ts.cursor.char_range())
            .map(|r| r.primary.index.0);
        assert_eq!(caret, Some(3), "光标应停在文档末尾（最后一行）");
    }

    /// 终端持有键盘焦点时（read_only），编辑区不得处理回车：
    /// 否则在终端里按回车，编辑区的段落会跟着换行。
    #[test]
    fn enter_is_ignored_when_read_only() {
        let mut doc = Document::new("第一段\n\n第二段", None);
        let blocks_before = doc.blocks.len();
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        state.active = doc.blocks.len() - 1;
        state.read_only = true;
        state.focus_req = Some(state.active);

        let ctx = egui::Context::default();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(
            doc.blocks.len(),
            blocks_before,
            "read_only 时回车不应拆分块"
        );
    }

    /// 大纲跳转：点击标题后应把该块设为活动块，并消费滚动请求
    /// （之前 scroll_req 只写不读，所以点大纲完全不会滚动）。
    /// 行首按退格要把当前块并回上一块（Typora 行为）：
    /// 当前块删空后再按一次，应当与上一块合并，而不是毫无反应。
    #[test]
    fn backspace_at_block_start_merges_previous_block() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("# 标题一\n## 标题二", None);
        assert_eq!(doc.blocks.len(), 2, "两行标题应解析成两个块");

        let mut state = EditorState::default();
        state.active = 1;
        state.focus_req = Some(1);

        let ctx = egui::Context::default();
        // 第一帧：让第二个块的 TextEdit 拿到焦点，光标落在行首
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        assert_eq!(
            ctx.memory(|m| m.focused()).is_some(),
            true,
            "编辑块应当拿到键盘焦点"
        );
        // 连续按 4 次退格：3 次删掉「标题二」，第 4 次（此时行已空）应当并回上一块
        for _ in 0..4 {
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
        }

        assert_eq!(
            doc.blocks.len(),
            1,
            "行首退格应把第二块并进第一块，实际仍有 {} 块：{:?}",
            doc.blocks.len(),
            doc.source()
        );
        assert_eq!(doc.source(), "# 标题一", "空行应被并入上一块");
    }

    /// 用同一个 Context 跑一帧编辑器（带可注入的输入事件）。
    fn run_editor_frame(
        ctx: &egui::Context,
        doc: &mut Document,
        state: &mut EditorState,
        theme: &Theme,
        images: &mut ImageCache,
        actions: &mut Vec<crate::render::Action>,
        events: Vec<egui::Event>,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let outline = doc.outline();
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let _ = show_editor(
                    ui, doc, state, theme, ctx, images, None, &outline, false, false, false, 1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
    }

    #[test]
    fn outline_jump_sets_active_block_and_consumes_scroll_request() {
        let mut text = String::new();
        for i in 0..40 {
            text.push_str(&format!("段落 {i}\n\n"));
        }
        text.push_str("# 目标标题\n\n结尾段落\n");

        let mut doc = Document::new(&text, None);
        let outline = doc.outline();
        let target = outline.last().expect("应有标题").block;
        assert!(target > 30, "标题应位于文档后部，实际 block={target}");

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        state.active = target;
        state.scroll_req = Some(target);

        frame(800.0, |ui| {
            let ctx = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &ctx,
                &mut images,
                None,
                &outline,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });

        assert_eq!(state.active, target);
        assert!(
            state.scroll_req.is_none(),
            "滚动请求应被 show_editor 消费，否则点大纲不会滚动"
        );
    }

    /// 10 万字文档的帧率验证：模拟滚动到不同位置，测量每帧耗时。
    #[test]
    fn bench_100k_doc_scroll() {
        use std::time::Instant;
        let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
        let mut text = String::new();
        for i in 0..470 {
            if i % 20 == 0 {
                text.push_str(&format!("## 标题 {i}\n\n"));
            }
            text.push_str(&para);
            text.push_str("\n\n");
        }
        println!("[100k] chars = {}", text.chars().count());

        let mut doc = Document::new(&text, None);
        let t = Instant::now();
        let _ = crate::ast::parse(&text);
        println!("[100k] parse = {:?}", t.elapsed());

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        let outline = doc.outline();
        let total = doc.blocks.len();
        println!("[100k] blocks = {total}");

        // 模拟滚动：每帧跳到文档的不同位置（会强制渲染一批全新的块）。
        let mut worst = std::time::Duration::ZERO;
        let mut sum = std::time::Duration::ZERO;
        let mut n = 0;
        for frac in 0..20 {
            let target = (total.saturating_sub(1) * frac) / 20;
            state.active = target;
            state.scroll_req = Some(target);
            let t = Instant::now();
            frame(1200.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &ctx,
                    &mut images,
                    None,
                    &outline,
                    false,
                    false,
                    false,
                    1,
                    &mut actions,
                );
            });
            let e = t.elapsed();
            sum += e;
            worst = worst.max(e);
            n += 1;
            if frac % 5 == 0 {
                println!("[100k] jump to block {target}/{total} = {e:?}");
            }
        }
        println!("[100k] avg frame = {:?}, worst = {:?}", sum / n, worst);

        // 连续输入：每帧改一个字，测编辑回写耗时
        let t = Instant::now();
        for k in 0..20 {
            let src = format!("连续输入测试 {k}");
            doc.edit_block(5, &src);
        }
        println!("[100k] 20 次 edit_block = {:?}", t.elapsed());
    }

    /// 50 万字（中文）整篇打开、滚动浏览与连续输入的实测。
    #[test]
    fn bench_500k_doc() {
        use std::time::Instant;
        let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
        let per = para.chars().count() + 2;
        let n = 500_000 / per + 1;
        let mut text = String::new();
        for i in 0..n {
            if i % 20 == 0 {
                text.push_str(&format!(
                    "## 标题 {i}

"
                ));
            }
            text.push_str(&para);
            text.push_str(
                "

",
            );
        }
        let chars = text.chars().count();
        println!("[500k] chars = {chars}");

        let t = Instant::now();
        let blocks = crate::ast::parse(&text);
        println!(
            "[500k] 打开时解析 = {:?}, blocks = {}",
            t.elapsed(),
            blocks.len()
        );

        let mut doc = Document::new(&text, None);
        let t = Instant::now();
        let outline = doc.outline();
        println!("[500k] 大纲 = {:?}", t.elapsed());
        let t = Instant::now();
        let _ = doc.stats();
        println!("[500k] 统计 = {:?}", t.elapsed());

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        let total = doc.blocks.len();

        // 冷启动第一帧（缓存全空）：打开文档那一瞬间的成本
        let ctx = egui::Context::default();
        let t = Instant::now();
        frame_warm(
            &ctx,
            1200.0,
            900.0,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &outline,
            &mut actions,
        );
        println!("[500k] 首帧（冷缓存）= {:?}", t.elapsed());

        // 热缓存下的滚动浏览：真实使用时的每帧成本
        let mut worst = std::time::Duration::ZERO;
        let mut sum = std::time::Duration::ZERO;
        let mut cnt = 0;
        for frac in 0..20 {
            let target = (total.saturating_sub(1) * frac) / 20;
            state.active = target;
            state.scroll_req = Some(target);
            let t = Instant::now();
            frame_warm(
                &ctx,
                1200.0,
                900.0,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &outline,
                &mut actions,
            );
            let e = t.elapsed();
            sum += e;
            worst = worst.max(e);
            cnt += 1;
        }
        println!(
            "[500k] 热缓存滚动 20 帧：avg = {:?}, worst = {:?}",
            sum / cnt,
            worst
        );

        // 连续输入：改一个字 + 重绘一帧，测真实打字的手感
        state.active = total / 2;
        state.scroll_req = None;
        let t = Instant::now();
        let mut worst_edit = std::time::Duration::ZERO;
        let mut sum_edit = std::time::Duration::ZERO;
        for k in 0..30 {
            doc.edit_block(total / 2, &format!("连续输入测试 {k}"));
            let t1 = Instant::now();
            frame_warm(
                &ctx,
                1200.0,
                900.0,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &outline,
                &mut actions,
            );
            let e = t1.elapsed();
            sum_edit += e;
            worst_edit = worst_edit.max(e);
        }
        let _ = t.elapsed();
        println!(
            "[500k] 每键一帧 30 次：avg = {:?}, worst = {:?}",
            sum_edit / 30,
            worst_edit
        );

        let t = Instant::now();
        doc.edit_block(total / 2, &format!("最终 {}", chars));
        println!("[500k] 单次 edit_block = {:?}", t.elapsed());
    }

    /// 性能基准（打印耗时）：10 万字级别的文档，看各环节开销。
    #[test]
    fn bench_large_doc() {
        use std::time::Instant;
        let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
        let mut text = String::new();
        for i in 0..300 {
            if i % 20 == 0 {
                text.push_str(&format!("## 标题 {i}\n\n"));
            }
            text.push_str(&para);
            text.push_str("\n\n");
        }
        println!("[bench] chars = {}", text.chars().count());

        let t = Instant::now();
        let blocks = crate::ast::parse(&text);
        println!(
            "[bench] parse = {:?}, blocks = {}",
            t.elapsed(),
            blocks.len()
        );

        let mut doc = Document::new(&text, None);
        let t = Instant::now();
        let _ = doc.stats();
        println!("[bench] stats = {:?}", t.elapsed());
        let t = Instant::now();
        let _ = doc.outline();
        println!("[bench] outline = {:?}", t.elapsed());

        let t = Instant::now();
        doc.edit_block(5, "改了一下的段落");
        println!("[bench] edit_block = {:?}", t.elapsed());

        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut state = EditorState::default();
        let outline = doc.outline();
        for i in 0..4 {
            let t = Instant::now();
            frame(1000.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &ctx,
                    &mut images,
                    None,
                    &outline,
                    false,
                    false,
                    false,
                    1,
                    &mut actions,
                );
            });
            println!("[bench] editor frame {i} = {:?}", t.elapsed());
        }
        // 视口只有 300pt 时，只要渲染少数几个块，用来对比每个块的成本。
        for i in 0..3 {
            let t = Instant::now();
            frame_sized(1000.0, 300.0, |ui| {
                let ctx = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &ctx,
                    &mut images,
                    None,
                    &outline,
                    false,
                    false,
                    false,
                    1,
                    &mut actions,
                );
            });
            println!("[bench] small-viewport frame {i} = {:?}", t.elapsed());
        }
    }
}
