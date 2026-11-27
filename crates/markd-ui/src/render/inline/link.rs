//! 渲染态文本里链接的位置查找与命中测试。

use super::*;

/// 按 `build_layout` 的绘制顺序累计字符数，记录每个链接标签在 galley 里的字符区间。
/// 这样点击文字时能判断点到的到底是不是某个链接。
pub(crate) fn collect_link_ranges(
    inlines: &[Inline],
    out: &mut Vec<(usize, usize, String)>,
    pos: &mut usize,
) {
    for inl in inlines {
        match inl {
            Inline::Text(t) => *pos += t.chars().count(),
            Inline::Strong(v)
            | Inline::Emph(v)
            | Inline::Strike(v)
            | Inline::Underline(v)
            | Inline::Highlight(v) => collect_link_ranges(v, out, pos),
            Inline::Code(t) => *pos += t.chars().count(),
            Inline::Link { label, url } => {
                let start = *pos;
                collect_link_ranges(label, out, pos);
                out.push((start, *pos, url.clone()));
            }
            Inline::Image { alt, url, .. } => {
                let txt = if alt.is_empty() { url } else { alt };
                *pos += format!("[图片: {}]", txt).chars().count();
            }
            Inline::Math(t) => *pos += math_pretty(t).chars().count(),
            Inline::Wiki(name) => *pos += format!("[[{}]]", name).chars().count(),
            Inline::Footnote(id) => *pos += format!("[{}]", id).chars().count(),
            // 内嵌 HTML：append_inline 只画正文、不画标签，这里必须同样只数正文，
            // 否则 span 之后的链接命中区间会整体错位。
            Inline::Span { content, .. } => collect_link_ranges(content, out, pos),
            Inline::Break => *pos += 1,
        }
    }
}

/// 点击字符下标 `idx` 处时，如果它落在某个链接标签里就返回该链接地址。
pub(crate) fn link_at(inlines: &[Inline], idx: usize) -> Option<String> {
    let mut ranges = Vec::new();
    let mut pos = 0usize;
    collect_link_ranges(inlines, &mut ranges, &mut pos);
    ranges
        .into_iter()
        // 点到标签最后一个字的右半部分时，光标下标会落在区间末尾，也算命中。
        .find(|(a, z, _)| idx >= *a && idx <= *z)
        .map(|(_, _, url)| url)
}

/// 编辑态（TextEdit 里显示的是原始 Markdown）的链接命中测试。
/// 光标下标对应原始缓冲区，所以直接扫描 `[标签](地址)` 的标签区间。
pub fn link_at_markdown(text: &str, idx: usize) -> Option<String> {
    let c: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < c.len() {
        if c[i] == '\\' {
            i += 2;
            continue;
        }
        if c[i] == '[' && !(i + 1 < c.len() && c[i + 1] == '[') {
            if let Some(close) = (i + 1..c.len()).find(|&k| c[k] == ']')
                && close + 1 < c.len()
                && c[close + 1] == '('
                && let Some(rel) = (close + 2..c.len()).find(|&k| c[k] == ')')
            {
                if idx > i && idx <= close {
                    let raw: String = c[close + 2..rel].iter().collect();
                    return Some(
                        raw.split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_matches(|ch| ch == '"' || ch == '\'')
                            .to_string(),
                    );
                }
                i = rel + 1;
                continue;
            }
        }
        i += 1;
    }
    None
}
