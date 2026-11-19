//! Author: aurelianus <aurelianus@126.com>

//! 自研 Markdown 解析器：块级 + 行内，完全不依赖第三方解析库。
//!
//! 设计要点：每个 [`Block`] 都保留自己的原始 `source`，这样编辑器可以在
//! 「当前块显示源码、其余块渲染成富文本」的 Hybrid View 之间无损往返。

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Strong(Vec<Inline>),
    Emph(Vec<Inline>),
    Strike(Vec<Inline>),
    Underline(Vec<Inline>),
    Highlight(Vec<Inline>),
    Code(String),
    Link {
        label: Vec<Inline>,
        url: String,
    },
    Image {
        alt: String,
        url: String,
        /// 可选显示宽度（像素）：来自图片后的 `{width=300}` 属性，
        /// 鼠标拖动缩放时写回源码。
        width: Option<u32>,
    },
    Math(String),
    Wiki(String),
    Footnote(String),
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub indent: usize,
    pub marker: String,
    pub checked: Option<bool>,
    pub content: Vec<Inline>,
    pub line: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    Mermaid,
    Flow,
    Sequence,
    Gantt,
}

/// 代码组里的一个语言标签：语言名 + 该标签下的代码。
///
/// 源码用四反引号的外层围栏表示：
/// ````text
/// ````code-group
/// ```java
/// ...
/// ```
/// ```python
/// ...
/// ```
/// ````
/// ````
#[derive(Clone, Debug, PartialEq)]
pub struct CodeGroupItem {
    pub lang: String,
    pub code: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
        source: String,
    },
    Paragraph {
        content: Vec<Inline>,
        source: String,
    },
    Quote {
        level: usize,
        content: Vec<Inline>,
        source: String,
    },
    ListItem {
        item: ListItem,
        source: String,
    },
    Code {
        lang: String,
        code: String,
        source: String,
        /// 引用层级：> 0 表示这段代码块写在引用内部（`> ```…`），
        /// 渲染时左侧带引用竖线，源码里的每一行都保留 `>` 前缀。
        quote: usize,
    },
    /// 代码组：同一个代码区用 Tab 切换多种语言（如 Java / Python）。
    CodeGroup {
        items: Vec<CodeGroupItem>,
        source: String,
    },
    Table {
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        align: Vec<Align>,
        source: String,
    },
    Rule {
        source: String,
    },
    Math {
        tex: String,
        source: String,
    },
    Diagram {
        kind: DiagramKind,
        code: String,
        source: String,
    },
    FrontMatter {
        yaml: String,
        source: String,
    },
    Footnote {
        id: String,
        content: Vec<Inline>,
        source: String,
    },
    Toc {
        source: String,
    },
    Blank {
        source: String,
    },
}

impl Block {
    pub fn source(&self) -> &str {
        match self {
            Block::Heading { source, .. }
            | Block::Paragraph { source, .. }
            | Block::Quote { source, .. }
            | Block::ListItem { source, .. }
            | Block::Code { source, .. }
            | Block::CodeGroup { source, .. }
            | Block::Table { source, .. }
            | Block::Rule { source }
            | Block::Math { source, .. }
            | Block::Diagram { source, .. }
            | Block::FrontMatter { source, .. }
            | Block::Footnote { source, .. }
            | Block::Toc { source }
            | Block::Blank { source } => source,
        }
    }

    pub fn source_mut(&mut self) -> &mut String {
        match self {
            Block::Heading { source, .. }
            | Block::Paragraph { source, .. }
            | Block::Quote { source, .. }
            | Block::ListItem { source, .. }
            | Block::Code { source, .. }
            | Block::CodeGroup { source, .. }
            | Block::Table { source, .. }
            | Block::Rule { source }
            | Block::Math { source, .. }
            | Block::Diagram { source, .. }
            | Block::FrontMatter { source, .. }
            | Block::Footnote { source, .. }
            | Block::Toc { source }
            | Block::Blank { source } => source,
        }
    }

    /// 多行块在编辑器里使用多行 TextEdit，Enter 用于块内换行。
    pub fn is_multiline(&self) -> bool {
        matches!(
            self,
            Block::Code { .. }
                | Block::CodeGroup { .. }
                | Block::Table { .. }
                | Block::Diagram { .. }
                | Block::Math { .. }
                | Block::FrontMatter { .. }
        )
    }

    pub fn heading(&self) -> Option<(u8, String)> {
        match self {
            Block::Heading { level, content, .. } => Some((*level, inline_plain(content))),
            _ => None,
        }
    }

    pub fn plain(&self) -> String {
        match self {
            Block::Heading { content, .. }
            | Block::Paragraph { content, .. }
            | Block::Quote { content, .. }
            | Block::Footnote { content, .. } => inline_plain(content),
            Block::ListItem { item, .. } => inline_plain(&item.content),
            Block::Code { code, .. } | Block::Diagram { code, .. } => code.clone(),
            Block::CodeGroup { items, .. } => items
                .iter()
                .map(|i| i.code.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
            Block::Math { tex, .. } => tex.clone(),
            Block::Table { head, rows, .. } => {
                let mut parts: Vec<String> = head.iter().map(|c| inline_plain(c)).collect();
                for row in rows {
                    parts.extend(row.iter().map(|c| inline_plain(c)));
                }
                parts.join(" ")
            }
            _ => String::new(),
        }
    }
}

pub fn inline_plain(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) | Inline::Math(t) | Inline::Wiki(t) => s.push_str(t),
            Inline::Strong(v)
            | Inline::Emph(v)
            | Inline::Strike(v)
            | Inline::Underline(v)
            | Inline::Highlight(v)
            | Inline::Link { label: v, .. } => {
                s.push_str(&inline_plain(v));
            }
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::Footnote(id) => s.push_str(id),
            Inline::Break => s.push(' '),
        }
    }
    s
}

