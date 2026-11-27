//! 内嵌 HTML（`<span style>` / `<div style>`）的渲染判据。
//!
//! 这几条是用户看得见的效果，必须钉住：字号颜色真落在字上、灰底框真按 padding
//! 撑开、点框能进编辑态。只测 `html_style` 的纯函数证明不了「画没画」，所以这里
//! 走真实的 `build_layout` / `render_block`，读 galley 的 TextFormat 与帧内形状。

use super::*;
use crate::ast::Block;
use crate::document::Document;
use crate::theme::Theme;

/// 装好中文字体、且已经跑过一帧的 Context：没跑过 `run` 的 Context 拿不到字体，
/// 直接排版会 panic；装字体后的第一帧带回字体图集 TexturesDelta，不清掉在调试档里会炸。
fn ctx_with_fonts() -> std::sync::Arc<egui::Context> {
    let ctx = std::sync::Arc::new(egui::Context::default());
    markd_platform::fonts::install(&ctx);
    let mut warm = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    warm.textures_delta.clear();
    drop(warm);
    ctx
}

/// 取段落的行内节点。
fn inlines_of(src: &str) -> Vec<Inline> {
    let doc = Document::new(src, None);
    match doc.blocks.first().expect("首块") {
        Block::Paragraph { content, .. } => content.clone(),
        other => panic!("用例需要段落，实际 {other:?}"),
    }
}

/// 找排版分段里正文等于 `text` 的那一段的样式。
fn format_of(job: &egui::text::LayoutJob, text: &str) -> egui::text::TextFormat {
    for s in &job.sections {
        let r = s.byte_range.start.0..s.byte_range.end.0;
        if &job.text[r.clone()] == text {
            return s.format.clone();
        }
    }
    panic!(
        "找不到分段 {text:?}，实际分段 {:?}",
        job.sections
            .iter()
            .map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect::<Vec<_>>()
    );
}

/// `<span style="color:red; font-size:18px">` 必须把颜色和字号真的排进 TextFormat，
/// 且只影响标签内的字——前后正文保持正文字号与正文色（否则会整段变色）。
#[test]
fn span_applies_color_and_size_only_to_its_own_text() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = ctx_with_fonts();
    let inlines = inlines_of("前<span style=\"color:red; font-size:18px\">红色大号</span>后");
    let job = crate::render::build_layout(&inlines, &theme, theme.body, theme.text, false);
    assert_eq!(job.text, "前红色大号后", "标签本身不进 galley");

    let hit = format_of(&job, "红色大号");
    assert_eq!(
        hit.color,
        egui::Color32::from_rgb(255, 0, 0),
        "span 颜色没生效"
    );
    assert_eq!(hit.font_id.size, 18.0, "span 字号没生效");
    for plain in ["前", "后"] {
        let out = format_of(&job, plain);
        assert_eq!(out.color, theme.text, "{plain} 不该被 span 染色");
        assert_eq!(out.font_id.size, theme.body, "{plain} 不该被 span 改字号");
    }

    // 真实排版证据：同一串字，带 span 的首行高度要严格大于全正文的对照。
    let with = ctx.fonts_mut(|f| f.layout_job(job));
    let plain_job = crate::render::build_layout(
        &[Inline::Text("前红色大号后".to_string())],
        &theme,
        theme.body,
        theme.text,
        false,
    );
    let without = ctx.fonts_mut(|f| f.layout_job(plain_job));
    assert!(
        with.rows[0].height() > without.rows[0].height(),
        "18px 段要把行高压起来：带 span={} 对照={}",
        with.rows[0].height(),
        without.rows[0].height()
    );
}

/// 没写 color / font-size 的 span 一律沿用正文样式（`font-weight` 这类暂不支持的
/// 属性不能把字变成乱色或 0 号）。认得的属性才生效。
#[test]
fn span_without_supported_style_inherits_body_text() {
    let theme = Theme::get(ThemeKind::Light);
    let inlines = inlines_of("<span style=\"font-weight:bold\">粗</span>");
    let job = crate::render::build_layout(&inlines, &theme, theme.body, theme.text, false);
    let s = format_of(&job, "粗");
    assert_eq!(s.color, theme.text);
    assert_eq!(s.font_id.size, theme.body);

    // 只认颜色，不认字号：字号仍是正文。单引号属性值也要认。
    let inlines = inlines_of("<span style='color:#00f'>蓝</span>");
    let job = crate::render::build_layout(&inlines, &theme, theme.body, theme.text, false);
    let s = format_of(&job, "蓝");
    assert_eq!(s.color, egui::Color32::from_rgb(0, 0, 255));
    assert_eq!(s.font_id.size, theme.body);
}

