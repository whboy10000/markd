//! HTML 内嵌标签的样式读取：把 `<span style="color:red; font-size:18px">`、
//! `<div style="background:#eee; padding:10px">` 里的样式写成渲染层能用的数值。
//!
//! 内嵌 HTML 不引入自定义语法：文档里存的就是原生标签，其它 Markdown 渲染器照常识别，
//! 本编辑器只负责在预览态把这些样式「画出来」。全部是纯函数（渲染每帧都要跑），
//! 颜色与字号的换算写成显式规则并逐例定值，不做运行时猜测。

/// 一条标签上读到的样式。每一项都是「解析成功才有值」，缺省即沿用主题默认。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CssStyle {
    /// 文字颜色。
    pub color: Option<[u8; 3]>,
    /// 字号（像素）。
    pub font_size_px: Option<f32>,
    /// 背景色。
    pub background: Option<[u8; 3]>,
    /// 内边距（像素）。
    pub padding_px: Option<f32>,
}

/// 从开始标签原文里取属性值：`<span style="color:red">` 取 `style` 得 `color:red`。
///
/// 属性名必须整词相等，`<span styles="x">` 不能被当成 `style`、`class="x"` 也不能命中；
/// 双引号、单引号、不带引号三种写法都认（不带引号时到空格或 `>` 为止）。
pub fn tag_attr(tag: &str, name: &str) -> Option<String> {
    let b = tag.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        // 属性名起点：跳过空白与尖括号、自闭合斜杠。
        while i < b.len() && (b[i].is_ascii_whitespace() || matches!(b[i], b'<' | b'>' | b'/')) {
            i += 1;
        }
        let start = i;
        while i < b.len()
            && !b[i].is_ascii_whitespace()
            && !matches!(b[i], b'=' | b'>' | b'<' | b'"' | b'\'')
        {
            i += 1;
        }
        if i == start {
            break;
        }
        let key = &tag[start..i];
        let mut eq = i;
        while eq < b.len() && b[eq].is_ascii_whitespace() {
            eq += 1;
        }
        if b.get(eq) != Some(&b'=') {
            if key.eq_ignore_ascii_case(name) {
                return Some(String::new());
            }
            continue;
        }
        let (value, next) = read_attr_value(tag, eq + 1)?;
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
        // 名字不是我们要的：整段属性值原样跳过，值里的空格不会被误当成属性分隔。
        i = next;
    }
    None
}

/// 取 `=` 之后的属性值（双引号、单引号、裸值三种），返回 (值, 值之后的下标)。
fn read_attr_value(tag: &str, from: usize) -> Option<(String, usize)> {
    let b = tag.as_bytes();
    let mut i = from;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    match b.get(i) {
        Some(&q @ (b'"' | b'\'')) => {
            // 引号没闭合就整体不认，别交出半截值。
            let close = (i + 1..b.len()).find(|&k| b[k] == q)?;
            Some((tag[i + 1..close].to_string(), close + 1))
        }
        _ => {
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                i += 1;
            }
            Some((tag[start..i].to_string(), i))
        }
    }
}

/// 从一条 `style` 声明里取某个属性的值：`background-color: #eee; padding: 10px` 取 `padding`。
///
/// 属性名整词相等才算命中，`background-color` 不会被 `color` 读到；
/// 值里的 `!important` 会被剥掉。后出现的同名属性覆盖先出现的（与浏览器一致）。
pub fn style_value(style: &str, prop: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for decl in style.split(';') {
        let Some((key, value)) = decl.split_once(':') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case(prop) {
            found = Some(strip_important(value.trim()).to_string());
        }
    }
    found
}

fn strip_important(value: &str) -> &str {
    match value.rfind('!') {
        Some(p) if value[p..].trim().eq_ignore_ascii_case("!important") => value[..p].trim_end(),
        _ => value,
    }
}

/// 读一条标签上的样式：`<span style="color:red; font-size:18px">` → 颜色 + 字号。
/// `em_base` 是换算 `em` / `rem` 时使用的基准字号（像素）。
pub fn style_of(tag: &str, em_base: f32) -> CssStyle {
    let Some(style) = tag_attr(tag, "style") else {
        return CssStyle::default();
    };
    CssStyle {
        color: style_value(&style, "color").and_then(|v| css_color(&v)),
        background: style_value(&style, "background")
            .or_else(|| style_value(&style, "background-color"))
            .and_then(|v| css_color(&v)),
        font_size_px: style_value(&style, "font-size").and_then(|v| css_len(&v, em_base)),
        padding_px: style_value(&style, "padding").and_then(|v| css_len(&v, em_base)),
    }
}