pub fn collect_wiki(inlines: &[Inline], out: &mut Vec<String>) {
    for i in inlines {
        match i {
            Inline::Wiki(name) => out.push(name.clone()),
            Inline::Strong(v)
            | Inline::Emph(v)
            | Inline::Strike(v)
            | Inline::Underline(v)
            | Inline::Highlight(v)
            | Inline::Link { label: v, .. } => {
                collect_wiki(v, out);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- 行内解析

/// 解析图片后面的可选属性块 `{width=300}`（也接受 `{ width=300 }`）。
/// 其它属性忽略但整块吃掉，避免属性泄漏成正文。
fn image_width_attr(c: &[char], i: usize) -> (Option<u32>, usize) {
    if c.get(i) != Some(&'{') {
        return (None, i);
    }
    let Some(close) = find_seq(c, i + 1, &['}']) else {
        return (None, i);
    };
    let inner: String = c[i + 1..close].iter().collect();
    let mut width = None;
    for part in inner.split([' ', ',']) {
        if let Some(v) = part.strip_prefix("width=") {
            if let Ok(n) = v.trim().trim_end_matches("px").parse::<u32>() {
                width = Some(n.clamp(16, 8000));
            }
        }
    }
    (width, close + 1)
}

fn find_seq(c: &[char], from: usize, pat: &[char]) -> Option<usize> {
    if pat.is_empty() {
        return Some(from);
    }
    let mut i = from;
    while i + pat.len() <= c.len() {
        if (0..pat.len()).all(|k| c[i + k] == pat[k]) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `open` 指向 `[`，返回 (标签, 目标, ')' 之后的索引)。
fn parse_link(c: &[char], open: usize) -> Option<(String, String, usize)> {
    let mut j = open + 1;
    while j < c.len() && c[j] != ']' {
        j += 1;
    }
    if j + 1 >= c.len() || c[j + 1] != '(' {
        return None;
    }
    let label: String = c[open + 1..j].iter().collect();
    let mut k = j + 2;
    while k < c.len() && c[k] != ')' {
        k += 1;
    }
    if k >= c.len() {
        return None;
    }
    let raw: String = c[j + 2..k].iter().collect();
    let url = raw
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .to_string();
    Some((label, url, k + 1))
}

/// 行内语法的「触发字符」：主循环只在这些字符上尝试构造语法，其余一律当作文字。
const INLINE_TRIGGERS: [u8; 11] = [
    b'\\', b'\n', b'`', b'!', b'[', b'$', b'*', b'_', b'<', b'~', b'=',
];

/// 256 格查找表，`TRIG[b]` 表示字节 `b` 是否是行内触发字符。
/// 多字节汉字的每个字节都 >= 0x80，永远落在这张表的 false 侧，所以按字节扫是安全的。
const fn trigger_table() -> [bool; 256] {
    let mut t = [false; 256];
    let mut i = 0usize;
    while i < INLINE_TRIGGERS.len() {
        t[INLINE_TRIGGERS[i] as usize] = true;
        i += 1;
    }
    t
}
static TRIG: [bool; 256] = trigger_table();

/// 文本里是否可能出现行内语法。
fn has_trigger(s: &str) -> bool {
    s.as_bytes().iter().any(|b| TRIG[*b as usize])
}

pub fn parse_inline(s: &str) -> Vec<Inline> {
    // 一个触发字符都没有 ⇒ 主循环只会把每个字符原样塞进 buf，结果必是单个 Text。
    // 提前返回省掉「整段转 Vec<char>」这次大分配：1000 万字里它占了解析时间的大头。
    if !has_trigger(s) {
        return if s.is_empty() {
            Vec::new()
        } else {
            vec![Inline::Text(s.to_string())]
        };
    }
    parse_inline_slow(s)
}

/// 逐字符的主循环。留在 `parse_inline_slow` 里不只是为了分层：
/// 测试用它作为参考实现，校验上面那条快路径逐字符等价。
fn parse_inline_slow(s: &str) -> Vec<Inline> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut i = 0usize;

    macro_rules! flush {
        () => {
            if !buf.is_empty() {
                out.push(Inline::Text(std::mem::take(&mut buf)));
            }
        };
    }

    while i < c.len() {
        let ch = c[i];

        if ch == '\\' && i + 1 < c.len() {
            buf.push(c[i + 1]);
            i += 2;
            continue;
        }
        if ch == '\n' {
            flush!();
            out.push(Inline::Break);
            i += 1;
            continue;
        }
        if ch == '`' {
            if let Some(end) = find_seq(&c, i + 1, &['`']) {
                let inner: String = c[i + 1..end].iter().collect();
                flush!();
                out.push(Inline::Code(inner));
                i = end + 1;
                continue;
            }
        }
        if ch == '!' && i + 1 < c.len() && c[i + 1] == '[' {
            if let Some((alt, url, end)) = parse_link(&c, i + 1) {
                flush!();
                // 图片后可跟 `{width=300}` 属性（拖动缩放写回），解析后保留。
                let (width, end) = image_width_attr(&c, end);
                out.push(Inline::Image { alt, url, width });
                i = end;
                continue;
            }
        }
        if ch == '[' {
            if i + 2 < c.len() && c[i + 1] == '[' {
                if let Some(end) = find_seq(&c, i + 2, &[b']' as char, b']' as char]) {
                    let name: String = c[i + 2..end].iter().collect();
                    flush!();
                    out.push(Inline::Wiki(name));
                    i = end + 2;
                    continue;
                }
            }
            if i + 1 < c.len() && c[i + 1] == '^' {
                if let Some(end) = find_seq(&c, i + 2, &[b']' as char]) {
                    let id: String = c[i + 2..end].iter().collect();
                    flush!();
                    out.push(Inline::Footnote(id));
                    i = end + 1;
                    continue;
                }
            }
            if let Some((label, url, end)) = parse_link(&c, i) {
                flush!();
                out.push(Inline::Link {
                    label: parse_inline(&label),
                    url,
                });
                i = end;
                continue;
            }
        }
        if ch == '$' {
            if let Some(end) = find_seq(&c, i + 1, &['$']) {
                let span: String = c[i..=end].iter().collect();
                let tex = inline_math_tex(&span)
                    .unwrap_or_else(|| c[i + 1..end].iter().collect::<String>());
                flush!();
                out.push(Inline::Math(tex));
                i = end + 1;
                continue;
            }
        }
        if (ch == '*' || ch == '_') && i + 1 < c.len() && c[i + 1] == ch {
            if let Some(end) = find_seq(&c, i + 2, &[ch, ch]) {
                let inner: String = c[i + 2..end].iter().collect();
                flush!();
                out.push(Inline::Strong(parse_inline(&inner)));
                i = end + 2;
                continue;
            }
        }
        if ch == '<' && i + 2 < c.len() && c[i + 1] == 'u' && c[i + 2] == '>' {
            if let Some(end) = find_seq(&c, i + 3, &['<', '/', 'u', '>']) {
                let inner: String = c[i + 3..end].iter().collect();
                flush!();
                out.push(Inline::Underline(parse_inline(&inner)));
                i = end + 4;
                continue;
            }
        }
        // 内联 HTML：<em>正文</em> 视为斜体（斜体按钮用的就是这套标记）
        if ch == '<' && i + 4 <= c.len() && c[i + 1..i + 4].iter().collect::<String>() == "em>" {
            if let Some(end) = find_seq(&c, i + 4, &['<', '/', 'e', 'm', '>']) {
                let inner: String = c[i + 4..end].iter().collect();
                flush!();
                out.push(Inline::Emph(parse_inline(&inner)));
                i = end + 5;
                continue;
            }
        }
        // 内联 HTML：<strong>正文</strong> 视为加粗（加粗按钮用的就是这套标记）
        if ch == '<' && i + 8 <= c.len() && c[i + 1..i + 8].iter().collect::<String>() == "strong>"
        {
            if let Some(end) = find_seq(&c, i + 8, &['<', '/', 's', 't', 'r', 'o', 'n', 'g', '>']) {
                let inner: String = c[i + 8..end].iter().collect();
                flush!();
                out.push(Inline::Strong(parse_inline(&inner)));
                i = end + 9;
                continue;
            }
        }
        if ch == '~' && i + 1 < c.len() && c[i + 1] == '~' {
            if let Some(end) = find_seq(&c, i + 2, &['~', '~']) {
                let inner: String = c[i + 2..end].iter().collect();
                flush!();
                out.push(Inline::Strike(parse_inline(&inner)));
                i = end + 2;
                continue;
            }
        }
        // 高亮 ==文字==：与删除线同理，成对才生效，落单的等号按普通文字处理。
        if ch == '=' && i + 1 < c.len() && c[i + 1] == '=' {
            if let Some(end) = find_seq(&c, i + 2, &['=', '=']) {
                let inner: String = c[i + 2..end].iter().collect();
                flush!();
                out.push(Inline::Highlight(parse_inline(&inner)));
                i = end + 2;
                continue;
            }
        }
        if ch == '*' || ch == '_' {
            if let Some(end) = find_seq(&c, i + 1, &[ch]) {
                if end > i + 1 {
                    let inner: String = c[i + 1..end].iter().collect();
                    flush!();
                    out.push(Inline::Emph(parse_inline(&inner)));
                    i = end + 1;
                    continue;
                }
            }
        }
        if ch == '<' {
            if let Some(end) = find_seq(&c, i + 1, &[b'>' as char]) {
                let inside: String = c[i + 1..end].iter().collect();
                if inside.starts_with("http://") || inside.starts_with("https://") {
                    flush!();
                    out.push(Inline::Link {
                        label: parse_inline(&inside),
                        url: inside,
                    });
                    i = end + 1;
                    continue;
                }
            }
        }
        buf.push(ch);
        i += 1;
    }
    flush!();
    out
}

// ---------------------------------------------------------------- 块级解析

fn heading(line: &str) -> Option<(u8, &str)> {
    let bytes = line.as_bytes();
    let mut n = 0usize;
    while n < bytes.len() && bytes[n] == b'#' {
        n += 1;
    }
    if (1..=6).contains(&n) && bytes.get(n) == Some(&b' ') {
        Some((n as u8, &line[n + 1..]))
    } else {
        None
    }
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
}

/// 拆掉行首连续的 `>` 与紧随的空格，返回 (引用层级, 去掉前缀的正文)。
fn quote_level(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let rest = t.trim_start_matches('>');
    let level = t.len() - rest.len();
    if level == 0 {
        return None;
    }
    Some((level, rest.trim_start_matches(' ')))
}

/// 引用里的围栏代码块起始行：`> ```lang` / `>> ~~~`。
fn quote_fence(line: &str) -> Option<(usize, &str)> {
    let (level, body) = quote_level(line)?;
    if body.starts_with("```") || body.starts_with("~~~") {
        Some((level, body))
    } else {
        None
    }
}

fn footnote_def(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with("[^") {
        return None;
    }
    let end = line.find("]:")?;
    Some((&line[2..end], line[end + 2..].trim()))
}

/// 返回 (缩进, 标记, 是否任务项及勾选状态, 正文)。
fn list_marker(line: &str) -> Option<(usize, String, Option<bool>, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let bytes = rest.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut marker_len = 0usize;
    let mut checked = None;
    // 无序标记："- "/"* "/"+ "；也接受单独一个 "-"（回车新建的空项经 trim_end
    // 后就是这样），保证空列表项仍是列表、再次回车能正确退出。
    if (bytes[0] == b'-' || bytes[0] == b'*' || bytes[0] == b'+')
        && (bytes.get(1) == Some(&b' ') || bytes.len() == 1)
    {
        marker_len = 1;
        let after = if bytes.len() > 1 { &rest[2..] } else { "" };
        // 也接受空的复选框 "- [ ]"（行尾无空格）：回车新建的空任务项会被
        // trim_end 成这种形式，必须仍解析为任务列表，才能在再次回车时退出。
        if after.starts_with("[ ] ")
            || after.starts_with("[x] ")
            || after.starts_with("[X] ")
            || after == "[ ]"
            || after == "[x]"
            || after == "[X]"
        {
            checked = Some(after.as_bytes()[1] != b' ');
            marker_len = 5;
        }
    } else {
        let mut k = 0usize;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        // 有序标记："1. "/"1) "；也接受结尾无空格的 "1."（回车新建的空项），
        // 使空有序项仍解析为列表项，再次回车可退出。
        if k > 0
            && k < bytes.len()
            && (bytes[k] == b'.' || bytes[k] == b')')
            && (k + 1 >= bytes.len() || bytes[k + 1] == b' ')
        {
            marker_len = k + 1;
        }
    }
    if marker_len == 0 {
        return None;
    }
    let marker = rest[..marker_len].trim_end().to_string();
    let body = rest[marker_len..].trim_start_matches(' ');
    Some((indent, marker, checked, body))
}

fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// 代码组外层围栏：4 个及以上反引号 + `code-group`。
fn code_group_open(t: &str) -> Option<usize> {
    let n = t.chars().take_while(|c| *c == '`').count();
    if n < 4 {
        return None;
    }
    let info = t[n..].trim();
    (info.eq_ignore_ascii_case("code-group") || info.eq_ignore_ascii_case("codegroup")).then_some(n)
}

/// 纯围栏闭合行：反引号数量 >= n，后面没有 info 文字。
fn is_fence_close(line: &str, n: usize) -> bool {
    let t = line.trim();
    let m = t.chars().take_while(|c| *c == '`').count();
    m >= n && t[m..].trim().is_empty()
}

/// 解析代码组内部的 ```lang 段落；每个段落是一组「打开围栏 → 代码 → 关闭围栏」。
fn parse_code_group_items(body: &[String]) -> Vec<CodeGroupItem> {
    let mut items = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let line = body[i].trim_start();
        let n = line.chars().take_while(|c| *c == '`').count();
        if n < 3 {
            i += 1;
            continue;
        }
        let lang = line[n..].trim().to_string();
        let mut code: Vec<String> = Vec::new();
        let mut j = i + 1;
        while j < body.len()
            && body[j]
                .trim_start()
                .chars()
                .take_while(|c| *c == '`')
                .count()
                < 3
        {
            code.push(body[j].clone());
            j += 1;
        }
        items.push(CodeGroupItem {
            lang,
            code: code.join("\n"),
        });
        i = if j < body.len() { j + 1 } else { j };
    }
    items
}

/// 由标签列表生成规范化的代码组源码（编辑器写回、插入模板共用）。
pub fn code_group_source(items: &[CodeGroupItem]) -> String {
    let mut out = String::from("````code-group\n");
    for it in items {
        out.push_str("```");
        out.push_str(&it.lang);
        out.push('\n');
        out.push_str(&it.code);
        out.push_str("\n```\n");
    }
    out.push_str("````");
    out
}

fn is_table_start(lines: &[&str], i: usize) -> bool {
    if i + 1 >= lines.len() || !lines[i].contains('|') {
        return false;
    }
    let sep = lines[i + 1].trim();
    sep.contains('|')
        && sep.contains('-')
        && sep
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
}

fn parse_align(line: &str) -> Vec<Align> {
    split_cells(line)
        .iter()
        .map(|c| {
            let t = c.trim();
            let center = t.starts_with(':') && t.ends_with(':');
            let right = !center && t.ends_with(':');
            if center {
                Align::Center
            } else if right {
                Align::Right
            } else {
                Align::Left
            }
        })
        .collect()
}

/// 用 pulldown-cmark 的 math 扩展识别一段 `$...$` 行内公式。
/// 仅用于确认语法并取出 LaTeX；正文解析仍由自研 parse_inline 完成。
pub fn inline_math_tex(source: &str) -> Option<String> {
    use pulldown_cmark::{Event, Options, Parser};
    Parser::new_ext(source, Options::ENABLE_MATH).find_map(|ev| match ev {
        Event::InlineMath(tex) => Some(tex.into_string()),
        _ => None,
    })
}

/// 识别块级公式 `$$...$$` 并取出其中的 LaTeX。
pub fn display_math_tex(source: &str) -> Option<String> {
    use pulldown_cmark::{Event, Options, Parser};
    Parser::new_ext(source, Options::ENABLE_MATH).find_map(|ev| match ev {
        Event::DisplayMath(tex) => Some(tex.into_string()),
        _ => None,
    })
}

/// 从原始 Markdown 中提取 ```math / ```latex 围栏内的 LaTeX。
///
/// 这里用 pulldown-cmark 只做「围栏识别」这一件事，正文块仍由自研解析器
/// 生成，以保留源码与 AST 的无损往返。不是数学围栏时返回 `None`。
pub fn math_fence_tex(source: &str) -> Option<String> {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

    let mut in_math = false;
    let mut tex = String::new();
    for ev in Parser::new(source) {
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let lang = info.trim().to_ascii_lowercase();
                if lang == "math" || lang == "latex" {
                    in_math = true;
                    tex.clear();
                }
            }
            Event::Text(t) if in_math => tex.push_str(&t),
            Event::End(TagEnd::CodeBlock) if in_math => return Some(tex),
            _ => {}
        }
    }
    None
}

/// 一条待解析的行内文本。能借用原文的一律借用（标题/引用/列表项/脚注/单行段落
/// 都是 `src` 的切片），只有需要拼行或切格子的才 own 一份，避免多一次全文拷贝。
enum JobText<'a> {
    Borrowed(&'a str),
    Owned(String),
}

impl JobText<'_> {
    fn as_str(&self) -> &str {
        match self {
            JobText::Borrowed(s) => s,
            JobText::Owned(s) => s,
        }
    }
}

/// 表格各单元格的输入下标，形状与 `Block::Table` 的 `head` / `rows` 一致。
struct TableShape {
    head: Vec<usize>,
    rows: Vec<Vec<usize>>,
}

/// 块扫描登记、扫描结束后统一执行的行内解析任务。
///
/// 为什么要摊开：`parse_inline` 是纯函数（同输入必同输出，且与块结构无关），
/// 而它连同其内部的 `Vec<char>`／`Vec<Inline>` 分配占到大文档解析时间的一半以上。
/// 摊平之后可以分片给多核，块结构、块源码、块下标仍由单线程扫描器原样决定，
/// 并行只影响「谁来算这份行内树」，不影响任何结构语义。
struct InlineJobs<'a> {
    inputs: Vec<JobText<'a>>,
    /// `(块下标, 输入下标)`：段落/标题/引用/列表项/脚注，一块一条。
    singles: Vec<(usize, usize)>,
    tables: Vec<(usize, TableShape)>,
}

