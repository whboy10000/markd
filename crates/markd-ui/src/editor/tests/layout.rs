use super::*;

/// 回归：编辑区正文（段落 / 活动块 / 引用）都必须按窗口宽度软换行。
#[test]
fn long_paragraph_wraps_in_editor_column() {
    let text = "word ".repeat(300);
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut label_size = egui::Vec2::ZERO;
    let mut edit_size = egui::Vec2::ZERO;
    let mut quote_size = egui::Vec2::ZERO;
    frame(700.0, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("t")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let outer_w = ui.available_width();
                let page_w = (outer_w * 0.9).max(200.0);
                ui.horizontal_top(|ui| {
                    let pad = ((outer_w - page_w) / 2.0).max(0.0);
                    ui.add_space(pad);
                    ui.vertical(|ui| {
                        ui.set_width(page_w);
                        label_size = ui.add(egui::Label::new(text.clone())).rect.size();
                        let mut buf = text.clone();
                        let out = TextEdit::multiline(&mut buf)
                            .desired_rows(1)
                            .desired_width(page_w)
                            .frame(egui::Frame::NONE)
                            .return_key(Option::<egui::KeyboardShortcut>::None)
                            .show(ui);
                        edit_size = out.response.response.rect.size();

                        // 引用块内部是横向布局，最容易退化成不换行。
                        let quote = Block::Quote {
                            level: 1,
                            content: vec![crate::ast::Inline::Text(text.clone())],
                            source: text.clone(),
                        };
                        let ctx_q = ui.ctx().clone();
                        let folded: HashSet<usize> = HashSet::new();
                        let mut env = RenderEnv {
                            theme: &theme,
                            ctx: &ctx_q,
                            images: &mut images,
                            base_dir: None,
                            outline: &[],
                            actions: &mut actions,
                            max_width: page_w,
                            cross_sel: None,
                            folded: &folded,
                            group_tab: 0,
                        };
                        quote_size = render::render_block(ui, 0, &quote, &mut env, false)
                            .rect
                            .size();
                    });
                });
            });
    });
    assert!(label_size.y > 40.0, "Label 没有换行：size={label_size:?}");
    assert!(label_size.x <= 700.0, "Label 超出窗口：size={label_size:?}");
    assert!(edit_size.y > 40.0, "TextEdit 没有换行：size={edit_size:?}");
    assert!(quote_size.y > 40.0, "引用块没有换行：size={quote_size:?}");
    assert!(quote_size.x <= 700.0, "引用块超出窗口：size={quote_size:?}");
}

/// 行号栏宽度必须固定为 70px 常量：不随字体、行数位数、窗口或模式变化；
/// 关闭行号时为 0。
#[test]
fn gutter_width_is_fixed_across_line_counts() {
    assert_eq!(gutter_width(true), 70.0);
    assert_eq!(gutter_width(true), GUTTER_WIDTH);
    assert_eq!(gutter_width(false), 0.0);
    // 固定间距常量也不变
    assert_eq!(GUTTER_PAD, 12.0);
}

/// 行号栏锚定在编辑区最左侧：代码窗口从 700px 拉到 1500px（全窗口 /
/// 部分窗口）时，源码模式与混合模式的行号 x 坐标都必须不变，
/// 且数字右缘始终位于固定 70px 行号栏内（距左缘 70-12=58px）。
#[test]
fn gutter_stays_fixed_across_window_widths_and_modes() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let src = "alpha\n\nbeta\n\ngamma";

    let run = |source_mode: bool, width: f32| -> Vec<(String, f32, f32, f32)> {
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
        let mut doc = Document::new(src, None);
        let mut state = EditorState::default();
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 900.0),
            )),
            ..Default::default()
        };
        let mut nums: Vec<(String, f32, f32, f32)> = Vec::new();
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
                    source_mode,
                    false,
                    false,
                    true,
                    1,
                    &mut actions,
                );
            });
        });
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                let s = t.galley.text();
                if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                    nums.push((
                        s.to_string(),
                        t.pos.x,
                        t.galley.size().x,
                        cs.clip_rect.left(),
                    ));
                }
            }
        }
        out.textures_delta.clear();
        nums
    };

    for source_mode in [true, false] {
        let narrow = run(source_mode, 700.0);
        let wide = run(source_mode, 1500.0);
        assert!(!narrow.is_empty(), "source_mode={source_mode} 应有行号");
        assert_eq!(
            narrow.len(),
            wide.len(),
            "source_mode={source_mode} 两种窗口宽度下行号数量应一致"
        );
        for (i, (text, x, _, clip_left)) in narrow.iter().enumerate() {
            assert_eq!(text, &wide[i].0, "source_mode={source_mode} 行号顺序应一致");
            assert!(
                (x - wide[i].1).abs() < 0.5,
                "source_mode={source_mode} 行号 {text} 的 x 随窗口宽度变化：{x} → {}",
                wide[i].1
            );
            let gutter_right = x + narrow[i].2 - clip_left;
            assert!(
                (gutter_right - (GUTTER_WIDTH - GUTTER_PAD)).abs() < 0.5,
                "source_mode={source_mode} 行号 {text} 右缘距编辑区左缘 {gutter_right:.1}px，\
                     应在固定 {:.0}px 行号栏内",
                GUTTER_WIDTH - GUTTER_PAD
            );
        }
    }
}

