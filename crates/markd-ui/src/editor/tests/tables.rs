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

/// 表格里拖选：只选中拖过的单元格，复制就是这几格 —— 不再「一选就整张表」。
/// 渲染态（表格不是活动块）与编辑态（点进表格里编辑）走两条不同的代码路径，
/// 都要覆盖：用户既可能直接在表上拖，也可能先点进来再拖。
#[test]
fn drag_in_table_selects_cells_not_whole_table() {
    // (表格是否为活动块, 用例名)
    for (editing, tag) in [(false, "渲染态表格"), (true, "编辑态表格")] {
        let theme = Theme::get(crate::theme::ThemeKind::Light);
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        // 表格放在第 2 块：active=0 时它是渲染态；active=2 时它是编辑态。
        let mut doc = Document::new(
            "前文一段\n\n| 甲列 | 乙列 |\n|:--|--:|\n| 丙 | 丁 |\n\n后文一段",
            None,
        );
        let mut state = EditorState {
            active: if editing { 2 } else { 0 },
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut run = |doc: &mut Document,
                       state: &mut EditorState,
                       events: Vec<egui::Event>|
         -> Vec<egui::epaint::ClippedShape> {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 900.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let _ = show_editor(
                        ui,
                        doc,
                        state,
                        &theme,
                        &ctx,
                        &mut images,
                        None,
                        &[],
                        false,
                        false,
                        false,
                        false,
                        1,
                        &mut actions,
                    );
                });
            });
            out.textures_delta.clear();
            out.shapes
        };
        let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        // 单元格文字的矩形：拿它当「这一格」的命中位置。
        fn cell_of(shapes: &[egui::epaint::ClippedShape], want: &str) -> Option<egui::Rect> {
            shapes.iter().find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text().trim() == want => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.rect.size()))
                }
                _ => None,
            })
        }
        // 选区底色矩形（半透明那层，可能包在 Shape::Vec 里）。
        fn sel_rects(
            shapes: &[egui::epaint::ClippedShape],
            color: egui::Color32,
        ) -> Vec<egui::Rect> {
            fn walk(shape: &egui::Shape, color: egui::Color32, out: &mut Vec<egui::Rect>) {
                match shape {
                    egui::Shape::Rect(r) if r.fill == color => out.push(r.rect),
                    egui::Shape::Vec(v) => {
                        for s in v {
                            walk(s, color, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut out = Vec::new();
            for cs in shapes {
                walk(&cs.shape, color, &mut out);
            }
            out
        }
        // 静置两帧：编辑态要先把表格草稿建起来。
        run(&mut doc, &mut state, vec![]);
        let shapes = run(&mut doc, &mut state, vec![]);
        let cell = |want: &str| -> egui::Rect {
            cell_of(&shapes, want).unwrap_or_else(|| panic!("{tag}：找不到单元格「{want}」"))
        };
        let (bing, ding, jia) = (cell("丙"), cell("丁"), cell("甲列"));

        // 从「丙」横拖到「丁」：一整行数据，正好是用户想复制的那一行。
        let a = bing.center();
        let b = ding.center();
        let step = |k: usize, from: egui::Pos2, to: egui::Pos2| {
            let t = k as f32 / 8.0;
            egui::pos2(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
        };
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerMoved(a), btn(a, true)],
        );
        for k in 1..=8 {
            run(
                &mut doc,
                &mut state,
                vec![egui::Event::PointerMoved(step(k, a, b))],
            );
        }
        run(&mut doc, &mut state, vec![btn(b, false)]);
        run(&mut doc, &mut state, vec![]);
        assert_eq!(
            state.table_sel,
            Some((2, (1, 0), (1, 1))),
            "{tag}：从丙拖到丁应只选中这两个单元格（行列含表头行，故是第 1 行）"
        );
        assert!(
            state.cross_sel.is_none(),
            "{tag}：表内拖选不该把整张表变成跨块选区"
        );
        assert_eq!(
            crate::editor::copy_selection(&doc, &state).as_deref(),
            Some("丙\t丁"),
            "{tag}：复制应只带上选中的单元格"
        );
        assert_eq!(
            crate::editor::copy_selection_markdown(&doc, &state).as_deref(),
            Some("| 丙 | 丁 |"),
            "{tag}：复制为 Markdown 应是这一行的表格源码"
        );

        // 高亮只落在选中的格子上：没选中的表头那格不该跟着发蓝。
        let sel_fill = crate::render::selection_overlay(&theme);
        let hi = sel_rects(&run(&mut doc, &mut state, vec![]), sel_fill);
        assert!(
            hi.iter().any(|r| r.contains(bing.center())),
            "{tag}：选中的单元格应有底色，实际底色 {hi:?}"
        );
        assert!(
            !hi.iter().any(|r| r.contains(jia.center())),
            "{tag}：没选中的表头单元格不该有底色，实际底色 {hi:?}"
        );

        // 拖出表格：仍按整块走跨块选区（要连着上下文一起选时靠它）。
        let far = egui::pos2(bing.center().x, bing.bottom() + 120.0);
        run(
            &mut doc,
            &mut state,
            vec![egui::Event::PointerMoved(a), btn(a, true)],
        );
        for k in 1..=8 {
            run(
                &mut doc,
                &mut state,
                vec![egui::Event::PointerMoved(step(k, a, far))],
            );
        }
        run(&mut doc, &mut state, vec![btn(far, false)]);
        run(&mut doc, &mut state, vec![]);
        let sel = state
            .cross_sel
            .unwrap_or_else(|| panic!("{tag}：拖出表格后应建立跨块选区"));
        assert_eq!(
            (sel.0.min(sel.2), if sel.0 <= sel.2 { sel.1 } else { sel.3 }),
            (2, 0),
            "{tag}：拖出表格后锚点应落在表格开头，实际选区 {sel:?}"
        );
        let copied = crate::editor::copy_selection(&doc, &state).unwrap_or_default();
        for w in ["甲列", "乙列", "丙", "丁"] {
            assert!(
                copied.contains(w),
                "{tag}：整张表都该被选走，漏了「{w}」，实选 {copied:?}"
            );
        }
        assert!(
            state.table_sel.is_none(),
            "{tag}：跨块选区与单元格选区不能同时存在"
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