impl<'a> InlineJobs<'a> {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            singles: Vec::new(),
            tables: Vec::new(),
        }
    }

    fn add(&mut self, text: JobText<'a>) -> usize {
        self.inputs.push(text);
        self.inputs.len() - 1
    }

    fn borrow(&mut self, s: &'a str) -> usize {
        self.add(JobText::Borrowed(s))
    }

    fn owned(&mut self, s: String) -> usize {
        self.add(JobText::Owned(s))
    }

    fn single(&mut self, block: usize, input: usize) {
        self.singles.push((block, input));
    }

    fn table(&mut self, block: usize, head: Vec<usize>, rows: Vec<Vec<usize>>) {
        self.tables.push((block, TableShape { head, rows }));
    }

    /// 跑完所有行内解析并写回。写回是「搬走」而不是复制：每个输入下标只被用一次。
    fn fill(self, blocks: &mut [Block], threads: usize) {
        let mut results = parse_inlines(&self.inputs, threads);
        for (b, n) in &self.singles {
            set_content(&mut blocks[*b], std::mem::take(&mut results[*n]));
        }
        for (b, shape) in &self.tables {
            let head: Vec<Vec<Inline>> = shape
                .head
                .iter()
                .map(|n| std::mem::take(&mut results[*n]))
                .collect();
            let rows: Vec<Vec<Vec<Inline>>> = shape
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|n| std::mem::take(&mut results[*n]))
                        .collect()
                })
                .collect();
            match &mut blocks[*b] {
                Block::Table { head: h, rows: w, .. } => {
                    *h = head;
                    *w = rows;
                }
                other => panic!("表格的行内结果写到了非表格块：{other:?}"),
            }
        }
    }
}