/// 源码模式行号栏宽度恒定验证：抓取行号数字的真实绘制坐标，
/// 回车增加一行后（行数 10→11，位数都被 max(3) 钳住），
/// 所有行号的 x 位置必须保持不动（宽度没变、正文没横移）。
#[test]
fn source_mode_gutter_x_stays_put_across_enter() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let src: String = (1..=10)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut doc = Document::new(&src, None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();

    // 跑一帧源码模式（行号开启），返回所有行号数字 (文本, 锚点位置)
    let mut run = |doc: &mut Document,
                   state: &mut EditorState,
                   events: Vec<egui::Event>|
     -> Vec<(String, egui::Pos2)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let mut nums: Vec<(String, egui::Pos2)> = Vec::new();
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
                    true, // source_mode
                    false,
                    false,
                    true, // show_line_numbers
                    1,
                    &mut actions,
                );
            });
        });
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                let s = t.galley.text();
                if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                    nums.push((s.to_string(), t.pos));
                }
            }
        }
        out.textures_delta.clear();
        nums
    };

    let key = |pressed: bool| -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        }]
    };
    let click = egui::pos2(400.0, 100.0);

    // 基线：10 行文档
    let before = run(&mut doc, &mut state, vec![]);
    assert_eq!(before.len(), 10, "应有 10 个行号数字，实际 {:?}", before);

    // 点击编辑区聚焦 TextEdit，然后回车新增一行
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
    );
    run(&mut doc, &mut state, key(true));
    run(&mut doc, &mut state, key(false));

    // 回车后：11 行
    let after = run(&mut doc, &mut state, vec![]);
    assert_eq!(after.len(), 11, "回车后应有 11 个行号数字");
    assert_eq!(
        doc.source().matches('\n').count() + 1,
        11,
        "文档应真的多了一行"
    );
    // 共同行号的 x 必须完全一致：行号栏宽度恒定，正文没有横移
    for (text, before_pos) in &before {
        let matched = after
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, p)| *p)
            .expect("回车后原有行号应仍在");
        assert_eq!(
            before_pos.x, matched.x,
            "行号 {text} 的 x 坐标在回车后发生了移动（行号栏宽度不稳定）"
        );
    }
}

/// 一帧里画出来的行号数字：锚点、字形矩形（相对锚点）、所在裁剪区左缘。
#[derive(Clone, Debug, PartialEq)]
struct NumberShape {
    text: String,
    pos: egui::Pos2,
    rect: egui::Rect,
    clip_left: f32,
}

/// 源码模式跑一帧，返回画出的行号。`time` 必须逐帧推进：egui 的滚轮是跨帧
/// 平滑的，时间不动画面就永远滚不到位。
#[allow(clippy::too_many_arguments)]
fn source_frame_numbers(
    doc: &mut Document,
    state: &mut EditorState,
    ctx: &egui::Context,
    theme: &Theme,
    images: &mut ImageCache,
    actions: &mut Vec<crate::render::Action>,
    events: Vec<egui::Event>,
    time: f64,
) -> Vec<NumberShape> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        time: Some(time),
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
                theme,
                &c,
                images,
                None,
                &[],
                true,
                false,
                false,
                true,
                1,
                actions,
            );
        });
    });
    let mut nums: Vec<NumberShape> = Vec::new();
    for cs in &out.shapes {
        if let egui::Shape::Text(t) = &cs.shape {
            let s = t.galley.text();
            if !s.is_empty() && s.chars().all(|ch| ch.is_ascii_digit()) {
                nums.push(NumberShape {
                    text: s.to_string(),
                    pos: t.pos,
                    rect: t.galley.rect,
                    clip_left: cs.clip_rect.left(),
                });
            }
        }
    }
    out.textures_delta.clear();
    nums
}

