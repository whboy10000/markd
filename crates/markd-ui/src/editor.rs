//! Author: aurelianus <aurelianus@126.com>

//! Hybrid View 编辑器：活动块显示源码，其余块实时渲染成富文本。

use crate::ast::Block;
use crate::document::{Document, OutlineItem};
use crate::render::{self, Action, ImageCache, RenderEnv};
use crate::theme::Theme;
use egui::{self, Id, Key, Sense, TextEdit};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    /// 已折叠的代码块下标集合（渲染态折叠，切换标签/文档时随 EditorState 一起保存）。
    pub folded: HashSet<usize>,
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

fn folds_path() -> PathBuf {
    let dir = crate::workspace::Config::dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("folds.json")
}

fn read_folds_map() -> HashMap<String, Vec<String>> {
    std::fs::read_to_string(folds_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_folds_map(map: &HashMap<String, Vec<String>>) {
    if let Ok(s) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(folds_path(), s);
    }
}

/// 可折叠的块：代码块或 Mermaid 图表块。
fn is_foldable(b: &Block) -> bool {
    matches!(b, Block::Code { .. } | Block::Diagram { .. })
}

/// 纯函数：折叠下标集合 → 各折叠块的 source 键（排序去重）。
fn fold_keys(doc: &Document, folded: &HashSet<usize>) -> Vec<String> {
    let mut keys: Vec<String> = folded
        .iter()
        .filter_map(|&i| doc.blocks.get(i))
        .filter(|b| is_foldable(b))
        .map(|b| b.source().to_string())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// 纯函数：source 键 → 当前文档里匹配的块下标集合。
fn fold_indices(doc: &Document, keys: &[String]) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        if is_foldable(b) && keys.iter().any(|k| k == b.source()) {
            out.insert(i);
        }
    }
    out
}

/// 打开文档时把已保存的折叠状态还原成块下标集合：按 source 匹配当前块。
pub fn load_folds(path: &Path, doc: &Document) -> HashSet<usize> {
    let map = read_folds_map();
    let Some(keys) = map.get(&path.to_string_lossy().to_string()) else {
        return HashSet::new();
    };
    fold_indices(doc, keys)
}

/// 保存当前文档的折叠状态（无折叠项时移除该路径的条目）。
pub fn save_folds(path: &Path, doc: &Document, folded: &HashSet<usize>) {
    let mut map = read_folds_map();
    let key = path.to_string_lossy().to_string();
    let keys = fold_keys(doc, folded);
    if keys.is_empty() {
        map.remove(&key);
    } else {
        map.insert(key, keys);
    }
    write_folds_map(&map);
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

/// 识别有序列表前缀里的起始序号与符号："1. " → (1, '.')，"3) " → (3, ')')。
fn ordered_prefix(prefix: &str) -> Option<(u64, char)> {
    let t = prefix.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    match t[digits.len()..].trim_end() {
        "." => Some((digits.parse().ok()?, '.')),
        ")" => Some((digits.parse().ok()?, ')')),
        _ => None,
    }
}

/// 任务项标记 `- [x]` → `- [ ]`（回车新建的任务项从不勾选状态开始）。
fn replace_task_checkbox(marker: &str) -> String {
    marker.replace("[x]", "[ ]").replace("[X]", "[ ]")
}

/// 有序列表回车新建下一项时的序号：`3.` → `4.`，`9)` → `10)`。
/// 非有序标记原样返回。
fn next_ordered_marker(marker: &str) -> String {
    let digits: String = marker.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return marker.to_string();
    }
    let tail = &marker[digits.len()..];
    if tail != "." && tail != ")" {
        return marker.to_string();
    }
    let n: u64 = digits.parse().unwrap_or(0);
    format!("{}{}", n + 1, tail)
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
            edits.push((i, format!("{p}{cleaned}")));
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
        state.active = first;
        state.focus_req = Some(first);
        state.last_cursor = doc
            .blocks
            .get(first)
            .map(|b| block_buffer(b).chars().count())
            .unwrap_or(0);
        state.sel = None;
        state.last_sel = None;
        return;
    }

    let idx = state.active;
    let Some(b) = doc.blocks.get(idx) else {
        return;
    };
    let cleaned = strip_block_prefix(b.source());
    doc.record();
    doc.edit_block(idx, &format!("{}{}", prefix, cleaned));
    state.last_cursor = (prefix.chars().count() + cleaned.chars().count()).min(usize::MAX);
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
        return;
    }
    let p = ">".repeat(level);
    let tpl = format!("{p} ```\n{p} \n{p} ```");
    doc.record();
    let idx = state.active;
    doc.insert_block_after(idx, &tpl);
    state.active = idx + 1;
    state.focus_req = Some(state.active);
    state.scroll_req = Some(state.active);
    state.scroll_top = false;
    state.last_cursor = 0;
    state.sel = None;
    state.last_sel = None;
}

/// 引用按钮的「再点一次取消」：目标块（跨块选区覆盖的块，或当前活动块）
/// 全部是引用时，去掉行首所有 `>` 还原成普通段落，返回 true。
fn unquote_blocks(doc: &mut Document, state: &mut EditorState) -> bool {
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
    state.focus_req = Some(first);
    state.last_cursor = doc
        .blocks
        .get(first)
        .map(|b| block_buffer(b).chars().count())
        .unwrap_or(0);
    state.sel = None;
    state.last_sel = None;
    true
}

