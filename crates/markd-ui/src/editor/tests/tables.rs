use super::*;

/// 应用级悬停验证：混合模式渲染表格后，鼠标悬停在单元格上
/// （悬停工具栏出现），表格的行/列分隔线必须一条不少、坐标不变。
#[test]
fn table_hover_keeps_separator_lines() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("前言\n\n甲|乙\n---|---\n一|二\n三|四", None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();

    let mut run = |doc: &mut Document,
                   state: &mut EditorState,
                   events: Vec<egui::Event>|
     -> Vec<(f32, f32, f32, f32, f32)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let mut segs: Vec<(f32, f32, f32, f32, f32)> = Vec::new();
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let _ = show_editor(
                    ui,
                    doc,
                    state,
                    &theme,
                    &c,
                    &mut images,
                    None,
                    &[],
                    false,
                    false,
                    false,
                    true,
                    1,
                    &mut actions,
                );
            });
        });
        for cs in &out.shapes {
            if let egui::Shape::LineSegment { points, stroke } = &cs.shape {
                segs.push((
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y,
                    stroke.width,
                ));
            }
        }
        out.textures_delta.clear();
        segs
    };

    let has = |segs: &[(f32, f32, f32, f32, f32)], s: &(f32, f32, f32, f32, f32)| {
        segs.iter().any(|t| {
            (t.0 - s.0).abs() < 0.5
                && (t.1 - s.1).abs() < 0.5
                && (t.2 - s.2).abs() < 0.5
                && (t.3 - s.3).abs() < 0.5
                && (t.4 - s.4).abs() < 0.1
        })
    };

    // 基线帧：渲染表格，直接以这一帧实际画出的分隔线为基准。
    // 不硬编码屏幕坐标——表格在 ScrollArea 里的位置会随文档/布局变化而浮动，
    // 这个测试真正要验证的是「悬停工具栏浮层出现后，表格自身的线不变」。
    let base = run(&mut doc, &mut state, vec![]);
    assert!(base.len() >= 3, "基线帧应渲染出表格分隔线，实际 {base:?}");
    // 取表格的水平/垂直分隔线（跨度≈整宽的横线，或贯穿表格的竖线），
    // 排除其它干扰线段。normalize 到 0.5px 容差。
    let table_lines: Vec<(f32, f32, f32, f32, f32)> = base
        .iter()
        .copied()
        .filter(|s| {
            let horizontal = (s.1 - s.3).abs() < 0.5 && (s.2 - s.0).abs() > 100.0;
            let vertical = (s.0 - s.2).abs() < 0.5 && (s.3 - s.1).abs() > 20.0;
            horizontal || vertical
        })
        .collect();
    assert!(
        table_lines.len() >= 3,
        "应识别出至少 2 横 1 竖的表格线，实际 {base:?}"
    );

    // 悬停表头单元格（表格可见区域内）两帧，悬停工具栏出现
    let hover = egui::pos2(300.0, 81.0);
    run(&mut doc, &mut state, vec![egui::Event::PointerMoved(hover)]);
    let hovering = run(&mut doc, &mut state, vec![]);

    // 悬停后：表格每条分隔线必须一条不少、坐标分毫未动
    // （允许悬停工具栏额外新增自己的线段，但不允许表格的线消失或移位）
    for s in &table_lines {
        assert!(
            has(&hovering, s),
            "悬停单元格后表格分隔线消失了：{s:?}，实际 {hovering:?}"
        );
    }
}

/// 编辑态表格单元格也要按对齐设置排版：之前点「居中/右对齐」只改了源码，
/// 单元格 TextEdit 永远左对齐，看起来像按钮失效。
#[test]
fn table_edit_cells_follow_alignment() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 1 | 2 |", None);
    let mut state = EditorState::default(); // active=0 → 表格编辑态
    let ctx = egui::Context::default();
    let mut run =
        |doc: &mut Document, state: &mut EditorState| -> Vec<egui::epaint::ClippedShape> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &c,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        true,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
            out.shapes
        };
    // 单元格文字「A」的绘制 x 坐标。
    let text_x = |shapes: &[egui::epaint::ClippedShape]| -> f32 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == "A" => Some(t.pos.x),
                _ => None,
            })
            .expect("应渲染出单元格文字 A")
    };

    // 进入编辑态：第一帧建立表格草稿。
    run(&mut doc, &mut state);
    let left_x = text_x(&run(&mut doc, &mut state));
    assert!(state.table_draft.is_some(), "表格应进入编辑态");

    // 改成居中：单元格文字应明显右移。
    if let Some((_, _, aligns)) = state.table_draft.as_mut() {
        *aligns = vec![crate::ast::Align::Center; 2];
    }
    let center_x = text_x(&run(&mut doc, &mut state));
    assert!(
        center_x > left_x + 10.0,
        "居中后编辑态单元格文字应右移：{left_x:.1} -> {center_x:.1}"
    );

    // 改成右对齐：继续右移。
    if let Some((_, _, aligns)) = state.table_draft.as_mut() {
        *aligns = vec![crate::ast::Align::Right; 2];
    }
    let right_x = text_x(&run(&mut doc, &mut state));
    assert!(
        right_x > center_x + 10.0,
        "右对齐后编辑态单元格文字应更靠右：{center_x:.1} -> {right_x:.1}"
    );
}