/// 一路向下滚到文末，等画面停稳后返回最后可见的那批行号。
/// 虚拟化之后 4 位数行号只有滚到文末才画得出来，测试要看它就得真滚过去。
fn scroll_source_to_bottom(
    doc: &mut Document,
    state: &mut EditorState,
    ctx: &egui::Context,
    theme: &Theme,
    images: &mut ImageCache,
    actions: &mut Vec<crate::render::Action>,
    time: &mut f64,
) -> Vec<NumberShape> {
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        // 负号 = 内容上移 = 往下翻；一次给够，余量由滚动区自己钳住。
        delta: egui::vec2(0.0, -50_000.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Default::default(),
    };
    *time += 1.0 / 60.0;
    let mut prev = source_frame_numbers(
        doc,
        state,
        ctx,
        theme,
        images,
        actions,
        vec![egui::Event::PointerMoved(egui::pos2(400.0, 300.0)), wheel],
        *time,
    );
    for _ in 0..600 {
        *time += 1.0 / 60.0;
        let next = source_frame_numbers(doc, state, ctx, theme, images, actions, vec![], *time);
        if next == prev && !ctx.input(|i| i.is_scrolling()) {
            return next;
        }
        prev = next;
    }
    panic!("滚动 600 帧后画面还没稳定");
}

/// 跨过 999 行的边界表现：行号栏宽度固定，不随位数 3→4 变化，
/// 所有行号 x 坐标保持不变（不再有"变宽一整个数字位"的跳动）。
/// 行号虚拟化后一帧只画视口内的行，4 位数那一档靠滚到文末来验。
#[test]
fn source_mode_gutter_stays_fixed_past_999_lines() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    // 999 行文档（位数 3）
    let src: String = (1..=999)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut doc = Document::new(&src, None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();
    let mut t = 0.0f64;
    let mut frame = |events: Vec<egui::Event>, t: &mut f64| {
        *t += 1.0 / 60.0;
        source_frame_numbers(
            &mut doc,
            &mut state,
            &ctx,
            &theme,
            &mut images,
            &mut actions,
            events,
            *t,
        )
    };

    let before = frame(vec![], &mut t);
    assert!(!before.is_empty(), "首屏该有行号");
    assert!(
        before.len() < 200,
        "虚拟化后一帧只该画视口内的行号，实际 {}",
        before.len()
    );

    // 聚焦后回车：999 → 1000 行，跨过位数边界
    let click = egui::pos2(400.0, 100.0);
    frame(
        vec![
            egui::Event::PointerMoved(click),
            egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut t,
    );
    frame(
        vec![egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut t,
    );
    let key = |pressed: bool| -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        }]
    };
    frame(key(true), &mut t);
    frame(key(false), &mut t);

    let after = frame(vec![], &mut t);
    assert!(
        after.len() < 200,
        "回车后一帧仍只该画视口内的行号，实际 {}",
        after.len()
    );
    // 行号栏宽度固定：跨过 999 行（3→4 位）时所有共有行号 x 坐标不变。
    for b in &before {
        let matched = after
            .iter()
            .find(|n| n.text == b.text)
            .unwrap_or_else(|| panic!("回车后原有行号 {} 不见了", b.text));
        let shift = matched.pos.x - b.pos.x;
        assert!(
            shift.abs() < 0.5,
            "行号 {} 右移 {shift:.2}px，行号栏宽度应固定不随位数变化",
            b.text
        );
    }

    // 文末：4 位数行号与同一屏的 3 位数、与首屏行号共用一个右对齐锚点。
    let bottom = scroll_source_to_bottom(
        &mut doc,
        &mut state,
        &ctx,
        &theme,
        &mut images,
        &mut actions,
        &mut t,
    );
    assert!(
        bottom.iter().any(|n| n.text == "1000"),
        "该滚到最后一行，实际 {:?}",
        bottom.iter().map(|n| &n.text).collect::<Vec<_>>()
    );
    // 行号右对齐到 gutter 内一条固定竖线：`Painter::text` 存的锚点是字形
    // 矩形左上角，位数变多时左缘自然左移，所以「同一列」要比右缘。
    let right = |n: &NumberShape| n.pos.x + n.rect.max.x;
    let four = bottom
        .iter()
        .find(|n| n.text.len() == 4)
        .expect("文末有 4 位数行号");
    let three = bottom
        .iter()
        .find(|n| n.text.len() == 3)
        .expect("文末有 3 位数行号");
    assert!(
        (right(four) - right(three)).abs() < 0.5,
        "位数 3→4 把行号右缘挪了 {:.2}px",
        right(four) - right(three)
    );
    assert!(
        (right(four) - right(&after[0])).abs() < 0.5,
        "文末行号右缘与首屏不一致：{:.2} vs {:.2}",
        right(four),
        right(&after[0])
    );
}