/// 部分源码模式（编辑态）里 HTML 标签必须原样看得见：`inline_job` 只把 `**`、
/// `` ` ``、`[..](..)` 这些 MD 标记渲染成 0.1px 透明字，`<span>` / `<div>` 没有隐藏
/// 规则，用户点开块就能直接改 style。对照用例（`**粗**` 会被藏）证明「没被藏」是真判据。
#[test]
fn edit_state_leaves_html_tags_visible() {
    let theme = Theme::get(ThemeKind::Light);
    let hidden = |text: &str| -> Vec<String> {
        let job =
            crate::render::inline_job(text, &theme, theme.body, theme.text, 700.0, None, None);
        assert_eq!(job.text, text, "编辑态缓冲区必须是原始 Markdown");
        job.sections
            .iter()
            .filter(|s| s.format.font_id.size < 1.0)
            .map(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].to_string())
            .collect()
    };
    assert_eq!(
        hidden("<span style=\"color:red\">红</span>"),
        Vec::<String>::new()
    );
    assert_eq!(
        hidden("<div style=\"background:#eee;\">灰</div>"),
        Vec::<String>::new()
    );
    assert!(
        !hidden("**粗**").is_empty(),
        "对照用例要能抓到被隐藏的标记，否则上面的断言是空转"
    );
}

/// span 标签不占 galley 字符位，链接命中区间必须按同一口径计数：
/// 若 `collect_link_ranges` 把 `<span …>` 的字符也算进去，点链接会整体错位、点不到。
#[test]
fn span_tags_do_not_shift_link_hit_ranges() {
    let inlines = inlines_of("<span style=\"color:red\">红字</span>[官网](https://e.com)尾");
    // 所见文本是「红字官网尾」：下标 2..4 落在「官网」上（含末字右边界）。
    assert_eq!(crate::ast::inline_plain(&inlines), "红字官网尾");
    for idx in [2usize, 3, 4] {
        assert_eq!(
            link_at(&inlines, idx).as_deref(),
            Some("https://e.com"),
            "点第 {idx} 个字应命中链接"
        );
    }
    assert_eq!(link_at(&inlines, 0), None, "span 正文不该被当成链接");
    assert_eq!(link_at(&inlines, 5), None, "链接之后的字不该命中");
}

type TextRects = Vec<(String, egui::Rect)>;
type FillRects = Vec<(egui::Color32, egui::Rect)>;

/// 渲染一帧，回传 (galley 正文, 文字矩形) 与不透明填充矩形；点击会往 `actions` 里塞 Action。
fn frame(
    ctx: &egui::Context,
    block: &Block,
    images: &mut ImageCache,
    actions: &mut Vec<Action>,
    click: Option<egui::Pos2>,
) -> (TextRects, FillRects) {
    let theme = Theme::get(ThemeKind::Light);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut events = Vec::new();
    if let Some(p) = click {
        for pressed in [true, false] {
            events.push(egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
        }
    }
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut env = RenderEnv {
                    theme: &theme,
                    ctx,
                    images,
                    base_dir: None,
                    outline: &outline,
                    actions,
                    max_width: 700.0,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                let _ = render_block(ui, 0, block, &mut env, false);
            });
        },
    );
    out.textures_delta.clear();
    let texts = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            // galley.rect 是相对 TextShape.pos 的，故换算成屏幕矩形要加 pos。
            egui::Shape::Text(ts) => Some((
                ts.galley.text().to_string(),
                egui::Rect::from_min_max(
                    ts.pos + ts.galley.rect.min.to_vec2(),
                    ts.pos + ts.galley.rect.max.to_vec2(),
                ),
            )),
            _ => None,
        })
        .collect();
    // 只收有底色的矩形：egui 的交互响应区、透明底色框不算「画了个盒子」。
    let rects = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Rect(r) if r.fill != egui::Color32::TRANSPARENT => Some((r.fill, r.rect)),
            _ => None,
        })
        .collect();
    (texts, rects)
}

/// 真正「垫在文字底下」的不透明矩形：排除 CentralPanel 自己那层整屏背景。
fn boxes_behind(
    rects: &[(egui::Color32, egui::Rect)],
    at: egui::Pos2,
) -> Vec<(egui::Color32, egui::Rect)> {
    rects
        .iter()
        .copied()
        .filter(|(_, r)| r.contains(at) && r.width() < 800.0)
        .collect()
}

fn div_block(src: &str) -> Block {
    let doc = Document::new(src, None);
    let b = doc.blocks.first().expect("首块");
    assert!(
        matches!(b, Block::Div { .. }),
        "用例需要 div 块，实际 {b:?}"
    );
    b.clone()
}

