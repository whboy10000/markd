use super::*;

/// 行内公式原生形状必须画在占位字符的基线上，而不是行顶/行底。
#[test]
fn inline_math_image_aligns_to_text_baseline() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |_root| {});
    out.textures_delta.clear();
    let mut cache = ImageCache::new();
    let math = cache
        .math_shapes("x^2 + y^2", 16.0, MathDisplay::Inline)
        .expect("公式原生渲染失败");
    let marker = '\u{E000}';
    let galley = ctx.fonts_mut(|f| {
        f.layout_no_wrap(
            marker.to_string(),
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        )
    });
    let scale = 1.0;
    let size = math.size * scale;
    let baseline = math.baseline * scale;
    // 墨迹偏移与生产代码（MathBuild::push）一致：段内左右各 1px 呼吸位，占位段
    // 只有一个字符、没有 extra_letter_spacing，所以不必再扣首字符位移。
    let ink_dx = 1.0 - math.ink.min.x * scale;
    let run = InlineMathRun {
        marker,
        math: Some(math),
        size,
        baseline,
        ink_dx,
        scale,
        tall: false,
    };
    let origin = egui::pos2(100.0, 50.0);
    let shapes = math_image_shapes(&galley, origin, &[run], theme.text, ctx.pixels_per_point());
    assert!(!shapes.is_empty(), "公式形状没有落到占位字符上");

    // 计算所有形状的包围盒。
    let bounds = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Mesh(m) => Some(m.calc_bounds()),
            egui::Shape::Rect(r) => Some(r.rect),
            _ => None,
        })
        .fold(egui::Rect::NOTHING, |acc, r| acc.union(r));
    assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
    // 公式内部基线（距顶部 baseline）必须落在文字基线上。
    let text_baseline = origin.y + galley.rows[0].glyphs[0].pos.y;
    assert!(
        (bounds.top() - (text_baseline - baseline)).abs() < 0.5,
        "公式基线未对齐文字基线：top={}, baseline={text_baseline}",
        bounds.top()
    );
}

/// 回归：跨块选区高亮必须 ① 覆盖最后一个字（不能每行少一截）
/// ② 画在文字下层（否则蓝底盖住文字，看起来"字没了"）。
#[test]
fn cross_sel_highlight_covers_last_char_and_sits_behind_text() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let block = Block::Paragraph {
        content: vec![Inline::Text("abcde".to_string())],
        source: "abcde".to_string(),
    };
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let mut env = RenderEnv {
                    theme: &theme,
                    ctx: &c,
                    images: &mut images,
                    base_dir: None,
                    outline: &outline,
                    actions: &mut actions,
                    max_width: 700.0,
                    cross_sel: Some((0, 0, 0, 5)),
                    folded: &folded,
                    group_tab: 0,
                };
                let _ = render_block(ui, 0, &block, &mut env, false);
            });
        },
    );
    // 选区底色被塞进 Shape::Vec 里，先摊平再看
    fn flatten<'a>(s: &'a egui::Shape, out: &mut Vec<&'a egui::Shape>) {
        match s {
            egui::Shape::Vec(v) => {
                for x in v {
                    flatten(x, out);
                }
            }
            other => out.push(other),
        }
    }
    let mut flat: Vec<&egui::Shape> = Vec::new();
    for cs in &out.shapes {
        flatten(&cs.shape, &mut flat);
    }
    let mut sel: Option<(usize, egui::Rect)> = None;
    let mut txt: Option<(usize, egui::Rect)> = None;
    for (i, shape) in flat.iter().enumerate() {
        match shape {
            egui::Shape::Rect(r) if r.fill == theme.selection => sel = Some((i, r.rect)),
            egui::Shape::Text(t) if t.galley.text() == "abcde" => {
                txt = Some((i, egui::Rect::from_min_size(t.pos, t.galley.size())))
            }
            _ => {}
        }
    }
    let (sel_idx, sel_rect) = sel.expect("应画出选区底色");
    let (txt_idx, txt_rect) = txt.expect("应渲染文字");
    assert!(
        sel_rect.right() >= txt_rect.right() - 0.5,
        "选区右缘必须覆盖最后一个字：选区={sel_rect:?} 文字={txt_rect:?}"
    );
    assert!(
        sel_idx < txt_idx,
        "选区底色必须在文字之前绘制（垫在文字下层）：{sel_idx} vs {txt_idx}"
    );
    out.textures_delta.clear();
}

