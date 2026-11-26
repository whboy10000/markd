//! Author: aurelianus <aurelianus@126.com>

//! 导出：把 AST 渲染成自包含的 HTML（可用浏览器打印为 PDF）。

use crate::ast::{self, Align, Block, DiagramKind, Inline};
use std::path::{Path, PathBuf};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn slug(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// 单个行内元素对应的「所见文本」字符数（与 `ast::inline_plain` 一致）。
fn inline_plain_len(i: &Inline) -> usize {
    match i {
        Inline::Text(t) | Inline::Code(t) | Inline::Math(t) | Inline::Wiki(t) => t.chars().count(),
        Inline::Strong(v)
        | Inline::Emph(v)
        | Inline::Strike(v)
        | Inline::Underline(v)
        | Inline::Highlight(v)
        | Inline::Link { label: v, .. } => v.iter().map(inline_plain_len).sum(),
        Inline::Image { alt, .. } => alt.chars().count(),
        Inline::Footnote(id) => id.chars().count(),
        Inline::Break => 1,
    }
}

fn slice_chars(s: &str, a: usize, b: usize) -> &str {
    let cs: Vec<(usize, char)> = s.char_indices().collect();
    let start = cs.get(a).map(|(i, _)| *i).unwrap_or(s.len());
    let end = if b >= cs.len() { s.len() } else { cs[b].0 };
    if start >= end { "" } else { &s[start..end] }
}

/// 只输出所见文本 [start, end) 区间的行内 HTML，并尽量保留包裹其外的格式标签
/// （加粗/斜体/链接等），供「块内选中一部分」的富文本复制使用。
fn inline_html_range(v: &[Inline], start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    for i in v {
        let len = inline_plain_len(i);
        let ipos = pos;
        pos += len;
        if pos <= start {
            continue;
        }
        if ipos >= end {
            break;
        }
        let local_lo = start.saturating_sub(ipos);
        let local_hi = (end - ipos).min(len);
        match i {
            Inline::Text(t) => out.push_str(&esc(slice_chars(t, local_lo, local_hi))),
            Inline::Code(t) => out.push_str(&format!(
                "<code>{}</code>",
                esc(slice_chars(t, local_lo, local_hi))
            )),
            Inline::Strong(ch) => out.push_str(&format!(
                "<strong>{}</strong>",
                inline_html_range(ch, local_lo, local_hi)
            )),
            Inline::Emph(ch) => out.push_str(&format!(
                "<em>{}</em>",
                inline_html_range(ch, local_lo, local_hi)
            )),
            Inline::Strike(ch) => out.push_str(&format!(
                "<del>{}</del>",
                inline_html_range(ch, local_lo, local_hi)
            )),
            Inline::Underline(ch) => out.push_str(&format!(
                "<u>{}</u>",
                inline_html_range(ch, local_lo, local_hi)
            )),
            Inline::Highlight(ch) => out.push_str(&format!(
                "<mark>{}</mark>",
                inline_html_range(ch, local_lo, local_hi)
            )),
            Inline::Link { label, url } => out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                esc(url),
                inline_html_range(label, local_lo, local_hi)
            )),
            Inline::Math(t) => out.push_str(&format!(
                "<code class=\"math\">\\({}\\)</code>",
                esc(slice_chars(t, local_lo, local_hi))
            )),
            Inline::Wiki(name) => out.push_str(&format!(
                "<span class=\"wiki\">[[{}]]</span>",
                esc(slice_chars(name, local_lo, local_hi))
            )),
            // 原子行内元素：只要与选区相交就完整输出。
            Inline::Image { alt, url, width } => {
                let w = width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default();
                out.push_str(&format!(
                    "<img src=\"{}\" alt=\"{}\"{w} />",
                    esc(url),
                    esc(alt)
                ));
            }
            Inline::Footnote(id) => out.push_str(&format!("<sup class=\"fn\">[{}]</sup>", esc(id))),
            Inline::Break => out.push_str("<br/>"),
        }
    }
    out
}

