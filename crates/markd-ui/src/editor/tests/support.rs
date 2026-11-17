use super::*;

pub(super) fn frame(width: f32, f: impl FnMut(&mut egui::Ui)) {
    frame_sized(width, 900.0, f);
}

/// 复用同一个 Context 跑一帧：模拟真实运行时（galley / 纹理缓存都是热的）。
#[allow(clippy::too_many_arguments)]
pub(super) fn frame_warm(
    ctx: &egui::Context,
    width: f32,
    height: f32,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    outline: &[crate::document::OutlineItem],
    actions: &mut Vec<crate::render::Action>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, height),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let _ = show_editor(
                ui, doc, state, theme, &c, images, None, outline, false, false, false, false, 1,
                actions,
            );
        });
    });
    out.textures_delta.clear();
}

pub(super) fn frame_sized(width: f32, height: f32, mut f: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, height),
        )),
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| f(ui));
    });
    // 无头环境下没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
    out.textures_delta.clear();
}

/// 点一下编辑区空白处（按下/松开分两帧），再跑两帧让焦点落地。
#[allow(deprecated)]
pub(super) fn click_blank(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    actions: &mut Vec<crate::render::Action>,
    pos: egui::Pos2,
) {
    run_editor_frame(
        ctx,
        doc,
        state,
        theme,
        images,
        actions,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    run_editor_frame(
        ctx,
        doc,
        state,
        theme,
        images,
        actions,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    // 给「请求焦点 + 设置光标」一帧的落地时间
    for _ in 0..2 {
        run_editor_frame(ctx, doc, state, theme, images, actions, vec![]);
    }
}

/// 模拟真实 app 的面板结构（menubar/toolbar/tabbar/sidebar + CentralPanel）跑一帧。
#[allow(clippy::too_many_arguments)]
pub(super) fn run_app_like_frame(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    actions: &mut Vec<crate::render::Action>,
    events: Vec<egui::Event>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let outline = doc.outline();
    let mut out = ctx.run_ui(input, |root| {
        let mut root = egui::Ui::new(
            root.clone(),
            egui::Id::unique("app_root"),
            egui::UiBuilder::new(),
        );
        egui::Panel::top("menubar").show(&mut root, |ui| {
            ui.label("menu");
        });
        egui::Panel::top("toolbar").show(&mut root, |ui| {
            ui.label("toolbar");
        });
        egui::Panel::bottom("statusbar").show(&mut root, |ui| {
            ui.label("status");
        });
        egui::Panel::left("sidebar")
            .exact_size(220.0)
            .show(&mut root, |ui| {
                ui.label("sidebar");
            });
        egui::Panel::top("tabbar").show(&mut root, |ui| {
            ui.label("tabs");
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(8))
            .show(&mut root, |ui| {
                let _ = show_editor(
                    ui, doc, state, theme, ctx, images, None, &outline, false, false, false, false,
                    1, actions,
                );
            });
    });
    out.textures_delta.clear();
}

/// 用同一个 Context 跑一帧编辑器（带可注入的输入事件）。
pub(super) fn run_editor_frame(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    actions: &mut Vec<crate::render::Action>,
    events: Vec<egui::Event>,
) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let outline = doc.outline();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let _ = show_editor(
                ui, doc, state, theme, ctx, images, None, &outline, false, false, false, false, 1,
                actions,
            );
        });
    });
    out.textures_delta.clear();
}