/// 双击选词边界：连续字母/数字/汉字算一个词，标点只选单字符。
#[test]
fn word_bounds_selects_word_or_char() {
    let t: Vec<char> = "hello 世界!".chars().collect();
    // "hello" = 0..5
    assert_eq!(word_bounds(&t, 2), (0, 5));
    // 空格（下标 5）只选它自己
    assert_eq!(word_bounds(&t, 5), (5, 6));
    // 连续汉字 "世界" = 6..8
    assert_eq!(word_bounds(&t, 6), (6, 8));
    assert_eq!(word_bounds(&t, 7), (6, 8));
    // 标点 "!" 只选单字符
    assert_eq!(word_bounds(&t, 8), (8, 9));
    // 空文本
    assert_eq!(word_bounds(&[], 0), (0, 0));
}

/// 单击链接文字的命中测试：渲染态按 AST 标签区间，编辑态按原始 Markdown。
#[test]
fn link_click_hit_test() {
    let inlines = crate::ast::parse_inline("看[这段](https://e.com)文字");
    assert_eq!(link_at(&inlines, 1).as_deref(), Some("https://e.com"));
    assert_eq!(link_at(&inlines, 3).as_deref(), Some("https://e.com"));
    assert_eq!(link_at(&inlines, 0), None);
    assert_eq!(link_at(&inlines, 4), None);

    let src = "看[这段](https://e.com)文字";
    assert_eq!(link_at_markdown(src, 2).as_deref(), Some("https://e.com"));
    assert_eq!(link_at_markdown(src, 4).as_deref(), Some("https://e.com"));
    assert_eq!(link_at_markdown(src, 0), None);
}

/// 编辑态排版必须保留原始字符，否则光标/字节偏移会和缓冲区错位。
#[test]
fn inline_job_keeps_raw_text() {
    let theme = Theme::get(ThemeKind::Light);
    let src = "**粗** *斜* `码` ~~删~~ [链](https://e.com) 普通文字";
    let job = inline_job(src, &theme, 16.0, theme.text, 400.0, None, None);
    assert_eq!(job.text, src, "排版后的文本必须与缓冲区完全一致");
}

/// 加粗文字的外观在「编辑态(inline_job)」与「渲染态(build_layout)」必须一致：
/// 都用 markd-bold 真粗体、都用普通正文色。否则鼠标离开加粗文字（退出编辑态）
/// 时会突然变色（强调色）且变细（wght 对静态字体不生效）。
#[test]
fn bold_looks_same_in_edit_and_render_modes() {
    use crate::ast::{Inline, parse_inline};
    let theme = Theme::get(ThemeKind::Light);
    let bold_family = egui::FontFamily::Name("markd-bold".into());

    // 编辑态：**粗** 的正文段（egui 0.37 LayoutSection 无 text 字段，按 byte_range 取）
    let edit = inline_job("**粗体**", &theme, 16.0, theme.text, 400.0, None, None);
    let edit_bold = edit
        .sections
        .iter()
        .find(|s| {
            let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
            edit.text.get(a..b) == Some("粗体")
        })
        .expect("应有加粗正文段");
    assert_eq!(edit_bold.format.font_id.family, bold_family);
    assert_eq!(edit_bold.format.color, theme.text);

    // 渲染态：解析出的 Inline::Strong
    let inlines = parse_inline("**粗体**");
    let render = build_layout(&inlines, &theme, 16.0, theme.text, false);
    let render_bold = render
        .sections
        .iter()
        .find(|s| {
            let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
            render.text.get(a..b) == Some("粗体")
        })
        .expect("应有加粗正文段");
    assert_eq!(
        render_bold.format.font_id.family, bold_family,
        "渲染态加粗字体应与编辑态一致（真粗体），实际 {:?}",
        render_bold.format.font_id.family
    );
    assert_eq!(
        render_bold.format.color, theme.text,
        "渲染态加粗颜色应与编辑态一致（不变色），实际 {:?}",
        render_bold.format.color
    );
}