/// CSS 颜色 → RGB：支持 `#rgb`、`#rrggbb`、`rgb()` / `rgba()` 与常用颜色名。
/// 带通道的百分号写法（`rgb(100% 0 0)`）与 HSL 不支持，返回 `None` 由调用方退回默认色。
pub fn css_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(hex) = t.strip_prefix('#') {
        return hex_color(hex);
    }
    let lower = t.to_ascii_lowercase();
    for (func, alpha) in [("rgb(", false), ("rgba(", true)] {
        if let Some(rest) = lower.strip_prefix(func) {
            let inner = rest.strip_suffix(')')?;
            let parts: Vec<&str> = inner.split([',', ' ']).filter(|p| !p.is_empty()).collect();
            if parts.len() < 3 {
                return None;
            }
            let rgb = [channel(parts[0])?, channel(parts[1])?, channel(parts[2])?];
            // 带透明度通道的写法按不透明处理会骗人（egui 里要另走一层遮罩），直接不认。
            if alpha && parts.get(3).and_then(|a| a.parse::<f32>().ok()) != Some(1.0) {
                return None;
            }
            return Some(rgb);
        }
    }
    NAMED_COLORS
        .iter()
        .find(|(name, _)| *name == lower.as_str())
        .map(|(_, rgb)| *rgb)
}

/// `rgb()` 的分量：整数或百分号，四舍五入到 0..=255。
fn channel(s: &str) -> Option<u8> {
    if let Some(p) = s.strip_suffix('%') {
        let v: f32 = p.trim().parse().ok()?;
        return Some((v.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8);
    }
    let v: f32 = s.trim().parse().ok()?;
    Some(v.clamp(0.0, 255.0).round() as u8)
}

fn hex_color(hex: &str) -> Option<[u8; 3]> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |c: char| u8::from_str_radix(&c.to_string(), 16).ok();
    match hex.len() {
        // 3/4 位是 #rgb / #rgba：每一位翻倍展开（#f0f → #ff00ff），alpha 位丢弃。
        3 | 4 => {
            let h: Vec<char> = hex.chars().take(3).collect();
            Some([nib(h[0])? * 17, nib(h[1])? * 17, nib(h[2])? * 17])
        }
        // 6/8 位是 #rrggbb / #rrggbbaa：这里不画半透明，取前三个通道。
        6 | 8 => Some([
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ]),
        _ => None,
    }
}