/// 把一段行内 Markdown 源码渲染成 HTML 片段（无外层段落标签）。
/// 供单块文本选区的富文本复制使用：选中的是一段源码，重新解析即可保留加粗/
/// 斜体/行内代码/链接等格式。
pub fn inline_markdown_html(src: &str) -> String {
    inline_html(&ast::parse_inline(src))
}

/// 把一段 Markdown 源码（可含多个块）渲染成 HTML 片段。
/// 供「复制整块源码」的按钮使用：表格/代码块等复制按钮除了纯文本，
/// 还要给出带格式的 HTML 版本。
pub fn source_html(src: &str) -> String {
    let blocks = ast::parse(src);
    let mut out = String::new();
    for b in &blocks {
        out.push_str(&block_html(b, &blocks));
    }
    out
}

fn inline_html(v: &[Inline]) -> String {
    let mut s = String::new();
    for i in v {
        match i {
            Inline::Text(t) => s.push_str(&esc(t)),
            Inline::Strong(x) => s.push_str(&format!("<strong>{}</strong>", inline_html(x))),
            Inline::Emph(x) => s.push_str(&format!("<em>{}</em>", inline_html(x))),
            Inline::Strike(x) => s.push_str(&format!("<del>{}</del>", inline_html(x))),
            Inline::Underline(x) => s.push_str(&format!("<u>{}</u>", inline_html(x))),
            Inline::Highlight(x) => s.push_str(&format!("<mark>{}</mark>", inline_html(x))),
            Inline::Code(t) => s.push_str(&format!("<code>{}</code>", esc(t))),
            Inline::Link { label, url } => {
                s.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    esc(url),
                    inline_html(label)
                ));
            }
            Inline::Image { alt, url, width } => {
                // 拖动缩放写回的宽度导出成 HTML 的 width 属性。
                let w = width.map(|w| format!(" width=\"{w}\"")).unwrap_or_default();
                s.push_str(&format!(
                    "<img src=\"{}\" alt=\"{}\"{w} />",
                    esc(url),
                    esc(alt)
                ));
            }
            Inline::Math(t) => s.push_str(&format!("<code class=\"math\">\\({}\\)</code>", esc(t))),
            Inline::Wiki(name) => {
                s.push_str(&format!("<span class=\"wiki\">[[{}]]</span>", esc(name)))
            }
            Inline::Footnote(id) => s.push_str(&format!("<sup class=\"fn\">[{}]</sup>", esc(id))),
            Inline::Break => s.push_str("<br/>"),
        }
    }
    s
}

/// 视频围栏（```video / ```bilibili）导出成 B 站的嵌入式外链播放器：
/// 浏览器打开导出文件就能播。语言标记不对、或链接认不出时返回 None，
/// 由调用方退回普通代码块。
fn video_html(lang: &str, code: &str) -> Option<String> {
    if !crate::video::is_video_lang(lang) {
        return None;
    }
    let src = crate::video::url_from_fence(code)?;
    // player_url 里的标识已在解析阶段校验过字符集，仍然按属性值转义一次。
    Some(format!(
        "<div class=\"video-embed\"><iframe src=\"{}\" title=\"bilibili 视频\" scrolling=\"no\" frameBorder=\"0\" allowfullscreen=\"allowfullscreen\" allow=\"autoplay; video-fullscreen; fullscreen\"></iframe><noscript><a href=\"{}\">在 B 站观看此视频</a></noscript></div>",
        esc(&src.player_url()),
        esc(&src.page_url())
    ))
}

fn align_attr(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn style(dark: bool) -> &'static str {
    if dark {
        "body{background:#202124;color:#dee0e3} a{color:#78b2fa} code,pre{background:#2a2c30} table th{background:#26282c} blockquote{border-color:#3e4046;color:#8c8e94} h1,h2{border-color:#3e4046}"
    } else {
        "body{background:#fff;color:#262628} a{color:#2e75b6} code,pre{background:#f5f5f7} table th{background:#f9f9fb} blockquote{border-color:#e0e0e4;color:#78787d} h1,h2{border-color:#eee}"
    }
}

