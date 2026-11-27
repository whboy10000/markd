use super::*;

/// 回归：表格里打字必须写回源码。
///
/// `text_differs` 那道防空操作的闸门比对的是 `active_buf`，而单元格里编辑的是表格草稿，
/// `active_buf` 那份整块源码从头到尾没变过 —— 闸门会把每次按键都判成「没改到字」，
/// 打字只留在草稿里，点出表格就整格回退。表格的比对基准必须是草稿重建出的源码。
#[test]
fn typing_in_a_cell_writes_back_to_source() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut doc = Document::new("| A | B |\n| --- | --- |\n| 甲 | 乙 |", None);
    let mut state = EditorState::default();
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut run = |doc: &mut Document,
                   state: &mut EditorState,
                   events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
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
                    false,
                    1,
                    &mut actions,
                );
            });
        });
        out.textures_delta.clear();
    };
    run(&mut doc, &mut state, vec![]); // 建草稿
    state.table_focus = Some((1, 0)); // 请求聚焦 (1,0)
    run(&mut doc, &mut state, vec![]); // 该帧真的 request_focus
    run(&mut doc, &mut state, vec![egui::Event::Text("X".into())]);
    assert!(
        doc.source().contains("X甲"),
        "打字应写回源码，实际 {:?}",
        doc.source()
    );
}

/// 没改字就不该写：只聚焦、不输入，源码与撤销栈都不动。
#[test]
fn focusing_a_cell_alone_does_not_dirty_the_doc() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let src = "| A | B |\n| --- | --- |\n| 甲 | 乙 |";
    let mut doc = Document::new(src, None);
    let mut state = EditorState::default();
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut run = |doc: &mut Document, state: &mut EditorState| {
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
                    false,
                    1,
                    &mut actions,
                );
            });
        });
        out.textures_delta.clear();
    };
    run(&mut doc, &mut state);
    state.table_focus = Some((1, 0));
    for _ in 0..4 {
        run(&mut doc, &mut state);
    }
    assert_eq!(doc.source(), src, "只聚焦不改字，源码不该动");
}