/// 行内分片用的线程数：上限 8，小文档在 `parse_inlines` 里另走串行。
fn inline_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(1)
}

/// 把一批行内文本解析成行内树：量少串行（起线程比解析本身贵），量大分片到多核。
fn parse_inlines(inputs: &[JobText], threads: usize) -> Vec<Vec<Inline>> {
    let serial = || inputs.iter().map(|t| parse_inline(t.as_str())).collect();
    if inputs.len() < 512 || threads < 2 {
        return serial();
    }
    let mut out = vec![Vec::<Inline>::new(); inputs.len()];
    let chunk = inputs.len().div_ceil(threads);
    std::thread::scope(|s| {
        for (dst, src) in out.chunks_mut(chunk).zip(inputs.chunks(chunk)) {
            s.spawn(move || {
                for (d, t) in dst.iter_mut().zip(src) {
                    *d = parse_inline(t.as_str());
                }
            });
        }
    });
    out
}

/// 把行内树写回它所属的块。只支持「带行内内容」的那几种块，
/// 写错类型说明扫描器与任务登记表脱节了——那是内部不变量被破坏，直接崩比丢内容好。
fn set_content(b: &mut Block, content: Vec<Inline>) {
    match b {
        Block::Heading { content: c, .. }
        | Block::Paragraph { content: c, .. }
        | Block::Quote { content: c, .. }
        | Block::Footnote { content: c, .. } => *c = content,
        Block::ListItem { item, .. } => item.content = content,
        other => panic!("行内结果写到了不支持行内的块：{other:?}"),
    }
}