pub fn to_html(title: &str, source: &str, dark: bool) -> String {
    let blocks = ast::parse(source);
    let mut out = String::new();
    let mut list_open: Option<&'static str> = None;

    for b in &blocks {
        match b {
            Block::ListItem { .. } => {}
            _ => {
                if let Some(tag) = list_open.take() {
                    out.push_str(&format!("</{}>", tag));
                }
            }
        }
        match b {
            Block::Heading { level, content, .. } => {
                let text = ast::inline_plain(content);
                out.push_str(&format!(
                    "<h{0} id=\"{1}\">{2}</h{0}>\n",
                    level,
                    esc(&slug(&text)),
                    inline_html(content)
                ));
            }
            Block::Paragraph { content, .. } => {
                out.push_str(&format!("<p>{}</p>\n", inline_html(content)));
            }
            Block::Quote { content, .. } => {
                out.push_str(&format!(
                    "<blockquote>{}</blockquote>\n",
                    inline_html(content)
                ));
            }
            Block::ListItem { item, .. } => {
                let tag = if item.checked.is_some() { "ul" } else { "ul" };
                if list_open != Some(tag) {
                    if let Some(prev) = list_open.take() {
                        out.push_str(&format!("</{}>", prev));
                    }
                    out.push_str(&format!("<{}>\n", tag));
                    list_open = Some(tag);
                }
                let cb = match item.checked {
                    Some(true) => "<input type=\"checkbox\" checked disabled/> ",
                    Some(false) => "<input type=\"checkbox\" disabled/> ",
                    None => "",
                };
                out.push_str(&format!(
                    "<li style=\"margin-left:{}em\">{}{}</li>\n",
                    item.indent / 2,
                    cb,
                    inline_html(&item.content)
                ));
            }
            Block::Code { lang, code, .. } => {
                if let Some(html) = video_html(lang, code) {
                    out.push_str(&html);
                    out.push('\n');
                } else {
                    out.push_str(&format!(
                        "<pre><code class=\"language-{}\">{}</code></pre>\n",
                        esc(lang),
                        esc(code)
                    ));
                }
            }
            Block::CodeGroup { items, .. } => {
                // 代码组：HTML 里保留 Tab 切换（与编辑器一致）。
                out.push_str("<div class=\"code-group\">\n<div class=\"code-group-tabs\">\n");
                for (i, it) in items.iter().enumerate() {
                    let label = if it.lang.trim().is_empty() {
                        "纯文本"
                    } else {
                        it.lang.as_str()
                    };
                    // 点击切换：内联脚本按 data-lang 找对应的代码面板，
                    // 不依赖全局函数，导出的 HTML 单文件即可用。
                    out.push_str(&format!(
                        "<button class=\"{}\" data-lang=\"{}\" onclick=\"var g=this.closest('.code-group');g.querySelectorAll('.code-group-tabs button').forEach(function(x){{x.classList.remove('active')}});this.classList.add('active');var l=this.dataset.lang;g.querySelectorAll('.code-group-panel').forEach(function(p){{p.classList.toggle('active',p.dataset.lang===l)}});\">{}</button>\n",
                        if i == 0 { "active" } else { "" },
                        esc(&it.lang),
                        esc(label)
                    ));
                }
                out.push_str("</div>\n");
                for (i, it) in items.iter().enumerate() {
                    out.push_str(&format!(
                        "<pre class=\"code-group-panel{}\" data-lang=\"{}\"><code class=\"language-{}\">{}</code></pre>\n",
                        if i == 0 { " active" } else { "" },
                        esc(&it.lang),
                        esc(&it.lang),
                        esc(&it.code)
                    ));
                }
                out.push_str("</div>\n");
            }
            Block::Table {
                head, rows, align, ..
            } => {
                out.push_str("<table><thead><tr>");
                for (i, c) in head.iter().enumerate() {
                    let a = align.get(i).copied().unwrap_or(Align::Left);
                    out.push_str(&format!(
                        "<th style=\"text-align:{}\">{}</th>",
                        align_attr(a),
                        inline_html(c)
                    ));
                }
                out.push_str("</tr></thead><tbody>");
                for row in rows {
                    out.push_str("<tr>");
                    for (i, c) in row.iter().enumerate() {
                        let a = align.get(i).copied().unwrap_or(Align::Left);
                        out.push_str(&format!(
                            "<td style=\"text-align:{}\">{}</td>",
                            align_attr(a),
                            inline_html(c)
                        ));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</tbody></table>\n");
            }
            Block::Rule { .. } => out.push_str("<hr/>\n"),
            Block::Math { tex, .. } => {
                out.push_str(&format!(
                    "<div class=\"math-block\">$${}$$</div>\n",
                    esc(tex)
                ));
            }
            Block::Diagram { kind, code, .. } => {
                let cls = match kind {
                    DiagramKind::Mermaid => "mermaid",
                    _ => "diagram",
                };
                out.push_str(&format!("<pre class=\"{}\">{}</pre>\n", cls, esc(code)));
            }
            Block::FrontMatter { yaml, .. } => {
                out.push_str(&format!(
                    "<pre class=\"front-matter\">---\n{}\n---</pre>\n",
                    esc(yaml)
                ));
            }
            Block::Footnote { id, content, .. } => {
                out.push_str(&format!(
                    "<div class=\"footnote\"><sup>{}</sup> {}</div>\n",
                    esc(id),
                    inline_html(content)
                ));
            }
            Block::Toc { .. } => {
                out.push_str("<nav class=\"toc\"><strong>目录</strong><ul>");
                for h in blocks.iter().filter_map(|x| x.heading()) {
                    out.push_str(&format!(
                        "<li style=\"margin-left:{}em\"><a href=\"#{}\">{}</a></li>",
                        (h.0 - 1) as usize,
                        esc(&slug(&h.1)),
                        esc(&h.1)
                    ));
                }
                out.push_str("</ul></nav>\n");
            }
            Block::Blank { .. } => {}
        }
    }
    if let Some(tag) = list_open.take() {
        out.push_str(&format!("</{}>", tag));
    }

    format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>{title}</title>
<style>
*{{box-sizing:border-box}}
{style}
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
line-height:1.75;max-width:760px;margin:0 auto;padding:48px 28px 96px;font-size:16px}}
h1,h2,h3,h4,h5,h6{{line-height:1.3;margin:1.6em 0 .6em}}
h1{{font-size:2em}} h2{{font-size:1.6em;padding-bottom:.3em;border-bottom:1px solid}}
h3{{font-size:1.3em}} p{{margin:.8em 0}}
a{{text-decoration:none}} a:hover{{text-decoration:underline}}
code{{font-family:"SF Mono",Menlo,Consolas,monospace;padding:.15em .35em;border-radius:4px;font-size:.9em}}
pre{{padding:16px;border-radius:8px;overflow:auto;line-height:1.5}}
pre code{{padding:0;background:none}}
blockquote{{margin:1em 0;padding:.4em 1em;border-left:4px solid}}
table{{border-collapse:collapse;width:100%;margin:1em 0}}
th,td{{border:1px solid #ddd;padding:8px 12px}} th{{font-weight:600}}
hr{{border:none;border-top:1px solid #ddd;margin:2em 0}}
img{{max-width:100%}} .wiki{{color:#8957e5}}
.video-embed{{position:relative;width:100%;padding-top:56.25%;margin:1em 0;background:#000;border-radius:8px;overflow:hidden}}
.video-embed iframe{{position:absolute;left:0;top:0;width:100%;height:100%;border:0}}
.math-block{{text-align:center;font-family:Georgia,serif;font-size:1.15em;margin:1.2em 0}}
.front-matter{{font-size:.85em;opacity:.8}}
.toc{{border:1px solid #ddd;border-radius:8px;padding:12px 20px;margin:1em 0}}
.toc ul{{margin:.4em 0}} .footnote{{font-size:.9em;opacity:.85}}
.code-group{{margin:1em 0;border:1px solid #ddd;border-radius:8px;overflow:hidden}}
.code-group-tabs{{display:flex;gap:2px;padding:4px;border-bottom:1px solid #ddd;flex-wrap:wrap}}
.code-group-tabs button{{background:none;border:none;padding:6px 12px;cursor:pointer;font:inherit;font-size:.85em;opacity:.65;border-radius:4px}}
.code-group-tabs button.active{{opacity:1;font-weight:600;background:rgba(127,127,127,.14)}}
.code-group-panel{{display:none;margin:0;border-radius:0}}
.code-group-panel.active{{display:block}}
@media print{{body{{padding:0;max-width:none}}}}
</style></head><body>
{body}
</body></html>
"#,
        title = esc(title),
        style = style(dark),
        body = out
    )
}

/// 把一个块渲染成 HTML 片段（不含外层样式）。供剪贴板富文本复制复用。
fn block_html(b: &Block, blocks: &[Block]) -> String {
    let mut out = String::new();
    match b {
        Block::Heading { level, content, .. } => {
            out.push_str(&format!("<h{0}>{1}</h{0}>", level, inline_html(content)));
        }
        Block::Paragraph { content, .. } => {
            out.push_str(&format!("<p>{}</p>", inline_html(content)));
        }
        Block::Quote { content, .. } => {
            out.push_str(&format!(
                "<blockquote>{}</blockquote>",
                inline_html(content)
            ));
        }
        Block::ListItem { item, .. } => {
            let cb = match item.checked {
                Some(true) => "<input type=\"checkbox\" checked disabled/> ",
                Some(false) => "<input type=\"checkbox\" disabled/> ",
                None => "",
            };
            out.push_str(&format!(
                "<li style=\"margin-left:{}em\">{}{}</li>",
                item.indent / 2,
                cb,
                inline_html(&item.content)
            ));
        }
        Block::Code { lang, code, .. } => {
            if let Some(html) = video_html(lang, code) {
                out.push_str(&html);
            } else {
                out.push_str(&format!(
                    "<pre><code class=\"language-{}\">{}</code></pre>",
                    esc(lang),
                    esc(code)
                ));
            }
        }
        Block::CodeGroup { items, .. } => {
            out.push_str("<div class=\"code-group\">\n<div class=\"code-group-tabs\">\n");
            for (i, it) in items.iter().enumerate() {
                let label = if it.lang.trim().is_empty() {
                    "纯文本"
                } else {
                    it.lang.as_str()
                };
                out.push_str(&format!(
                    "<button class=\"{}\" data-lang=\"{}\">{}</button>\n",
                    if i == 0 { "active" } else { "" },
                    esc(&it.lang),
                    esc(label)
                ));
            }
            out.push_str("</div>\n");
            for (i, it) in items.iter().enumerate() {
                out.push_str(&format!(
                    "<pre class=\"code-group-panel{}\" data-lang=\"{}\"><code class=\"language-{}\">{}</code></pre>\n",
                    if i == 0 { " active" } else { "" },
                    esc(&it.lang),
                    esc(&it.lang),
                    esc(&it.code)
                ));
            }
            out.push_str("</div>\n");
        }
        Block::Table {
            head, rows, align, ..
        } => {
            out.push_str("<table><thead><tr>");
            for (i, c) in head.iter().enumerate() {
                let a = align.get(i).copied().unwrap_or(Align::Left);
                out.push_str(&format!(
                    "<th style=\"text-align:{}\">{}</th>",
                    align_attr(a),
                    inline_html(c)
                ));
            }
            out.push_str("</tr></thead><tbody>");
            for row in rows {
                out.push_str("<tr>");
                for (i, c) in row.iter().enumerate() {
                    let a = align.get(i).copied().unwrap_or(Align::Left);
                    out.push_str(&format!(
                        "<td style=\"text-align:{}\">{}</td>",
                        align_attr(a),
                        inline_html(c)
                    ));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>");
        }
        Block::Rule { .. } => out.push_str("<hr/>"),
        Block::Math { tex, .. } => {
            out.push_str(&format!("<div class=\"math-block\">$${}$$</div>", esc(tex)));
        }
        Block::Diagram { kind, code, .. } => {
            let cls = match kind {
                DiagramKind::Mermaid => "mermaid",
                _ => "diagram",
            };
            out.push_str(&format!("<pre class=\"{}\">{}</pre>", cls, esc(code)));
        }
        Block::FrontMatter { yaml, .. } => {
            out.push_str(&format!(
                "<pre class=\"front-matter\">---\n{}\n---</pre>",
                esc(yaml)
            ));
        }
        Block::Footnote { id, content, .. } => {
            out.push_str(&format!(
                "<div class=\"footnote\"><sup>{}</sup> {}</div>",
                esc(id),
                inline_html(content)
            ));
        }
        Block::Toc { .. } => {
            out.push_str("<nav class=\"toc\"><strong>目录</strong><ul>");
            for h in blocks.iter().filter_map(|x| x.heading()) {
                out.push_str(&format!(
                    "<li style=\"margin-left:{}em\"><a href=\"#{}\">{}</a></li>",
                    (h.0 - 1) as usize,
                    esc(&slug(&h.1)),
                    esc(&h.1)
                ));
            }
            out.push_str("</ul></nav>");
        }
        Block::Blank { .. } => {}
    }
    out
}

/// 把跨块选区渲染成 HTML 片段，供「带格式复制」写入剪贴板。
/// `sel` 为 (起块, 起字符, 止块, 止字符)（所见文本坐标，顺序不限）。
/// 首尾块按选中字符区间裁剪并保留行内格式，中间块完整输出。
pub fn cross_sel_html(blocks: &[Block], sel: (usize, usize, usize, usize)) -> String {
    let (a, ac, b, bc) = if (sel.0, sel.1) <= (sel.2, sel.3) {
        (sel.0, sel.1, sel.2, sel.3)
    } else {
        (sel.2, sel.3, sel.0, sel.1)
    };
    let mut out = String::new();
    for idx in a..=b {
        let Some(block) = blocks.get(idx) else {
            continue;
        };
        match block {
            Block::Heading { level, content, .. } => {
                let lo = if idx == a { ac } else { 0 };
                let hi = if idx == b { bc } else { usize::MAX };
                out.push_str(&format!(
                    "<h{0}>{1}</h{0}>",
                    level,
                    inline_html_range(content, lo, hi)
                ));
            }
            Block::Paragraph { content, .. } => {
                let lo = if idx == a { ac } else { 0 };
                let hi = if idx == b { bc } else { usize::MAX };
                out.push_str(&format!("<p>{}</p>", inline_html_range(content, lo, hi)));
            }
            Block::Quote { content, .. } => {
                let lo = if idx == a { ac } else { 0 };
                let hi = if idx == b { bc } else { usize::MAX };
                out.push_str(&format!(
                    "<blockquote>{}</blockquote>",
                    inline_html_range(content, lo, hi)
                ));
            }
            Block::ListItem { item, .. } => {
                let lo = if idx == a { ac } else { 0 };
                let hi = if idx == b { bc } else { usize::MAX };
                let cb = match item.checked {
                    Some(true) => "<input type=\"checkbox\" checked disabled/> ",
                    Some(false) => "<input type=\"checkbox\" disabled/> ",
                    None => "",
                };
                out.push_str(&format!(
                    "<li style=\"margin-left:{}em\">{}{}</li>",
                    item.indent / 2,
                    cb,
                    inline_html_range(&item.content, lo, hi)
                ));
            }
            // 代码块：正文逐字符可映射（拖选可以只选其中几行），按区间裁剪。
            Block::Code { .. } => {
                let lo = if idx == a { ac } else { 0 };
                let hi = if idx == b { bc } else { usize::MAX };
                let code = block.plain();
                let slice: String = code.chars().skip(lo).take(hi.saturating_sub(lo)).collect();
                out.push_str(&selected_text_html(block, &slice));
            }
            // 图表/表格/公式等原子块：整块输出。
            _ => out.push_str(&block_html(block, blocks)),
        }
    }
    out
}

/// 把「某一块中被选中的源码片段」渲染成带格式的 HTML。
/// 选中片段来自编辑缓冲区（标题=纯文字、代码块=纯代码、其余=Markdown 源码），
/// 因此重新解析这段 Markdown 即可保留行内加粗/斜体/代码/链接等格式。
pub fn selected_text_html(block: &Block, text: &str) -> String {
    match block {
        Block::Code { lang, .. } => format!(
            "<pre><code class=\"language-{}\">{}</code></pre>",
            esc(lang),
            esc(text)
        ),
        Block::Diagram { .. } => format!("<pre>{}</pre>", esc(text)),
        Block::Heading { level, .. } => {
            format!("<h{0}>{1}</h{0}>", level, inline_markdown_html(text))
        }
        Block::Quote { .. } => format!("<blockquote>{}</blockquote>", inline_markdown_html(text)),
        Block::ListItem { item, .. } => format!(
            "<li style=\"margin-left:{}em\">{}</li>",
            item.indent / 2,
            inline_markdown_html(text)
        ),
        Block::Footnote { .. } => {
            format!(
                "<div class=\"footnote\">{}</div>",
                inline_markdown_html(text)
            )
        }
        // 表格/代码组是结构化块，单独取一段源码无法反映结构；用普通段落承载
        // 选中文本，至少保留行内格式与转义。
        _ => format!("<p>{}</p>", inline_markdown_html(text)),
    }
}

/// 单块内所选字符区间的 HTML 片段（所见文本坐标）。
pub fn block_range_html(block: &Block, a: usize, z: usize) -> String {
    match block {
        Block::Heading { level, content, .. } => {
            format!("<h{0}>{1}</h{0}>", level, inline_html_range(content, a, z))
        }
        Block::Paragraph { content, .. } => {
            format!("<p>{}</p>", inline_html_range(content, a, z))
        }
        Block::Quote { content, .. } => format!(
            "<blockquote>{}</blockquote>",
            inline_html_range(content, a, z)
        ),
        Block::ListItem { item, .. } => format!(
            "<li style=\"margin-left:{}em\">{}</li>",
            item.indent / 2,
            inline_html_range(&item.content, a, z)
        ),
        _ => block_html(block, &[]),
    }
}

pub fn export_html(
    path: Option<&Path>,
    source: &str,
    title: &str,
    dark: bool,
) -> std::io::Result<PathBuf> {
    let target = match path {
        Some(p) => p.with_extension("html"),
        None => std::env::current_dir()?.join("untitled.html"),
    };
    crate::workspace::write(&target, &to_html(title, source, dark))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_contains_structure() {
        let src = "# 标题\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main(){}\n```\n";
        let h = to_html("T", src, false);
        assert!(h.contains("<h1"));
        assert!(h.contains("<table>"));
        assert!(h.contains("language-rust"));
        assert!(h.contains("<!doctype html>"));
    }

    #[test]
    fn cross_sel_html_preserves_block_and_inline_format() {
        let src = "# 标题 **粗**\n\n段落含 `代码` 与 *斜体*\n\n```rust\nfn main(){}\n```\n\n| A | B |\n| --- | --- |\n| 1 | 2 |";
        let blocks = ast::parse(src);
        // 全选：标题 0 到表格末尾
        let last = blocks.len() - 1;
        let h = cross_sel_html(&blocks, (0, 0, last, 100));
        assert!(h.contains("<h1>"), "{h}");
        assert!(h.contains("<strong>粗</strong>"), "{h}");
        assert!(h.contains("<code>代码</code>"), "{h}");
        assert!(h.contains("<em>斜体</em>"), "{h}");
        assert!(h.contains("language-rust"), "{h}");
        assert!(h.contains("<table>"), "{h}");
    }

    #[test]
    fn cross_sel_html_slices_endpoint_inline() {
        let src = "甲乙**粗体丙**丁";
        let blocks = ast::parse(src);
        let content = match &blocks[0] {
            Block::Paragraph { content, .. } => content.clone(),
            _ => panic!("应为段落"),
        };
        // 只选「粗体丙」：所见文本 2..5
        let h = inline_html_range(&content, 2, 5);
        assert_eq!(h, "<strong>粗体丙</strong>", "裁剪应保留加粗标签：{h}");
        // 选「乙粗体」：所见文本 1..4，跨越普通文字与加粗起点
        let h2 = inline_html_range(&content, 1, 4);
        assert!(h2.contains("乙"), "{h2}");
        assert!(h2.contains("<strong>粗体</strong>"), "{h2}");
    }

    #[test]
    fn escapes_html() {
        let h = to_html("T", "<script>alert(1)</script>", false);
        assert!(!h.contains("<script>"));
        assert!(h.contains("&lt;script&gt;"));
    }

    /// 代码组导出成可点击切换的 Tab 结构，两个语言各占一个面板。
    #[test]
    fn code_group_exports_tabs() {
        let src = r#####"````code-group
```java
int a = 1;
```
```python
a = 1
```
````"#####;
        let h = to_html("T", src, false);
        assert!(h.contains("code-group-tabs"), "{h}");
        assert!(h.contains("data-lang=\"java\""), "{h}");
        assert!(h.contains("data-lang=\"python\""), "{h}");
        assert!(
            h.contains("code-group-panel active"),
            "默认第一个面板可见：{h}"
        );
        assert!(h.contains("int a = 1;") && h.contains("a = 1"), "{h}");
    }

    /// 视频围栏导出成 B 站嵌入式播放器 iframe，16:9 容器由 CSS 保证。
    #[test]
    fn video_fence_exports_embed_player() {
        let src = "```video\nhttps://www.bilibili.com/bangumi/play/ep406221\n```";
        let h = to_html("T", src, false);
        assert!(h.contains("<div class=\"video-embed\">"), "{h}");
        assert!(h.contains("<iframe src="), "{h}");
        assert!(h.contains("player.bilibili.com/player.html"), "{h}");
        assert!(h.contains("ep_id=406221"), "{h}");
        // 属性值里的 & 必须转义，否则 HTML 无效
        assert!(h.contains("isOutside=true&amp;"), "{h}");
        assert!(!h.contains("<pre>"), "视频不该导出成代码块：{h}");
        assert!(h.contains("padding-top:56.25%"), "缺少 16:9 容器样式：{h}");
    }

    /// 只有 video/bilibili 标记的围栏才转 iframe：其它语言、认不出的链接都保持代码块。
    #[test]
    fn video_export_falls_back_to_code() {
        // 代码注释里贴 B 站链接是常事，绝不能被当成视频。
        let other = to_html(
            "T",
            "```rust\n// https://www.bilibili.com/video/BV1GJ411x7h7\nfn main(){}\n```",
            false,
        );
        assert!(other.contains("language-rust"), "{other}");
        assert!(!other.contains("<div class=\"video-embed\""), "{other}");
        assert!(!other.contains("<iframe"), "{other}");
        assert!(other.contains("BV1GJ411x7h7"), "{other}");
        let bad = to_html(
            "T",
            "```video\nhttps://example.com/not-bilibili\n```",
            false,
        );
        assert!(bad.contains("language-video"), "{bad}");
        assert!(bad.contains("example.com"), "{bad}");
        assert!(!bad.contains("<iframe"), "{bad}");
    }
}