/// 光标在加粗范围内时露出这一对 `**`（部分源码模式），方便看清哪段加粗、
/// 哪段没有；光标在加粗外仍隐藏。斜体/删除线保持“配对标记始终隐藏”。
#[test]
fn inline_job_reveals_paired_bold_markers_inside_span() {
    let theme = Theme::get(ThemeKind::Light);
    // "**粗**"：*0 *1 粗2 *3 *4；加粗 span = (0, 3)
    for cursor in [1, 2, 3] {
        let job = inline_job(
            "**粗**",
            &theme,
            16.0,
            theme.text,
            400.0,
            Some(cursor),
            None,
        );
        // 首尾 section 即开头/结尾的标记：显现时是正常 16px 字体，
        // 隐藏时是 0.1px 透明字体。
        let first = job.sections.first().expect("应有排版段");
        let last = job.sections.last().expect("应有排版段");
        assert!(
            first.format.font_id.size >= 15.0 && last.format.font_id.size >= 15.0,
            "cursor={cursor} 时加粗标记应显现，实际 {job:?}"
        );
        // 露出的标记用普通字重，不能跟着加粗。
        assert_eq!(
            first.format.font_id.family,
            FontFamily::Proportional,
            "露出的 ** 应是普通字重"
        );
    }

    // 光标在加粗范围之外（标记前/闭标记之后）→ 仍隐藏。
    for cursor in [0, 4, 5] {
        let job = inline_job(
            "**粗**",
            &theme,
            16.0,
            theme.text,
            400.0,
            Some(cursor),
            None,
        );
        let first = job.sections.first().expect("应有排版段");
        let last = job.sections.last().expect("应有排版段");
        assert!(
            first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
            "cursor={cursor} 时加粗标记应隐藏，实际 {job:?}"
        );
    }

    // 斜体/删除线的配对标记仍保持隐藏（只对加粗做源码露出）。
    for (src, cursor) in [("~~删~~", 2), ("*斜*", 1)] {
        let job = inline_job(src, &theme, 16.0, theme.text, 400.0, Some(cursor), None);
        let first = job.sections.first().expect("应有排版段");
        let last = job.sections.last().expect("应有排版段");
        assert!(
            first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
            "cursor={cursor} 时 [{src}] 的配对标记应保持隐藏，实际 {job:?}"
        );
    }
}

/// 行内代码例外：光标进入反引号内部时露出这一对反引号（源码模式），
/// 光标在代码外时仍然隐藏；字号 0.88、背景透明（圆角代码片单独绘制）。
#[test]
fn inline_job_reveals_code_ticks_inside_span() {
    let theme = Theme::get(ThemeKind::Light);
    let code_size = 16.0 * 0.88;

    // 光标在内容里（插入点在首反引号之后、尾反引号之前）→ 露出。
    for cursor in [1, 2] {
        let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(cursor), None);
        let first = job.sections.first().expect("应有排版段");
        let last = job.sections.last().expect("应有排版段");
        for f in [&first.format, &last.format] {
            assert!(
                (f.font_id.size - code_size).abs() < 0.01,
                "cursor={cursor} 时行内代码的反引号应显现且用代码字号，实际 {job:?}"
            );
            assert_eq!(
                f.background,
                egui::Color32::TRANSPARENT,
                "背景交给圆角代码片绘制"
            );
        }
    }

    // 光标在代码外 → 仍隐藏。
    for cursor in [0, 3] {
        let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(cursor), None);
        let first = job.sections.first().expect("应有排版段");
        assert!(
            first.format.font_id.size < 1.0,
            "cursor={cursor} 时代码外的反引号应隐藏"
        );
    }

    // 正文与反引号同字号，且文字使用行内代码配色。
    let job = inline_job("`码`", &theme, 16.0, theme.text, 400.0, Some(1), None);
    for section in &job.sections {
        assert!(
            (section.format.font_id.size - code_size).abs() < 0.01,
            "行内代码字号应为正文的 0.88：{}",
            section.format.font_id.size
        );
        assert_eq!(section.format.background, egui::Color32::TRANSPARENT);
        assert_eq!(section.format.color, theme.inline_code_fg());
    }
}

/// 光标落在链接源码范围内时露出 `[标签](地址)`（URL 用链接色），
/// 光标在链接外时仍隐藏标记、只显示带下划线的标签。
#[test]
fn inline_job_reveals_link_source_inside_span() {
    let theme = Theme::get(ThemeKind::Light);
    let src = "[点我](https://e.com)";

    // 光标在标签/地址里 → 露出完整源码，全部字符都是正常字号。
    for cursor in [1, 2, 5, 18] {
        let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(cursor), None);
        assert!(
            job.sections.iter().all(|s| s.format.font_id.size >= 15.0),
            "cursor={cursor} 时链接源码应显现: {job:?}"
        );
    }

    // 露出的 URL 用链接色，方便辨认和修改（标签文字仍是正文色）。
    let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(1), None);
    let url_section = job
        .sections
        .iter()
        .find(|s| s.format.color == theme.link && s.format.font_id.size >= 15.0)
        .expect("应能找到 URL 的排版段");
    assert_eq!(
        url_section.format.color, theme.link,
        "露出的 URL 应用链接色: {job:?}"
    );

    // 光标在链接外 → 方括号/圆括号仍隐藏，只剩标签文字。
    for cursor in [0, 19] {
        let job = inline_job(src, &theme, 16.0, theme.text, 600.0, Some(cursor), None);
        let first = job.sections.first().expect("应有排版段");
        let last = job.sections.last().expect("应有排版段");
        assert!(
            first.format.font_id.size < 1.0 && last.format.font_id.size < 1.0,
            "cursor={cursor} 时链接标记应隐藏: {job:?}"
        );
    }
}

