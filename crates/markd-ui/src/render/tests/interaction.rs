use super::*;

/// 表格宽度跟随可用宽度：三列应铺满编辑区，而不是被内容挤成一个小方块。
#[test]
fn table_resizes_with_available_width() {
    use crate::ast::Align;
    let theme = Theme::get(ThemeKind::Dark);
    let mut images = ImageCache::new();
    let outline: Vec<OutlineItem> = Vec::new();
    let mk = |s: &str| vec![Inline::Text(s.to_string())];
    let block = Block::Table {
        head: vec![mk("名称"), mk("说明"), mk("状态")],
        rows: vec![vec![mk("a"), mk("b"), mk("c")]],
        align: vec![Align::Left, Align::Left, Align::Left],
        source: String::new(),
    };
    egui::__run_test_ui(|ui| {
        ui.set_width(800.0);
        let ctx = ui.ctx().clone();
        let mut actions: Vec<Action> = Vec::new();
        let folded: HashSet<usize> = HashSet::new();
        let mut env = RenderEnv {
            theme: &theme,
            ctx: &ctx,
            images: &mut images,
            base_dir: None,
            outline: &outline,
            actions: &mut actions,
            max_width: 800.0,
            cross_sel: None,
            folded: &folded,
            group_tab: 0,
        };
        let resp = render_block(ui, 0, &block, &mut env, false);
        let w = resp.rect.width();
        assert!(w > 700.0, "表格应铺满可用宽度，实际 {w}");
    });
}

/// 单击链接不跳转，只把光标放进链接（随后露出源码供修改）；
/// 只有 Cmd 或 Ctrl+单击才真正打开链接。
#[test]
fn clicking_link_requires_command_or_ctrl() {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let press = |pos: egui::Pos2, down: bool, modifiers: egui::Modifiers| -> Vec<egui::Event> {
        let mut events = Vec::new();
        if down {
            events.push(egui::Event::PointerMoved(pos));
        }
        // RawInput 没有 modifiers 字段；修饰键只能通过 ModifiersChanged 事件传入。
        events.push(egui::Event::ModifiersChanged(modifiers));
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers,
        });
        events
    };

    // 每个场景用独立的 Context：共享 Context 时第二次点击会被判成双击而提前 return。
    let click = |modifiers: egui::Modifiers| -> (egui::Rect, Vec<Action>) {
        let ctx = egui::Context::default();
        let link = Block::Paragraph {
            content: crate::ast::parse_inline("[点我](https://e.com)"),
            source: "[点我](https://e.com)".to_string(),
        };
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let mut run = |events: Vec<egui::Event>, actions: &mut Vec<Action>| -> egui::Rect {
            let mut rect = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let ctx = ui.ctx().clone();
                        let folded: HashSet<usize> = HashSet::new();
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx,
                            images: &mut images,
                            base_dir: None,
                            outline: &outline,
                            actions,
                            max_width: 700.0,
                            cross_sel: None,
                            folded: &folded,
                            group_tab: 0,
                        };
                        rect = render_block(ui, 0, &link, &mut env, false).rect;
                    });
                },
            );
            out.textures_delta.clear();
            rect
        };
        let rect = run(vec![], &mut actions);
        let pos = rect.center();
        let _ = run(press(pos, true, modifiers), &mut actions);
        let _ = run(press(pos, false, modifiers), &mut actions);
        (rect, actions)
    };

    // 普通单击：只激活编辑，不打开链接。
    let (rect, actions) = click(egui::Modifiers::default());
    assert!(!rect.is_negative());
    assert!(
        actions.iter().any(|a| matches!(a, Action::Caret(..))),
        "普通单击链接应把光标放进链接: {actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::OpenLink(_))),
        "普通单击不应打开链接: {actions:?}"
    );

    // Cmd / Ctrl+单击：打开链接，不再触发编辑光标。
    for modifiers in [
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    ] {
        let (_, actions) = click(modifiers);
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::OpenLink(u) if u == "https://e.com")),
            "{modifiers:?} + 单击应打开链接: {actions:?}"
        );
        assert!(
            !actions.iter().any(|a| matches!(a, Action::Caret(..))),
            "{modifiers:?} + 单击不应同时激活编辑块: {actions:?}"
        );
    }
}

/// 单击普通文字应放置光标（独立 ctx，避免与链接点击连成双击）。
#[test]
fn clicking_text_places_caret() {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let run = |block: &Block,
               events: Vec<egui::Event>,
               images: &mut ImageCache,
               actions: &mut Vec<Action>|
     -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let folded: HashSet<usize> = HashSet::new();
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &c,
                        images,
                        base_dir: None,
                        outline: &outline,
                        actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    rect = render_block(ui, 0, block, &mut env, false).rect;
                });
            },
        );
        out.textures_delta.clear();
        rect
    };
    let plain = Block::Paragraph {
        content: vec![Inline::Text("普通文字".to_string())],
        source: "普通文字".to_string(),
    };
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let rect = run(&plain, vec![], &mut images, &mut actions);
    let pos = rect.center();
    let _ = run(
        &plain,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut images,
        &mut actions,
    );
    let _ = run(
        &plain,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut images,
        &mut actions,
    );
    assert!(
        actions.iter().any(|a| matches!(a, Action::Caret(..))),
        "点普通文字应放置光标: {actions:?}"
    );
}

/// 点击一行文字右侧的空白（超出 galley 宽度）也应命中该块：
/// 光标应落到该行最后一个字符之后，而不是毫无反应。
#[test]
fn clicking_row_blank_places_caret_at_line_end() {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let run = |block: &Block,
               events: Vec<egui::Event>,
               images: &mut ImageCache,
               actions: &mut Vec<Action>|
     -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let ctx = ui.ctx().clone();
                    let folded: HashSet<usize> = HashSet::new();
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images,
                        base_dir: None,
                        outline: &outline,
                        actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    rect = render_block(ui, 0, block, &mut env, false).rect;
                });
            },
        );
        out.textures_delta.clear();
        rect
    };
    let plain = Block::Paragraph {
        content: vec![Inline::Text("短文字".to_string())],
        source: "短文字".to_string(),
    };
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let rect = run(&plain, vec![], &mut images, &mut actions);
    // 点击文字右侧的行内空白：块文字只有三个字（约几十像素），
    // 点击点放在行右缘内、文字 galley 之外。
    let pos = egui::Pos2::new(rect.right() - 20.0, rect.center().y);
    assert!(
        pos.x > rect.min.x + 100.0,
        "点击点必须落在文字 galley 之外的空白区"
    );
    let press = |pos: egui::Pos2, down: bool| -> Vec<egui::Event> {
        let mut events = Vec::new();
        events.push(egui::Event::PointerMoved(pos));
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::default(),
        });
        events
    };
    let _ = run(&plain, press(pos, true), &mut images, &mut actions);
    let _ = run(&plain, press(pos, false), &mut images, &mut actions);
    assert!(
        actions.iter().any(|a| matches!(a, Action::Caret(..))),
        "点行尾空白应放置光标到行末: {actions:?}"
    );
}
