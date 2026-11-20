//! 引用块左侧竖线：渲染态与被点进编辑态时必须画在同一个地方。
//!
//! 两态由不同代码画：渲染态在 `render::block`，编辑态在 `editor::view`。
//! 基准必须一致（都用文字 galley 的矩形，不用 `TextEdit` 的响应矩形），
//! 否则点一下引用，竖线就横移/长高一截，用户看到的是「边框跳了一下」。
//!
//! 这里装真实字体（`markd_platform::fonts::install`）：回退字体的行高差 3px，
//! 量不出用户实际看到的坐标。

use super::*;

struct Q {
    doc: Document,
    state: EditorState,
    theme: Theme,
    images: ImageCache,
    actions: Vec<crate::render::Action>,
    ctx: egui::Context,
}

impl Q {
    fn new(text: &str) -> Self {
        let ctx = egui::Context::default();
        markd_platform::fonts::install(&ctx);
        Self {
            doc: Document::new(text, None),
            // 默认把「活动块」指到不存在的下标：整篇都走只读渲染路径。
            // `EditorState::default().active` 是 0，直接拿来当渲染态等于自欺欺人。
            state: EditorState {
                active: usize::MAX / 2,
                ..Default::default()
            },
            theme: Theme::get(crate::theme::ThemeKind::Light),
            images: ImageCache::new(),
            actions: Vec::new(),
            ctx,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>, W: f32) -> Vec<egui::epaint::ClippedShape> {
        let Self {
            doc,
            state,
            theme,
            images,
            actions,
            ctx,
        } = self;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(W, 800.0),
                )),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        theme,
                        ctx,
                        images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        true,
                        1,
                        actions,
                    );
                });
            },
        );
        // 无头环境没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
        out.textures_delta.clear();
        out.shapes
    }

    /// 本帧画出的引用竖线：`(左缘, 线宽, 线高)`。竖线是唯一满足
    /// 「强调色 + 宽 ≤ 6 + 高 > 8」的矩形，按这三项比就够了；
    /// 绝对 y 不比——上下文中别的块改高度会让整块平移，那不是本测试的口径。
    fn bars(shapes: &[egui::epaint::ClippedShape], accent: egui::Color32) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for s in shapes {
            if let egui::Shape::Rect(r) = &s.shape
                && r.fill == accent
                && r.rect.width() <= 6.0
                && r.rect.height() > 8.0
            {
                // 单位：百分之一像素，避免浮点相等比较。
                out.push((
                    (r.rect.min.x * 100.0).round() as i32,
                    (r.rect.width() * 100.0).round() as i32,
                    (r.rect.height() * 100.0).round() as i32,
                ));
            }
        }
        out.sort();
        out
    }

    /// 本帧某段文字的矩形（左缘、右缘、上缘、下缘），用于确认「线动没动」
    /// 不是「文字动」的误报。单位同样是百分之一像素。
    fn text_rect(
        shapes: &[egui::epaint::ClippedShape],
        needle: &str,
    ) -> Option<(i32, i32, i32, i32)> {
        for s in shapes {
            if let egui::Shape::Text(t) = &s.shape {
                let g = t.galley.text().to_string();
                if g.contains(needle) {
                    let r = egui::Rect::from_min_size(t.pos, t.galley.size());
                    return Some((
                        (r.min.x * 100.0).round() as i32,
                        (r.max.x * 100.0).round() as i32,
                        (r.min.y * 100.0).round() as i32,
                        (r.max.y * 100.0).round() as i32,
                    ));
                }
            }
        }
        None
    }
}

/// `(用例名, 文档, 在引用里要找的文字)`
fn cases() -> Vec<(&'static str, String, &'static str)> {
    let long = "很长的一句話".repeat(20);
    vec![
        ("单层", "> 引用一句话\n\n正文段落".to_string(), "引用"),
        ("两层嵌套", "> > 嵌套引用\n\n正文段落".to_string(), "嵌套"),
        ("三层嵌套", "> > > 三层引用\n\n正文段落".to_string(), "三层"),
        (
            "引用前有块",
            "标题段\n\n> 引用一句话\n\n正文段落".to_string(),
            "引用",
        ),
        (
            "引用前有引用",
            "> 第一句\n\n> 第二句\n\n正文段落".to_string(),
            "第二句",
        ),
        ("长引用换行", format!("> {long}\n\n正文段落"), "很长的"),
    ]
}

#[test]
fn quote_bar_sits_in_the_same_place_in_both_states() {
    for w in [520.0f32, 1280.0] {
        for (name, src, needle) in cases() {
            // 渲染态
            let mut q = Q::new(&src);
            let quote_idx = q
                .doc
                .blocks
                .iter()
                .position(|b| b.source().starts_with('>'))
                .unwrap_or_else(|| panic!("{name}：夹具里没有引用块"));
            let mut rendered = Vec::new();
            for _ in 0..4 {
                rendered = q.frame(vec![], w);
            }
            // 编辑态：把引用块自己点进来
            let mut q2 = Q::new(&src);
            q2.state.active = quote_idx;
            q2.state.focus_req = Some(quote_idx);
            q2.state.last_cursor = 2;
            let mut editing = Vec::new();
            for _ in 0..4 {
                editing = q2.frame(vec![], w);
            }
            // 判活：编辑态真吃得下键盘输入，否则两态比的是同一份渲染代码。
            let before = q2.doc.source();
            q2.frame(vec![egui::Event::Text("X".into())], w);
            assert_ne!(
                q2.doc.source(),
                before,
                "{name}：设了 active={quote_idx} 却没进编辑态（输入没写回），本对比无效"
            );
            let what = format!("[{name}] 宽 {w:.0}");
            let r = Q::bars(&rendered, q.theme.accent);
            let e = Q::bars(&editing, q.theme.accent);
            assert!(!r.is_empty(), "{what}：渲染态一帧里没画出引用竖线");
            assert_eq!(r, e, "{what}：两态竖线（左缘, 线宽, 线高）不一致");
            let (Some(a), Some(b)) = (
                Q::text_rect(&rendered, needle),
                Q::text_rect(&editing, needle),
            ) else {
                panic!("{what}：两态之一找不到引用文字，断言无从谈起");
            };
            // 文字左缘必须逐像素相同：左缘一动，竖线与正文之间的距离就变了。
            // 右缘不比：软换行的断行点两态本来就差一点（渲染态 Label 用满宽度，
            // 编辑态 TextEdit 的 desired_width 留了 8px），实测一份 120 字的长引用
            // 在 520px 宽下末行差 16px——那是「换到第几个字」的差，不是位置偏移。
            assert_eq!(
                a.0, b.0,
                "{what}：两态引用文字的左缘不一致（a={a:?} b={b:?}）"
            );
            // 纵向只卡 0.5px 以内：进编辑态后块间距实测会差 0.41px
            // （连续引用用的 1px 补白 vs TextEdit 的行首留白），肉眼看不出来；
            // 但历史上「进编辑态整块往下跳 5px」那种量级必须被这里拦住。
            let dy = (a.2 - b.2).abs() as f32 / 100.0;
            assert!(
                dy < 0.5,
                "{what}：两态引用文字的上缘差了 {dy:.2}px（a={a:?} b={b:?}）"
            );
        }
    }
}
