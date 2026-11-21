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

/// 渲染一帧代码面板，回传该帧所有文字、文字中心位置与该 galley 的着色分段数。
/// `pointer` 喂鼠标位置；`click` 喂按下 / 松开（Some(true) / Some(false)）。
fn panel_frame(
    ctx: &egui::Context,
    block: &Block,
    images: &mut ImageCache,
    actions: &mut Vec<Action>,
    pointer: Option<egui::Pos2>,
    click: Option<bool>,
) -> Vec<(String, egui::Pos2, usize)> {
    let theme = Theme::get(ThemeKind::Light);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut events = Vec::new();
    if let Some(p) = pointer {
        events.push(egui::Event::PointerMoved(p));
    }
    if let (Some(pressed), Some(p)) = (click, pointer) {
        events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
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
    out.shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(ts) => Some((
                ts.galley.text().to_string(),
                ts.pos + ts.galley.rect.center().to_vec2(),
                ts.galley.job.sections.len(),
            )),
            _ => None,
        })
        .collect()
}

fn names(items: &[(String, egui::Pos2, usize)]) -> Vec<&str> {
    items.iter().map(|(t, _, _)| t.as_str()).collect()
}

fn has(items: &[(String, egui::Pos2, usize)], text: &str) -> bool {
    items.iter().any(|(t, _, _)| t == text)
}

/// YAML Front Matter 的面板不画功能栏：静止态只有 yaml 代码区；指针进入面板时
/// 折叠 / 删除 / 复制浮现在右上角，代码区位置不变（不然后果是一 hover 整篇往下跳）。
/// 普通代码块作为对照，静止态仍要有完整功能栏。
#[test]
fn front_matter_panel_hides_its_header_until_hovered() {
    let ctx = egui::Context::default();
    let fm = Block::FrontMatter {
        yaml: "title: 标题\ntags: [a, b]".to_string(),
        source: "---\ntitle: 标题\ntags: [a, b]\n---".to_string(),
    };
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();

    // 1) 静止态：功能栏与分隔线整条不画，yaml 仍按语法分段着色。
    let rest = panel_frame(&ctx, &fm, &mut images, &mut actions, None, None);
    let code = rest
        .iter()
        .find(|(t, _, _)| t.contains("title: 标题") && t.contains("tags: [a, b]"))
        .expect("代码区应含两行 yaml");
    assert_eq!(code.0, "title: 标题\ntags: [a, b]\n", "{:?}", names(&rest));
    assert!(code.1.y > 0.0, "代码区要有正常的位置");
    assert!(
        code.2 > 1,
        "yaml 代码区应按语法分多段着色，实际 section={}",
        code.2
    );
    for gone in ["yaml", "折叠", "删除", "复制", "YAML Front Matter"] {
        assert!(
            !has(&rest, gone),
            "Front Matter 静止态不该有功能栏，却出现了 `{gone}`：{:?}",
            names(&rest)
        );
    }
    assert!(
        actions.is_empty(),
        "没点任何东西时不该产生 action：{actions:?}"
    );
    let y_rest = code.1.y;

    // 2) 指针进面板：三个按钮浮现，且代码区一个字都没挪。
    let hover_point = code.1;
    let _ = panel_frame(
        &ctx,
        &fm,
        &mut images,
        &mut actions,
        Some(hover_point),
        None,
    );
    let hover = panel_frame(
        &ctx,
        &fm,
        &mut images,
        &mut actions,
        Some(hover_point),
        None,
    );
    for want in ["折叠", "删除", "复制"] {
        assert!(
            has(&hover, want),
            "悬停面板应浮现 `{want}` 按钮：{:?}",
            names(&hover)
        );
    }
    assert!(
        !has(&hover, "yaml"),
        "浮现的应是按钮条，不是原来的功能栏：{:?}",
        names(&hover)
    );
    let y_hover = hover
        .iter()
        .find(|(t, _, _)| t.contains("title: 标题"))
        .expect("悬停时代码区仍在")
        .1
        .y;
    assert!(
        (y_hover - y_rest).abs() < 0.5,
        "hover 不许把代码区顶下去：静止 {y_rest}，悬停 {y_hover}"
    );

    // 3) 真点悬浮条里的「删除」：只弹确认窗，不许顺手把选区/光标按字符锚进 yaml。
    let del = hover
        .iter()
        .find(|(t, _, _)| t == "删除")
        .map(|(_, p, _)| *p)
        .expect("没有删除按钮");
    actions.clear();
    let mut items: Vec<(String, egui::Pos2, usize)> = Vec::new();
    for pressed in [true, false] {
        items = panel_frame(
            &ctx,
            &fm,
            &mut images,
            &mut actions,
            Some(del),
            Some(pressed),
        );
    }
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::AskDeleteBlock(0))),
        "点悬浮条「删除」应产生 AskDeleteBlock 交给 app 层弹确认窗：{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::CrossDrag(..))),
        "点悬浮条按钮不该同时按字符锚出选区：{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Caret(..))),
        "点悬浮条按钮不该把光标塞进代码：{actions:?}"
    );
    assert!(
        items.iter().any(|(t, _, _)| t == "删除"),
        "点击过程中悬浮条不该自己消失：{:?}",
        names(&items)
    );

    // 4) 反空转：普通代码块走同一套面板代码，静止态仍必须有功能栏。
    let code_block = Block::Code {
        lang: "rust".to_string(),
        code: "fn main() {}".to_string(),
        source: "```rust\nfn main() {}\n```".to_string(),
        quote: 0,
    };
    actions.clear();
    let rest_code = panel_frame(&ctx, &code_block, &mut images, &mut actions, None, None);
    for want in ["rust", "折叠", "删除", "复制"] {
        assert!(
            has(&rest_code, want),
            "普通代码块的功能栏不许跟着消失，缺 `{want}`：{:?}",
            names(&rest_code)
        );
    }
    assert!(actions.is_empty(), "未点击时不该产生 action：{actions:?}");
}