/// 行内代码样式对齐方案 B / C：字号 0.88、品红/天蓝文字、透明背景
/// （圆角代码片由 inline_code_bg_shapes 单独绘制，选区蓝底自然盖在上面）。
#[test]
fn inline_code_uses_scheme_b_and_c() {
    for kind in [ThemeKind::Light, ThemeKind::Dark] {
        let theme = Theme::get(kind);
        for job in [
            inline_job("前 `码` 后", &theme, 16.0, theme.text, 400.0, None, None),
            build_layout(
                &crate::ast::parse_inline("前 `码` 后"),
                &theme,
                16.0,
                theme.text,
                false,
            ),
        ] {
            let chip = job
                .sections
                .iter()
                .find(|s| s.format.color == theme.inline_code_fg())
                .unwrap_or_else(|| panic!("{kind:?} 应有行内代码段：{job:?}"));
            assert_eq!(
                chip.format.background,
                egui::Color32::TRANSPARENT,
                "背景交给圆角代码片绘制，TextFormat 不再填底色"
            );
            assert_eq!(chip.format.font_id.family, egui::FontFamily::Monospace);
            assert!(
                (chip.format.font_id.size - 16.0 * 0.88).abs() < 0.01,
                "行内代码字号应为正文的 0.88：{}",
                chip.format.font_id.size
            );
        }
    }
    // 方案 B / C 的具体配色。
    let light = Theme::get(ThemeKind::Light);
    assert_eq!(
        light.inline_code_bg(),
        egui::Color32::from_rgb(239, 241, 245)
    );
    assert_eq!(
        light.inline_code_fg(),
        egui::Color32::from_rgb(214, 51, 108)
    );
    assert_eq!(
        light.inline_code_border(),
        Some(egui::Color32::from_rgb(226, 230, 238))
    );
    let dark = Theme::get(ThemeKind::Dark);
    assert_eq!(
        dark.inline_code_fg(),
        egui::Color32::from_rgb(125, 211, 252)
    );
    assert_eq!(dark.inline_code_border(), None, "方案 C 不描边");
}

/// 落单的标记（用户正在输入、尚未闭合）仍要显现，否则按键像没生效。
#[test]
fn inline_job_shows_lone_bold_marker() {
    let theme = Theme::get(ThemeKind::Light);
    let job = inline_job("**粗体", &theme, 16.0, theme.text, 400.0, Some(2), None);
    let first = job.sections.first().expect("应有排版段");
    assert!(
        first.format.font_id.size >= 15.0,
        "未闭合的 ** 应显现: {job:?}"
    );
}

/// 行内代码片：代码段要有圆角底色；浅色方案 B 额外加一圈 1px 描边，
/// 深色方案 C 只铺底色。
#[test]
fn inline_code_pills_match_scheme_b_and_c() {
    for (kind, expected) in [(ThemeKind::Dark, 1usize), (ThemeKind::Light, 2usize)] {
        let theme = Theme::get(kind);
        egui::__run_test_ui(|ui| {
            let job = build_layout(
                &crate::ast::parse_inline("前 `码` 后"),
                &theme,
                16.0,
                theme.text,
                false,
            );
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let shapes = inline_code_bg_shapes(&galley, Pos2::ZERO, &theme);
            assert_eq!(
                shapes.len(),
                expected,
                "{kind:?} 代码片形状数量不对（底色 + 可选描边）：{shapes:?}"
            );
            // 底色矩形应落在代码字符附近，而不是整行。
            match &shapes[0] {
                egui::Shape::Rect(r) => {
                    assert!(r.rect.width() > 0.0 && r.rect.width() < galley.size().x);
                    assert!(r.rect.left() > 0.0, "不应从行首开始：{:?}", r.rect);
                }
                other => panic!("代码片底色应为矩形：{other:?}"),
            }
        });
    }
}