/// 去掉每一行开头的 `>` 前缀与紧随的一个空格（正文自身的缩进要保留）。
fn strip_quote_lines(src: &str, level: usize) -> String {
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
fn quote_level_of(b: &Block) -> Option<usize> {
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
fn strip_block_prefix(src: &str) -> String {
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
fn wrap_cross_sel(doc: &mut Document, state: &mut EditorState, open: &str, close: &str) {
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
/// 表格工具栏的操作。行/列下标按「含表头」计：0 = 表头行，数据行从 1 起。
#[derive(Clone, Copy, Debug)]
pub enum TableOp {
    /// 在指定行下方插入空行（指定表头行 0 时插到数据区最前）
    InsertRow(usize),
    /// 删除指定数据行（表头行 0 不可删）
    DeleteRow(usize),
    /// 在指定列右侧插入空列
    InsertCol(usize),
    /// 删除指定列（至少保留一列）
    DeleteCol(usize),
    /// 整表所有列设置为同一种对齐
    SetAlign(crate::ast::Align),
    /// 删除整张表格（变成空段落）
    Delete,
}

/// 对字符串网格执行表格结构操作，返回是否发生变化。
/// 行/列下标按「含表头」计：0 = 表头行，数据行从 1 起（grid[0] 即表头）。
pub(crate) fn table_op_on_grid(
    grid: &mut Vec<Vec<String>>,
    align: &mut Vec<crate::ast::Align>,
    op: TableOp,
) -> bool {
    use crate::ast::Align;
    let cols = grid
        .first()
        .map(|r| r.len())
        .into_iter()
        .fold(0usize, usize::max);
    match op {
        TableOp::InsertRow(r) => {
            // 在第 r 行下方插入：r=0（表头）→ 数据区最前；其余插到其后
            grid.insert((r + 1).min(grid.len()), vec![String::new(); cols.max(1)]);
            true
        }
        TableOp::DeleteRow(r) => {
            // 表头行不可删
            if r >= 1 && grid.len() > 1 {
                grid.remove(r.min(grid.len() - 1));
                true
            } else {
                false
            }
        }
        TableOp::InsertCol(c) => {
            let at = (c + 1).min(cols.max(1));
            for row in grid.iter_mut() {
                row.insert(at.min(row.len()), String::new());
            }
            align.insert(at.min(align.len()), Align::Left);
            true
        }
        TableOp::DeleteCol(c) => {
            if cols > 1 {
                let at = c.min(cols - 1);
                for row in grid.iter_mut() {
                    if at < row.len() {
                        row.remove(at);
                    }
                }
                if at < align.len() {
                    align.remove(at);
                }
                true
            } else {
                false
            }
        }
        TableOp::SetAlign(a) => {
            for al in align.iter_mut() {
                *al = a;
            }
            true
        }
        TableOp::Delete => false,
    }
}

/// 对第 idx 块（表格）执行结构操作：解析块、改结构、重建源码写回。
pub fn apply_table_op(doc: &mut Document, idx: usize, op: TableOp) {
    use crate::ast::Block;
    let Some(Block::Table {
        head, rows, align, ..
    }) = doc.blocks.get(idx)
    else {
        return;
    };
    let head = head.clone();
    let rows = rows.clone();
    let mut align = align.clone();
    if let TableOp::Delete = op {
        doc.record();
        doc.edit_block(idx, "");
        return;
    }
    let mut grid: Vec<Vec<String>> =
        vec![head.iter().map(|c| crate::ast::inline_plain(c)).collect()];
    grid.extend(rows.iter().map(|r| {
        r.iter()
            .map(|c| crate::ast::inline_plain(c))
            .collect::<Vec<String>>()
    }));
    if !table_op_on_grid(&mut grid, &mut align, op) {
        return;
    }
    doc.record();
    doc.edit_block(idx, &table_source(&grid, &align));
}

pub(crate) fn table_source(grid: &[Vec<String>], align: &[crate::ast::Align]) -> String {
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
    /// 代码块：`quote` 表示它写在引用内部（源码每行带 `>` 前缀），
    /// 写回时要逐行补回前缀，编辑时左侧照渲染态显示引用竖线。
    Code {
        lang: String,
        quote: usize,
    },
    /// 图表块（mermaid 等）：编辑时隐藏 ``` 围栏，只编辑图形描述代码。
    Diagram(String),
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
    fn of(b: &Block) -> Self {
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
fn merge_into_previous_block(doc: &mut Document, state: &mut EditorState, cur: &str) {
    let active = state.active;
    if active == 0 {
        return;
    }
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
fn quote_editable_text(src: &str) -> String {
    src.trim_end()
        .trim_start_matches('>')
        .trim_start_matches(' ')
        .to_string()
}

/// 块的可编辑文本：引用只隐藏 `>`、标题/列表等只保留文字，其余块就是 Markdown 源码。
fn block_editable_text(b: &Block) -> String {
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

/// 行号栏（gutter）的固定宽度：70px，三种模式（源码 / 混合 / 专注）共用。
///
/// 设计要点：宽度是常量，不随窗口大小、字体字号、行数位数或模式变化，
/// 也不参与版心居中 —— 行号栏始终锚定在编辑区最左侧，正文紧接其后。
/// 超过 4 位的超大文档行号会挤占行号栏左侧留白，但不会改变栏宽。
const GUTTER_WIDTH: f32 = 70.0;

fn gutter_width(show: bool) -> f32 {
    if show { GUTTER_WIDTH } else { 0.0 }
}

/// 行号数字右缘相对 gutter 右缘的缩进（行号与正文之间的间距），三种模式共用。
/// 固定 70px 栏宽下留 12px，保证最大字号（26pt）的 4 位行号仍能完整显示。
const GUTTER_PAD: f32 = 12.0;

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

/// 规范化跨块选区顺序：(起块, 起字符, 止块, 止字符)。
fn cross_sel_range(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (a, ac, b, bc) = sel;
    if (a, ac) <= (b, bc) {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    }
}

/// 渲染态用 text_response 自己画文字选区的块。
/// 其余块（代码块、图表、表格、公式…）没有可映射的字符位置，拖选时按「整块」处理。
fn text_selection_block(b: &Block) -> bool {
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
fn atomic_block_selected(sel: Option<(usize, usize, usize, usize)>, idx: usize) -> bool {
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
fn apply_cross_sel_edit(doc: &mut Document, state: &mut EditorState, insert: Option<&str>) -> bool {
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
    // 是否显示行号（配置里的 show_line_numbers）。
    show_line_numbers: bool,
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
        // 行号栏宽度固定（三种模式共用），不随行数位数、窗口、模式变化。
        let gutter_w = gutter_width(show_line_numbers);
        // 每个源码行首字符的下标（CCursor 按 char 计数）。
        let line_starts: Vec<usize> = {
            let mut v = vec![0usize];
            let mut cnt = 0usize;
            for c in src.chars() {
                if c == '\n' {
                    v.push(cnt + 1);
                }
                cnt += 1;
            }
            v
        };
        // 上一帧的视觉行映射 (源码行号, 行顶 y, 行高)：光标行底色要垫在文字
        // 下层，必须在 TextEdit 之前画，只能用上一帧的数据（误差一帧，不可见）。
        let rows_state: std::cell::RefCell<Vec<(usize, f32, f32)>> =
            std::cell::RefCell::new(Vec::new());
        // 当前光标所在的源码行（1 起）。
        let cur_line = {
            let cur = state.last_cursor.min(src.chars().count());
            match line_starts.binary_search(&cur) {
                Ok(i) => i + 1,
                Err(i) => i.min(line_starts.len()).max(1),
            }
        };
        // 纵向 + 横向都要滚：长行不换行，超出视口的部分靠横向滚动露出
        //（光标移动时 TextEdit 会调 scroll_to_rect，ScrollArea 跟随平移）。
        let scroll = egui::ScrollArea::both()
            .id_salt("source_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 行号列与光标行底色的 x 边界：必须用 ScrollArea 自己的裁剪区。
                // 之前用外层 ui 的 clip_rect，其左缘在 ScrollArea 内容裁剪区之左，
                // 数字几乎全被裁掉，只剩最右侧一个"点"。
                let vpl = ui.clip_rect().left();
                let vpr = ui.clip_rect().right();
                // 不换行：一行源码 = 一个视觉行。换行会让一个源码行占多行，
                // 行号随之忽上忽下（用户反馈的"跳动"）。
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let mut job = egui::text::LayoutJob::default();
                    // 行距放大到字号的 1.4 倍：行与行（含空行）之间的间隔
                    // 更舒展，行号数字不再显得挤在一起。
                    let mut fmt = theme.mono_fmt();
                    fmt.line_height = Some(theme.mono * 1.4);
                    job.append(buf.as_str(), 0.0, fmt);
                    job.wrap.max_width = f32::INFINITY;
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    rows_state.borrow_mut().clear();
                    let mut acc = 0usize;
                    for row in &galley.rows {
                        let start_char = acc;
                        acc += row.char_count_including_newline().0;
                        let line_no = match line_starts.binary_search(&start_char) {
                            Ok(i) => i + 1,
                            Err(i) => i.min(line_starts.len()).max(1),
                        };
                        rows_state
                            .borrow_mut()
                            .push((line_no, row.pos.y, row.row.size.y));
                        let _ = start_char;
                    }
                    galley
                };
                // 长行宽度估算（ASCII 半宽、CJK 全宽）：编辑框按最长行撑宽，
                // 配合横向滚动；窄于视口时照常占满。
                let content_w = src
                    .lines()
                    .map(|l| {
                        let w: f32 = l
                            .chars()
                            .map(|c| if c.is_ascii() { 0.62 } else { 1.25 })
                            .sum();
                        w * theme.mono
                    })
                    .fold(0.0f32, f32::max);
                let edit_w = (ui.available_width() - gutter_w).max(content_w);

                // 当前光标行的整行淡色底：必须在 TextEdit 之前画才能垫在文字下层。
                if show_line_numbers {
                    let band = rows_state
                        .borrow()
                        .iter()
                        .find(|(l, _, _)| *l == cur_line)
                        .map(|(_, y, h)| (*y, *h));
                    if let Some((y, h)) = band {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::Pos2::new(vpl, ui.cursor().top() + y),
                                egui::Pos2::new(vpr, ui.cursor().top() + y + h),
                            ),
                            0.0,
                            theme.table_head,
                        );
                    }
                }
                let out = ui.horizontal(|ui| {
                    if show_line_numbers {
                        // 固定 70px 行号栏：扣掉水平 item_spacing，正文左缘正好落在
                        // 行号栏右缘；数字本身在下面按视口固定 x 补画。
                        let spacing = ui.spacing().item_spacing.x;
                        ui.add_space((gutter_w - spacing).max(0.0));
                    }
                    let te = TextEdit::multiline(&mut src)
                        .id(Id::new(("source_editor", tab_key)))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(edit_w)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::ZERO)
                        .layouter(&mut layouter)
                        .code_editor();
                    te.show(ui)
                });
                let out = out.inner;
                // 行号：当帧 galley 单遍扫描，固定画在视口左缘（不随横向滚动移动）；
                // 光标所在行的行号用主题色高亮。
                if show_line_numbers {
                    let mut per_line: Vec<Option<f32>> = vec![None; line_starts.len() + 1];
                    let mut acc = 0usize;
                    for row in &out.galley.rows {
                        let start_char = acc;
                        acc += row.char_count_including_newline().0;
                        let line_no = match line_starts.binary_search(&start_char) {
                            Ok(i) => i + 1,
                            Err(i) => i.min(line_starts.len()),
                        };
                        if per_line.get(line_no).map_or(false, |s| s.is_none()) {
                            per_line[line_no] = Some(row.pos.y + row.row.size.y * 0.5);
                        }
                    }
                    let p = ui.painter();
                    // 数字右对齐到 gutter 右缘内固定间距，与混合模式完全一致。
                    let num_right = vpl + gutter_w - GUTTER_PAD;
                    for (line_no, y) in per_line.iter().enumerate().skip(1) {
                        if let Some(y) = y {
                            let color = if line_no == cur_line {
                                theme.accent
                            } else {
                                theme.dim.gamma_multiply(0.7)
                            };
                            p.text(
                                egui::Pos2::new(num_right, out.galley_pos.y + y),
                                egui::Align2::RIGHT_CENTER,
                                &line_no.to_string(),
                                egui::FontId::monospace(theme.mono),
                                color,
                            );
                        }
                    }
                }
                changed = out.response.changed();
                editor_rect = out.response.rect;
                out
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
    // 松开鼠标即清掉按下锚点：只在一次拖拽期间有效。
    if !ctx.input(|i| i.pointer.primary_down()) {
        state.press_caret = None;
    }
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
        Some(b) => block_editable_text(b),
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
    // 引用块左侧竖线的矩形：编辑态与渲染态返回的 resp.rect 基准不同
    // （编辑态是整个横向区域、渲染态只是文字），在这里统一换算成竖线本身，
    // 帧末用它把相邻引用块的竖线补接起来。
    let mut quote_bars: Vec<(usize, egui::Rect)> = Vec::new();
    // 列表项编辑态按下 Tab / Shift+Tab：记下方向，帧末统一调整缩进层级。
    let mut list_tab: Option<bool> = None;

    let scroll_out = egui::ScrollArea::vertical()
        .id_salt("editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let blocks = &doc.blocks;
            // 每个块起始的源码行号（Document::source 以 \n 连接各块源码）。
            // 用于编辑区左侧的行号显示。
            let mut block_start_lines = Vec::with_capacity(blocks.len());
            let mut line_acc = 1usize;
            for b in blocks.iter() {
                block_start_lines.push(line_acc);
                line_acc += b.source().split('\n').count();
            }
            // 正文宽度随窗口浮动，固定占可用宽度的约 90%；不设死宽度，
            // 所有块都按这个宽度换行，窄窗口下也不会把长段落横向裁掉。
            // 行号栏固定锚定在编辑区最左侧（70px，与源码模式共用），不参与版心
            // 居中，因此全窗口 / 部分窗口、源码 / 混合 / 专注模式下栏宽都恒定。
            let outer_w = ui.available_width();
            let page_w = (outer_w * 0.9).max(200.0);
            let gutter_w = gutter_width(show_line_numbers);
            let viewport_left = ui.clip_rect().left();
            ui.horizontal_top(|ui| {
                // 显示行号时行号栏从最左侧开始，正文紧随其后；关闭行号时保持版心居中。
                let pad = if show_line_numbers {
                    0.0
                } else {
                    ((outer_w - page_w) / 2.0).max(0.0)
                };
                // 扣掉水平 item_spacing，让正文左缘正好落在行号栏右缘。
                let spacing = ui.spacing().item_spacing.x;
                ui.add_space((pad + gutter_w - spacing).max(0.0));
                ui.vertical(|ui| {
                    ui.set_width(page_w - gutter_w);
                    // 拖选时把越界指针钳进视口（与 render.rs 的 text_response 一致）：
                    // 鼠标甩到可视区域外时，越界点可能落在块间空档，钳到边缘才能
                    // 持续命中块、让选区跟着自动滚动一直延伸。但按下的点必须在
                    // 视口内，否则点工具条会被钳成「点了最上面的代码块」。
                    let raw_pos = ctx.input(|i| i.pointer.interact_pos());
                    let clip = ui.clip_rect();
                    let in_viewport = raw_pos.is_some_and(|p| clip.contains(p));
                    let drag_pos = raw_pos.map(|p| {
                        if in_viewport {
                            p
                        } else {
                            egui::Pos2::new(p.x, p.y.clamp(clip.top() + 1.0, clip.bottom() - 1.0))
                        }
                    });
                    // 行号统一画在编辑区左侧固定列里，正文左缘不可用 resp.rect.left()
                    // 推：列表/任务项的 resp 是去掉行首缩进与标记（复选框/圆点/编号）
                    // 后的内容 Label，按它会算出落在正文里的 x，行号就叠进任务列表了。
                    for idx in 0..blocks.len() {
                        let block = &blocks[idx];
                        let compact = matches!(block, Block::ListItem { .. } | Block::Blank { .. });
                        // 连续引用行之间只留最小间距，左侧竖线才能连成一条。
                        // 引用里的代码块同样算作引用行，竖线保持连续。
                        let quote_run = quote_level_of(block).is_some()
                            && blocks
                                .get(idx + 1)
                                .is_some_and(|b| quote_level_of(b).is_some());
                        let is_active = idx == active;
                        // 跨块选区进行中时，活动块也按渲染态绘制：否则它渲染成
                        // TextEdit、既不能作为拖选终点、也不参与选区高亮，
                        // 表现为「选区在这一块断掉」。
                        let editing = is_active && !state.read_only && state.cross_sel.is_none();

                        // 代码块/图表/表格等不经过 text_response，没有自己的文字
                        // 选区高亮。它们多半画在不透明面板里（panel_frame 自带底色），
                        // 垫在下面的蓝底会被面板底色盖住，所以改为在块内容之上叠加
                        // 一层半透明选区色——文字依然清晰，又能看出这块被选中了。
                        let sel_overlay = (!editing
                            && !text_selection_block(block)
                            && atomic_block_selected(state.cross_sel, idx))
                        .then(|| {
                            let c = theme.selection;
                            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110)
                        });

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
                            // 功能区背景与渲染态一致：浅色 #F2F2F4 / 深色 panel
                            let tb_fill = if theme.kind == crate::theme::ThemeKind::Light {
                                egui::Color32::from_rgb(0xF2, 0xF2, 0xF4)
                            } else {
                                theme.panel
                            };
                            // 光标所在单元格要在借用 draft 前读出（Copy 类型），
                            // 否则 state 同时被可变借用 + 读取会编译失败
                            let focus = state.table_focus;
                            let inner = frame.show(ui, |ui| {
                                let mut deleted = false;
                                if let Some((_, grid, aligns)) = state.table_draft.as_mut() {
                                    // ---------------- 功能区（常驻） ----------------
                                    // 行/列操作基准 = 光标所在单元格（table_focus），
                                    // 其次最近悬停记忆，最后默认尾部单元格。
                                    let cell_key = Id::new(("table_cell_ref", idx));
                                    let (hr, hc) = focus
                                        .or_else(|| {
                                            ui.ctx().memory(|m| {
                                                m.data.get_temp::<(usize, usize)>(cell_key)
                                            })
                                        })
                                        .unwrap_or((
                                            grid.len().saturating_sub(1),
                                            grid.first()
                                                .map(|r| r.len())
                                                .unwrap_or(1)
                                                .saturating_sub(1),
                                        ));
                                    let mut op: Option<TableOp> = None;
                                    egui::Frame::NONE
                                        .fill(tb_fill)
                                        .corner_radius(3.0)
                                        .inner_margin(4.0)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                        ui.style_mut().spacing.item_spacing.x = 4.0;
                                        let btn = |ui: &mut egui::Ui, label: &str, active: bool| {
                                            let txt =
                                                egui::RichText::new(label).size(theme.body * 0.85);
                                            let txt =
                                                if active { txt.color(theme.accent) } else { txt };
                                            ui.button(txt).clicked()
                                        };
                                        let all =
                                            |a: crate::ast::Align| aligns.iter().all(|x| *x == a);
                                        use crate::ast::Align;
                                        if btn(ui, "+ 行", false) {
                                            op = Some(TableOp::InsertRow(hr));
                                        }
                                        if btn(ui, "− 行", false) {
                                            op = Some(TableOp::DeleteRow(hr));
                                        }
                                        if btn(ui, "+ 列", false) {
                                            op = Some(TableOp::InsertCol(hc));
                                        }
                                        if btn(ui, "− 列", false) {
                                            op = Some(TableOp::DeleteCol(hc));
                                        }
                                        ui.separator();
                                        if btn(ui, "左对齐", all(Align::Left)) {
                                            op = Some(TableOp::SetAlign(Align::Left));
                                        }
                                        if btn(ui, "居中", all(Align::Center)) {
                                            op = Some(TableOp::SetAlign(Align::Center));
                                        }
                                        if btn(ui, "右对齐", all(Align::Right)) {
                                            op = Some(TableOp::SetAlign(Align::Right));
                                        }
                                        // 最右侧右上角：删除表格（最右）+ 复制，与渲染态一致
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .button(
                                                        egui::RichText::new("删除表格")
                                                            .size(theme.body * 0.85),
                                                    )
                                                    .clicked()
                                                {
                                                    op = Some(TableOp::Delete);
                                                }
                                                if ui
                                                    .button(
                                                        egui::RichText::new("复制")
                                                            .size(theme.body * 0.85),
                                                    )
                                                    .clicked()
                                                {
                                                    ctx.copy_text(table_source(grid, aligns));
                                                    actions.push(render::Action::Notify(
                                                        "复制成功".to_string(),
                                                    ));
                                                }
                                            },
                                        );
                                        if let Some(op) = op {
                                            match op {
                                                TableOp::Delete => deleted = true,
                                                other => {
                                                    if table_op_on_grid(grid, aligns, other) {
                                                        edited = true;
                                                    }
                                                }
                                            }
                                        }
                                    });
                                        });
                                    ui.add_space(2.0);
                                    // 列宽与渲染态保持一致：平分可用宽度。
                                    // 以前单元格固定 140px，一点进编辑整张表就缩水成一小块。
                                    let cols = grid.first().map(|r| r.len()).unwrap_or(1).max(1);
                                    let gap =
                                        ui.spacing().item_spacing.x * cols.saturating_sub(1) as f32;
                                    let col_w = ((ui.available_width() - gap).max(120.0)
                                        / cols as f32)
                                        .max(56.0);
                                    ui.set_min_width(col_w * cols as f32 + gap);
                                    // 各行单元格矩形：Grid 之后画标题行补缝与行/列分隔线，
                                    // 与渲染态（render.rs 表格分支）同一套画法。
                                    let mut row_rects: Vec<Vec<egui::Rect>> = Vec::new();
                                    egui::Grid::new(("table_edit", idx))
                                        .min_col_width(col_w)
                                        .max_col_width(col_w)
                                        .show(ui, |ui| {
                                            for (ri, row) in grid.iter_mut().enumerate() {
                                                let mut cell_rects: Vec<egui::Rect> = Vec::new();
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
                                                            // 按表格对齐方式排版单元格文字：之前编辑态的
                                                            // 单元格永远左对齐，点「居中/右对齐」看不到
                                                            // 任何变化，像是按钮失效（实际上源码已改）。
                                                            .align(
                                                                match aligns
                                                                    .get(ci)
                                                                    .copied()
                                                                    .unwrap_or(
                                                                        crate::ast::Align::Left,
                                                                    ) {
                                                                    crate::ast::Align::Left => {
                                                                        egui::Align2::LEFT_TOP
                                                                    }
                                                                    crate::ast::Align::Center => {
                                                                        egui::Align2::CENTER_TOP
                                                                    }
                                                                    crate::ast::Align::Right => {
                                                                        egui::Align2::RIGHT_TOP
                                                                    }
                                                                },
                                                            )
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
                                                    cell_rects.push(r.rect);
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
                                                row_rects.push(cell_rects);
                                                ui.end_row();
                                            }
                                        });
                                    // 与渲染态一致：标题行底色铺满整行 + 行/列之间画单线分隔。
                                    // 之前编辑态只有外框描边、单元格又是同色填充，
                                    // 一点进表格格子之间的线就全没了。
                                    let p = ui.painter();
                                    if let Some(head) = row_rects.first()
                                        && head.len() == cols
                                    {
                                        let top =
                                            head.iter().map(|x| x.top()).fold(f32::MAX, f32::min);
                                        let bottom = head
                                            .iter()
                                            .map(|x| x.bottom())
                                            .fold(f32::MIN, f32::max);
                                        for k in 1..cols {
                                            let left = head[k - 1].right();
                                            let right = head[k].left();
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
                                    let stroke = egui::Stroke::new(1.0, theme.table_line);
                                    // 行分隔线：相邻两行单元格之间的中线
                                    for i in 1..row_rects.len() {
                                        let next_top = row_rects[i].first().map(|x| x.top());
                                        let prev_bottom = row_rects[i - 1]
                                            .iter()
                                            .map(|x| x.bottom())
                                            .fold(f32::MIN, f32::max);
                                        if let Some(t) = next_top {
                                            let y = (t + prev_bottom) / 2.0;
                                            let left = row_rects[i]
                                                .iter()
                                                .map(|x| x.left())
                                                .fold(f32::MAX, f32::min);
                                            let right = row_rects[i]
                                                .iter()
                                                .map(|x| x.right())
                                                .fold(f32::MIN, f32::max);
                                            p.line_segment(
                                                [
                                                    egui::Pos2::new(left, y),
                                                    egui::Pos2::new(right, y),
                                                ],
                                                stroke,
                                            );
                                        }
                                    }
                                    // 列分隔线：相邻单元格右/左缘的中点，纵贯整个网格
                                    let grid_top =
                                        row_rects.first().and_then(|r| r.first()).map(|x| x.top());
                                    let grid_bottom = row_rects
                                        .last()
                                        .and_then(|r| r.first())
                                        .map(|x| x.bottom());
                                    if let (Some(top), Some(bottom)) = (grid_top, grid_bottom) {
                                        let mut xs: Vec<f32> = Vec::new();
                                        for row in &row_rects {
                                            for k in 1..row.len() {
                                                let x = (row[k - 1].right() + row[k].left()) / 2.0;
                                                if !xs.iter().any(|v| (*v - x).abs() < 0.5) {
                                                    xs.push(x);
                                                }
                                            }
                                        }
                                        for x in xs {
                                            p.line_segment(
                                                [
                                                    egui::Pos2::new(x, top),
                                                    egui::Pos2::new(x, bottom),
                                                ],
                                                stroke,
                                            );
                                        }
                                    }
                                }
                                // 删除表格：清空草稿并把写回源码置空（整块变空段落）
                                if deleted {
                                    state.table_draft = None;
                                    table_edit_src = Some(String::new());
                                    changed = true;
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
                            let id = Id::new(("block_edit", tab_key, idx));
                            // 标题只编辑文字、代码块只编辑代码（不含围栏），像 Typora 一样直接改。
                            // 代码块的行数 + 1 对齐渲染态：渲染态的 code_job 会在末尾
                            // 补一个换行（多一行视觉空行），编辑态用 desired_rows 预留
                            // 同样高度，点击进入/退出编辑时内容不跳。
                            let rows = if matches!(edit_kind, EditKind::Code { .. }) {
                                active_buf.lines().count().max(1) + 1
                            } else if block.is_multiline() {
                                2
                            } else {
                                1
                            };
                            // 整段只有图片（一张或多张连续图片）时，编辑态在上方
                            // 原样露出 `![文件名](路径)` 源码、下方保留图片预览；
                            // 点击别处就退出编辑，只剩图片。
                            let image_blocks: Option<Vec<(String, String, Option<u32>)>> =
                                match block {
                                    Block::Paragraph { content, .. } => {
                                        let has_image = content
                                            .iter()
                                            .any(|i| matches!(i, crate::ast::Inline::Image { .. }));
                                        let only_images = has_image
                                            && content.iter().all(|i| {
                                                matches!(
                                                    i,
                                                    crate::ast::Inline::Image { .. }
                                                        | crate::ast::Inline::Break
                                                )
                                            });
                                        only_images.then(|| {
                                            content
                                                .iter()
                                                .filter_map(|i| match i {
                                                    crate::ast::Inline::Image {
                                                        alt,
                                                        url,
                                                        width,
                                                    } => Some((alt.clone(), url.clone(), *width)),
                                                    _ => None,
                                                })
                                                .collect()
                                        })
                                    }
                                    _ => None,
                                };
                            // 编辑态行内排版：缓冲区还是原始 Markdown，但标记被隐藏，
                            // 看起来就是渲染后的富文本，而不是源码。
                            // 光标位置传给排版器：紧挨光标的标记要显示出来，
                            // 不然在加粗文字旁边按 Shift+8 打的星号会看不见。
                            let caret = state.last_cursor;
                            // 上一帧的选区：行内代码被选中时用它把代码片底色换成选区色。
                            let sel = state.sel;
                            let mut layouter =
                                |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                    let job = render::inline_job(
                                        buf.as_str(),
                                        theme,
                                        theme.body,
                                        theme.text,
                                        wrap,
                                        Some(caret),
                                        sel,
                                    );
                                    ui.fonts_mut(|f| f.layout_job(job))
                                };
                            // 代码块编辑时也走高亮排版：缓冲区是纯代码，用同一套词法着色，
                            // 边打字边能看到关键字/字符串的颜色，而不是退化成一片纯文本。
                            let code_lang = match &edit_kind {
                                EditKind::Code { lang: l, .. } | EditKind::Diagram(l) => l.as_str(),
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
                                // 代码块：外层 panel_frame 提供盒子（按钮行 + 代码同盒，
                                // 与渲染态一致），TextEdit 自身不再带底色边距。
                                // 宽度对齐渲染态：盒子内可用宽 = page_w - 两侧边距 24。
                                EditKind::Code { .. } => te
                                    .font(egui::FontId::new(
                                        theme.mono,
                                        egui::FontFamily::Monospace,
                                    ))
                                    .text_color(theme.text)
                                    .layouter(&mut code_layouter)
                                    .desired_width((page_w - 24.0).max(80.0))
                                    .frame(egui::Frame::NONE),
                                EditKind::Diagram(_) => te
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
                                // 段落/列表：TextEdit 主题默认的垂直 margin 会让
                                // 编辑态比渲染态（Label 无 margin）高约 5px，点击进入
                                // 编辑时整块往下跳。归零后两态高度一致，块不再跳动。
                                EditKind::Source
                                | EditKind::List { .. }
                                | EditKind::Quote { .. } => {
                                    if image_blocks.is_some() {
                                        // 图片源码原样显示 `![文件名](路径)`：
                                        // 隐藏标记的 inline_job 会把地址折掉，
                                        // 看不到完整路径，不符合图片源码模式的预期。
                                        te.font(egui::FontId::new(
                                            theme.body * 0.85,
                                            egui::FontFamily::Monospace,
                                        ))
                                        .text_color(theme.dim)
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .return_key(Option::<egui::KeyboardShortcut>::None)
                                        .desired_width(page_w)
                                    } else {
                                        te.font(egui::FontId::new(
                                            theme.body,
                                            egui::FontFamily::Proportional,
                                        ))
                                        .frame(egui::Frame::NONE)
                                        .margin(egui::Margin::ZERO)
                                        .return_key(Option::<egui::KeyboardShortcut>::None)
                                        .layouter(&mut layouter)
                                    }
                                }
                            };
                            // 代码块编辑态：按钮行（删除/复制/语言）与代码同在一个
                            // panel_frame 盒子里，与渲染态完全一致——否则点击进入
                            // 编辑后按钮会"跳到"盒子右上方之外。
                            let (out, row_resp) = if let EditKind::Code { lang, quote } = &edit_kind
                            {
                                let quote_level = *quote;
                                let mut copied = false;
                                // 引用内的代码块：左侧留出缩进 + 竖线位置，
                                // 编辑时也保持引用竖线，与渲染态一致。
                                let hr = ui.horizontal(|ui| {
                                    if quote_level > 0 {
                                        ui.add_space(quote_level.min(4) as f32 * 8.0 + 11.0);
                                    }
                                    // 外层横向布局会传染给面板内容，显式回到纵向。
                                    let fr = ui
                                        .vertical(|ui| {
                                            render::panel_frame(theme).show(ui, |ui| {
                                                // 功能栏：左语言 + 右操作按钮（与渲染态同款布局）
                                                render::code_header_frame(theme).show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        if let Some(l) = render::language_combo(
                                                            ui,
                                                            ("code_lang_edit", tab_key, idx),
                                                            lang,
                                                        ) {
                                                            lang_change = Some(l);
                                                        }
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if ui
                                                                    .button(
                                                                        egui::RichText::new("折叠")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    actions.push(
                                                                        render::Action::FoldToggle(
                                                                            idx,
                                                                        ),
                                                                    );
                                                                    // 折叠的是活动块：把它让给相邻块，立即退出编辑态看到折叠效果。
                                                                    new_active = Some(
                                                                        if idx + 1
                                                                            < doc.blocks.len()
                                                                        {
                                                                            idx + 1
                                                                        } else {
                                                                            idx.saturating_sub(1)
                                                                        },
                                                                    );
                                                                }
                                                                if ui
                                                                    .button(
                                                                        egui::RichText::new("删除")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    actions.push(
                                                                render::Action::AskDeleteBlock(idx),
                                                            );
                                                                }
                                                                if ui
                                                                    .button(
                                                                        egui::RichText::new("复制")
                                                                            .size(
                                                                                theme.body * 0.85,
                                                                            ),
                                                                    )
                                                                    .on_hover_cursor(
                                                                        egui::CursorIcon::Default,
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    copied = true;
                                                                }
                                                            },
                                                        );
                                                    });
                                                });
                                                ui.add_space(6.0);
                                                render::code_divider(ui, theme);
                                                ui.add_space(6.0);
                                                te.show(ui)
                                            })
                                        })
                                        .inner;
                                    if quote_level > 0 {
                                        // 竖线高度跟随整个代码面板（含功能栏），
                                        // 连续引用时用帧末的补接逻辑连成一条。
                                        let rect = fr.response.rect;
                                        ui.painter().rect_filled(
                                            egui::Rect::from_min_max(
                                                egui::pos2(rect.left() - 11.0, rect.top()),
                                                egui::pos2(rect.left() - 8.0, rect.bottom()),
                                            ),
                                            1.0,
                                            theme.accent,
                                        );
                                    }
                                    fr
                                });
                                let fr = hr.inner;
                                if copied {
                                    ctx.copy_text(active_buf.clone());
                                    actions.push(Action::Notify("复制成功".to_string()));
                                }
                                (fr.inner, hr.response)
                            } else if let EditKind::List {
                                indent,
                                marker,
                                checked,
                            } = &edit_kind
                            {
                                let indent_px = (*indent).min(12) as f32 * 8.0;
                                let hr = ui.horizontal(|ui| {
                                    ui.add_space(indent_px);
                                    // 与渲染态一致：标记贴近正文、复选框与圆点放大。
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    match checked {
                                        Some(c) => {
                                            ui.spacing_mut().icon_width = 19.0;
                                            ui.spacing_mut().icon_width_inner = 11.0;
                                            ui.spacing_mut().icon_spacing = 3.0;
                                            let v = &mut ui.visuals_mut().widgets;
                                            v.inactive.bg_stroke =
                                                egui::Stroke::new(1.0, theme.dim);
                                            v.hovered.bg_stroke =
                                                egui::Stroke::new(1.0, theme.accent);
                                            v.active.bg_stroke =
                                                egui::Stroke::new(1.0, theme.accent);
                                            let mut cb_state = *c;
                                            let cb = ui.checkbox(&mut cb_state, "");
                                            if cb.clicked() {
                                                actions.push(render::Action::ToggleTask(idx));
                                            }
                                        }
                                        None => {
                                            // 与渲染态一致：层级的标记样式随缩进变化。
                                            let level = render::list_level(*indent);
                                            if marker.ends_with('.') || marker.ends_with(')') {
                                                let mut f = theme.fmt(theme.body);
                                                f.color = theme.dim;
                                                let mut job = egui::text::LayoutJob::default();
                                                job.append(
                                                    &render::ordered_list_marker(marker, level),
                                                    0.0,
                                                    f,
                                                );
                                                ui.label(job);
                                            } else {
                                                let d = (theme.body * 0.6).clamp(7.0, 14.0);
                                                let color =
                                                    theme.dim.lerp_to_gamma(theme.text, 0.55);
                                                render::draw_list_bullet(ui, level, color, d);
                                            }
                                        }
                                    }
                                    // Tab / Shift+Tab 在列表项里表示调整缩进层级，
                                    // 不能让它变成 \t 插进正文，也不能把焦点移走。
                                    let shift_tab = ui.input(|i| {
                                        i.events.iter().find_map(|e| match e {
                                            egui::Event::Key {
                                                key: Key::Tab,
                                                pressed: true,
                                                modifiers,
                                                ..
                                            } => Some(modifiers.shift),
                                            _ => None,
                                        })
                                    });
                                    if let Some(shift_tab) = shift_tab {
                                        list_tab = Some(shift_tab);
                                        ui.ctx().input_mut(|i| {
                                            i.events.retain(|e| {
                                                !matches!(
                                                    e,
                                                    egui::Event::Key {
                                                        key: Key::Tab,
                                                        pressed: true,
                                                        ..
                                                    }
                                                )
                                            });
                                        });
                                    }
                                    let te = te
                                        .event_filter(egui::EventFilter {
                                            tab: true,
                                            ..Default::default()
                                        })
                                        .desired_width((ui.available_width() - 8.0).max(80.0));
                                    te.show(ui)
                                });
                                (hr.inner, hr.response)
                            } else if let EditKind::Quote { level } = &edit_kind {
                                // 引用编辑态与渲染态一致：左侧竖线 + 缩进，正文加粗、
                                // 引用配色，不显示 `>` 源码标记。
                                let indent_px = (*level).min(4) as f32 * 8.0;
                                let bold_family = egui::FontFamily::Name("markd-bold".into());
                                let mut quote_layouter =
                                    |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                                        let mut job = render::inline_job(
                                            buf.as_str(),
                                            theme,
                                            theme.body,
                                            theme.quote,
                                            wrap,
                                            Some(caret),
                                            sel,
                                        );
                                        // 只把引用正文（普通文字，颜色为引用色）换成粗体；
                                        // 透明标记、链接、行内代码等保持原样式。
                                        for section in &mut job.sections {
                                            if section.format.font_id.family
                                                == egui::FontFamily::Proportional
                                                && section.format.color == theme.quote
                                            {
                                                section.format.font_id = egui::FontId::new(
                                                    theme.body,
                                                    bold_family.clone(),
                                                );
                                            }
                                        }
                                        ui.fonts_mut(|f| f.layout_job(job))
                                    };
                                let hr = ui.horizontal(|ui| {
                                    ui.add_space(indent_px);
                                    // 竖线先不占固定高度：给正文留出「线宽 + 间距」，
                                    // 等 TextEdit 排版完成后再按它的实际高度画，
                                    // 软换行/多行时竖线会一直延伸到最底部。
                                    ui.add_space(11.0);
                                    let te = te
                                        .text_color(theme.quote)
                                        .layouter(&mut quote_layouter)
                                        .desired_width((ui.available_width() - 8.0).max(80.0));
                                    let out = te.show(ui);
                                    let r = out.response.rect;
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(r.left() - 11.0, r.top()),
                                            egui::pos2(r.left() - 8.0, r.bottom()),
                                        ),
                                        1.0,
                                        theme.accent,
                                    );
                                    out
                                });
                                (hr.inner, hr.response)
                            } else {
                                // 图片块：上方露出 `![文件名](路径)` 源码，
                                // 下方保留图片预览（可拖动右下角缩放）；
                                // 点别处即退出这种模式。
                                let (out, preview) = if let Some(imgs) = &image_blocks {
                                    let mut prev: Option<egui::Response> = None;
                                    let out = ui
                                        .vertical(|ui| {
                                            let out = te.show(ui);
                                            ui.add_space(2.0);
                                            for (no, (alt, url, width)) in imgs.iter().enumerate() {
                                                let (r, resized) = render::image_preview(
                                                    ui,
                                                    alt,
                                                    url,
                                                    theme,
                                                    ctx,
                                                    images,
                                                    base_dir,
                                                    *width,
                                                    (idx, no),
                                                );
                                                if let Some(w) = resized {
                                                    actions.push(render::Action::ResizeImage(
                                                        idx, no, w,
                                                    ));
                                                }
                                                prev = Some(match prev.take() {
                                                    Some(p) => p.union(r),
                                                    None => r,
                                                });
                                            }
                                            out
                                        })
                                        .inner;
                                    (out, prev)
                                } else {
                                    (te.show(ui), None)
                                };
                                let mut row = out.response.response.clone();
                                if let Some(p) = preview {
                                    row = row.union(p);
                                }
                                (out, row)
                            };
                            // 编辑态里 Cmd/Ctrl+单击链接才打开；普通点击只把光标
                            // 放进链接，并露出 `[标签](地址)` 源码供修改。
                            if out.response.response.clicked()
                                && ui.input(|i| i.modifiers.command || i.modifiers.ctrl)
                            {
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
                            // 活动块（含空行）起拖由 text_response 的兜底分支接管：
                            // 拖出本块落到别的渲染态块时以光标为锚点建立跨块选区。
                            // 这里不再在按下帧预置选区，否则普通点击会残留。
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
                            // 在编辑态块内按下鼠标 → 记下按下位置，作为从本块
                            // 拖出时的跨块选区锚点（否则锚点会变成拖出时已到行尾的光标）。
                            let pressed_now = ui
                                .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
                            if pressed_now
                                && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                                && resp.response.rect.contains(p)
                            {
                                // 用点击位置直接在 galley 上映射字符：按下那一帧
                                // TextEdit 可能还没返回 cursor_range，cursor_idx
                                // 仍是初始值（行尾），会把锚点记错。
                                let ci = out.galley.cursor_from_pos(p - out.galley_pos).index.0;
                                state.press_caret = Some((idx, ci.min(len)));
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
                            // 整行响应 = TextEdit ∪ 行首标记区：点复选框/圆点不算
                            // 「点在空白」，否则空白点击会把活动块切到文档末尾。
                            resp.response.union(row_resp)
                        } else {
                            let mut env = RenderEnv {
                                theme,
                                ctx,
                                images,
                                base_dir,
                                outline,
                                actions,
                                max_width: page_w,
                                cross_sel: state.cross_sel,
                                folded: &state.folded,
                            };
                            render::render_block(ui, idx, block, &mut env, focus_mode)
                        };
                        // 非文本块的选区叠加层：必须画在块内容之后（面板底色会盖住
                        // 画在前面的形状），半透明所以不影响阅读。
                        if let Some(color) = sel_overlay {
                            if resp.rect.is_positive() {
                                ui.painter().rect_filled(resp.rect.expand(1.0), 3.0, color);
                            }
                        }
                        // 非文本块也参与跨块拖选：指针拖到它上面时把选区端点延伸进来。
                        // 这类块没有可映射的字符位置，按「整块」计——向下拖取块尾、
                        // 向上拖取块首，保证选区能跨过代码块/图表继续延伸。
                        if !editing && !text_selection_block(block) && resp.rect.is_positive() {
                            let over = drag_pos.is_some_and(|p| resp.rect.contains(p));
                            let down = ctx.input(|i| i.pointer.primary_down());
                            let pressed = ctx
                                .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
                            let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
                            let len = block.plain().chars().count();
                            if over && down {
                                // 只有代码块/图表能在按下时开启选区。表格等仍需保留
                                // 「点击进入单元格编辑」，不能一按下就切到选区状态。
                                if pressed
                                    && in_viewport
                                    && matches!(block, Block::Code { .. } | Block::Diagram { .. })
                                {
                                    actions.push(Action::CrossDrag(idx, 0, true));
                                } else if !pressed && let Some((a, _, _, _)) = state.cross_sel {
                                    actions.push(Action::CrossDrag(
                                        idx,
                                        if idx >= a { len } else { 0 },
                                        false,
                                    ));
                                } else if !pressed && in_viewport && dragging {
                                    // 起点在活动块的 TextEdit 里：以那里为锚点。
                                    actions.push(Action::ShiftClick(
                                        idx,
                                        if idx >= state.active { len } else { 0 },
                                    ));
                                }
                            }
                        }
                        // 行号：画在编辑区最左侧固定 70px 行号栏内，右缘固定缩进，
                        // 不随窗口宽度（版心居中）或模式移动。
                        if show_line_numbers {
                            let num_x = viewport_left + gutter_w - GUTTER_PAD;
                            if num_x - theme.body > ui.clip_rect().left() {
                                ui.painter().text(
                                    egui::Pos2::new(num_x, resp.rect.top() + theme.body * 0.5),
                                    egui::Align2::RIGHT_CENTER,
                                    &block_start_lines[idx].to_string(),
                                    egui::FontId::proportional(theme.body),
                                    theme.dim.gamma_multiply(0.75),
                                );
                            }
                        }
                        if resp.rect.is_positive() {
                            block_rects.push((idx, resp.rect));
                            if let Some(level) = quote_level_of(block) {
                                // 渲染态 resp 是文字/面板矩形（竖线在左侧 11px 处）；
                                // 编辑态 resp 是整条横向区域（文字前留了缩进 + 11px）。
                                let x = if editing {
                                    resp.rect.left() + level.min(4) as f32 * 8.0
                                } else {
                                    resp.rect.left() - 11.0
                                };
                                quote_bars.push((
                                    idx,
                                    egui::Rect::from_min_max(
                                        egui::pos2(x, resp.rect.top()),
                                        egui::pos2(x + 3.0, resp.rect.bottom()),
                                    ),
                                ));
                            }
                        }
                        if resp.clicked() {
                            new_active = Some(idx);
                            state.clicked = true;
                            // 点击进入编辑：清除跨块选区（拖选才产生它）
                            if state.cross_sel.is_some() {
                                state.cross_sel = None;
                            }
                        }
                        if is_active {
                            active_rect = Some(resp.rect);
                            if !editing {
                                state.sel = None;
                            }
                        }
                        if quote_run {
                            ui.add_space(1.0);
                        } else {
                            ui.add_space(if compact { 1.0 } else { 6.0 });
                        }
                    }
                    // 连续引用的竖线要连成一条：egui 会在相邻块之间自动留
                    // item_spacing，单块各自的竖线在块间距处会断开。这里用同色
                    // 小矩形把相邻两个引用块竖线之间的空隙补上。
                    for pair in quote_bars.windows(2) {
                        let ((i, a), (j, b)) = (pair[0], pair[1]);
                        if j != i + 1 {
                            continue;
                        }
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(a.left(), a.bottom()),
                                egui::pos2(a.left() + 3.0, b.top() + 1.0),
                            ),
                            0.0,
                            theme.accent,
                        );
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

            // 跨块拖选聚合：渲染态块上报的 CrossDrag 在这里聚成全局选区，
            // 并从 actions 中移除（app 层无需关心）。拖选开始时放弃编辑态
            // 焦点，否则活动块的 TextEdit 会抢先消费键盘输入。
            // 双击选词 / 三击选整段 / Shift+点击扩选 也在此聚成 cross_sel。
            actions.retain(|a| match a {
                Action::CrossDrag(i, c, start) => {
                    if *start {
                        state.cross_sel = Some((*i, *c, *i, *c));
                        if let Some(fid) = ctx.memory(|m| m.focused()) {
                            ctx.memory_mut(|m| m.surrender_focus(fid));
                        }
                    } else if let Some((a, ac, _, _)) = state.cross_sel {
                        state.cross_sel = Some((a, ac, *i, *c));
                    }
                    false
                }
                // 双击选词：把跨块选区直接设成词区间
                Action::WordSel(i, a, z) => {
                    state.cross_sel = Some((*i, *a, *i, *z));
                    false
                }
                // 三击选中整块（整行/整段）
                Action::BlockSel(i) => {
                    let len = doc
                        .blocks
                        .get(*i)
                        .map(|b| b.plain().chars().count())
                        .unwrap_or(0);
                    state.cross_sel = Some((*i, 0, *i, len));
                    false
                }
                // Shift+点击：有选区时保持锚点、把终点移到点击处；
                // 无选区时从光标（活动块 + 光标字符）选到点击处
                Action::ShiftClick(i, c) => {
                    if let Some((a, ac, _, _)) = state.cross_sel {
                        state.cross_sel = Some((a, ac, *i, *c));
                    } else {
                        let buf_len = doc
                            .blocks
                            .get(active)
                            .map(|b| b.plain().chars().count())
                            .unwrap_or(0);
                        // 优先用「按下鼠标时的位置」当锚点（从编辑态段落拖出的场景），
                        // 没有记录时退回当前光标。
                        let anchor = state
                            .press_caret
                            .filter(|(b, _)| *b == active)
                            .map(|(_, c)| c.min(buf_len))
                            .unwrap_or_else(|| state.last_cursor.min(buf_len));
                        state.cross_sel = Some((active, anchor, *i, *c));
                        // 从编辑态的 TextEdit 里拖出来时，放弃它的焦点，
                        // 否则 TextEdit 会继续吃键盘/拖拽、和跨块选区打架。
                        if let Some(fid) = ctx.memory(|m| m.focused()) {
                            ctx.memory_mut(|m| m.surrender_focus(fid));
                        }
                    }
                    false
                }
                _ => true,
            });
            // 拖选越界自动滚动：按住左键拖到编辑区可视范围之外时，
            // 按超出距离持续滚动，选区随之延伸（Typora / VS Code 行为）。
            // 指针停住不动时系统不再送鼠标事件，必须主动请求重绘，
            // 否则只会滚一帧就停住（用户反馈的「拖到底不会继续滚」）。
            // 用 scroll_to_rect 而不是 scroll_with_delta：后者的位移在
            // ScrollArea 里不会生效（实测 offset 始终为 0）。
            // press_caret 也要算进来：在活动段落（TextEdit）里一直往下拖、
            // 段落本身比视口高时还没有跨块选区，但同样应该自动滚动。
            if (state.cross_sel.is_some() || state.press_caret.is_some())
                && ctx.input(|i| i.pointer.primary_down())
                && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
            {
                let view = ui.clip_rect();
                let (overflow, below) = if p.y > view.bottom() {
                    (p.y - view.bottom(), true)
                } else if p.y < view.top() {
                    (view.top() - p.y, false)
                } else {
                    (0.0, false)
                };
                if overflow > 0.0 {
                    // 越界越远滚得越快。
                    let step = (overflow * 0.6).clamp(3.0, 40.0);
                    let y = if below {
                        view.bottom() + step
                    } else {
                        view.top() - step
                    };
                    // 在视口边缘外放一个 1px 的虚拟目标，让 ScrollArea 把内容滚过去。
                    let target =
                        egui::Rect::from_min_size(egui::pos2(view.left(), y), egui::vec2(1.0, 1.0));
                    ui.scroll_to_rect_animation(target, None, egui::style::ScrollAnimation::none());
                    ctx.request_repaint();
                }
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
        // 空白点击：清除跨块选区（拖选才产生它）
        state.cross_sel = None;
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

    // ---------------- 跨块选区：输入替换 / 退格删除 ----------------
    // 拖选出的跨块选区存在时，键盘输入优先作用于选区（Typora 行为）：
    // 打字 → 删除选中内容并插入新文字；退格 → 删除选中内容；
    // 选区由拖选建立时编辑框已失焦，不会与 TextEdit 的输入处理冲突。
    if state.cross_sel.is_some() {
        let typed: Option<String> = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) => Some(t.clone()),
                _ => None,
            })
        });
        if backspace || typed.is_some() {
            apply_cross_sel_edit(doc, state, typed.as_deref());
            ctx.request_repaint();
            return EditorEvent::Changed;
        }
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
        && matches!(edit_kind, EditKind::Code { .. } | EditKind::Diagram(_))
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
        // 列表项（含任务项）/ 引用块：编辑缓冲已去掉行首标记，不能用
        // inherited_marker(buf)，要从 EditKind 拿到当前标记来继承。
        // Typora 行为：非空项回车继承同类标记；空项再回车退出该块级格式。
        let (marker, exit_prefix) = match &edit_kind {
            EditKind::List {
                indent,
                marker,
                checked,
                ..
            } => {
                // 已勾选的任务项回车，新项从「未勾选」开始（Typora 行为）；
                // 有序列表回车时序号自动 +1。
                let inherited = if checked == &Some(true) {
                    replace_task_checkbox(marker)
                } else if checked.is_none() {
                    next_ordered_marker(marker)
                } else {
                    marker.clone()
                };
                let prefix = format!("{}{} ", " ".repeat(*indent), inherited);
                if buf.trim().is_empty() {
                    // 空列表项回车：当前块去掉标记变普通段落，不再插入新块
                    (String::new(), true)
                } else {
                    (prefix, false)
                }
            }
            EditKind::Quote { level } => {
                if buf.trim().is_empty() {
                    // 空引用回车：当前块去掉 `>` 变普通段落，不再插入新块
                    (String::new(), true)
                } else {
                    // 新行继承相同引用层级（连续 `>`）。
                    (format!("{} ", ">".repeat((*level).max(1))), false)
                }
            }
            EditKind::Source => (inherited_marker(&buf), false),
            _ => (String::new(), false),
        };
        doc.record();
        if exit_prefix {
            // 退出列表 / 引用：当前空块直接写回正文（无标记），光标留在原块
            doc.edit_block(active, buf.trim_end());
            state.focus_req = Some(active);
            state.last_cursor = 0;
            return EditorEvent::Changed;
        }
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
        if let Some(shift_tab) = list_tab {
            // 列表项：Tab 增加一级缩进（成为子列表），Shift+Tab 退回上一级。
            if let EditKind::List {
                indent,
                marker,
                checked,
            } = &edit_kind
            {
                let step = state.tab_size.max(1);
                let new_indent = if shift_tab {
                    indent.saturating_sub(step)
                } else {
                    indent + step
                };
                if new_indent != *indent {
                    doc.record();
                    let ek = EditKind::List {
                        indent: new_indent,
                        marker: marker.clone(),
                        checked: *checked,
                    };
                    doc.edit_block(active, &ek.rebuild(&active_buf));
                    // 保持焦点与光标：缩进只改源码前缀，正文缓冲不变。
                    state.focus_req = Some(active);
                    return EditorEvent::Changed;
                }
            }
        } else if !matches!(edit_kind, EditKind::List { .. }) {
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
    }

    EditorEvent::None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 折叠状态往返：source 键不随下标变化，折叠块之前插入块不会导致错位。
    #[test]
    fn fold_keys_indices_roundtrip() {
        let src = "# 标题\n\n```rust\nfn a() {}\n```\n\n正文\n\n```rust\nfn b() {}\n```";
        let doc = Document::new(src, None);
        let code_idx: Vec<usize> = doc
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b, Block::Code { .. }))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(code_idx.len(), 2, "应有且仅有两个代码块: {code_idx:?}");

        // 折叠第二个代码块 → 得到它的 source 键。
        let mut folded = HashSet::new();
        folded.insert(code_idx[1]);
        let keys = fold_keys(&doc, &folded);
        assert_eq!(keys.len(), 1, "只有一个折叠代码块: {keys:?}");
        assert!(
            keys[0].contains("fn b()"),
            "键应指向 fn b 的代码块: {keys:?}"
        );

        // 在该代码块之前插入一个段落，其下标后移一位，但 source 键不变，
        // 还原时仍应命中 fn b 的代码块（下标已变）。
        let mut doc2 = Document::new(src, None);
        doc2.insert_block_after(code_idx[1] - 1, "插入的段落");
        let restored = fold_indices(&doc2, &keys);
        assert_eq!(restored.len(), 1, "应恰好还原一个折叠块: {restored:?}");
        let ri = *restored.iter().next().unwrap();
        assert!(
            doc2.blocks[ri].source().contains("fn b()"),
            "source 匹配应抵消下标偏移，实际命中: {:?}",
            doc2.blocks[ri].source()
        );

        // 空键集合应还原成空集。
        assert!(fold_indices(&doc2, &[]).is_empty());
    }

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
                    ui, doc, state, theme, &c, images, None, outline, false, false, false, false,
                    1, actions,
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
                            let folded: HashSet<usize> = HashSet::new();
                            let mut env = RenderEnv {
                                theme: &theme,
                                ctx: &ctx_q,
                                images: &mut images,
                                base_dir: None,
                                outline: &[],
                                actions: &mut actions,
                                max_width: page_w,
                                cross_sel: None,
                                folded: &folded,
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

        let src = EditKind::Code {
            lang: "rust".to_string(),
            quote: 0,
        }
        .rebuild("fn main() {}");
        assert_eq!(src, "```rust\nfn main() {}\n```");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { lang, code, .. } if lang == "rust" && code == "fn main() {}"),
            "代码块写回后应能还原成同一段代码"
        );

        // 多行代码同样可往返
        let src = EditKind::Code {
            lang: String::new(),
            quote: 0,
        }
        .rebuild("a\nb");
        let blocks = crate::ast::parse(&src);
        assert!(matches!(&blocks[0], Block::Code { code, .. } if code == "a\nb"));

        // 末尾空行（缓冲以 \n 结尾）也要能往返：在末行回车产生的空行不丢失
        let src = EditKind::Code {
            lang: "rust".to_string(),
            quote: 0,
        }
        .rebuild("a\n");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { code, .. } if code == "a\n"),
            "{src}"
        );
        let src = EditKind::Code {
            lang: "rust".to_string(),
            quote: 0,
        }
        .rebuild("\n");
        let blocks = crate::ast::parse(&src);
        assert!(
            matches!(&blocks[0], Block::Code { code, .. } if code == "\n"),
            "{src}"
        );
    }

    /// 引用块进入编辑态时隐藏 `>`，保留行内 Markdown；写回时补回相同层级。
    #[test]
    fn quote_edit_hides_marker_and_roundtrips() {
        let doc = Document::new("> 引用 **加粗** 文字", None);
        let b = &doc.blocks[0];
        assert!(
            matches!(EditKind::of(b), EditKind::Quote { level: 1 }),
            "引用应识别为 Quote 编辑类型"
        );
        assert_eq!(block_buffer(b), "引用 **加粗** 文字", "编辑缓冲不应含 `>`");
        assert_eq!(
            rebuild_source(b, &block_buffer(b)),
            "> 引用 **加粗** 文字",
            "写回应补回 `>` 且不丢失行内格式"
        );

        // 多级引用：连续 `>` 的层级要原样保留
        let doc = Document::new(">> 嵌套引用", None);
        let b = &doc.blocks[0];
        assert!(matches!(EditKind::of(b), EditKind::Quote { level: 2 }));
        assert_eq!(block_buffer(b), "嵌套引用");
        assert_eq!(rebuild_source(b, "嵌套引用"), ">> 嵌套引用");
    }

    /// 引用按钮是开关：已是引用的块再点一次变回普通段落，普通段落点一次变引用。
    #[test]
    fn quote_command_toggles_quote_off() {
        let mut doc = Document::new("引用文字", None);
        let mut state = EditorState::default();
        state.active = 0;
        assert!(matches!(doc.blocks[0], Block::Paragraph { .. }));

        apply_prefix(&mut doc, &mut state, "> ");
        assert!(
            matches!(doc.blocks[0], Block::Quote { .. }),
            "第一次点击应加上引用：{}",
            doc.blocks[0].source()
        );

        apply_prefix(&mut doc, &mut state, "> ");
        assert!(
            matches!(doc.blocks[0], Block::Paragraph { .. }),
            "再次点击应还原成普通段落：{}",
            doc.blocks[0].source()
        );
        assert_eq!(doc.blocks[0].source(), "引用文字", "取消引用不应丢文字");
    }

    /// 跨块拖选后点引用：选中的引用块整体还原，空行保持原样。
    #[test]
    fn quote_command_toggles_off_across_selection() {
        let mut doc = Document::new("> 甲\n\n> 乙", None);
        let mut state = EditorState::default();
        state.cross_sel = Some((0, 0, 2, 1));

        apply_prefix(&mut doc, &mut state, "> ");
        assert!(
            doc.blocks.iter().all(|b| !matches!(b, Block::Quote { .. })),
            "选区内的引用块应全部还原：{:?}",
            doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
        );
        assert!(state.cross_sel.is_none(), "处理完应清空跨块选区");

        // 选区里混有普通段落时仍然统一加引用，不做「取消」。
        let mut doc = Document::new("> 甲\n\n乙", None);
        let mut state = EditorState::default();
        state.cross_sel = Some((0, 0, 2, 1));
        apply_prefix(&mut doc, &mut state, "> ");
        assert!(
            doc.blocks
                .iter()
                .filter(|b| !b.source().trim().is_empty())
                .all(|b| matches!(b, Block::Quote { .. })),
            "混选时应统一加引用：{:?}",
            doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
        );

        // 选区里包含「引用内代码块」：取消引用要把代码块每行的 `>` 都去掉，
        // 变成普通代码块，而不是只剥掉第一行前缀把代码弄坏。
        let mut doc = Document::new("> 说明\n> ```rust\n> let a = 1;\n> ```", None);
        let mut state = EditorState::default();
        state.cross_sel = Some((0, 0, 1, 0));
        apply_prefix(&mut doc, &mut state, "> ");
        assert!(matches!(doc.blocks[0], Block::Paragraph { .. }));
        assert!(matches!(
            &doc.blocks[1],
            Block::Code { quote: 0, code, .. } if code == "let a = 1;"
        ));
    }

    /// 引用里的代码块：编辑缓冲只含代码，写回时逐行补回 `>`，往返不丢引用。
    #[test]
    fn quoted_code_edit_roundtrips() {
        let doc = Document::new("> ```rust\n> let a = 1;\n> let b = 2;\n> ```", None);
        let b = &doc.blocks[0];
        assert!(matches!(EditKind::of(b), EditKind::Code { quote: 1, .. }));
        assert_eq!(block_buffer(b), "let a = 1;\nlet b = 2;");
        let src = rebuild_source(b, &block_buffer(b));
        assert_eq!(src, "> ```rust\n> let a = 1;\n> let b = 2;\n> ```");
        let re = crate::ast::parse(&src);
        assert!(matches!(
            &re[0],
            Block::Code { quote: 1, code, .. } if code == "let a = 1;\nlet b = 2;"
        ));
    }

    /// 在引用内代码块里打字：写回源码时每行仍带 `>`，不会把代码块挤出引用。
    #[test]
    fn typing_in_quoted_code_stays_quoted() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let ctx = egui::Context::default();
        let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
            let mut images = ImageCache::new();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
        };
        let mut doc = Document::new("> ```rust\n> fn main() {}\n> ```", None);
        let mut state = EditorState {
            active: 0,
            focus_req: Some(0),
            last_cursor: 12,
            ..Default::default()
        };
        // 第一帧把焦点交给代码块的 TextEdit，第二帧输入字符。
        run(&mut doc, &mut state, vec![]);
        run(&mut doc, &mut state, vec![egui::Event::Text("x".into())]);
        let src = doc.blocks[0].source().to_string();
        assert!(
            src.starts_with("> ```rust\n> "),
            "代码块应仍在引用内：{src}"
        );
        assert!(src.contains('x'), "输入的字符应进入引用内代码块：{src}");
        assert!(src.ends_with("> ```"), "闭合围栏应保留引用前缀：{src}");
    }

    /// 光标在引用里点「代码块」：插入的代码块仍属于引用（quote 层级），
    /// 而不是把引用拆成两段、代码块跑到引用外面。
    #[test]
    fn insert_code_block_inside_quote_stays_quoted() {
        let mut doc = Document::new("> 引用文字", None);
        let mut state = EditorState::default();
        state.active = 0;
        insert_code_block(&mut doc, &mut state);
        assert_eq!(state.active, 1, "新代码块应成为活动块");
        match &doc.blocks[1] {
            Block::Code {
                quote, lang, code, ..
            } => {
                assert_eq!(*quote, 1);
                assert_eq!(lang, "");
                assert_eq!(code, "");
            }
            other => panic!("应插入引用内的空代码块，实际 {other:?}"),
        }
        // 引用本身还在，代码块没有把引用拆掉。
        assert!(matches!(doc.blocks[0], Block::Quote { .. }));
        // 普通段落里插入仍是普通代码块。
        let mut doc = Document::new("普通段落", None);
        let mut state = EditorState::default();
        state.active = 0;
        insert_code_block(&mut doc, &mut state);
        assert!(matches!(&doc.blocks[1], Block::Code { quote: 0, .. }));
    }

    /// 插入图片后光标落在图片下方的空段落：图片块不处于编辑态，
    /// 这样插图完成即可看到渲染后的预览，而不是 `![](...)` 源码。
    #[test]
    fn insert_image_leaves_preview_rendered() {
        let mut doc = Document::new("段落", None);
        let mut state = EditorState::default();
        state.active = 0;
        insert_image(&mut doc, &mut state, "![a](imgs/a.png)");
        assert!(
            matches!(&doc.blocks[1], Block::Paragraph { source, .. } if source == "![a](imgs/a.png)"),
            "图片应插成独立段落：{:?}",
            doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
        );
        assert!(
            matches!(&doc.blocks[2], Block::Blank { .. }),
            "图片后应补一个空段落"
        );
        assert_eq!(state.active, 2, "光标应落在图片下方的空段落");
        assert_eq!(state.focus_req, Some(2));
    }

    /// 点击渲染态的图片：该块进入编辑态，界面上露出 `![文件名](路径)` 源码；
    /// 点击别处后图片恢复渲染。
    #[test]
    fn clicking_rendered_image_reveals_source() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let dir = std::env::temp_dir().join(format!("markd-img-click-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(120, 80, image::Rgba([200, 40, 40, 255]))
            .save(dir.join("a.png"))
            .unwrap();
        let path = dir.join("note.md");
        std::fs::write(&path, "").unwrap();
        let mut doc = Document::new("![a](a.png)\n\n文字", Some(path));
        let mut state = EditorState::default();
        state.active = 2;
        let ctx = egui::Context::default();
        let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
            let mut images = ImageCache::new();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        Some(&dir),
                        &[],
                        false,
                        false,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
        };
        // 先渲染一帧，让图片纹理与布局就绪
        run(&mut doc, &mut state, vec![]);
        let p = egui::pos2(400.0, 25.0);
        run(
            &mut doc,
            &mut state,
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(state.active, 0, "点击图片应进入该块的源码编辑态");
    }

    /// 列表交互对齐 Typora：
    /// - 回车后下一行仍是同类型列表（有序序号 +1、任务项回到未勾选）；
    /// - Tab 让当前项变成子列表（缩进一级），Shift+Tab 退回上一级。
    #[test]
    fn list_enter_and_tab_match_typora() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let ctx = egui::Context::default();

        let key = |k: egui::Key, shift: bool| -> Vec<egui::Event> {
            vec![egui::Event::Key {
                key: k,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    shift,
                    ..Default::default()
                },
            }]
        };

        let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
            let mut images = ImageCache::new();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
        };

        // Tab：第二项缩进成子列表；Shift+Tab 退回。
        let mut doc = Document::new("- 父项\n- 子项", None);
        let mut state = EditorState {
            active: 1,
            focus_req: Some(1),
            tab_size: 2,
            ..Default::default()
        };
        run(&mut doc, &mut state, key(egui::Key::Tab, false));
        assert_eq!(
            doc.blocks[1].source(),
            "  - 子项",
            "Tab 应把列表项缩进为子列表"
        );
        run(&mut doc, &mut state, key(egui::Key::Tab, true));
        assert_eq!(doc.blocks[1].source(), "- 子项", "Shift+Tab 应退回上一级");

        // 无序列表回车：下一行还是无序列表。
        let mut doc = Document::new("- 父项", None);
        let mut state = EditorState {
            active: 0,
            focus_req: Some(0),
            last_cursor: 2,
            tab_size: 2,
            ..Default::default()
        };
        run(&mut doc, &mut state, key(egui::Key::Enter, false));
        assert!(
            matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.marker == "-"),
            "回车后应新建同类型的无序列表项，实际 {:?}",
            doc.blocks
        );

        // 有序列表回车：序号自动 +1。
        let mut doc = Document::new("3. 序号", None);
        let mut state = EditorState {
            active: 0,
            focus_req: Some(0),
            last_cursor: 2,
            tab_size: 2,
            ..Default::default()
        };
        run(&mut doc, &mut state, key(egui::Key::Enter, false));
        assert!(
            matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.marker == "4."),
            "有序列表回车序号应 +1，实际 {:?}",
            doc.blocks
        );

        // 任务列表回车：新项保持任务列表，但回到未勾选。
        let mut doc = Document::new("- [x] 完成", None);
        let mut state = EditorState {
            active: 0,
            focus_req: Some(0),
            last_cursor: 2,
            tab_size: 2,
            ..Default::default()
        };
        run(&mut doc, &mut state, key(egui::Key::Enter, false));
        assert!(
            matches!(
                &doc.blocks[1],
                Block::ListItem { item, .. } if item.checked == Some(false)
            ),
            "任务列表回车应新建未勾选的任务项，实际 {:?}",
            doc.blocks
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

    /// 加标题必须保留正文里的数字编号：如「1.2 核心架构」加 H1 后仍是
    /// 「# 1.2 核心架构」。「1.2」是章节号不是有序列表标记，不能被剥掉。
    #[test]
    fn heading_keeps_section_numbers() {
        for (src, prefix, want) in [
            ("1.2 核心架构", "# ", "# 1.2 核心架构"),
            ("3.1.4 测试章节", "## ", "## 3.1.4 测试章节"),
            ("2024 年度总结", "### ", "### 2024 年度总结"),
        ] {
            let mut doc = Document::new(src, None);
            let mut st = EditorState::default();
            apply_prefix(&mut doc, &mut st, prefix);
            assert_eq!(doc.source(), want, "加标题不应剥掉正文数字：{src}");
        }
    }

    /// 真正的有序列表标记（数字 + . 或 ) + 空格）仍要被剥掉，换成标题。
    #[test]
    fn heading_strips_real_ordered_list_marker() {
        for (src, prefix, want) in [
            ("1. 第一项", "# ", "# 第一项"),
            ("12) 第十二项", "## ", "## 第十二项"),
            ("- 列表项", "# ", "# 列表项"),
        ] {
            let mut doc = Document::new(src, None);
            let mut st = EditorState::default();
            apply_prefix(&mut doc, &mut st, prefix);
            assert_eq!(doc.source(), want, "真实块级标记应被剥掉：{src}");
        }
    }

    /// 鼠标拖选多个段落后再点列表：选区覆盖的每一段都要转成列表，
    /// 空行保持原样；有序列表按选区顺序依次编号。
    #[test]
    fn apply_prefix_to_cross_selection() {
        let mut doc = Document::new("甲\n\n乙\n\n丙", None);
        let mut st = EditorState {
            cross_sel: Some((0, 0, 4, 1)),
            ..Default::default()
        };
        apply_prefix(&mut doc, &mut st, "- ");
        assert_eq!(doc.blocks[0].source(), "- 甲");
        assert!(
            matches!(doc.blocks[1], Block::Blank { .. }),
            "空行不应变成空列表项"
        );
        assert_eq!(doc.blocks[2].source(), "- 乙");
        assert_eq!(doc.blocks[4].source(), "- 丙");
        assert!(st.cross_sel.is_none(), "转换后选区应清除");
        assert_eq!(st.active, 0, "光标应落到第一段");

        // 反向拖选同样有效，有序列表要依次编号。
        let mut doc = Document::new("甲\n\n乙", None);
        let mut st = EditorState {
            cross_sel: Some((2, 0, 0, 1)),
            ..Default::default()
        };
        apply_prefix(&mut doc, &mut st, "1. ");
        assert_eq!(doc.blocks[0].source(), "1. 甲");
        assert_eq!(doc.blocks[2].source(), "2. 乙");
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

    /// 表格结构操作：插删行列、对齐、表头保护。
    #[test]
    fn table_op_grid() {
        use crate::ast::Align;
        let mut grid = vec![
            vec!["A".to_string(), "B".to_string()],
            vec!["1".to_string(), "2".to_string()],
        ];
        let mut align = vec![Align::Left, Align::Left];
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::InsertRow(0)
        ));
        assert_eq!(grid.len(), 3, "表头下插入空行");
        assert_eq!(grid[1], vec![String::new(), String::new()]);
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::DeleteRow(1)
        ));
        assert_eq!(grid.len(), 2);
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::InsertCol(0)
        ));
        assert_eq!(grid[0].len(), 3, "第一列右侧插入空列");
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::DeleteCol(0)
        ));
        assert_eq!(grid[0].len(), 2);
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::SetAlign(Align::Center)
        ));
        assert_eq!(align, vec![Align::Center, Align::Center]);
        // 表头行不可删；只剩一列时删列无效
        assert!(!table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::DeleteRow(0)
        ));
        assert!(table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::DeleteCol(1)
        ));
        assert_eq!(grid[0].len(), 1, "删掉最后一列后仍保留一列");
        assert!(!table_op_on_grid(
            &mut grid,
            &mut align,
            TableOp::DeleteCol(0)
        ));
        assert_eq!(grid.len(), 2);
    }

    /// 列表项的所见即所得编辑：编辑缓冲只含正文（不含 "- [ ]" 等行首标记），
    /// 写回时由 rebuild 补回标记，源码无损往返。
    #[test]
    fn list_item_edit_roundtrip() {
        for src in [
            "- [ ] 未完成任务",
            "- [x] 已完成任务",
            "* 普通列表项",
            "1. 编号一项",
            "  - 缩进列表项",
        ] {
            let doc = Document::new(src, None);
            match doc.blocks.first() {
                Some(b) => {
                    assert!(
                        matches!(EditKind::of(b), EditKind::List { .. }),
                        "{src} 应识别为 List 编辑类型"
                    );
                    let buf = block_buffer(b);
                    assert!(
                        !buf.contains("- [") && !buf.starts_with('-') && !buf.starts_with('1'),
                        "编辑缓冲不应含行首标记: {src} -> {buf:?}"
                    );
                    assert_eq!(rebuild_source(b, &buf), src, "往返必须无损: {src}");
                }
                other => panic!("{src} 应解析为列表块，实际 {other:?}"),
            }
        }
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
                        ui, doc, state, theme, ctx, images, None, &outline, false, false, false,
                        false, 1, actions,
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

    /// 行尾空白点击链路验证：
    /// 1) 编辑态直接点击行尾空白 —— TextEdit 应把光标钳到该行最后一个字符
    ///    之后，且持有焦点（光标闪烁的前提）；
    /// 2) 渲染态点击非活动块行尾空白 —— 产生 Caret 动作进入编辑态后，
    ///    pending_caret 传入的超宽 x 也要钳到行末。
    #[test]
    fn click_row_blank_puts_caret_at_line_end_with_focus() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();

        // ---- 1) 编辑态：点 TextEdit 内文字右侧的空白 ----
        let mut doc = Document::new("短文字", None);
        let mut state = EditorState::default(); // active=0 → 块 0 编辑态
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
        // 页宽 720 居中（pad 40），文字只有三个字：点击点 (400, 20) 在
        // TextEdit 之内、文字 galley 之外的空白区。
        let pos = egui::pos2(400.0, 20.0);
        run_editor_frame(
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
        run_editor_frame(
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
        let id = egui::Id::new(("block_edit", 1, 0));
        let caret = egui::TextEdit::load_state(&ctx, id)
            .and_then(|ts| ts.cursor.char_range())
            .map(|r| r.primary.index.0);
        assert_eq!(
            caret,
            Some(3),
            "编辑态点行尾空白，光标应落在最后一个字符之后，实际 {caret:?}"
        );
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id),
            "点击后 TextEdit 必须持有焦点（否则光标不闪烁）"
        );

        // ---- 2) 渲染态点击非活动块行尾空白 → Caret 动作 → 进入编辑 ----
        let mut doc = Document::new("短文字\n\n第二段", None);
        let mut state = EditorState::default();
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
        // 第三块（"第二段"）是渲染态。先模拟渲染态块点击行尾空白产生的
        // Caret 动作：app.rs 会设置 active / focus_req / pending_caret。
        // local 偏移故意给一个超出文字宽度的 x，编辑态必须钳到行末。
        let id2 = egui::Id::new(("block_edit", 1, 2));
        state.active = 2;
        state.focus_req = Some(2);
        state.pending_caret = Some(egui::vec2(400.0, 2.0));
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        let caret = egui::TextEdit::load_state(&ctx, id2)
            .and_then(|ts| ts.cursor.char_range())
            .map(|r| r.primary.index.0);
        assert_eq!(
            caret,
            Some(3),
            "渲染态点行尾空白进入编辑后，光标应钳到行末，实际 {caret:?}"
        );
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(id2),
            "Caret 动作后块必须获得焦点（光标闪烁）"
        );
    }

    /// 行号栏宽度必须固定为 70px 常量：不随字体、行数位数、窗口或模式变化；
    /// 关闭行号时为 0。
    #[test]
    fn gutter_width_is_fixed_across_line_counts() {
        assert_eq!(gutter_width(true), 70.0);
        assert_eq!(gutter_width(true), GUTTER_WIDTH);
        assert_eq!(gutter_width(false), 0.0);
        // 固定间距常量也不变
        assert_eq!(GUTTER_PAD, 12.0);
    }

    /// 行号栏锚定在编辑区最左侧：代码窗口从 700px 拉到 1500px（全窗口 /
    /// 部分窗口）时，源码模式与混合模式的行号 x 坐标都必须不变，
    /// 且数字右缘始终位于固定 70px 行号栏内（距左缘 70-12=58px）。
    #[test]
    fn gutter_stays_fixed_across_window_widths_and_modes() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let src = "alpha\n\nbeta\n\ngamma";

        let run = |source_mode: bool, width: f32| -> Vec<(String, f32, f32, f32)> {
            let mut images = ImageCache::new();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let mut doc = Document::new(src, None);
            let mut state = EditorState::default();
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 900.0),
                )),
                ..Default::default()
            };
            let mut nums: Vec<(String, f32, f32, f32)> = Vec::new();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        &mut doc,
                        &mut state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        source_mode,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            for cs in &out.shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    let s = t.galley.text();
                    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                        nums.push((
                            s.to_string(),
                            t.pos.x,
                            t.galley.size().x,
                            cs.clip_rect.left(),
                        ));
                    }
                }
            }
            out.textures_delta.clear();
            nums
        };

        for source_mode in [true, false] {
            let narrow = run(source_mode, 700.0);
            let wide = run(source_mode, 1500.0);
            assert!(!narrow.is_empty(), "source_mode={source_mode} 应有行号");
            assert_eq!(
                narrow.len(),
                wide.len(),
                "source_mode={source_mode} 两种窗口宽度下行号数量应一致"
            );
            for (i, (text, x, _, clip_left)) in narrow.iter().enumerate() {
                assert_eq!(text, &wide[i].0, "source_mode={source_mode} 行号顺序应一致");
                assert!(
                    (x - wide[i].1).abs() < 0.5,
                    "source_mode={source_mode} 行号 {text} 的 x 随窗口宽度变化：{x} → {}",
                    wide[i].1
                );
                let gutter_right = x + narrow[i].2 - clip_left;
                assert!(
                    (gutter_right - (GUTTER_WIDTH - GUTTER_PAD)).abs() < 0.5,
                    "source_mode={source_mode} 行号 {text} 右缘距编辑区左缘 {gutter_right:.1}px，\
                     应在固定 {:.0}px 行号栏内",
                    GUTTER_WIDTH - GUTTER_PAD
                );
            }
        }
    }

    /// 源码模式行号栏宽度恒定验证：抓取行号数字的真实绘制坐标，
    /// 回车增加一行后（行数 10→11，位数都被 max(3) 钳住），
    /// 所有行号的 x 位置必须保持不动（宽度没变、正文没横移）。
    #[test]
    fn source_mode_gutter_x_stays_put_across_enter() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let src: String = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut doc = Document::new(&src, None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();

        // 跑一帧源码模式（行号开启），返回所有行号数字 (文本, 锚点位置)
        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>|
         -> Vec<(String, egui::Pos2)> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut nums: Vec<(String, egui::Pos2)> = Vec::new();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        true, // source_mode
                        false,
                        false,
                        true, // show_line_numbers
                        1,
                        &mut actions,
                    );
                });
            });
            for cs in &out.shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    let s = t.galley.text();
                    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                        nums.push((s.to_string(), t.pos));
                    }
                }
            }
            out.textures_delta.clear();
            nums
        };

        let key = |pressed: bool| -> Vec<egui::Event> {
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }]
        };
        let click = egui::pos2(400.0, 100.0);

        // 基线：10 行文档
        let before = run(&mut doc, &mut state, vec![]);
        assert_eq!(before.len(), 10, "应有 10 个行号数字，实际 {:?}", before);

        // 点击编辑区聚焦 TextEdit，然后回车新增一行
        run(
            &mut doc,
            &mut state,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        run(&mut doc, &mut state, key(true));
        run(&mut doc, &mut state, key(false));

        // 回车后：11 行
        let after = run(&mut doc, &mut state, vec![]);
        assert_eq!(after.len(), 11, "回车后应有 11 个行号数字");
        assert_eq!(
            doc.source().matches('\n').count() + 1,
            11,
            "文档应真的多了一行"
        );
        // 共同行号的 x 必须完全一致：行号栏宽度恒定，正文没有横移
        for (text, before_pos) in &before {
            let matched = after
                .iter()
                .find(|(t, _)| t == text)
                .map(|(_, p)| *p)
                .expect("回车后原有行号应仍在");
            assert_eq!(
                before_pos.x, matched.x,
                "行号 {text} 的 x 坐标在回车后发生了移动（行号栏宽度不稳定）"
            );
        }
    }

    /// 跨过 999 行的边界表现：行号栏宽度固定，不随位数 3→4 变化，
    /// 所有行号 x 坐标保持不变（不再有"变宽一整个数字位"的跳动）。
    #[test]
    fn source_mode_gutter_stays_fixed_past_999_lines() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        // 999 行文档（位数 3）
        let src: String = (1..=999)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut doc = Document::new(&src, None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();

        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>|
         -> Vec<(String, egui::Pos2)> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut nums: Vec<(String, egui::Pos2)> = Vec::new();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        true,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            for cs in &out.shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    let s = t.galley.text();
                    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                        nums.push((s.to_string(), t.pos));
                    }
                }
            }
            out.textures_delta.clear();
            nums
        };

        let key = |pressed: bool| -> Vec<egui::Event> {
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }]
        };
        let click = egui::pos2(400.0, 100.0);

        let before = run(&mut doc, &mut state, vec![]);
        assert_eq!(before.len(), 999, "应有 999 个行号数字");

        // 聚焦后回车：999 → 1000 行，跨过位数边界
        run(
            &mut doc,
            &mut state,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        run(&mut doc, &mut state, key(true));
        run(&mut doc, &mut state, key(false));

        let after = run(&mut doc, &mut state, vec![]);
        assert_eq!(after.len(), 1000, "回车后应有 1000 个行号数字");

        // 行号栏宽度固定：跨过 999 行（3→4 位）时所有共有行号 x 坐标不变，
        // 不应有"变宽一整个数字位、整体右移"的跳动。
        for (text, before_pos) in &before {
            let matched = after
                .iter()
                .find(|(t, _)| t == text)
                .map(|(_, p)| *p)
                .expect("回车后原有行号应仍在");
            let shift = matched.x - before_pos.x;
            assert!(
                shift.abs() < 0.5,
                "行号 {text} 右移 {shift:.2}px，行号栏宽度应固定不随位数变化"
            );
        }
    }

    /// 行号数字必须完整显示：三位行号（999 行文档）的 galley 矩形
    /// 不得越过 ScrollArea 裁剪区左缘，否则数字被裁得只剩部分。
    #[test]
    fn source_mode_line_numbers_not_clipped() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let src: String = (1..=999)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut doc = Document::new(&src, None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &c,
                    &mut images,
                    None,
                    &[],
                    true,
                    false,
                    false,
                    true,
                    1,
                    &mut actions,
                );
            });
        });
        out.textures_delta.clear();

        let mut checked = 0usize;
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                let s = t.galley.text();
                if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                    // galley 矩形是相对锚点的，屏幕左缘 = pos.x + rect.left()
                    let screen_left = t.pos.x + t.galley.rect.left();
                    assert!(
                        screen_left >= cs.clip_rect.left() - 0.5,
                        "行号 {s} 左缘 {screen_left:.1} 越过裁剪区左缘 {:.1}，被裁剪",
                        cs.clip_rect.left()
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked >= 900, "至少应检查 900 个行号，实际 {checked}");
    }

    /// 应用级悬停验证：混合模式渲染表格后，鼠标悬停在单元格上
    /// （悬停工具栏出现），表格的行/列分隔线必须一条不少、坐标不变。
    #[test]
    fn table_hover_keeps_separator_lines() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("前言\n\n甲|乙\n---|---\n一|二\n三|四", None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();

        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>|
         -> Vec<(f32, f32, f32, f32, f32)> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut segs: Vec<(f32, f32, f32, f32, f32)> = Vec::new();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            for cs in &out.shapes {
                if let egui::Shape::LineSegment { points, stroke } = &cs.shape {
                    segs.push((
                        points[0].x,
                        points[0].y,
                        points[1].x,
                        points[1].y,
                        stroke.width,
                    ));
                }
            }
            out.textures_delta.clear();
            segs
        };

        let has = |segs: &[(f32, f32, f32, f32, f32)], s: &(f32, f32, f32, f32, f32)| {
            segs.iter().any(|t| {
                (t.0 - s.0).abs() < 0.5
                    && (t.1 - s.1).abs() < 0.5
                    && (t.2 - s.2).abs() < 0.5
                    && (t.3 - s.3).abs() < 0.5
                    && (t.4 - s.4).abs() < 0.1
            })
        };

        // 基线帧：渲染表格，直接以这一帧实际画出的分隔线为基准。
        // 不硬编码屏幕坐标——表格在 ScrollArea 里的位置会随文档/布局变化而浮动，
        // 这个测试真正要验证的是「悬停工具栏浮层出现后，表格自身的线不变」。
        let base = run(&mut doc, &mut state, vec![]);
        assert!(base.len() >= 3, "基线帧应渲染出表格分隔线，实际 {base:?}");
        // 取表格的水平/垂直分隔线（跨度≈整宽的横线，或贯穿表格的竖线），
        // 排除其它干扰线段。normalize 到 0.5px 容差。
        let table_lines: Vec<(f32, f32, f32, f32, f32)> = base
            .iter()
            .copied()
            .filter(|s| {
                let horizontal = (s.1 - s.3).abs() < 0.5 && (s.2 - s.0).abs() > 100.0;
                let vertical = (s.0 - s.2).abs() < 0.5 && (s.3 - s.1).abs() > 20.0;
                horizontal || vertical
            })
            .collect();
        assert!(
            table_lines.len() >= 3,
            "应识别出至少 2 横 1 竖的表格线，实际 {base:?}"
        );

        // 悬停表头单元格（表格可见区域内）两帧，悬停工具栏出现
        let hover = egui::pos2(300.0, 81.0);
        run(&mut doc, &mut state, vec![egui::Event::PointerMoved(hover)]);
        let hovering = run(&mut doc, &mut state, vec![]);

        // 悬停后：表格每条分隔线必须一条不少、坐标分毫未动
        // （允许悬停工具栏额外新增自己的线段，但不允许表格的线消失或移位）
        for s in &table_lines {
            assert!(
                has(&hovering, s),
                "悬停单元格后表格分隔线消失了：{s:?}，实际 {hovering:?}"
            );
        }
    }

    /// 编辑态表格单元格也要按对齐设置排版：之前点「居中/右对齐」只改了源码，
    /// 单元格 TextEdit 永远左对齐，看起来像按钮失效。
    #[test]
    fn table_edit_cells_follow_alignment() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("| A | B |\n| --- | --- |\n| 1 | 2 |", None);
        let mut state = EditorState::default(); // active=0 → 表格编辑态
        let ctx = egui::Context::default();
        let mut run =
            |doc: &mut Document, state: &mut EditorState| -> Vec<egui::epaint::ClippedShape> {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let c = ui.ctx().clone();
                        let _ = show_editor(
                            ui,
                            doc,
                            state,
                            &theme,
                            &c,
                            &mut images,
                            None,
                            &[],
                            false,
                            false,
                            false,
                            true,
                            1,
                            &mut actions,
                        );
                    });
                });
                out.textures_delta.clear();
                out.shapes
            };
        // 单元格文字「A」的绘制 x 坐标。
        let text_x = |shapes: &[egui::epaint::ClippedShape]| -> f32 {
            shapes
                .iter()
                .find_map(|cs| match &cs.shape {
                    egui::Shape::Text(t) if t.galley.text() == "A" => Some(t.pos.x),
                    _ => None,
                })
                .expect("应渲染出单元格文字 A")
        };

        // 进入编辑态：第一帧建立表格草稿。
        run(&mut doc, &mut state);
        let left_x = text_x(&run(&mut doc, &mut state));
        assert!(state.table_draft.is_some(), "表格应进入编辑态");

        // 改成居中：单元格文字应明显右移。
        if let Some((_, _, aligns)) = state.table_draft.as_mut() {
            *aligns = vec![crate::ast::Align::Center; 2];
        }
        let center_x = text_x(&run(&mut doc, &mut state));
        assert!(
            center_x > left_x + 10.0,
            "居中后编辑态单元格文字应右移：{left_x:.1} -> {center_x:.1}"
        );

        // 改成右对齐：继续右移。
        if let Some((_, _, aligns)) = state.table_draft.as_mut() {
            *aligns = vec![crate::ast::Align::Right; 2];
        }
        let right_x = text_x(&run(&mut doc, &mut state));
        assert!(
            right_x > center_x + 10.0,
            "右对齐后编辑态单元格文字应更靠右：{center_x:.1} -> {right_x:.1}"
        );
    }

    /// 代码块编辑态与渲染态的按钮行位置必须一致：
    /// 点击进入编辑后，"复制"按钮相对盒子顶部的偏移不得变化
    /// （否则按钮看起来"跳到盒子右上方外"、整块跳动）。
    /// 注意：文档前面放一个段落，代码块保持渲染态，才能对比两态。
    #[test]
    fn code_block_edit_toolbar_stays_inside_box() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("前言\n\n```rust\nfn main() {}\n```", None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();

        // 抓取信息：(盒子矩形, "复制"文本锚点位置)
        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>|
         -> Option<(egui::Rect, egui::Pos2)> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut box_rect: Option<egui::Rect> = None;
            let mut copy_pos: Option<egui::Pos2> = None;
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
            for cs in &out.shapes {
                match &cs.shape {
                    // 盒子 = 最大的 code_bg 填充矩形
                    egui::Shape::Rect(r) if r.fill == theme.code_bg && r.rect.width() > 200.0 => {
                        let area = r.rect.width() * r.rect.height();
                        let bigger = box_rect.is_none_or(|b| area > b.width() * b.height());
                        if bigger {
                            box_rect = Some(r.rect);
                        }
                    }
                    egui::Shape::Text(t) if t.galley.text() == "复制" => {
                        copy_pos = Some(t.pos);
                    }
                    _ => {}
                }
            }
            box_rect.zip(copy_pos)
        };

        let rel = |r: &Option<(egui::Rect, egui::Pos2)>| -> f32 {
            let (box_r, copy) = r.as_ref().expect("应渲染出代码块盒子与复制按钮");
            copy.y - box_r.top()
        };

        // 渲染态：按钮相对盒子顶部的偏移（active=0 指向段落，代码块渲染）
        let before = run(&mut doc, &mut state, vec![]);
        let off_before = rel(&before);

        // 点击代码块（在段落下方）进入编辑态
        let click = egui::pos2(400.0, 90.0);
        run(
            &mut doc,
            &mut state,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let after = run(&mut doc, &mut state, vec![]);
        let off_after = rel(&after);
        assert!(
            (off_before - off_after).abs() < 2.0,
            "进入编辑后按钮偏移变了：渲染态 {off_before:.1}px → 编辑态 {off_after:.1}px，按钮跳动了"
        );
        // 盒子顶部不得明显移动（TextEdit 与 Label 的响应矩形有 ~2px 的固有差，
        // 肉眼不可见；之前是行高不一致导致的 5px，已修复）
        let (b1, _) = before.as_ref().expect("渲染态盒子");
        let (b2, _) = after.as_ref().expect("编辑态盒子");
        assert!(
            (b1.top() - b2.top()).abs() < 3.0,
            "盒子顶部移动了：渲染态 {:.1} → 编辑态 {:.1}",
            b1.top(),
            b2.top()
        );
        // 编辑态按钮必须在盒子内部（右缘不超过盒子右缘）
        let (b2, copy2) = after.as_ref().expect("编辑态盒子");
        assert!(
            copy2.y > b2.top() && copy2.y < b2.bottom() && copy2.x < b2.right(),
            "编辑态按钮 ({:.1},{:.1}) 跑到盒子外 [{:.1},{:.1} {}x{}]",
            copy2.x,
            copy2.y,
            b2.min.x,
            b2.min.y,
            b2.width(),
            b2.height()
        );
    }

    /// 跨块选区：在渲染态块上按住拖到另一块 → 选区建立；
    /// 退格 → 两段（含中间空行）合并成一段。
    #[test]
    fn cross_block_drag_select_and_delete() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        assert_eq!(doc.blocks.len(), 5, "3 段 + 2 空行 = 5 块");
        let mut state = EditorState::default(); // active=0 段落编辑态
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

        // 在"第二段"（渲染态块）按下并拖到"第三段"
        let start = egui::pos2(400.0, 60.0);
        let end = egui::pos2(400.0, 100.0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        println!("[帧2 按下] actions={actions:?} cross={:?}", state.cross_sel);
        actions.clear();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(end)],
        );
        println!("[帧3 移动] actions={actions:?} cross={:?}", state.cross_sel);
        actions.clear();
        // egui 的 drag_started 晚一帧生效：再保持一帧按下状态（指针停在块 4），
        // 让终点块以「选区进行中 + 指针压在本块上」接棒上报选区终点。
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(end)],
        );
        println!("[帧4 保持] actions={actions:?} cross={:?}", state.cross_sel);
        actions.clear();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let sel = state
            .cross_sel
            .expect("跨块拖选后应建立选区，实际 None——检查渲染态块的拖选感应");
        let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert!(
            (lo_b, hi_b) == (2, 4),
            "选区应覆盖第 2 块到第 4 块，实际 {sel:?}"
        );

        // 退格：删除选区 → 两段合并
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
        assert!(state.cross_sel.is_none(), "删除后选区应清除");
        assert_eq!(
            doc.source(),
            "第一段\n\n第二段",
            "退格应把选中的第二、三段合并成一段"
        );
    }

    /// 跨块拖选必须能跨过代码块/图表这类「非文本块」：
    /// 拖到它上面时选区端点落在该块内（向下取块尾、向上取块首），
    /// 并且整块画出选区底色，而不是像以前一样在中间断掉。
    #[test]
    fn drag_selection_extends_into_code_block() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let text = "alpha\n\n`code` beta\n\n```rust\nfn main() {}\n```\n\ngamma";
        let mut doc = Document::new(text, None);
        // alpha(0) / 空行(1) / 行内代码段(2) / 空行(3) / 代码块(4) / 空行(5) / gamma(6)
        assert_eq!(doc.blocks.len(), 7);
        let code_idx = 4usize;
        let code_len = doc.blocks[code_idx].plain().chars().count();
        let mut state = EditorState::default();
        state.active = 2;
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut Document,
                         state: &mut EditorState,
                         actions: &mut Vec<crate::render::Action>|
         -> Vec<egui::epaint::ClippedShape> {
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
                        ui,
                        doc,
                        state,
                        &theme,
                        &ctx,
                        &mut images,
                        None,
                        &outline,
                        false,
                        false,
                        false,
                        false,
                        1,
                        actions,
                    );
                });
            });
            out.textures_delta.clear();
            out.shapes
        };
        for _ in 0..3 {
            let _ = frame(vec![], &mut doc, &mut state, &mut actions);
        }
        let shapes = frame(vec![], &mut doc, &mut state, &mut actions);
        let find = |name: &str| -> egui::Pos2 {
            for cs in &shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    if t.galley.text().starts_with(name) {
                        return t.pos;
                    }
                }
            }
            panic!("{name} 未渲染");
        };
        let alpha = find("alpha");
        let code = find("fn main");
        let gamma = find("gamma");
        let press = |pos: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]
        };

        // 向下拖：从 alpha 拖到代码块上，端点应落在代码块末尾。
        let _ = frame(
            press(alpha + egui::vec2(3.0, 8.0)),
            &mut doc,
            &mut state,
            &mut actions,
        );
        let on_code = code + egui::vec2(60.0, 8.0);
        let _ = frame(
            vec![egui::Event::PointerMoved(on_code)],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let _ = frame(
            vec![egui::Event::PointerMoved(on_code)],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let _ = frame(
            vec![egui::Event::PointerButton {
                pos: on_code,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let sel = state.cross_sel.expect("向下拖到代码块应建立选区");
        assert_eq!(
            sel,
            (0, 0, code_idx, code_len),
            "选区终点应落在代码块末尾，实际 {sel:?}"
        );

        // 代码块被选区覆盖时，应有一层半透明选区色叠在代码内容之上。
        let shapes = frame(vec![], &mut doc, &mut state, &mut actions);
        let code_center = code + egui::vec2(60.0, 8.0);
        let sel_fill = {
            let c = theme.selection;
            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110)
        };
        let highlighted = shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Rect(r) => r.fill == sel_fill && r.rect.contains(code_center),
            _ => false,
        });
        assert!(highlighted, "代码块应画出选区叠加层");

        // 向上拖：从 gamma 拖到代码块上，起点应落在代码块开头。
        state.cross_sel = None;
        let _ = frame(vec![], &mut doc, &mut state, &mut actions);
        let _ = frame(
            press(gamma + egui::vec2(3.0, 8.0)),
            &mut doc,
            &mut state,
            &mut actions,
        );
        let _ = frame(
            vec![egui::Event::PointerMoved(on_code)],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let _ = frame(
            vec![egui::Event::PointerButton {
                pos: on_code,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let sel = state.cross_sel.expect("向上拖到代码块应建立选区");
        let (s, sc, e, _) = (sel.0.min(sel.2), sel.1, sel.0.max(sel.2), sel.3);
        assert_eq!(
            (s, sc, e),
            (code_idx, 0, 6),
            "选区起点应落在代码块开头，实际 {sel:?}"
        );
    }

    /// 视口外的按下（工具条/菜单）不能把指针钳进视口当成点了最上面的块：
    /// 否则拖选好正文后点「引用」，选区会被 (0, n) 的零长度选区顶掉，
    /// 格式加到第一段（用户反馈的引用 bug）。
    #[test]
    fn press_outside_viewport_does_not_clobber_cross_sel() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default(); // active = 0：第一段编辑态，其余渲染态
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut Document,
                         state: &mut EditorState|
         -> Vec<egui::epaint::ClippedShape> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            };
            let outline = doc.outline();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &ctx,
                        &mut images,
                        None,
                        &outline,
                        false,
                        false,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
            out.shapes
        };
        let _ = frame(vec![], &mut doc, &mut state);
        // 已有拖选：第二段整段选中
        state.cross_sel = Some((2, 0, 2, 3));
        // 在视口上方（工具条位置）按下并松开
        let outside = egui::pos2(300.0, -20.0);
        let _ = frame(
            vec![
                egui::Event::PointerMoved(outside),
                egui::Event::PointerButton {
                    pos: outside,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            &mut doc,
            &mut state,
        );
        let _ = frame(
            vec![egui::Event::PointerButton {
                pos: outside,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut doc,
            &mut state,
        );
        assert_eq!(
            state.cross_sel,
            Some((2, 0, 2, 3)),
            "工具条上的点击不应覆盖已有跨块选区"
        );
        // 没有选区时，视口外按下也不该凭空建立选区
        state.cross_sel = None;
        let _ = frame(
            vec![
                egui::Event::PointerMoved(outside),
                egui::Event::PointerButton {
                    pos: outside,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: outside,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            &mut doc,
            &mut state,
        );
        assert!(
            state.cross_sel.is_none(),
            "视口外的点击不应建立跨块选区，实际 {:?}",
            state.cross_sel
        );
    }

    /// 拖到视口外要能自动滚动，并且选区跟着一直延伸到底：
    /// 指针停住不动时系统不再送鼠标事件，必须自己请求重绘；
    /// 越界点也要钳到视口边缘才能持续命中块。
    #[test]
    fn drag_beyond_viewport_autoscrolls_and_extends_selection() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let text = (0..60)
            .map(|i| format!("段落{i}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut doc = Document::new(&text, None);
        let mut state = EditorState::default();
        state.active = 0;
        let ctx = egui::Context::default();
        let mut now = 0.0f64;
        let mut frame = |events: Vec<egui::Event>,
                         doc: &mut Document,
                         state: &mut EditorState,
                         actions: &mut Vec<crate::render::Action>|
         -> Vec<egui::epaint::ClippedShape> {
            // 故意用较矮的窗口，让视口很快被段落填满。
            // 时间必须往前走：ScrollArea 的滚动位移按动画时间轴推进。
            now += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 300.0),
                )),
                time: Some(now),
                predicted_dt: 1.0 / 60.0,
                events,
                ..Default::default()
            };
            let outline = doc.outline();
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &ctx,
                        &mut images,
                        None,
                        &outline,
                        false,
                        false,
                        false,
                        false,
                        1,
                        actions,
                    );
                });
            });
            out.textures_delta.clear();
            out.shapes
        };
        for _ in 0..3 {
            let _ = frame(vec![], &mut doc, &mut state, &mut actions);
        }
        // 在第二个段落（渲染态）按下。
        let start = egui::pos2(300.0, 62.0);
        let _ = frame(
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            &mut doc,
            &mut state,
            &mut actions,
        );
        // 指针甩到视口下方之外并保持按住：靠自动滚动持续延伸选区。
        let outside = egui::pos2(300.0, 900.0);
        let _ = frame(
            vec![egui::Event::PointerMoved(outside)],
            &mut doc,
            &mut state,
            &mut actions,
        );
        let early = state.cross_sel.expect("越界拖选后应仍有选区");
        let early_end = early.0.max(early.2);
        for _ in 0..40 {
            let _ = frame(
                vec![egui::Event::PointerMoved(outside)],
                &mut doc,
                &mut state,
                &mut actions,
            );
        }
        let sel = state.cross_sel.expect("越界拖选后应仍有选区");
        let (start_b, end_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!(start_b, 2, "锚点应保持在按下的段落，实际 {sel:?}");
        assert!(
            end_b > early_end + 3,
            "选区应随自动滚动继续延伸：首帧 {early_end} → 最终 {end_b}，实际 {sel:?}"
        );
    }

    /// 双击选词 / 三击选整段 / Shift+点击 三个 Action 聚合成 cross_sel。
    #[test]
    fn mouse_selection_actions_set_cross_sel() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let ctx = egui::Context::default();

        // 双击选词
        {
            let mut doc = Document::new("hello world\n\n第二块", None);
            let mut state = EditorState::default();
            let mut actions = vec![crate::render::Action::WordSel(0, 0, 5)];
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![],
            );
            assert_eq!(state.cross_sel, Some((0, 0, 0, 5)), "双击应选中词区间");
        }
        // 三击选整段
        {
            let mut doc = Document::new("整段文字\n\n第二块", None);
            let mut state = EditorState::default();
            let mut actions = vec![crate::render::Action::BlockSel(0)];
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
                state.cross_sel,
                Some((0, 0, 0, "整段文字".chars().count())),
                "三击应选中整段"
            );
        }
        // Shift+点击：无选区时从光标锚点到点击处
        {
            let mut doc = Document::new("第一段\n\n第二段", None);
            let mut state = EditorState::default();
            state.active = 0;
            state.last_cursor = 2;
            let mut actions = vec![crate::render::Action::ShiftClick(2, 3)];
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
                state.cross_sel,
                Some((0, 2, 2, 3)),
                "Shift+点击应从光标选到点击处"
            );
        }
    }

    /// 跨块选区复制文本：首尾块取片段、中间块取全文、块间空行连接。
    #[test]
    fn cross_sel_text_joins_fragments() {
        let doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        // 块 2 "第二段" chars 1..3 = "二段"；块 4 "第三段" chars 0..2 = "第三"
        let text = cross_sel_text(&doc, Some((2, 1, 4, 2))).expect("选区应产生文本");
        assert_eq!(text, "二段\n\n第三");
        // 反向（从尾拖回头）
        let text = cross_sel_text(&doc, Some((4, 2, 2, 1))).expect("反向选区应产生相同文本");
        assert_eq!(text, "二段\n\n第三");
        assert!(cross_sel_text(&doc, None).is_none(), "无选区无文本");
    }

    /// 代码块这类非文本块是原子的：作为选区端点时整块计入复制文本，
    /// 不按拖选时打上的「块首/块尾」端点字符截断。
    #[test]
    fn cross_sel_text_includes_whole_code_block() {
        let doc = Document::new("alpha\n\n```rust\nfn main() {}\n```\n\ngamma", None);
        // alpha(0) / 空行(1) / 代码块(2) / 空行(3) / gamma(4)
        let code_len = doc.blocks[2].plain().chars().count();
        let text = cross_sel_text(&doc, Some((0, 0, 2, code_len))).expect("向下选区应有文本");
        assert_eq!(text, "alpha\n\nfn main() {}", "向下拖到代码块应整块计入");
        // 向上拖：终点字符是 0（块首标记），仍应整块计入。
        let text = cross_sel_text(&doc, Some((4, 6, 2, 0))).expect("向上选区应有文本");
        assert_eq!(text, "fn main() {}\n\ngamma", "向上拖到代码块也应整块计入");
    }

    /// 回归：反向（向上）拖选不能漏块。
    /// 渲染按块 idx 升序进行；此前起始块的 `resp.dragged()` 也上报 CrossDrag，
    /// 向上拖时它晚于上方指针块被处理，把选区终点覆盖回起始块——选区只剩
    /// 起始一块，上方段落看上去没被选中、也复制不到。
    #[test]
    fn cross_block_drag_upward_keeps_endpoint() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
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

        // 在「第三段」（块 4, y=100）按下，向上拖到「第二段」（块 2, y=60）。
        let start = egui::pos2(400.0, 100.0);
        let end = egui::pos2(400.0, 60.0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        for _ in 0..2 {
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::PointerMoved(end)],
            );
            actions.clear();
        }
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let sel = state.cross_sel.expect("向上拖选后应建立选区");
        let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!(
            (lo_b, hi_b),
            (2, 4),
            "向上拖选应覆盖第 2 到第 4 块，实际 {sel:?}"
        );
    }

    /// 回归：光标所在的空行（空活动块）按下向下拖，必须能选中下方块。
    /// 活动块用 TextEdit 渲染、没有 text_response 的 CrossDrag 路径，
    /// 此前从空行起拖锚定不了选区，表现是「什么也选中不了」。
    #[test]
    fn cross_drag_from_empty_active_block_selects_downward() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
        state.active = 1; // 块 1 是空行（Blank），且为活动块
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

        // 在空行（约 y=40）按下，向下拖到「第三段」（块 4, y=100）。
        let start = egui::pos2(400.0, 40.0);
        let end = egui::pos2(400.0, 100.0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        for _ in 0..2 {
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::PointerMoved(end)],
            );
            actions.clear();
        }
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let sel = state
            .cross_sel
            .expect("从空行起拖应建立跨块选区，实际 None——检查空活动块的锚定");
        let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!(
            (lo_b, hi_b),
            (1, 4),
            "从空行向下拖选应覆盖第 1 到第 4 块，实际 {sel:?}"
        );
    }

    /// 普通点击空行（不拖动）不应残留跨块选区。
    #[test]
    fn click_empty_active_block_leaves_no_selection() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
        state.active = 1;
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
        let p = egui::pos2(400.0, 40.0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!(
            state.cross_sel.is_none(),
            "普通点击空行不应残留选区，实际 {:?}",
            state.cross_sel
        );
    }

    // 临时探针：定位渲染态/编辑态盒子顶差 5px 的来源
    #[test]
    fn probe_code_block_gap() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("前言\n\n```rust\nfn main() {}\n```", None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();

        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>,
                       tag: &str| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
            println!("--- [{tag}] ---");
            for cs in &out.shapes {
                match &cs.shape {
                    egui::Shape::Rect(r) if r.fill == theme.code_bg && r.rect.width() > 200.0 => {
                        println!(
                            "  code_bg 盒子顶 {:.1} 底 {:.1}",
                            r.rect.top(),
                            r.rect.bottom()
                        );
                    }
                    egui::Shape::Text(t) => {
                        let s = t.galley.text();
                        if s.contains("前言") || s.contains("main") {
                            println!(
                                "  文本 {:?} 顶 {:.1} 底 {:.1}",
                                s.chars().take(6).collect::<String>(),
                                t.pos.y + t.galley.rect.top(),
                                t.pos.y + t.galley.rect.bottom(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        };

        run(&mut doc, &mut state, vec![], "渲染态");
        let click = egui::pos2(400.0, 90.0);
        run(
            &mut doc,
            &mut state,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            "点击按下",
        );
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            "点击抬起",
        );
        run(&mut doc, &mut state, vec![], "编辑态");
    }

    // 临时探针：三段文档的文本块坐标
    #[test]
    fn probe_para_positions() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &c,
                    &mut images,
                    None,
                    &[],
                    false,
                    false,
                    false,
                    true,
                    1,
                    &mut actions,
                );
            });
        });
        out.textures_delta.clear();
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                let s: String = t.galley.text().chars().take(6).collect();
                if !s.trim().is_empty() {
                    println!(
                        "文本 {:?} 锚({:.1},{:.1}) rect={:?}",
                        s, t.pos.x, t.pos.y, t.galley.rect
                    );
                }
            }
        }
    }

    /// 空段落行首退格并入上一块（代码块）时，光标应落在代码正文最后一行末尾，
    /// 而不是被钳制到中间行。此前用整段源码长度算 cursor，含围栏导致偏到中间。
    #[test]
    fn backspace_into_code_block_lands_at_end() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let mut doc = Document::new("```rust\nfn a() {\n    let x = 1;\n}\n```\n", None);
        let code_body = "fn a() {\n    let x = 1;\n}";
        let mut state = EditorState::default();
        state.active = doc.blocks.len() - 1; // 末尾空段落
        state.last_cursor = 0;
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        // 空段落行首退格
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
        assert!(
            matches!(doc.blocks.get(state.active), Some(Block::Code { .. })),
            "退格后活动块应是代码块: {:?}",
            doc.blocks.get(state.active)
        );
        assert_eq!(
            state.last_cursor,
            code_body.chars().count(),
            "光标应落在代码正文末尾（{}），实际 {}",
            code_body.chars().count(),
            state.last_cursor
        );
    }

    /// 回归：从「已激活（编辑态）的块」开始拖选，必须能建立跨块选区。
    /// 用户先点进某一段（变编辑态），再从这段往下拖时，TextEdit 吃掉了
    /// 按下事件，跨块选区根本建立不了 —— 表现为「有些文字能拖选、有些不能」。
    #[test]
    fn drag_from_active_block_extends_across_blocks() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
        state.active = 0; // 段 0 处于编辑态（TextEdit）
        state.focus_req = Some(0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        let start = egui::pos2(400.0, 20.0);
        let mid = egui::pos2(400.0, 60.0);
        let end = egui::pos2(400.0, 100.0);
        // 在编辑态块上按下
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        for p in [mid, end, end] {
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::PointerMoved(p)],
            );
            actions.clear();
        }
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let sel = state.cross_sel.expect("从编辑态块起拖应建立跨块选区");
        let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!((lo, hi), (0, 4), "选区应覆盖第 0~4 块，实际 {sel:?}");
    }

    /// 回归：拖选的终点落在「编辑态块」上时，选区必须继续延伸到该块。
    /// 编辑态块渲染成 TextEdit、不走 text_response，之前终点会停在上一个块。
    #[test]
    fn drag_ending_on_active_block_extends_selection() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
        let mut state = EditorState::default();
        state.active = 4; // 最后一块是编辑态
        state.focus_req = Some(4);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        let start = egui::pos2(400.0, 20.0);
        let end = egui::pos2(400.0, 100.0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        for p in [end, end] {
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::PointerMoved(p)],
            );
            actions.clear();
        }
        let sel = state.cross_sel.expect("向下拖应建立跨块选区");
        let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
        assert_eq!(
            (lo, hi),
            (0, 4),
            "选区的终点应延伸到编辑态块 4，实际 {sel:?}"
        );
    }

    /// Typora 式任务列表：非空任务项回车 → 下一行继承任务标记；
    /// 空任务项再回车 → 退出列表，变回普通段落。
    #[test]
    fn enter_task_list_inherits_then_exits() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();

        let enter = || {
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]
        };
        let is_task =
            |b: &Block| matches!(b, Block::ListItem { item, .. } if item.checked == Some(false));

        // 从普通段落出发，先点「任务列表」转换成任务项（由 app 层 Command::Task 完成，
        // 这里用等价的 insert_task 模拟转换）。
        let mut doc = Document::new("- [ ] 写文档", None);
        let mut state = EditorState::default();
        state.active = 0;
        state.focus_req = Some(0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );

        // 光标到「写文档」末尾回车 → 下一行应继承任务标记
        state.last_cursor = "写文档".chars().count();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            enter(),
        );
        assert_eq!(doc.blocks.len(), 2, "回车应拆出新块");
        assert!(
            is_task(&doc.blocks[1]),
            "非空任务项回车，下一行应仍是任务列表: {:?}",
            doc.blocks[1]
        );

        // 现在光标在新空任务项上，再回车 → 退出列表变普通段落（空行）
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            enter(),
        );
        assert!(
            !matches!(doc.blocks.last(), Some(Block::ListItem { .. })),
            "空任务项再回车应退出列表、不再是任务项: {:?}",
            doc.blocks.last()
        );
    }

    /// Typora 式有序/无序列表：非空项回车继承标记，空项回车退出变普通段落。
    #[test]
    fn enter_list_inherits_then_exits() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let enter = || {
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }]
        };

        for (src, body, label) in [
            ("1. 第一项", "第一项", "有序"),
            ("- 第一项", "第一项", "无序"),
        ] {
            let mut doc = Document::new(src, None);
            let mut state = EditorState::default();
            state.active = 0;
            state.focus_req = Some(0);
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![],
            );
            state.last_cursor = body.chars().count();
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                enter(),
            );
            assert!(
                matches!(&doc.blocks[1], Block::ListItem { .. }),
                "{label}列表非空项回车，下一行应仍是列表: {:?}",
                doc.blocks[1]
            );
            // 空项再回车退出：不新增块（当前空项写回成普通段落）
            let n_before = doc.blocks.len();
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                enter(),
            );
            assert_eq!(
                doc.blocks.len(),
                n_before,
                "{label}列表空项回车退出时不应新增块: {:?}",
                doc.blocks
            );
            assert!(
                !matches!(doc.blocks.last(), Some(Block::ListItem { .. })),
                "{label}列表空项再回车应退出: {:?}",
                doc.blocks.last()
            );
        }
    }

    /// 已勾选任务项回车，新项应为未勾选状态。
    #[test]
    fn enter_on_checked_task_inherits_unchecked() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let ctx = egui::Context::default();
        let mut doc = Document::new("- [x] 已完成", None);
        let mut state = EditorState::default();
        state.active = 0;
        state.focus_req = Some(0);
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        state.last_cursor = "已完成".chars().count();
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
        assert!(
            matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.checked == Some(false)),
            "已勾选任务项回车，新项应未勾选: {:?}",
            doc.blocks[1]
        );
    }

    /// 任务列表项里按回车应当换行（拆出新块）；普通段落同理。
    #[test]
    fn enter_splits_task_item_and_paragraph() {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();

        // 任务列表：光标在「任务一」末尾回车，应产生新的一行
        let mut doc = Document::new("- [ ] 任务一\n- [ ] 任务二", None);
        let mut state = EditorState::default();
        state.active = 0;
        state.focus_req = Some(0);

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
        // 光标放到第一项文字末尾再回车
        state.last_cursor = "任务一".chars().count();
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
        assert!(
            doc.blocks.len() > 2,
            "任务列表项里回车应换行拆块，实际块数 {}",
            doc.blocks.len()
        );

        // 普通段落：光标在中间回车，拆成两段
        let mut doc = Document::new("第一行内容", None);
        let mut state = EditorState::default();
        state.active = 0;
        state.focus_req = Some(0);
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
        state.last_cursor = 3;
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
            2,
            "段落里回车应拆成两块，实际块数 {}",
            doc.blocks.len()
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
                    ui, doc, state, theme, ctx, images, None, &outline, false, false, false, false,
                    1, actions,
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
                    false,
                    1,
                    &mut actions,
                );
            });
            println!("[bench] small-viewport frame {i} = {:?}", t.elapsed());
        }
    }
}
