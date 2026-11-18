//! 行内 `LayoutJob` 构建：样式 span、选区高亮与公式美化。

use super::*;

/// 行内公式在 galley 里占用的位置信息。排版时先放一个透明占位字符，
/// 拿到 galley 后再把 RaTeX 原生形状画到占位字符的基线上。
pub(crate) struct InlineMathRun {
    pub(crate) marker: char,
    /// 原生显示列表形状（已按正文字号渲染）。
    pub(crate) math: Option<RenderedMath>,
    /// 缩放后的显示尺寸。
    pub(crate) size: egui::Vec2,
    /// 缩放后基线到公式顶部的距离。
    pub(crate) baseline: f32,
    /// 占位字符实际占用的宽度（用于水平居中）。
    pub(crate) reserved: f32,
    /// 应用的缩放比例。
    pub(crate) scale: f32,
}

/// 行内公式排版上下文：负责取原生形状、算显示尺寸并记录占位字符。
pub(crate) struct MathBuild<'a> {
    pub ctx: &'a egui::Context,
    pub images: &'a mut ImageCache,
    pub max_width: f32,
    pub runs: Vec<InlineMathRun>,
}

impl MathBuild<'_> {
    fn push(&mut self, tex: &str, job: &mut LayoutJob, theme: &Theme, size: f32) {
        // 行内公式用正文字号渲染，与文字基线对齐。
        let Some(math) = self.images.math_shapes(tex, size, MathDisplay::Inline) else {
            return;
        };
        // 行内公式不能高过行距，否则会压到上下行；宽公式也不能撑出版心。
        let max_h = (size * 1.45).max(18.0);
        let max_w = (self.max_width * 0.92).max(24.0);
        let scale = (max_h / math.size.y.max(1.0))
            .min(max_w / math.size.x.max(1.0))
            .min(1.0);
        let display = math.size * scale;
        let baseline = math.baseline * scale;

        let marker = char::from_u32(0xE000 + self.runs.len() as u32).unwrap_or('\u{FFFC}');
        let font_id = theme.fmt(size).font_id;
        let marker_w = self.ctx.fonts_mut(|f| f.glyph_width(&font_id, marker));
        let reserved = display.x.max(marker_w);
        let mut f = theme.fmt(size);
        f.color = Color32::TRANSPARENT;
        f.extra_letter_spacing = (reserved - marker_w).max(0.0);
        job.append(&marker.to_string(), 0.0, f);
        self.runs.push(InlineMathRun {
            marker,
            math: Some(math),
            size: display,
            baseline,
            reserved,
            scale,
        });
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_inline(
    job: &mut LayoutJob,
    inl: &Inline,
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    highlight: bool,
    linked: bool,
    mut math: Option<&mut MathBuild<'_>>,
) {
    match inl {
        Inline::Text(t) => {
            let mut f = if bold {
                theme.bold_fmt(size, color)
            } else {
                theme.fmt(size)
            };
            f.color = if linked { theme.link } else { color };
            f.italics = italic;
            if strike {
                f.strikethrough = Stroke::new(1.0, f.color);
            }
            if underline {
                f.underline = Stroke::new(1.0, f.color);
            }
            if highlight {
                f.background = theme.highlight_bg();
            }
            if linked {
                f.underline = Stroke::new(1.0, theme.link);
            }
            job.append(t, 0.0, f);
        }
        Inline::Strong(v) => {
            // 继承当前正文色，用 markd-bold 真粗体字族——与编辑态 inline_job 一致。
            // 之前换成 bold_color() 强调色 + wght 伪粗，导致鼠标离开加粗文字
            // （从编辑态切到渲染态）时突然变色、且变细。
            for x in v {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    true,
                    italic,
                    strike,
                    underline,
                    highlight,
                    linked,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Underline(v) => {
            for x in v {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    bold,
                    italic,
                    strike,
                    true,
                    highlight,
                    linked,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Emph(v) => {
            for x in v {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    bold,
                    true,
                    strike,
                    underline,
                    highlight,
                    linked,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Strike(v) => {
            for x in v {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    bold,
                    italic,
                    true,
                    underline,
                    highlight,
                    linked,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Highlight(v) => {
            for x in v {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    bold,
                    italic,
                    strike,
                    underline,
                    true,
                    linked,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Code(t) => {
            // 行内代码样式对齐方案 B / C：字号缩到 0.88，圆角代码片由
            // inline_code_bg_shapes 单独绘制（TextFormat 的背景画不了圆角）。
            job.append(t, 0.0, inline_code_format(theme, size));
        }
        Inline::Link { label, .. } => {
            for x in label {
                append_inline(
                    job,
                    x,
                    theme,
                    size,
                    color,
                    bold,
                    italic,
                    strike,
                    underline,
                    highlight,
                    true,
                    math.as_deref_mut(),
                );
            }
        }
        Inline::Image { alt, url, .. } => {
            let txt = if alt.is_empty() { url } else { alt };
            let mut f = theme.fmt(size * 0.88);
            f.color = theme.dim;
            job.append(&format!("[图片: {}]", txt), 0.0, f);
        }
        Inline::Math(t) => {
            if let Some(m) = math.as_mut() {
                m.push(t, job, theme, size);
            } else {
                let mut f = theme.mono_fmt();
                f.color = theme.heading;
                job.append(&math_pretty(t), 0.0, f);
            }
        }
        Inline::Wiki(name) => {
            let mut f = theme.fmt(size);
            f.color = theme.accent;
            job.append(&format!("[[{}]]", name), 0.0, f);
        }
        Inline::Footnote(id) => {
            let mut f = theme.fmt(size * 0.75);
            f.color = theme.accent;
            job.append(&format!("[{}]", id), 0.0, f);
        }
        Inline::Break => job.append("\n", 0.0, theme.fmt(size)),
    }
}

/// 行内代码的排版格式：字号 0.88、品红/天蓝文字、垂直居中对齐。
/// 背景不在这里画——TextFormat 的背景只能是直角矩形，圆角代码片
/// 由 [`inline_code_bg_shapes`] 在文字下层单独绘制。
pub fn inline_code_format(theme: &Theme, size: f32) -> TextFormat {
    TextFormat {
        font_id: FontId::new(size * 0.88, FontFamily::Monospace),
        color: theme.inline_code_fg(),
        background: Color32::TRANSPARENT,
        expand_bg: 0.0,
        valign: egui::Align::Center,
        ..Default::default()
    }
}

pub fn build_layout(
    inlines: &[Inline],
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    // 长 URL / 无空格的中文也要按宽度折断，避免溢出窗口被裁掉。
    job.wrap.break_anywhere = true;
    for i in inlines {
        append_inline(
            &mut job, i, theme, size, color, bold, false, false, false, false, false, None,
        );
    }
    if inlines.is_empty() {
        job.append(" ", 0.0, theme.fmt(size));
    }
    job
}

/// 与 [`build_layout`] 相同，但行内公式会以 RaTeX 图片占位并记录位置。
pub(crate) fn build_layout_with_math(
    inlines: &[Inline],
    theme: &Theme,
    size: f32,
    color: Color32,
    bold: bool,
    math: &mut MathBuild<'_>,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.break_anywhere = true;
    for i in inlines {
        append_inline(
            &mut job,
            i,
            theme,
            size,
            color,
            bold,
            false,
            false,
            false,
            false,
            false,
            Some(&mut *math),
        );
    }
    if inlines.is_empty() {
        job.append(" ", 0.0, theme.fmt(size));
    }
    job
}

pub fn math_pretty(tex: &str) -> String {
    let mut s = tex.to_string();
    const PAIRS: &[(&str, &str)] = &[
        ("\\times", "×"),
        ("\\cdot", "·"),
        ("\\int", "∫"),
        ("\\sum", "∑"),
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\pi", "π"),
        ("\\infty", "∞"),
        ("\\sqrt", "√"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\neq", "≠"),
        ("\\rightarrow", "→"),
        ("\\Rightarrow", "⇒"),
        ("\\in", "∈"),
        ("\\partial", "∂"),
        ("\\nabla", "∇"),
        ("\\approx", "≈"),
        ("\\pm", "±"),
        ("\\frac", ""),
        ("\\left", ""),
        ("\\right", ""),
        ("{", ""),
        ("}", ""),
        ("$$", ""),
    ];
    for (a, b) in PAIRS {
        s = s.replace(a, b);
    }
    s
}

/// 编辑态的行内排版：**缓冲区仍然是原始 Markdown**（保证光标/字节偏移正确），
/// 但把 `**`、`*`、`~~`、`` ` ``、`[label](url)` 这些标记用 0.1px 透明字渲染，
/// 于是看起来就是渲染后的富文本，而不是源码。
pub fn inline_job(
    text: &str,
    theme: &Theme,
    size: f32,
    color: Color32,
    wrap_width: f32,
    cursor: Option<usize>,
    // 当前文本选区（缓冲区字符下标）。行内代码的底色改由
    // inline_code_bg_shapes 画在文字下层，选区天然盖在上面，这里不再使用。
    _sel: Option<(usize, usize)>,
) -> LayoutJob {
    let marker = TextFormat {
        font_id: FontId::new(0.1, FontFamily::Proportional),
        color: Color32::TRANSPARENT,
        line_height: None,
        ..Default::default()
    };
    let bold_family = FontFamily::Name("markd-bold".into());

    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    job.wrap.break_anywhere = true;

    let chars: Vec<char> = text.chars().collect();

    // 预扫描行内标记 run：星号按「粗体 run(n>=2) / 斜体 run(n==1)」分组，
    // 波浪号、反引号各自一组。组内成对的标记隐藏渲染；落单的（手打的 *、
    // 敲了一半的 **、未闭合的反引号）按普通文字显示——否则打字时看不见刚
    // 敲的星号，像按键没生效。斜体/删除线的配对标记始终隐藏；加粗和行内代码
    // 在光标进入其内部时露出这一对标记（部分源码模式），方便看清哪些字加了
    // 格式、哪些没加、以及标记落点。
    let mut lone: Vec<usize> = Vec::new();
    // 所有反引号位置，用于判断光标是否落在某个行内代码内部。
    let mut tick: Vec<usize> = Vec::new();
    // 成对加粗 run 的起始下标（每两个一组：开标记、闭标记）。
    let bold_runs: Vec<usize>;
    {
        let mut ital: Vec<usize> = Vec::new();
        let mut bold: Vec<usize> = Vec::new();
        let mut tilde: Vec<usize> = Vec::new();
        let mut eq: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < chars.len() {
            match chars[i] {
                '*' => {
                    let n = chars[i..].iter().take_while(|&&x| x == '*').count();
                    if n == 1 {
                        ital.push(i);
                    } else {
                        bold.push(i);
                    }
                    i += n;
                }
                '~' if chars.get(i + 1) == Some(&'~') => {
                    tilde.push(i);
                    i += 2;
                }
                '=' if chars.get(i + 1) == Some(&'=') => {
                    eq.push(i);
                    i += 2;
                }
                '`' => {
                    tick.push(i);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        // 同组奇数个时，最后一个 run 没有配对对象（落单）
        let mark_lone = |v: &[usize], out: &mut Vec<usize>| {
            if v.len() % 2 == 1 {
                out.push(v[v.len() - 1]);
            }
        };
        mark_lone(&ital, &mut lone);
        mark_lone(&bold, &mut lone);
        mark_lone(&tilde, &mut lone);
        mark_lone(&eq, &mut lone);
        mark_lone(&tick, &mut lone);
        bold_runs = bold;
    }
    // 光标落在某对反引号之间（含两反引号中间的空代码）→ 露出这一对反引号。
    // 加粗同理：光标在 `**粗体**` 内部时露出这一对 `**`。
    let mut reveal_tick: Vec<usize> = Vec::new();
    let mut reveal_bold: Vec<usize> = Vec::new();
    if let Some(c) = cursor {
        for pair in tick.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            if a < c && c <= b {
                reveal_tick.push(a);
                reveal_tick.push(b);
            }
        }
        for pair in bold_runs.chunks_exact(2) {
            let (a, b) = (pair[0], pair[1]);
            if a < c && c <= b {
                reveal_bold.push(a);
                reveal_bold.push(b);
            }
        }
    }

    let fmt_now = |code: bool, italic: bool, bold: bool, strike: bool, highlight: bool| {
        if code {
            inline_code_format(theme, size)
        } else {
            TextFormat {
                font_id: FontId::new(
                    size,
                    if bold {
                        bold_family.clone()
                    } else {
                        FontFamily::Proportional
                    },
                ),
                color,
                italics: italic,
                background: if highlight {
                    theme.highlight_bg()
                } else {
                    Color32::TRANSPARENT
                },
                strikethrough: if strike {
                    Stroke::new(1.0, color)
                } else {
                    Stroke::NONE
                },
                line_height: None,
                ..Default::default()
            }
        }
    };

    let mut i = 0usize;
    let mut bold = false;
    let mut italic = false;
    let mut strike = false;
    let mut highlight = false;
    let mut code = false;

    fn push(job: &mut LayoutJob, c: char, f: &TextFormat) {
        let mut buf = [0u8; 4];
        job.append(c.encode_utf8(&mut buf), 0.0, f.clone());
    }

    // 判断下标 i 处是否是完整标签（如 "<strong>"）
    fn tag_at(chars: &[char], i: usize, tag: &str) -> bool {
        let t: Vec<char> = tag.chars().collect();
        i + t.len() <= chars.len() && chars[i..i + t.len()] == t[..]
    }

    while i < chars.len() {
        let c = chars[i];

        // <strong> / </strong>：与 ** 一致地隐藏标签、切换粗体；
        // 配不上对（孤立的开/闭标签）就按普通文字原样显示
        if c == '<' && tag_at(&chars, i, "<strong>") {
            let f = if bold {
                fmt_now(code, italic, bold, strike, highlight)
            } else {
                marker.clone()
            };
            for k in i..i + 8 {
                push(&mut job, chars[k], &f);
            }
            bold = !bold;
            i += 8;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</strong>") {
            let f = if bold {
                marker.clone()
            } else {
                fmt_now(code, italic, bold, strike, highlight)
            };
            for k in i..i + 9 {
                push(&mut job, chars[k], &f);
            }
            bold = !bold;
            i += 9;
            continue;
        }
        // <em> / </em>：与 * 一致地隐藏标签、切换斜体；
        // 配不上对（孤立的开/闭标签）就按普通文字原样显示
        if c == '<' && tag_at(&chars, i, "<em>") {
            let f = if italic {
                fmt_now(code, italic, bold, strike, highlight)
            } else {
                marker.clone()
            };
            for k in i..i + 4 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 4;
            continue;
        }
        if c == '<' && tag_at(&chars, i, "</em>") {
            let f = if italic {
                marker.clone()
            } else {
                fmt_now(code, italic, bold, strike, highlight)
            };
            for k in i..i + 5 {
                push(&mut job, chars[k], &f);
            }
            italic = !italic;
            i += 5;
            continue;
        }
        if c == '`' {
            let f = if lone.contains(&i) {
                fmt_now(code, italic, bold, strike, highlight)
            } else if reveal_tick.contains(&i) {
                // 光标在代码内：这一对反引号按代码样式显示，和正文连成一片，
                // 用户可以直接删改这段源码。
                fmt_now(true, italic, bold, strike, highlight)
            } else {
                marker.clone()
            };
            push(&mut job, c, &f);
            code = !code;
            i += 1;
            continue;
        }
        if c == '*' {
            let n = chars[i..].iter().take_while(|&&x| x == '*').count();
            let f = if lone.contains(&i) {
                fmt_now(code, italic, bold, strike, highlight)
            } else if reveal_bold.contains(&i) {
                // 光标在加粗范围内：露出这一对 **（普通字重，避免闭合标记
                // 跟着变粗、看起来像多敲了一个星号）。
                fmt_now(code, italic, false, strike, highlight)
            } else {
                marker.clone()
            };
            for _ in 0..n {
                push(&mut job, '*', &f);
            }
            if n >= 2 {
                bold = !bold;
            } else {
                italic = !italic;
            }
            i += n;
            continue;
        }
        if c == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            let visible = lone.contains(&i);
            let f = if visible {
                fmt_now(code, italic, bold, strike, highlight)
            } else {
                marker.clone()
            };
            push(&mut job, '~', &f);
            push(&mut job, '~', &f);
            strike = !strike;
            i += 2;
            continue;
        }
        if c == '=' && i + 1 < chars.len() && chars[i + 1] == '=' {
            let visible = lone.contains(&i);
            let f = if visible {
                fmt_now(code, italic, bold, strike, highlight)
            } else {
                marker.clone()
            };
            push(&mut job, '=', &f);
            push(&mut job, '=', &f);
            highlight = !highlight;
            i += 2;
            continue;
        }
        if c == '[' {
            let close = chars[i..].iter().position(|ch| *ch == ']').map(|k| i + k);
            if let Some(close) = close {
                if close + 1 < chars.len() && chars[close + 1] == '(' {
                    if let Some(rel) = chars[close + 1..].iter().position(|ch| *ch == ')') {
                        let end = close + 1 + rel;
                        let link_fmt = TextFormat {
                            font_id: FontId::new(size, FontFamily::Proportional),
                            color: theme.link,
                            underline: Stroke::new(1.0, theme.link),
                            line_height: None,
                            ..Default::default()
                        };
                        // 光标在这个链接的源码范围内（标签或地址）→ 露出
                        // `[标签](地址)`，进入可编辑的源码模式。
                        let revealed = cursor.is_some_and(|c| i < c && c <= end);
                        if revealed {
                            let plain = fmt_now(false, italic, bold, strike, highlight);
                            let url_fmt = TextFormat {
                                font_id: FontId::new(size, FontFamily::Proportional),
                                color: theme.link,
                                line_height: None,
                                ..Default::default()
                            };
                            push(&mut job, chars[i], &plain); // '['
                            for k in i + 1..close {
                                push(&mut job, chars[k], &plain);
                            }
                            push(&mut job, chars[close], &plain); // ']'
                            push(&mut job, chars[close + 1], &plain); // '('
                            for k in close + 2..end {
                                push(&mut job, chars[k], &url_fmt);
                            }
                            push(&mut job, chars[end], &plain); // ')'
                        } else {
                            push(&mut job, '[', &marker);
                            for k in i + 1..close {
                                push(&mut job, chars[k], &link_fmt);
                            }
                            for k in close..=end {
                                push(&mut job, chars[k], &marker);
                            }
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }

        let fmt = fmt_now(code, italic, bold, strike, highlight);
        push(&mut job, c, &fmt);
        i += 1;
    }
    job
}
