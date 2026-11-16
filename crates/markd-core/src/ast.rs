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
    Code(String),
    Link { label: Vec<Inline>, url: String },
    Image { alt: String, url: String },
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
            | Inline::Link { label: v, .. } => {
                collect_wiki(v, out);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- 行内解析

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

pub fn parse_inline(s: &str) -> Vec<Inline> {
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
                out.push(Inline::Image { alt, url });
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
                let tex: String = c[i + 1..end].iter().collect();
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
    if (bytes[0] == b'-' || bytes[0] == b'*' || bytes[0] == b'+') && bytes.get(1) == Some(&b' ') {
        marker_len = 1;
        let after = &rest[2..];
        if after.starts_with("[ ] ") || after.starts_with("[x] ") || after.starts_with("[X] ") {
            checked = Some(after.as_bytes()[1] != b' ');
            marker_len = 5;
        }
    } else {
        let mut k = 0usize;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k > 0
            && k + 1 < bytes.len()
            && (bytes[k] == b'.' || bytes[k] == b')')
            && bytes[k + 1] == b' '
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

pub fn parse(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.split('\n').collect();
    let mut out: Vec<Block> = Vec::new();
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
            out.push(Block::Heading {
                level,
                content: parse_inline(rest),
                source: line.to_string(),
            });
            i += 1;
            continue;
        }
        if t.starts_with('>') {
            let raw = t.trim_start_matches('>');
            let level = t.len() - t.trim_start_matches('>').len();
            let body = raw.trim_start_matches(' ');
            out.push(Block::Quote {
                level,
                content: parse_inline(body),
                source: line.to_string(),
            });
            i += 1;
            continue;
        }
        if is_table_start(&lines, i) {
            let head: Vec<Vec<Inline>> = split_cells(lines[i])
                .iter()
                .map(|c| parse_inline(c))
                .collect();
            let align = parse_align(lines[i + 1]);
            let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
            let mut j = i + 2;
            while j < lines.len() && !lines[j].trim().is_empty() && lines[j].contains('|') {
                rows.push(
                    split_cells(lines[j])
                        .iter()
                        .map(|c| parse_inline(c))
                        .collect(),
                );
                j += 1;
            }
            let source = lines[i..j].join("\n");
            out.push(Block::Table {
                head,
                rows,
                align,
                source,
            });
            i = j;
            continue;
        }
        if let Some((indent, marker, checked, body)) = list_marker(line) {
            let item = ListItem {
                indent,
                marker,
                checked,
                content: parse_inline(body),
                line: line.to_string(),
            };
            out.push(Block::ListItem {
                item,
                source: line.to_string(),
            });
            i += 1;
            continue;
        }
        if let Some((id, body)) = footnote_def(t) {
            out.push(Block::Footnote {
                id: id.to_string(),
                content: parse_inline(body),
                source: line.to_string(),
            });
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
        out.push(Block::Paragraph {
            content: parse_inline(&source),
            source,
        });
    }

    if out.is_empty() {
        out.push(Block::Blank {
            source: String::new(),
        });
    }
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
}