/// `<div style="background:#eee; padding:10px">` 要真的画出灰底框，四周比文字各多出
/// 10px 内边距，点框内文字要发出 Caret 进编辑态。
#[test]
fn div_paints_background_box_sized_by_padding_and_is_clickable() {
    let ctx = ctx_with_fonts();
    let block = div_block("<div style=\"background:#eee; padding:10px;\">灰色背景提示框</div>");
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let (texts, rects) = frame(&ctx, &block, &mut images, &mut actions, None);

    let text = texts
        .iter()
        .find(|(t, _)| t.contains("灰色背景提示框"))
        .map(|(_, r)| *r)
        .expect("框内要有正文");
    let gray = egui::Color32::from_rgb(0xee, 0xee, 0xee);
    let behind = boxes_behind(&rects, text.center());
    assert_eq!(
        behind.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        vec![gray],
        "文字底下要正好垫一层 #eee 框（多了就是凭空画框，少了就是没生效）"
    );
    let box_rect = behind[0].1;
    assert!(
        box_rect.contains(text.min) && box_rect.contains(text.max - egui::vec2(0.1, 0.1)),
        "文字要落在框内: box={box_rect:?} text={text:?}"
    );
    let insets = [
        text.min.y - box_rect.min.y,
        box_rect.max.y - text.max.y,
        text.min.x - box_rect.min.x,
        box_rect.max.x - text.max.x,
    ];
    for (side, v) in ["top", "bottom", "left", "right"].iter().zip(insets) {
        assert!(
            (v - 10.0).abs() < 0.6,
            "{side} 内边距应为 10px，实际 {v}: box={box_rect:?} text={text:?}"
        );
    }

    // 点击框内文字 → Caret(0, …)，编辑器据此进入部分源码模式。
    let mut click_actions: Vec<Action> = Vec::new();
    frame(
        &ctx,
        &block,
        &mut images,
        &mut click_actions,
        Some(text.center()),
    );
    assert!(
        click_actions
            .iter()
            .any(|a| matches!(a, Action::Caret(0, _))),
        "点击 div 应发出 Caret: {click_actions:?}"
    );
}

/// `<div>` 里套 `<span>`：两者都要生效——盒子按 div 的样式画，字按 span 的样式排。
#[test]
fn div_and_span_styles_stack() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = ctx_with_fonts();
    let block = div_block(
        "<div style=\"background:rgb(250,250,250);\"><span style=\"color:#ff00ff; font-size:22px\">粉字</span></div>",
    );
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let (texts, rects) = frame(&ctx, &block, &mut images, &mut actions, None);
    let text = texts
        .iter()
        .find(|(t, _)| t.contains("粉字"))
        .map(|(_, r)| *r)
        .expect("div 内文字要画出来");
    assert!(
        boxes_behind(&rects, text.center())
            .iter()
            .any(|(f, _)| *f == egui::Color32::from_rgb(250, 250, 250)),
        "div 底色要生效: {:?}",
        rects.iter().map(|(f, _)| *f).collect::<Vec<_>>()
    );

    let Block::Div { content, .. } = &block else {
        unreachable!()
    };
    let job = crate::render::build_layout(content, &theme, theme.body, theme.text, false);
    let s = format_of(&job, "粉字");
    assert_eq!(
        s.color,
        egui::Color32::from_rgb(255, 0, 255),
        "div 里的 span 颜色要生效"
    );
    assert_eq!(s.font_id.size, 22.0, "div 里的 span 字号要生效");
}

/// 没写 background 的 div 不凭空画盒子（免得用户以为插了个带底色的框），
/// 但 padding 仍然把文字推进去。
#[test]
fn div_without_background_paints_no_fill_but_still_insets_text() {
    let ctx = ctx_with_fonts();
    let block = div_block("<div style=\"padding:20px;\">只有边距</div>");
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let (texts, rects) = frame(&ctx, &block, &mut images, &mut actions, None);
    let text = texts
        .iter()
        .find(|(t, _)| t.contains("只有边距"))
        .map(|(_, r)| *r)
        .expect("正文要画出来");
    assert!(
        boxes_behind(&rects, text.center()).is_empty(),
        "没写 background 就不该在文字底下垫不透明底色: {:?}",
        rects.iter().map(|(f, _)| *f).collect::<Vec<_>>()
    );
    // 20px 内边距把文字从面板左边推出去（Frame 的 margin 生效）。
    assert!(
        text.min.x > 15.0,
        "padding 应把文字右推，实际 left={}",
        text.min.x
    );
}

/// 多行 div：正文整体要进同一个 galley，框也要跟着长高（跨行内容不能只渲染第一行）。
#[test]
fn div_keeps_every_line_in_its_galley() {
    let ctx = ctx_with_fonts();
    let style = "background:rgb(1,2,3); padding:6px;";
    let one = div_block(&format!("<div style=\"{style}\">\n第一行\n</div>"));
    let two = div_block(&format!("<div style=\"{style}\">\n第一行\n第二行\n</div>"));
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut body_of = |block: &Block| {
        let (texts, rects) = frame(&ctx, block, &mut images, &mut actions, None);
        let body = texts
            .iter()
            .find(|(t, _)| t.contains("第一行"))
            .expect("正文要画出来");
        let fill = rects
            .iter()
            .find(|(f, r)| *f == egui::Color32::from_rgb(1, 2, 3) && r.contains(body.1.center()))
            .expect("rgb() 底色要垫在文字底下");
        // 底色框要比文字每边多出 6px padding，故用框高比较两行与单行。
        (body.0.clone(), fill.1.height())
    };
    let (body_one, h_one) = body_of(&one);
    assert!(
        !body_one.contains("第二行"),
        "单行 div 不该有第二行: {body_one:?}"
    );
    let (body_two, h_two) = body_of(&two);
    for line in ["第一行", "第二行"] {
        assert!(body_two.contains(line), "跨行 div 缺 {line}: {body_two:?}");
    }
    assert!(
        h_two > h_one + 1.0,
        "两行的框要比单行高：单行={h_one} 两行={h_two}"
    );
}