/// 常用 CSS 颜色名（小写）。命名色只列日常会写到的这批，其余按不识别处理。
const NAMED_COLORS: [(&str, [u8; 3]); 40] = [
    ("black", [0, 0, 0]),
    ("white", [255, 255, 255]),
    ("red", [255, 0, 0]),
    ("green", [0, 128, 0]),
    ("blue", [0, 0, 255]),
    ("yellow", [255, 255, 0]),
    ("cyan", [0, 255, 255]),
    ("aqua", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("fuchsia", [255, 0, 255]),
    ("gray", [128, 128, 128]),
    ("grey", [128, 128, 128]),
    ("silver", [192, 192, 192]),
    ("maroon", [128, 0, 0]),
    ("navy", [0, 0, 128]),
    ("teal", [0, 128, 128]),
    ("olive", [128, 128, 0]),
    ("lime", [0, 255, 0]),
    ("orange", [255, 165, 0]),
    ("purple", [128, 0, 128]),
    ("brown", [165, 42, 42]),
    ("pink", [255, 192, 203]),
    ("gold", [255, 215, 0]),
    ("indigo", [75, 0, 130]),
    ("violet", [238, 130, 238]),
    ("turquoise", [64, 224, 208]),
    ("salmon", [250, 128, 114]),
    ("khaki", [240, 230, 140]),
    ("beige", [245, 245, 220]),
    ("crimson", [220, 20, 60]),
    ("coral", [255, 127, 80]),
    ("orchid", [218, 112, 214]),
    ("tomato", [255, 99, 71]),
    ("skyblue", [135, 206, 235]),
    ("royalblue", [65, 105, 225]),
    ("darkred", [139, 0, 0]),
    ("darkblue", [0, 0, 139]),
    ("darkgray", [169, 169, 169]),
    ("darkgrey", [169, 169, 169]),
    ("lightgray", [211, 211, 211]),
];

/// CSS 长度 → 像素：`px` 原样、`em` / `rem` 按基准字号、`pt` 按 96/72 换算，
/// 纯数字（含 `0`）按像素。其余单位（`%`、`vw`、`calc()`）返回 `None`。
pub fn css_len(s: &str, em_base: f32) -> Option<f32> {
    let (num, unit) = split_len(s.trim())?;
    let v: f32 = num.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    match unit.to_ascii_lowercase().as_str() {
        "" | "px" => Some(v),
        "em" | "rem" => Some(v * em_base),
        "pt" => Some(v * 96.0 / 72.0),
        _ => None,
    }
}

/// 把 `18px` 拆成 `("18", "px")`；纯数字 `0` 拆成 `("0", "")`。
/// 数值部分只吃符号、数字与小数点——不支持科学计数法，
/// 否则 `1.5em` 的 `e` 会被当成指数，单位就丢了。
fn split_len(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let mut i = 0usize;
    if i < b.len() && matches!(b[i], b'-' | b'+') {
        i += 1;
    }
    let digits = i;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if i == digits {
        return None;
    }
    Some((&s[..i], &s[i..]))
}

/// 片段里是否出现本编辑器认识的 HTML 标签：`<span>` `<div>` `<br>` `<hr>`（含闭合写法）。
///
/// 口径与解析器一致——解析器只把这四种标签变成样式/断行/分隔线，`<table>`、`<p>`
/// 之类插进文档只会变成一堆字面文字，所以插入弹窗宁可拒绝也不落进正文。
/// 标签名必须整词结束（`<spanx>` 不算），大小写都认。
pub fn looks_like_html(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if b[j] == b'/' {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        let name = &src[start..j];
        let ends_tag = matches!(b.get(j), Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\t'));
        if !name.is_empty()
            && ends_tag
            && ["span", "div", "br", "hr"]
                .iter()
                .any(|supported| name.eq_ignore_ascii_case(supported))
        {
            return true;
        }
        i = j.max(i + 1);
    }
    false
}

/// HTML 内嵌片段的落点形态：`html_drop` 的判定结果，插入弹窗按它决定走哪条路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlDrop {
    /// 单行、纯行内的片段（`<span …>` / `<br>`）：插到光标处，和加粗同一套口径。
    Inline,
    /// 块级片段（`<div …>` / `<hr>` / 跨行）：整块插入，不塞进当前段落。
    Block,
}

use crate::ast::{Block, Inline};

/// 行内节点里是否真的出现了 HTML 标签（`span` 或断行），嵌套一层也算。
fn inlines_have_html(items: &[Inline]) -> bool {
    items.iter().any(|i| match i {
        Inline::Break | Inline::Span { .. } => true,
        Inline::Strong(content)
        | Inline::Emph(content)
        | Inline::Strike(content)
        | Inline::Underline(content)
        | Inline::Highlight(content) => inlines_have_html(content),
        Inline::Link { label, .. } => inlines_have_html(label),
        _ => false,
    })
}

/// 块里是否出现 HTML 节点。`<hr>` 与 `---` 归一成同一个 `Block::Rule`，所以整块算数。
fn block_has_html(b: &Block) -> bool {
    match b {
        // div 块本身就是 HTML 标签，块级判定看得到；hr 与 `---` 归一成同一个 Rule。
        Block::Div { .. } | Block::Rule { .. } => true,
        Block::Heading { content, .. }
        | Block::Paragraph { content, .. }
        | Block::Quote { content, .. }
        | Block::Footnote { content, .. } => inlines_have_html(content),
        Block::ListItem { item, .. } => inlines_have_html(&item.content),
        _ => false,
    }
}

/// 判定一段用户输入的 HTML 片段该怎么落文档；`None` = 该拒绝。
///
/// 两处把关：标签得是认识的（`looks_like_html`），且解析完真的成了 HTML 节点——
/// 不成对、拼错的标签解析后只剩一堆字面文字，插进正文只会得到一行 `<span` 源码。
/// 单行且整段就是行内内容才算 `Inline`，其余（div、hr、跨行）走 `Block`。
pub fn html_drop(src: &str) -> Option<HtmlDrop> {
    if !looks_like_html(src) {
        return None;
    }
    let blocks = crate::ast::parse(src);
    if !blocks.iter().any(block_has_html) {
        return None;
    }
    let one_line = match blocks.as_slice() {
        [Block::Paragraph { content, .. }] => !content.is_empty() && !src.contains('\n'),
        _ => false,
    };
    Some(if one_line {
        HtmlDrop::Inline
    } else {
        HtmlDrop::Block
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_drop_routes_inline_and_block() {
        // 行内：单段 span / 裸 br / 混在文字里
        assert_eq!(
            html_drop(r#"<span style="color:red">红</span>"#),
            Some(HtmlDrop::Inline)
        );
        assert_eq!(html_drop("<br>"), Some(HtmlDrop::Inline));
        assert_eq!(
            html_drop(r#"<SPAN style="color:red">红</SPAN>"#),
            Some(HtmlDrop::Inline)
        );
        assert_eq!(
            html_drop(r#"前<span style="color:red">中</span>后"#),
            Some(HtmlDrop::Inline)
        );
        // 块级：div 框、分隔线、跨行片段
        assert_eq!(
            html_drop(r#"<div style="background:#eee; padding:10px;">提示框</div>"#),
            Some(HtmlDrop::Block)
        );
        assert_eq!(html_drop("<hr>"), Some(HtmlDrop::Block));
        assert_eq!(html_drop("<hr />"), Some(HtmlDrop::Block));
        assert_eq!(
            html_drop("<span style=\"color:red\">甲</span>\n\n<hr>"),
            Some(HtmlDrop::Block)
        );
        assert_eq!(
            html_drop("<span style=\"color:red\">甲\n乙</span>"),
            Some(HtmlDrop::Block),
            "跨行的片段整块插，不能把换行塞进当前段落"
        );
        // 句中混写的 `<hr>` 解析器仍当文字（与块解析同口径），但同一行的 `<br>` 有效，
        // 所以这条算行内片段：落的只是解析器真正认下的那部分。空 span 同样成对、合法。
        assert_eq!(html_drop("<hr/><br>"), Some(HtmlDrop::Inline));
        assert_eq!(html_drop("<span></span>"), Some(HtmlDrop::Inline));
        // 拒绝：没有标签、标签不认识、标签没成对、只有普通 Markdown
        for bad in [
            "just words",
            "",
            "---",
            "**粗**",
            "<table><tr><td>1</td></tr></table>",
            "<p>段落</p>",
            r#"<span style="color:red">没闭合"#,
            "a < 1 and b > 2",
        ] {
            assert_eq!(html_drop(bad), None, "{bad} 不该被接受");
        }
    }

    #[test]
    fn looks_like_html_only_for_supported_tags() {
        for ok in [
            "<span style=\"color:red\">红</span>",
            "<DIV style=\"background:#eee\">灰</div>",
            "<SPAN style=\"color:red\">红</SPAN>",
            "<BR>",
            "<HR>",
            "<br>",
            "<br/>",
            "<hr>",
            "第一段\n\n<hr>\n",
            "前<span>中</span>后",
        ] {
            assert!(looks_like_html(ok), "{ok} 应判定为可插入的 HTML");
        }
        for no in [
            "just words",
            "",
            "**粗**",
            "[官网](https://e.com)",
            "<table><tr><td>1</td></tr></table>",
            "<p>段落</p>",
            "<img src=\"a.png\">",
            "<spanx>a</spanx>",
            "a < 1 and b > 2",
            "<spanx style=\"color:red\">",
            // 只有开始、没有 > 的半截标签也不算可插入片段
            "<span",
            "<div",
            "<br",
            "<hr",
        ] {
            assert!(!looks_like_html(no), "{no} 不该判定为可插入的 HTML");
        }
    }

    #[test]
    fn tag_attr_reads_style() {
        assert_eq!(
            tag_attr(r#"<span style="color:red">"#, "style").as_deref(),
            Some("color:red")
        );
        assert_eq!(
            tag_attr("<span style='color:red'>", "style").as_deref(),
            Some("color:red")
        );
        assert_eq!(
            tag_attr("<span style=color:red>", "style").as_deref(),
            Some("color:red")
        );
        // 属性名整词相等：复数形式、别的属性都不能命中。
        assert_eq!(tag_attr(r#"<span styles="x">"#, "style"), None);
        assert_eq!(tag_attr(r#"<span class="note">"#, "style"), None);
        assert_eq!(tag_attr("<span>", "style"), None);
        // 多个属性：值里带空格（字体名）也不能截断。
        assert_eq!(
            tag_attr(
                r#"<span class="a" style="font-family: 'Song Ti', serif" data-x="1">"#,
                "style"
            )
            .as_deref(),
            Some("font-family: 'Song Ti', serif")
        );
        assert_eq!(
            tag_attr(r#"<span data-x="1" STYLE="color:red">"#, "style").as_deref(),
            Some("color:red")
        );
    }

    #[test]
    fn style_value_is_exact_token() {
        let s = "background-color:#eee; padding:10px";
        assert_eq!(style_value(s, "padding").as_deref(), Some("10px"));
        assert_eq!(style_value(s, "background-color").as_deref(), Some("#eee"));
        // background-color 不等于 color，否则灰底会被当成灰字。
        assert_eq!(style_value(s, "color"), None);
        assert_eq!(style_value("COLOR : Red ", "color").as_deref(), Some("Red"));
        assert_eq!(
            style_value("color:red !important", "color").as_deref(),
            Some("red")
        );
        // 同名属性后者覆盖前者（浏览器口径）。
        assert_eq!(
            style_value("color:red;color:blue", "color").as_deref(),
            Some("blue")
        );
        // 没有冒号的残段、空声明都跳过。
        assert_eq!(style_value("color", "color"), None);
        assert_eq!(style_value("", "color"), None);
    }

    #[test]
    fn css_color_forms() {
        assert_eq!(css_color("#fff"), Some([255, 255, 255]));
        assert_eq!(css_color("#FFF"), Some([255, 255, 255]));
        assert_eq!(css_color("#abcdef"), Some([171, 205, 239]));
        assert_eq!(css_color("#12345678"), Some([18, 52, 86]));
        assert_eq!(css_color("#eee "), Some([238, 238, 238]));
        assert_eq!(css_color("red"), Some([255, 0, 0]));
        assert_eq!(css_color(" LightGray "), Some([211, 211, 211]));
        assert_eq!(css_color("rgb(1, 2, 3)"), Some([1, 2, 3]));
        assert_eq!(css_color("rgb(1 2 3)"), Some([1, 2, 3]));
        assert_eq!(css_color("rgba(1,2,3,1)"), Some([1, 2, 3]));
        assert_eq!(css_color("rgb(100%,50%,0%)"), Some([255, 128, 0]));
        assert_eq!(css_color("#ab"), None);
        assert_eq!(css_color("#gghhii"), None);
        assert_eq!(css_color("notacolor"), None);
        assert_eq!(css_color(""), None);
        assert_eq!(css_color("hsl(0,100%,50%)"), None);
        assert_eq!(css_color("rgba(1,2,3,0.5)"), None, "半透明不当实色用");
    }

    #[test]
    fn css_len_units() {
        assert_eq!(css_len("18px", 16.0), Some(18.0));
        assert_eq!(css_len(" 18px ", 16.0), Some(18.0));
        assert_eq!(css_len("1.5em", 16.0), Some(24.0));
        assert_eq!(css_len("2rem", 16.0), Some(32.0));
        assert_eq!(css_len("12pt", 16.0), Some(16.0));
        assert_eq!(css_len("0", 16.0), Some(0.0));
        assert_eq!(css_len("20PX", 16.0), Some(20.0));
        assert_eq!(css_len("50%", 16.0), None);
        assert_eq!(css_len("-1px", 16.0), None, "负字号或负内边距会破坏排版");
        // 指数写法会被读成「数字 + 未知单位」，宁可当不识别，也不要把 em 单位吃掉。
        assert_eq!(css_len("1e2px", 16.0), None);
        assert_eq!(css_len("abc", 16.0), None);
        assert_eq!(css_len("px", 16.0), None);
        assert_eq!(css_len("", 16.0), None);
    }

    #[test]
    fn style_of_composes() {
        let s = style_of(r#"<span style="color:red; font-size:18px">"#, 16.0);
        assert_eq!(s.color, Some([255, 0, 0]));
        assert_eq!(s.font_size_px, Some(18.0));
        assert_eq!(s.background, None);
        let d = style_of(r#"<div style="background:#eee; padding:10px">"#, 16.0);
        assert_eq!(d.background, Some([238, 238, 238]));
        assert_eq!(d.padding_px, Some(10.0));
        // background-color 也当背景读。
        assert_eq!(
            style_of(r#"<div style="background-color:red">"#, 16.0).background,
            Some([255, 0, 0])
        );
        assert_eq!(style_of("<div>", 16.0), CssStyle::default());
    }
}
