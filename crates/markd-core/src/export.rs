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

fn inline_html(v: &[Inline]) -> String {
    let mut s = String::new();
    for i in v {
        match i {
            Inline::Text(t) => s.push_str(&esc(t)),
            Inline::Strong(x) => s.push_str(&format!("<strong>{}</strong>", inline_html(x))),
            Inline::Emph(x) => s.push_str(&format!("<em>{}</em>", inline_html(x))),
            Inline::Strike(x) => s.push_str(&format!("<del>{}</del>", inline_html(x))),
            Inline::Underline(x) => s.push_str(&format!("<u>{}</u>", inline_html(x))),
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
                out.push_str(&format!(
                    "<pre><code class=\"language-{}\">{}</code></pre>\n",
                    esc(lang),
                    esc(code)
                ));
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
}