/// 行号数字必须完整显示：字形矩形左缘不得越过滚动区裁剪区左缘，
/// 否则数字被裁得只剩一部分。首屏（1〜2 位）与文末（3→4 位最宽一档）都查。
#[test]
fn source_mode_line_numbers_not_clipped() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let src: String = (1..=999)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut doc = Document::new(&src, None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();
    let mut t = 0.0f64;
    let check = |nums: &[NumberShape], where_: &str| {
        for n in nums {
            // galley 矩形是相对锚点的，屏幕左缘 = pos.x + rect.left()
            let screen_left = n.pos.x + n.rect.left();
            assert!(
                screen_left >= n.clip_left - 0.5,
                "{where_}的行号 {} 左缘 {screen_left:.1} 越过裁剪区左缘 {:.1}，被裁剪",
                n.text,
                n.clip_left
            );
        }
    };

    t += 1.0 / 60.0;
    let nums = source_frame_numbers(
        &mut doc,
        &mut state,
        &ctx,
        &theme,
        &mut images,
        &mut actions,
        vec![],
        t,
    );
    check(&nums, "首屏");
    assert!(nums.len() >= 10, "视口内该画出一批行号，实际 {}", nums.len());
    assert!(
        nums.len() < 200,
        "一帧不该再画完整篇 999 个行号，实际 {}",
        nums.len()
    );

    let bottom = scroll_source_to_bottom(
        &mut doc,
        &mut state,
        &ctx,
        &theme,
        &mut images,
        &mut actions,
        &mut t,
    );
    check(&bottom, "文末");
    assert!(
        bottom.iter().any(|n| n.text == "999"),
        "该滚到最后一行，实际 {:?}",
        bottom.iter().map(|n| &n.text).collect::<Vec<_>>()
    );
}

/// 代码块编辑态与渲染态的按钮行位置必须一致：
/// 点击进入编辑后，"复制"按钮相对盒子顶部的偏移不得变化
/// （否则按钮看起来"跳到盒子右上方外"、整块跳动）。
/// 注意：文档前面放一个段落，代码块保持渲染态，才能对比两态。
#[test]
fn code_block_edit_toolbar_stays_inside_box() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("前言\n\n```rust\nfn main() {}\n```", None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();

    // 抓取信息：(盒子矩形, "复制"文本锚点位置)
    let mut run = |doc: &mut Document,
                   state: &mut EditorState,
                   events: Vec<egui::Event>|
     -> Option<(egui::Rect, egui::Pos2)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let mut box_rect: Option<egui::Rect> = None;
        let mut copy_pos: Option<egui::Pos2> = None;
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
        for cs in &out.shapes {
            match &cs.shape {
                // 盒子 = 最大的 code_bg 填充矩形
                egui::Shape::Rect(r) if r.fill == theme.code_bg && r.rect.width() > 200.0 => {
                    let area = r.rect.width() * r.rect.height();
                    let bigger = box_rect.is_none_or(|b| area > b.width() * b.height());
                    if bigger {
                        box_rect = Some(r.rect);
                    }
                }
                egui::Shape::Text(t) if t.galley.text() == "复制" => {
                    copy_pos = Some(t.pos);
                }
                _ => {}
            }
        }
        box_rect.zip(copy_pos)
    };

    let rel = |r: &Option<(egui::Rect, egui::Pos2)>| -> f32 {
        let (box_r, copy) = r.as_ref().expect("应渲染出代码块盒子与复制按钮");
        copy.y - box_r.top()
    };

    // 渲染态：按钮相对盒子顶部的偏移（active=0 指向段落，代码块渲染）
    let before = run(&mut doc, &mut state, vec![]);
    let off_before = rel(&before);

    // 点击代码块（在段落下方）进入编辑态
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
    );
    let after = run(&mut doc, &mut state, vec![]);
    let off_after = rel(&after);
    assert!(
        (off_before - off_after).abs() < 2.0,
        "进入编辑后按钮偏移变了：渲染态 {off_before:.1}px → 编辑态 {off_after:.1}px，按钮跳动了"
    );
    // 盒子顶部不得明显移动（TextEdit 与 Label 的响应矩形有 ~2px 的固有差，
    // 肉眼不可见；之前是行高不一致导致的 5px，已修复）
    let (b1, _) = before.as_ref().expect("渲染态盒子");
    let (b2, _) = after.as_ref().expect("编辑态盒子");
    assert!(
        (b1.top() - b2.top()).abs() < 3.0,
        "盒子顶部移动了：渲染态 {:.1} → 编辑态 {:.1}",
        b1.top(),
        b2.top()
    );
    // 编辑态按钮必须在盒子内部（右缘不超过盒子右缘）
    let (b2, copy2) = after.as_ref().expect("编辑态盒子");
    assert!(
        copy2.y > b2.top() && copy2.y < b2.bottom() && copy2.x < b2.right(),
        "编辑态按钮 ({:.1},{:.1}) 跑到盒子外 [{:.1},{:.1} {}x{}]",
        copy2.x,
        copy2.y,
        b2.min.x,
        b2.min.y,
        b2.width(),
        b2.height()
    );
}
