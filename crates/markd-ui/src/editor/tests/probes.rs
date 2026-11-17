use super::*;

// 临时探针：定位渲染态/编辑态盒子顶差 5px 的来源
#[test]
fn probe_code_block_gap() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("前言\n\n```rust\nfn main() {}\n```", None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();

    let mut run =
        |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>, tag: &str| {
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
            println!("--- [{tag}] ---");
            for cs in &out.shapes {
                match &cs.shape {
                    egui::Shape::Rect(r) if r.fill == theme.code_bg && r.rect.width() > 200.0 => {
                        println!(
                            "  code_bg 盒子顶 {:.1} 底 {:.1}",
                            r.rect.top(),
                            r.rect.bottom()
                        );
                    }
                    egui::Shape::Text(t) => {
                        let s = t.galley.text();
                        if s.contains("前言") || s.contains("main") {
                            println!(
                                "  文本 {:?} 顶 {:.1} 底 {:.1}",
                                s.chars().take(6).collect::<String>(),
                                t.pos.y + t.galley.rect.top(),
                                t.pos.y + t.galley.rect.bottom(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        };

    run(&mut doc, &mut state, vec![], "渲染态");
    let click = egui::pos2(400.0, 90.0);
    run(
        &mut doc,
        &mut state,
        vec![
            egui::Event::PointerMoved(click),
            egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        "点击按下",
    );
    run(
        &mut doc,
        &mut state,
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        "点击抬起",
    );
    run(&mut doc, &mut state, vec![], "编辑态");
}

// 临时探针：三段文档的文本块坐标
#[test]
fn probe_para_positions() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
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
    for cs in &out.shapes {
        if let egui::Shape::Text(t) = &cs.shape {
            let s: String = t.galley.text().chars().take(6).collect();
            if !s.trim().is_empty() {
                println!(
                    "文本 {:?} 锚({:.1},{:.1}) rect={:?}",
                    s, t.pos.x, t.pos.y, t.galley.rect
                );
            }
        }
    }
}
