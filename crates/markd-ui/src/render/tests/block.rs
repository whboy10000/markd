use super::*;

/// 渲染一个代码块（含右上角复制按钮），确认控件树可正常构建、
/// 且未点击时不会产生任何 action（避免误触发复制）。
#[test]
fn code_block_renders_with_copy_button() {
    let theme = Theme::get(ThemeKind::Light);
    let mut images = ImageCache::new();
    let outline: Vec<OutlineItem> = Vec::new();
    let block = Block::Code {
        lang: "rust".to_string(),
        code: "fn main() {}".to_string(),
        source: "```rust\nfn main() {}\n```".to_string(),
        quote: 0,
    };
    egui::__run_test_ui(|ui| {
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
        let _ = render_block(ui, 0, &block, &mut env, false);
        assert!(actions.is_empty(), "未点击时不应产生 action");
    });
}

/// 折叠态的代码块不渲染代码，只留一行提示，整体高度应明显矮于展开态。
#[test]
fn folded_code_block_renders_collapsed() {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let block = Block::Code {
        lang: "rust".to_string(),
        code: "fn main() {}\nfn foo() {}\nfn bar() {}\nfn baz() {}".to_string(),
        source: "```rust\nfn main() {}\nfn foo() {}\nfn bar() {}\nfn baz() {}\n```".to_string(),
        quote: 0,
    };
    // 展开/折叠各用独立 ctx：折叠动画跨帧，共享 ctx 会让「折叠态」仍处于
    // 动画起点（openness≈1），看起来像没折叠。
    let render_height = |folded: bool| -> f32 {
        let ctx = egui::Context::default();
        let mut images = ImageCache::new();
        let mut actions: Vec<Action> = Vec::new();
        let set: HashSet<usize> = if folded {
            [0].into_iter().collect()
        } else {
            HashSet::new()
        };
        let mut height = 0.0;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
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
                        cross_sel: None,
                        folded: &set,
                        group_tab: 0,
                    };
                    height = render_block(ui, 0, &block, &mut env, false).rect.height();
                });
            },
        );
        out.textures_delta.clear();
        height
    };
    let expanded_h = render_height(false);
    assert!(expanded_h > 40.0, "展开态应占较大高度: {expanded_h}");
    let collapsed_h = render_height(true);
    assert!(
        collapsed_h < expanded_h,
        "折叠态应比展开态矮: expanded={expanded_h} collapsed={collapsed_h}"
    );
}

/// 代码块右上角必须真的渲染出「折叠」按钮，且点击它会产生 FoldToggle。
/// 这是「用户看不到折叠按钮」的直接回归：按钮文本「折叠」要出现在渲染输出里。
#[test]
fn code_block_fold_button_clickable() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let block = Block::Code {
        lang: "rust".to_string(),
        code: "fn main() {}".to_string(),
        source: "```rust\nfn main() {}\n```".to_string(),
        quote: 0,
    };
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();

    // 第一帧：渲染代码块，从 shapes 里定位「折叠」按钮文本的中心。
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut fold_pos = egui::Pos2::ZERO;
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        for cs in &out.shapes {
            if let egui::Shape::Text(ts) = &cs.shape
                && ts.galley.text() == "折叠"
            {
                fold_pos = ts.pos + ts.galley.rect.center().to_vec2();
            }
        }
        out.textures_delta.clear();
    }
    assert!(fold_pos != egui::Pos2::ZERO, "代码块应渲染出「折叠」按钮");

    // 按下 + 松开：真实点击这个按钮。
    actions.clear();
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(fold_pos),
                    egui::Event::PointerButton {
                        pos: fold_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        out.textures_delta.clear();
    }
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos: fold_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        out.textures_delta.clear();
    }
    assert!(
        actions.iter().any(|a| matches!(a, Action::FoldToggle(0))),
        "点击「折叠」按钮应产生 FoldToggle: {actions:?}"
    );
}

/// 回归：点击 Mermaid 图表块的「折叠」按钮必须只折叠、不激活块。
/// 之前图表分支用整个盒子的 response 做块定位，点折叠按钮会连带着
/// `new_active` 把块切进编辑态、显示源码——表现为「点了没折叠反而显示源码」。
#[test]
fn diagram_fold_button_does_not_activate_block() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let block = Block::Diagram {
        kind: crate::ast::DiagramKind::Flow,
        code: "flowchart TD\n  A[开始] --> B[结束]".to_string(),
        source: "```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n```".to_string(),
    };
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();

    // 第一帧：定位「折叠」按钮。
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut fold_pos = egui::Pos2::ZERO;
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        for cs in &out.shapes {
            if let egui::Shape::Text(ts) = &cs.shape
                && ts.galley.text() == "折叠"
            {
                fold_pos = ts.pos + ts.galley.rect.center().to_vec2();
            }
        }
        out.textures_delta.clear();
    }
    assert!(fold_pos != egui::Pos2::ZERO, "图表块应渲染出「折叠」按钮");

    // 按下 + 松开：真实点击，记录块响应是否被「误点」。
    actions.clear();
    let mut block_clicked = false;
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(fold_pos),
                    egui::Event::PointerButton {
                        pos: fold_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let resp = render_block(ui, 0, &block, &mut env, false);
                    block_clicked = resp.clicked();
                });
            },
        );
        out.textures_delta.clear();
    }
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos: fold_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let resp = render_block(ui, 0, &block, &mut env, false);
                    block_clicked = resp.clicked();
                });
            },
        );
        out.textures_delta.clear();
    }
    assert!(
        actions.iter().any(|a| matches!(a, Action::FoldToggle(0))),
        "点击「折叠」应产生 FoldToggle: {actions:?}"
    );
    assert!(
        !block_clicked,
        "点击「折叠」按钮不应激活图表块（否则会进入编辑态显示源码）"
    );
}

/// 回归：点击 Mermaid 图表块的「删除」按钮必须产生 AskDeleteBlock
/// （app 层据此弹确认窗），且不误激活块。行为与代码块删除按钮一致。
#[test]
fn diagram_delete_button_requests_confirm() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let block = Block::Diagram {
        kind: crate::ast::DiagramKind::Flow,
        code: "flowchart TD\n  A[开始] --> B[结束]".to_string(),
        source: "```mermaid\nflowchart TD\n  A[开始] --> B[结束]\n```".to_string(),
    };
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();

    // 第一帧：定位「删除」按钮。
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut delete_pos = egui::Pos2::ZERO;
    {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let _ = render_block(ui, 0, &block, &mut env, false);
                });
            },
        );
        for cs in &out.shapes {
            if let egui::Shape::Text(ts) = &cs.shape
                && ts.galley.text() == "删除"
            {
                delete_pos = ts.pos + ts.galley.rect.center().to_vec2();
            }
        }
        out.textures_delta.clear();
    }
    assert!(delete_pos != egui::Pos2::ZERO, "图表块应渲染出「删除」按钮");

    // 按下 + 松开：真实点击。
    actions.clear();
    let mut block_clicked = false;
    for pressed in [true, false] {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerButton {
                    pos: delete_pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &ctx,
                        images: &mut images,
                        base_dir: None,
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    let resp = render_block(ui, 0, &block, &mut env, false);
                    block_clicked |= resp.clicked();
                });
            },
        );
        out.textures_delta.clear();
    }
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::AskDeleteBlock(0))),
        "点击「删除」应产生 AskDeleteBlock 交给 app 层弹确认窗: {actions:?}"
    );
    assert!(!block_clicked, "点击「删除」按钮不应激活图表块");
}