/// 块级解析入口：结构与行内内容都算好。
pub fn parse(src: &str) -> Vec<Block> {
    parse_with(src, inline_threads())
}

/// `parse` 的实现，但行内阶段固定用 `threads` 个分片。
/// 暴露给测试：并行与串行的结果必须逐块相同，这是「并行不改语义」的证明。
pub(crate) fn parse_with(src: &str, threads: usize) -> Vec<Block> {
    let lines: Vec<&str> = src.split('\n').collect();
    let mut out: Vec<Block> = Vec::new();
    let mut jobs = InlineJobs::new();
    let mut i = 0usize;

    // YAML Front Matter（仅文档开头）
    if lines.first().map(|l| l.trim()) == Some("---") {
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim() != "---" {
            j += 1;
        }
        if j < lines.len() {
            let yaml = lines[i + 1..j].join("\n");
            let source = lines[i..=j].join("\n");
            out.push(Block::FrontMatter { yaml, source });
            i = j + 1;
        }
    }

    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_end();

        if t.is_empty() {
            out.push(Block::Blank {
                source: String::new(),
            });
            i += 1;
            continue;
        }
        if is_rule(t) {
            out.push(Block::Rule {
                source: line.to_string(),
            });
            i += 1;
            continue;
        }

        // 引用里的围栏代码块：`> ```lang` … `> ````
        // 必须排在普通围栏和引用段落之前，否则会被当成一行带反引号的引用文字。
        if let Some((qlevel, body)) = quote_fence(t) {
            let fence3 = &body[..3];
            let info = body[3..].trim().to_string();
            let p = ">".repeat(qlevel);
            let mut raw: Vec<String> = Vec::new();
            let mut parts = vec![line.to_string()];
            let mut j = i + 1;
            let mut closed = false;
            while j < lines.len() {
                let Some((lv, text)) = quote_level(lines[j]) else {
                    break;
                };
                if lv != qlevel {
                    break;
                }
                if text.trim_start().starts_with(fence3) {
                    parts.push(lines[j].to_string());
                    j += 1;
                    closed = true;
                    break;
                }
                raw.push(text.to_string());
                parts.push(lines[j].to_string());
                j += 1;
            }
            if !closed {
                // 未闭合时按引用结束处收尾：补一行闭合围栏，写回仍是合法代码块。
                parts.push(format!("{p} {fence3}"));
            }
            out.push(Block::Code {
                lang: info,
                code: raw.join("\n"),
                source: parts.join("\n"),
                quote: qlevel,
            });
            i = j;
            continue;
        }

        // 代码组：````code-group 外层围栏，内部是多个 ```lang 代码块。
        // 必须排在普通围栏之前：普通围栏会把 4 个反引号当成「以 3 个反引号开头」。
        if let Some(n) = code_group_open(t) {
            let fence = "`".repeat(n);
            let mut body: Vec<String> = Vec::new();
            let mut j = i + 1;
            let mut closed = false;
            while j < lines.len() {
                if is_fence_close(lines[j], n) {
                    closed = true;
                    j += 1;
                    break;
                }
                body.push(lines[j].to_string());
                j += 1;
            }
            let mut parts = vec![line.to_string()];
            parts.extend(body.iter().cloned());
            if closed {
                parts.push(lines[j - 1].to_string());
            } else {
                // 未闭合：补一行闭合围栏，写回仍是合法代码组。
                parts.push(fence);
            }
            let mut items = parse_code_group_items(&body);
            if items.is_empty() {
                items.push(CodeGroupItem {
                    lang: String::new(),
                    code: body.join("\n"),
                });
            }
            out.push(Block::CodeGroup {
                items,
                source: parts.join("\n"),
            });
            i = j;
            continue;
        }

        // 围栏代码块 / 图表 / 数学块
        if t.starts_with("```") || t.starts_with("~~~") {
            let fence3 = &t[..3];
            let info = t[3..].trim().to_string();
            let mut j = i + 1;
            let mut body: Vec<String> = Vec::new();
            while j < lines.len() && !lines[j].trim_start().starts_with(fence3) {
                body.push(lines[j].to_string());
                j += 1;
            }
            let closing = lines
                .get(j)
                .map(|s| s.to_string())
                .unwrap_or_else(|| fence3.to_string());
            let mut parts = vec![line.to_string()];
            parts.extend(body.iter().cloned());
            parts.push(closing);
            let source = parts.join("\n");
            let code = body.join("\n");
            let lang = info.to_lowercase();
            let block = if lang.starts_with("mermaid") {
                Block::Diagram {
                    kind: DiagramKind::Mermaid,
                    code,
                    source,
                }
            } else if lang == "flow" || lang == "flowchart" {
                Block::Diagram {
                    kind: DiagramKind::Flow,
                    code,
                    source,
                }
            } else if lang == "sequence" || lang == "seq" {
                Block::Diagram {
                    kind: DiagramKind::Sequence,
                    code,
                    source,
                }
            } else if lang == "gantt" {
                Block::Diagram {
                    kind: DiagramKind::Gantt,
                    code,
                    source,
                }
            } else if lang == "math" || lang == "latex" {
                Block::Math { tex: code, source }
            } else {
                Block::Code {
                    lang: info,
                    code,
                    source,
                    quote: 0,
                }
            };
            out.push(block);
            i = if j < lines.len() { j + 1 } else { j };
            continue;
        }
        if t.starts_with("$$") {
            let mut j = i + 1;
            let mut body: Vec<String> = Vec::new();
            while j < lines.len() && !lines[j].trim().starts_with("$$") {
                body.push(lines[j].to_string());
                j += 1;
            }
            let closing = lines
                .get(j)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "$$".to_string());
            let mut parts = vec![line.to_string()];
            parts.extend(body.iter().cloned());
            parts.push(closing);
            out.push(Block::Math {
                tex: body.join("\n"),
                source: parts.join("\n"),
            });
            i = if j < lines.len() { j + 1 } else { j };
            continue;
        }
        if t.eq_ignore_ascii_case("[toc]") {
            out.push(Block::Toc {
                source: line.to_string(),
            });
            i += 1;
            continue;
        }
        if let Some((level, rest)) = heading(t) {
            let job = jobs.borrow(rest);
            out.push(Block::Heading {
                level,
                content: Vec::new(),
                source: line.to_string(),
            });
            jobs.single(out.len() - 1, job);
            i += 1;
            continue;
        }
        if t.starts_with('>') {
            let raw = t.trim_start_matches('>');
            let level = t.len() - t.trim_start_matches('>').len();
            let body = raw.trim_start_matches(' ');
            let job = jobs.borrow(body);
            out.push(Block::Quote {
                level,
                content: Vec::new(),
                source: line.to_string(),
            });
            jobs.single(out.len() - 1, job);
            i += 1;
            continue;
        }
        if is_table_start(&lines, i) {
            let head_cells = split_cells(lines[i]);
            let align = parse_align(lines[i + 1]);
            let mut row_cells: Vec<Vec<String>> = Vec::new();
            let mut j = i + 2;
            while j < lines.len() && !lines[j].trim().is_empty() && lines[j].contains('|') {
                row_cells.push(split_cells(lines[j]));
                j += 1;
            }
            let source = lines[i..j].join("\n");
            let head: Vec<usize> = head_cells.into_iter().map(|c| jobs.owned(c)).collect();
            let rows: Vec<Vec<usize>> = row_cells
                .into_iter()
                .map(|r| r.into_iter().map(|c| jobs.owned(c)).collect())
                .collect();
            out.push(Block::Table {
                head: Vec::new(),
                rows: Vec::new(),
                align,
                source,
            });
            jobs.table(out.len() - 1, head, rows);
            i = j;
            continue;
        }
        if let Some((indent, marker, checked, body)) = list_marker(line) {
            let job = jobs.borrow(body);
            let item = ListItem {
                indent,
                marker,
                checked,
                content: Vec::new(),
                line: line.to_string(),
            };
            out.push(Block::ListItem {
                item,
                source: line.to_string(),
            });
            jobs.single(out.len() - 1, job);
            i += 1;
            continue;
        }
        if let Some((id, body)) = footnote_def(t) {
            let job = jobs.borrow(body);
            out.push(Block::Footnote {
                id: id.to_string(),
                content: Vec::new(),
                source: line.to_string(),
            });
            jobs.single(out.len() - 1, job);
            i += 1;
            continue;
        }

        // 普通段落：合并连续的非特殊行
        let start = i;
        while i < lines.len() {
            let l = lines[i];
            let tt = l.trim_end();
            if tt.is_empty()
                || is_rule(tt)
                || tt.starts_with("```")
                || tt.starts_with("~~~")
                || tt.starts_with("$$")
                || heading(tt).is_some()
                || tt.starts_with('>')
                || list_marker(l).is_some()
                || is_table_start(&lines, i)
                || tt.eq_ignore_ascii_case("[toc]")
                || footnote_def(tt).is_some()
            {
                break;
            }
            i += 1;
        }
        let source = lines[start..i].join("\n");
        // 只有一行时源码就是那一行本身，任务直接借用原始文本，不再拷一份
        let job = if i == start + 1 {
            jobs.borrow(lines[start])
        } else {
            jobs.owned(source.clone())
        };
        out.push(Block::Paragraph {
            content: Vec::new(),
            source,
        });
        jobs.single(out.len() - 1, job);
    }

    if out.is_empty() {
        out.push(Block::Blank {
            source: String::new(),
        });
    }
    jobs.fill(&mut out, threads);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_and_paragraph() {
        let b = parse("# 标题\n\n正文一段");
        assert!(matches!(b[0], Block::Heading { level: 1, .. }));
        assert!(b.iter().any(|x| matches!(x, Block::Paragraph { .. })));
    }

    /// ```math 围栏应被识别为数学代码块，并提取出 LaTeX 正文。
    #[test]
    fn math_fence_source() {
        let src = "```math\n\\frac{a}{b} = c\n```";
        assert_eq!(math_fence_tex(src).as_deref(), Some("\\frac{a}{b} = c\n"));
        assert!(math_fence_tex("```rust\nfn main() {}\n```").is_none());
        assert!(math_fence_tex("$$\nE = mc^2\n$$").is_none());
        assert!(matches!(parse(src)[0], Block::Math { .. }));
    }

    /// 块级公式 `$$...$$` 由 pulldown-cmark 识别为 DisplayMath。
    #[test]
    fn display_math_source() {
        assert_eq!(
            display_math_tex("$$\nE = mc^2\n$$").as_deref(),
            Some("\nE = mc^2\n")
        );
        assert!(display_math_tex("普通文本").is_none());
        assert!(matches!(parse("$$\nE = mc^2\n$$")[0], Block::Math { .. }));
    }

    /// 快路径必须与逐字符主循环逐结果相同。触发字符表一旦与实际语法脱节，
    /// 表现是「某类标记悄悄不生效」，只有这种对照测试能拦住。
    #[test]
    fn inline_fast_path_matches_char_loop() {
        let mut cases: Vec<String> = vec![
            String::new(),
            "纯文本".into(),
            "hello world 123".into(),
            "第一段很长的中文正文，没有任何标记。".into(),
            "**加粗**普通*斜体*混排".into(),
            "`代码`与 [链接](http://a.b) ".into(),
            "[[双链]]与[^注1]".into(),
            "$x^2$ 与 $$块$$".into(),
            "==高亮== 与 ~~删除~~".into(),
            "<u>下划线</u><em>斜体</em><strong>粗</strong>".into(),
            "<https://a.b>".into(),
            "转义 \\* 不是斜体".into(),
            "换行\n第二行".into(),
            "落单的 = 号 与 < 号 与 ~ 号".into(),
            "! 感叹号 后面不是图片".into(),
        ];
        // 每个 ASCII 字符嵌进「甲X乙」，覆盖表里表外的全部取值
        for b in 0u8..128 {
            cases.push(format!("甲{}乙", b as char));
            cases.push(format!("甲{}{}乙", b as char, b as char));
        }
        for s in &cases {
            assert_eq!(
                parse_inline(s),
                parse_inline_slow(s),
                "快路径与主循环结果不一致：{s:?}"
            );
        }
    }

    /// 行内解析被推迟到扫描结束后分片执行，写回必须「谁的正文回谁的块」。
    /// 这里给每个块一个独有标号，逐块核对正文里只出现自己的标号；
    /// 登记表与块下标一旦错位，别的块的标号就会串进来。
    #[test]
    fn deferred_inline_fills_the_right_block() {
        let mut src = String::new();
        for k in 0..60usize {
            src.push_str(&format!("# 标题{k} **粗{k}**\n\n"));
            src.push_str(&format!("段落{k} `码{k}` 与 [链{k}](http://a)\n\n"));
            src.push_str(&format!("> 引用{k} *斜{k}*\n\n"));
            src.push_str(&format!("- 项{k} ==亮{k}==\n\n"));
            src.push_str(&format!("[^{k}]: 脚注{k} **黑{k}**\n\n"));
            src.push_str(&format!("| 甲{k} | 乙{k} |\n|---|---|\n| 丙{k} | 丁{k} |\n\n"));
        }
        let blocks = parse(&src);
        let mut kinds: std::collections::HashMap<&'static str, usize> =
            std::collections::HashMap::new();
        for b in &blocks {
            let plain = b.plain();
            // 每个块的正文里只能有它自己那一个数字标号。
            let mut nums: Vec<String> = plain
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            nums.sort();
            nums.dedup();
            assert!(
                nums.len() <= 1,
                "块正文里串进了多个标号：{plain:?} → {nums:?}"
            );
            let name = match b {
                Block::Heading { .. } => "Heading",
                Block::Paragraph { .. } => "Paragraph",
                Block::Quote { .. } => "Quote",
                Block::ListItem { .. } => "ListItem",
                Block::Footnote { .. } => "Footnote",
                Block::Table { .. } => "Table",
                _ => continue,
            };
            *kinds.entry(name).or_insert(0usize) += 1;
        }
        // 六种带行内内容的块都得真的出现，否则这条测试什么都没检查。
        for name in [
            "Heading",
            "Paragraph",
            "Quote",
            "ListItem",
            "Footnote",
            "Table",
        ] {
            assert!(
                kinds.get(name).copied().unwrap_or(0) >= 60,
                "{name} 应有 60 块，实际 {:?}",
                kinds.get(name)
            );
        }
    }

    /// 并行与串行必须给出逐块相同的结果：这是「行内并行不改语义」的正面证明。
    /// 512 条以下走串行分支，所以样例做得足够大，把两种分支都覆盖上。
    #[test]
    fn parallel_inline_matches_serial() {
        let mut src = String::new();
        for k in 0..200usize {
            src.push_str(&format!("# 标题{k} **粗{k}** $x_{k}$\n\n"));
            src.push_str(&format!("段落{k} 有 `代码{k}`、[[双链{k}]] 和 <u>下划线{k}</u>。\n\n"));
            src.push_str(&format!("> 引用{k} ~~删{k}~~ ==亮{k}== [^注{k}]\n\n"));
            src.push_str(&format!("- [ ] 任务{k} *斜{k}*[链接](u/{k})\n\n"));
            src.push_str(&format!("| 甲{k} | 乙{k} |\n|:-:|---|\n| `c{k}` | **d{k}** |\n\n"));
            src.push_str("普通段落，没有任何标记，走行内快路径。\n\n");
        }
        let one = parse_with(&src, 1);
        let many = parse_with(&src, 8);
        assert_eq!(one.len(), many.len());
        for (i, (a, b)) in one.iter().zip(&many).enumerate() {
            assert_eq!(a, b, "块 {i} 在并行与串行下不同：{a:?} vs {b:?}");
        }
        assert!(
            one.len() > 1000,
            "样例应足够大以覆盖并行分支，实际 {} 块",
            one.len()
        );
    }

    /// 行内公式由 pulldown-cmark 的 math 扩展识别，并保留在 AST 中。
    #[test]
    fn inline_math_source() {
        assert_eq!(
            inline_math_tex("$x^2 + y^2$").as_deref(),
            Some("x^2 + y^2")
        );
        assert!(inline_math_tex("普通文本").is_none());
        let parsed = parse_inline("前 $x^2$ 后");
        assert!(
            parsed
                .iter()
                .any(|i| matches!(i, Inline::Math(t) if t == "x^2")),
            "应解析出行内 Math: {parsed:?}"
        );
    }

    /// <u>文字</u> 解析为下划线行内样式。
    #[test]
    fn inline_underline() {
        let v = parse_inline("<u>下划线文字</u>");
        assert!(
            v.iter().any(|i| matches!(i, Inline::Underline(_))),
            "应解析出 Underline: {:?}",
            v
        );
    }

    /// 空的列表标记（回车新建的空项）仍要解析为列表项，
    /// 这样再次回车才能正确退出列表。
    #[test]
    fn empty_task_item_parses_as_list() {
        for src in [
            "- [ ]", "- [ ] ", "- [x]", "- [X]", // 任务项
            "-", "*", "+", // 无序项
            "1.", "1)", "12.", // 有序项
        ] {
            let blocks = parse(src);
            assert!(
                matches!(blocks.first(), Some(Block::ListItem { .. })),
                "{src:?} 应解析为列表项: {blocks:?}"
            );
        }
        // 源码无损往返：规范化前后都保持原样
        let blocks = parse("- [ ]");
        assert_eq!(blocks[0].source(), "- [ ]");
    }

    /// <strong>文字</strong>（加粗按钮的标记）解析为加粗行内样式。
    #[test]
    fn inline_strong_html() {
        let v = parse_inline("前缀<strong>加粗文字</strong>后缀");
        assert!(
            v.iter().any(|i| matches!(i, Inline::Strong(_))),
            "应解析出 Strong: {:?}",
            v
        );
        // 配不上对的孤立标签按普通文字处理，不能吞掉正文
        let v = parse_inline("<strong>只有开头");
        assert!(
            !v.iter().any(|i| matches!(i, Inline::Strong(_))),
            "孤立的开标签不应产生加粗: {:?}",
            v
        );
    }

    /// <em>文字</em>（斜体按钮的标记）解析为斜体行内样式。
    #[test]
    fn inline_em_html() {
        let v = parse_inline("前缀<em>斜体文字</em>后缀");
        assert!(
            v.iter().any(|i| matches!(i, Inline::Emph(_))),
            "应解析出 Emph: {:?}",
            v
        );
        // 配不上对的孤立标签按普通文字处理，不能吞掉正文
        let v = parse_inline("<em>只有开头");
        assert!(
            !v.iter().any(|i| matches!(i, Inline::Emph(_))),
            "孤立的开标签不应产生斜体: {:?}",
            v
        );
    }

    #[test]
    fn inline_styles() {
        let v = parse_inline("**粗** *斜* `码` [[双链]] $x^2$ [链接](https://e.com)");
        assert!(v.iter().any(|i| matches!(i, Inline::Strong(_))));
        assert!(v.iter().any(|i| matches!(i, Inline::Emph(_))));
        assert!(v.iter().any(|i| matches!(i, Inline::Code(_))));
        assert!(v.iter().any(|i| matches!(i, Inline::Wiki(_))));
        assert!(v.iter().any(|i| matches!(i, Inline::Math(_))));
        assert!(v.iter().any(|i| matches!(i, Inline::Link { .. })));
    }

    /// 图片宽度属性 `{width=300}`：解析出宽度（拖动缩放写回就靠它）。
    #[test]
    fn image_width_attr() {
        let v = parse_inline("![a](imgs/a.png){width=300}");
        assert!(
            matches!(
                &v[0],
                Inline::Image {
                    width: Some(300),
                    ..
                }
            ),
            "{v:?}"
        );
        let v = parse_inline("![a](imgs/a.png){ width=240 }");
        assert!(
            matches!(
                &v[0],
                Inline::Image {
                    width: Some(240),
                    ..
                }
            ),
            "{v:?}"
        );
        // 没有属性时保持 None，图片后面的普通文字不受影响。
        let v = parse_inline("![a](imgs/a.png) 后面 {正常文字}");
        assert!(matches!(&v[0], Inline::Image { width: None, .. }), "{v:?}");
        assert!(v.iter().any(|i| matches!(i, Inline::Text(_))));
    }

    #[test]
    fn list_task_and_table() {
        let b = parse("- [x] 完成\n- [ ] 待办\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n");
        assert!(b.iter().any(|x| matches!(
            x,
            Block::ListItem {
                item: ListItem {
                    checked: Some(true),
                    ..
                },
                ..
            }
        )));
        assert!(b.iter().any(|x| matches!(x, Block::Table { .. })));
    }

    #[test]
    fn fenced_code_and_diagram() {
        let b = parse("```rust\nfn main() {}\n```\n\n```mermaid\ngraph LR\nA[X] --> B[Y]\n```\n");
        assert!(
            b.iter()
                .any(|x| matches!(x, Block::Code { lang, .. } if lang == "rust"))
        );
        assert!(b.iter().any(|x| matches!(
            x,
            Block::Diagram {
                kind: DiagramKind::Mermaid,
                ..
            }
        )));
    }

    /// 引用里的围栏代码块：`> ```lang` 起、`> ``` ` 止，正文去掉引用前缀，
    /// 源码保留每行的 `>` 前缀，写回后仍是引用内的代码块。
    #[test]
    fn quoted_fenced_code() {
        let src = "> 前言\n> ```rust\n> fn main() {}\n> \n> println!(\"hi\");\n> ```\n> 后记\n";
        let b = parse(src);
        let code = b
            .iter()
            .find_map(|x| match x {
                Block::Code {
                    lang, code, quote, ..
                } => Some((lang.clone(), code.clone(), *quote)),
                _ => None,
            })
            .expect("应解析出引用内的代码块");
        assert_eq!(code.0, "rust");
        assert_eq!(code.1, "fn main() {}\n\nprintln!(\"hi\");");
        assert_eq!(code.2, 1, "引用层级应为 1");
        // 代码块前后的引用段落仍然存在，代码块没有被拆成多行引用文字。
        assert_eq!(
            b.iter()
                .filter(|x| matches!(x, Block::Quote { .. }))
                .count(),
            2
        );
        let block_src = b
            .iter()
            .find(|x| matches!(x, Block::Code { .. }))
            .unwrap()
            .source()
            .to_string();
        assert_eq!(
            block_src,
            "> ```rust\n> fn main() {}\n> \n> println!(\"hi\");\n> ```"
        );
        // 源码写回（文档 source 由各块 source 用 \n 连接）能往返。
        assert_eq!(
            b.iter().map(|x| x.source()).collect::<Vec<_>>().join("\n"),
            src
        );
    }

    #[test]
    fn front_matter_and_toc() {
        let b = parse("---\ntitle: T\n---\n\n[TOC]\n\n# H\n");
        assert!(matches!(b[0], Block::FrontMatter { .. }));
        assert!(b.iter().any(|x| matches!(x, Block::Toc { .. })));
    }

    #[test]
    fn soft_break() {
        let v = parse_inline("第一行\n第二行");
        assert!(v.iter().any(|i| matches!(i, Inline::Break)));
    }

    #[test]
    fn source_roundtrip() {
        let src = "# H\n\n段落**粗**\n\n- a\n- b\n\n```js\nlet x=1;\n```\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let blocks = parse(src);
        let joined = blocks
            .iter()
            .map(|b| b.source())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(joined.trim_end(), src.trim_end());
    }

    /// 复制按钮复制的正是 `Block::Code.code`：纯代码，不含围栏与语言标记。
    #[test]
    fn code_block_copy_content() {
        let code = parse("```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n")
            .iter()
            .find_map(|b| match b {
                Block::Code { code, .. } => Some(code.clone()),
                _ => None,
            })
            .expect("应解析出代码块");
        assert_eq!(code, "fn main() {\n    println!(\"hi\");\n}");
        assert!(!code.contains("```"));
        assert!(!code.starts_with("rust"));

        let no_lang = parse("```\nalpha\nbeta\n```\n")
            .iter()
            .find_map(|b| match b {
                Block::Code { code, .. } => Some(code.clone()),
                _ => None,
            });
        assert_eq!(no_lang, Some("alpha\nbeta".to_string()));
    }

    /// 代码组：外层 4 个反引号，内部按 ```lang 分段；源码与标签列表无损往返。
    #[test]
    fn code_group_parses_languages_and_roundtrips() {
        let src = r#####"````code-group
```java
int a = 1;
```
```python
a = 1
```
````"#####;
        let blocks = parse(src);
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        match &blocks[0] {
            Block::CodeGroup { items, source } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].lang, "java");
                assert_eq!(items[0].code, "int a = 1;");
                assert_eq!(items[1].lang, "python");
                assert_eq!(items[1].code, "a = 1");
                assert_eq!(source, src, "源码应原样保留");
                assert_eq!(code_group_source(items), src, "规范化源码应无损往返");
            }
            other => panic!("应解析为代码组: {other:?}"),
        }
    }

    /// 空代码的标签也能往返（插入模板后还没输入代码时的状态）。
    #[test]
    fn code_group_empty_tabs_roundtrip() {
        let items = vec![
            CodeGroupItem {
                lang: "java".to_string(),
                code: String::new(),
            },
            CodeGroupItem {
                lang: "python".to_string(),
                code: String::new(),
            },
        ];
        let src = code_group_source(&items);
        match parse(&src).first() {
            Some(Block::CodeGroup { items: got, .. }) => assert_eq!(got, &items),
            other => panic!("应解析为代码组: {other:?}"),
        }
    }
}
