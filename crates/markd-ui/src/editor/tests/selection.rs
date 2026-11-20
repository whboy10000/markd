use super::*;

/// 跨块选区：在渲染态块上按住拖到另一块 → 选区建立；
/// 退格 → 两段（含中间空行）合并成一段。
#[test]
fn cross_block_drag_select_and_delete() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    assert_eq!(doc.blocks.len(), 5, "3 段 + 2 空行 = 5 块");
    let mut state = EditorState::default(); // active=0 段落编辑态
    let ctx = egui::Context::default();

    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    // 在"第二段"（渲染态块）按下并拖到"第三段"
    let start = egui::pos2(400.0, 60.0);
    let end = egui::pos2(400.0, 100.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    println!("[帧2 按下] actions={actions:?} cross={:?}", state.cross_sel);
    actions.clear();
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerMoved(end)],
    );
    println!("[帧3 移动] actions={actions:?} cross={:?}", state.cross_sel);
    actions.clear();
    // egui 的 drag_started 晚一帧生效：再保持一帧按下状态（指针停在块 4），
    // 让终点块以「选区进行中 + 指针压在本块上」接棒上报选区终点。
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerMoved(end)],
    );
    println!("[帧4 保持] actions={actions:?} cross={:?}", state.cross_sel);
    actions.clear();
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let sel = state
        .cross_sel
        .expect("跨块拖选后应建立选区，实际 None——检查渲染态块的拖选感应");
    let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert!(
        (lo_b, hi_b) == (2, 4),
        "选区应覆盖第 2 块到第 4 块，实际 {sel:?}"
    );

    // 退格：删除选区 → 两段合并
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(state.cross_sel.is_none(), "删除后选区应清除");
    assert_eq!(
        doc.source(),
        "第一段\n\n第二段",
        "退格应把选中的第二、三段合并成一段"
    );
}

/// 跨块拖选必须能跨过代码块/图表这类「非文本块」：
/// 拖到它上面时选区端点落在该块内（向下取块尾、向上取块首），
/// 并且整块画出选区底色，而不是像以前一样在中间断掉。
#[test]
fn drag_selection_extends_into_code_block() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let text = "alpha\n\n`code` beta\n\n```rust\nfn main() {}\n```\n\ngamma";
    let mut doc = Document::new(text, None);
    // alpha(0) / 空行(1) / 行内代码段(2) / 空行(3) / 代码块(4) / 空行(5) / gamma(6)
    assert_eq!(doc.blocks.len(), 7);
    let code_idx = 4usize;
    let code_len = doc.blocks[code_idx].plain().chars().count();
    let mut state = EditorState::default();
    state.active = 2;
    let ctx = egui::Context::default();
    let mut frame = |events: Vec<egui::Event>,
                     doc: &mut Document,
                     state: &mut EditorState,
                     actions: &mut Vec<crate::render::Action>|
     -> Vec<egui::epaint::ClippedShape> {
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
                    ui,
                    doc,
                    state,
                    &theme,
                    &ctx,
                    &mut images,
                    None,
                    &outline,
                    false,
                    false,
                    false,
                    false,
                    1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
        out.shapes
    };
    for _ in 0..3 {
        let _ = frame(vec![], &mut doc, &mut state, &mut actions);
    }
    let shapes = frame(vec![], &mut doc, &mut state, &mut actions);
    let find = |name: &str| -> egui::Pos2 {
        for cs in &shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                if t.galley.text().starts_with(name) {
                    return t.pos;
                }
            }
        }
        panic!("{name} 未渲染");
    };
    let alpha = find("alpha");
    let code = find("fn main");
    let gamma = find("gamma");
    let press = |pos: egui::Pos2| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    };

    // 向下拖：从 alpha 拖到代码块上，端点应落在代码块末尾。
    let _ = frame(
        press(alpha + egui::vec2(3.0, 8.0)),
        &mut doc,
        &mut state,
        &mut actions,
    );
    let on_code = code + egui::vec2(60.0, 8.0);
    let _ = frame(
        vec![egui::Event::PointerMoved(on_code)],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let _ = frame(
        vec![egui::Event::PointerMoved(on_code)],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let _ = frame(
        vec![egui::Event::PointerButton {
            pos: on_code,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let sel = state.cross_sel.expect("向下拖到代码块应建立选区");
    assert_eq!(
        sel,
        (0, 0, code_idx, code_len),
        "选区终点应落在代码块末尾，实际 {sel:?}"
    );

    // 代码块被选区覆盖时：逐行叠半透明选区色，而不是把整块（含功能栏）糊一层。
    // 选区底色包在 Shape::Vec 里，要递归展开才看得到。
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
    let shapes = frame(vec![], &mut doc, &mut state, &mut actions);
    let code_center = code + egui::vec2(60.0, 8.0);
    let sel_fill = crate::render::selection_overlay(&theme);
    let mut hi: Vec<egui::Rect> = Vec::new();
    for cs in &shapes {
        walk(&cs.shape, sel_fill, &mut hi);
    }
    assert!(
        hi.iter().any(|r| r.contains(code_center)),
        "代码块应画出选区底色，实际高亮 {hi:?}"
    );
    // 逐行铺色：任何一块选区色都不该横跨代码首行的顶边——整块叠色会从面板
    // 顶（功能栏之上）糊下来，把「折叠/删除/复制」那一行也染蓝。
    assert!(
        hi.iter()
            .all(|r| r.bottom() <= code.y + 1.0 || r.top() >= code.y - 1.0),
        "选区底色不该盖住代码块功能栏：{hi:?}（代码首行顶边 {}）",
        code.y
    );

    // 向上拖：从 gamma 拖到代码块上，起点应落在代码块开头。
    state.cross_sel = None;
    let _ = frame(vec![], &mut doc, &mut state, &mut actions);
    let _ = frame(
        press(gamma + egui::vec2(3.0, 8.0)),
        &mut doc,
        &mut state,
        &mut actions,
    );
    let _ = frame(
        vec![egui::Event::PointerMoved(on_code)],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let _ = frame(
        vec![egui::Event::PointerButton {
            pos: on_code,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let sel = state.cross_sel.expect("向上拖到代码块应建立选区");
    let (s, sc, e, _) = (sel.0.min(sel.2), sel.1, sel.0.max(sel.2), sel.3);
    assert_eq!(
        (s, sc, e),
        (code_idx, 0, 6),
        "选区起点应落在代码块开头，实际 {sel:?}"
    );
}

/// 视口外的按下（工具条/菜单）不能把指针钳进视口当成点了最上面的块：
/// 否则拖选好正文后点「引用」，选区会被 (0, n) 的零长度选区顶掉，
/// 格式加到第一段（用户反馈的引用 bug）。
#[test]
fn press_outside_viewport_does_not_clobber_cross_sel() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default(); // active = 0：第一段编辑态，其余渲染态
    let ctx = egui::Context::default();
    let mut frame = |events: Vec<egui::Event>,
                     doc: &mut Document,
                     state: &mut EditorState|
     -> Vec<egui::epaint::ClippedShape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let outline = doc.outline();
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
                    &outline,
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
    let _ = frame(vec![], &mut doc, &mut state);
    // 已有拖选：第二段整段选中
    state.cross_sel = Some((2, 0, 2, 3));
    // 在视口上方（工具条位置）按下并松开
    let outside = egui::pos2(300.0, -20.0);
    let _ = frame(
        vec![
            egui::Event::PointerMoved(outside),
            egui::Event::PointerButton {
                pos: outside,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut doc,
        &mut state,
    );
    let _ = frame(
        vec![egui::Event::PointerButton {
            pos: outside,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut doc,
        &mut state,
    );
    assert_eq!(
        state.cross_sel,
        Some((2, 0, 2, 3)),
        "工具条上的点击不应覆盖已有跨块选区"
    );
    // 没有选区时，视口外按下也不该凭空建立选区
    state.cross_sel = None;
    let _ = frame(
        vec![
            egui::Event::PointerMoved(outside),
            egui::Event::PointerButton {
                pos: outside,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: outside,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ],
        &mut doc,
        &mut state,
    );
    assert!(
        state.cross_sel.is_none(),
        "视口外的点击不应建立跨块选区，实际 {:?}",
        state.cross_sel
    );
}

/// 拖到视口外要能自动滚动，并且选区跟着一直延伸到底：
/// 指针停住不动时系统不再送鼠标事件，必须自己请求重绘；
/// 越界点也要钳到视口边缘才能持续命中块。
#[test]
fn drag_beyond_viewport_autoscrolls_and_extends_selection() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let text = (0..60)
        .map(|i| format!("段落{i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut doc = Document::new(&text, None);
    let mut state = EditorState::default();
    state.active = 0;
    let ctx = egui::Context::default();
    let mut now = 0.0f64;
    let mut frame = |events: Vec<egui::Event>,
                     doc: &mut Document,
                     state: &mut EditorState,
                     actions: &mut Vec<crate::render::Action>|
     -> Vec<egui::epaint::ClippedShape> {
        // 故意用较矮的窗口，让视口很快被段落填满。
        // 时间必须往前走：ScrollArea 的滚动位移按动画时间轴推进。
        now += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 300.0),
            )),
            time: Some(now),
            predicted_dt: 1.0 / 60.0,
            events,
            ..Default::default()
        };
        let outline = doc.outline();
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
                    &outline,
                    false,
                    false,
                    false,
                    false,
                    1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
        out.shapes
    };
    for _ in 0..3 {
        let _ = frame(vec![], &mut doc, &mut state, &mut actions);
    }
    // 在第二个段落（渲染态）按下。
    let start = egui::pos2(300.0, 62.0);
    let _ = frame(
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut doc,
        &mut state,
        &mut actions,
    );
    // 指针甩到视口下方之外并保持按住：靠自动滚动持续延伸选区。
    let outside = egui::pos2(300.0, 900.0);
    let _ = frame(
        vec![egui::Event::PointerMoved(outside)],
        &mut doc,
        &mut state,
        &mut actions,
    );
    let early = state.cross_sel.expect("越界拖选后应仍有选区");
    let early_end = early.0.max(early.2);
    for _ in 0..40 {
        let _ = frame(
            vec![egui::Event::PointerMoved(outside)],
            &mut doc,
            &mut state,
            &mut actions,
        );
    }
    let sel = state.cross_sel.expect("越界拖选后应仍有选区");
    let (start_b, end_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!(start_b, 2, "锚点应保持在按下的段落，实际 {sel:?}");
    assert!(
        end_b > early_end + 3,
        "选区应随自动滚动继续延伸：首帧 {early_end} → 最终 {end_b}，实际 {sel:?}"
    );
}

/// 双击选词 / 三击选整段 / Shift+点击 三个 Action 聚合成 cross_sel。
#[test]
fn mouse_selection_actions_set_cross_sel() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let ctx = egui::Context::default();

    // 双击选词
    {
        let mut doc = Document::new("hello world\n\n第二块", None);
        let mut state = EditorState::default();
        let mut actions = vec![crate::render::Action::WordSel(0, 0, 5)];
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        assert_eq!(state.cross_sel, Some((0, 0, 0, 5)), "双击应选中词区间");
    }
    // 三击选整段
    {
        let mut doc = Document::new("整段文字\n\n第二块", None);
        let mut state = EditorState::default();
        let mut actions = vec![crate::render::Action::BlockSel(0)];
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        assert_eq!(
            state.cross_sel,
            Some((0, 0, 0, "整段文字".chars().count())),
            "三击应选中整段"
        );
    }
    // Shift+点击：无选区时从光标锚点到点击处
    {
        let mut doc = Document::new("第一段\n\n第二段", None);
        let mut state = EditorState::default();
        state.active = 0;
        state.last_cursor = 2;
        let mut actions = vec![crate::render::Action::ShiftClick(2, 3)];
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
        assert_eq!(
            state.cross_sel,
            Some((0, 2, 2, 3)),
            "Shift+点击应从光标选到点击处"
        );
    }
}

/// 回归：反向（向上）拖选不能漏块。
/// 渲染按块 idx 升序进行；此前起始块的 `resp.dragged()` 也上报 CrossDrag，
/// 向上拖时它晚于上方指针块被处理，把选区终点覆盖回起始块——选区只剩
/// 起始一块，上方段落看上去没被选中、也复制不到。
#[test]
fn cross_block_drag_upward_keeps_endpoint() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    let ctx = egui::Context::default();

    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    // 在「第三段」（块 4, y=100）按下，向上拖到「第二段」（块 2, y=60）。
    let start = egui::pos2(400.0, 100.0);
    let end = egui::pos2(400.0, 60.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    actions.clear();
    for _ in 0..2 {
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(end)],
        );
        actions.clear();
    }
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let sel = state.cross_sel.expect("向上拖选后应建立选区");
    let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!(
        (lo_b, hi_b),
        (2, 4),
        "向上拖选应覆盖第 2 到第 4 块，实际 {sel:?}"
    );
}

/// 回归：光标所在的空行（空活动块）按下向下拖，必须能选中下方块。
/// 活动块用 TextEdit 渲染、没有 text_response 的 CrossDrag 路径，
/// 此前从空行起拖锚定不了选区，表现是「什么也选中不了」。
#[test]
fn cross_drag_from_empty_active_block_selects_downward() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    state.active = 1; // 块 1 是空行（Blank），且为活动块
    let ctx = egui::Context::default();

    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    // 在空行（约 y=40）按下，向下拖到「第三段」（块 4, y=100）。
    let start = egui::pos2(400.0, 40.0);
    let end = egui::pos2(400.0, 100.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    actions.clear();
    for _ in 0..2 {
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(end)],
        );
        actions.clear();
    }
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let sel = state
        .cross_sel
        .expect("从空行起拖应建立跨块选区，实际 None——检查空活动块的锚定");
    let (lo_b, hi_b) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!(
        (lo_b, hi_b),
        (1, 4),
        "从空行向下拖选应覆盖第 1 到第 4 块，实际 {sel:?}"
    );
}

/// 普通点击空行（不拖动）不应残留跨块选区。
#[test]
fn click_empty_active_block_leaves_no_selection() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    state.active = 1;
    let ctx = egui::Context::default();

    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );
    let p = egui::pos2(400.0, 40.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    actions.clear();
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        state.cross_sel.is_none(),
        "普通点击空行不应残留选区，实际 {:?}",
        state.cross_sel
    );
}

/// 回归：从「已激活（编辑态）的块」开始拖选，必须能建立跨块选区。
/// 用户先点进某一段（变编辑态），再从这段往下拖时，TextEdit 吃掉了
/// 按下事件，跨块选区根本建立不了 —— 表现为「有些文字能拖选、有些不能」。
#[test]
fn drag_from_active_block_extends_across_blocks() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    state.active = 0; // 段 0 处于编辑态（TextEdit）
    state.focus_req = Some(0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    let start = egui::pos2(400.0, 20.0);
    let mid = egui::pos2(400.0, 60.0);
    let end = egui::pos2(400.0, 100.0);
    // 在编辑态块上按下
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    actions.clear();
    for p in [mid, end, end] {
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(p)],
        );
        actions.clear();
    }
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let sel = state.cross_sel.expect("从编辑态块起拖应建立跨块选区");
    let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!((lo, hi), (0, 4), "选区应覆盖第 0~4 块，实际 {sel:?}");
}

/// 回归：拖选的终点落在「编辑态块」上时，选区必须继续延伸到该块。
/// 编辑态块渲染成 TextEdit、不走 text_response，之前终点会停在上一个块。
#[test]
fn drag_ending_on_active_block_extends_selection() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let mut doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    let mut state = EditorState::default();
    state.active = 4; // 最后一块是编辑态
    state.focus_req = Some(4);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    let start = egui::pos2(400.0, 20.0);
    let end = egui::pos2(400.0, 100.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    actions.clear();
    for p in [end, end] {
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerMoved(p)],
        );
        actions.clear();
    }
    let sel = state.cross_sel.expect("向下拖应建立跨块选区");
    let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!(
        (lo, hi),
        (0, 4),
        "选区的终点应延伸到编辑态块 4，实际 {sel:?}"
    );
}

/// 回归：编辑态（活动块）内拖动文字必须形成选区。
///
/// 标题（大字号、无自定义 layouter）与含行内代码/加粗等隐藏标记的段落，
/// 此前只依赖 TextEdit 原生选区，在这类 galley 上常常只定位光标、拖不出
/// 高亮（用户反馈「点标题能动光标，拖却选不中」）。现在统一用 cross_sel
/// 承载活动块的拖动选区，标题与行内代码都应能选中使用所见文本坐标的区间。
#[test]
fn drag_selection_in_active_heading_and_inline_code_sets_cross_sel() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();

    // 跑一帧并返回所有文本 shape 的落点，按内容定位（不写死坐标）。
    let locate = |doc: &mut Document,
                  state: &mut EditorState,
                  images: &mut ImageCache,
                  actions: &mut Vec<crate::render::Action>,
                  needle: &str|
     -> Option<egui::Pos2> {
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
                    ui, doc, state, &theme, &c, images, None, &[], false, false, false, true, 1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape
                && t.galley.text().contains(needle)
            {
                return Some(egui::pos2(
                    t.pos.x + 5.0,
                    t.pos.y + t.galley.rect.center().y,
                ));
            }
        }
        None
    };

    // (名称, 文档, 目标块, needle)
    let cases: Vec<(&str, &str, usize, &str)> = vec![
        ("标题", "# 标题测试内容\n\n普通段落", 0, "标题测试"),
        (
            "行内代码",
            "带有 `inline_code` 和 **粗体** 的段落\n\n第二段文字",
            0,
            "带有",
        ),
        (
            "特殊符号",
            "含 ~~删除线~~ 与 [链接](http://x) 的段落\n\n第二段文字",
            0,
            "含",
        ),
    ];

    for (name, src, target, needle) in cases {
        let mut doc = Document::new(src, None);
        let mut state = EditorState::default();
        state.active = target; // 目标块为活动块（编辑态）
        let mut images = ImageCache::new();
        let mut actions = Vec::new();
        let p0 = locate(&mut doc, &mut state, &mut images, &mut actions, needle)
            .unwrap_or_else(|| panic!("{name}：未找到文本 {needle}"));

        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![
                egui::Event::PointerMoved(p0),
                egui::Event::PointerButton {
                    pos: p0,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        actions.clear();
        // 细粒度移动数帧，模拟真实鼠标；decide drag 后应建立 cross_sel。
        let mut x = p0.x;
        for _ in 0..6 {
            x += 12.0;
            run_editor_frame(
                &ctx,
                &mut doc,
                &mut state,
                &theme,
                &mut images,
                &mut actions,
                vec![egui::Event::PointerMoved(egui::pos2(x, p0.y))],
            );
            actions.clear();
        }
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(x, p0.y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );

        let sel = state.cross_sel.unwrap_or_else(|| {
            panic!("{name}：编辑态内拖动应建立跨块选区，实际 cross_sel=None")
        });
        assert_eq!(
            sel.0, target,
            "{name}：选区起点块应为 {target}，实际 {sel:?}"
        );
        let text = super::super::selection::cross_sel_text(&doc, state.cross_sel)
            .expect("{name}：选区应产生可见文本");
        assert!(
            !text.trim().is_empty(),
            "{name}：选区文本不应为空，实际 {text:?}"
        );
    }
}

/// 回归：新按下必须重新锁定锚点，不能沿用上一次点击/拖拽的旧锚点。
///
/// 用户反馈「混合文档（标题/引用/普通段落/行内代码）拖拽时出现时不出现、
/// 选区乱跑」。根因：按下帧若因命中判定失败（点在块间距/视口边缘、或刚切换
/// 编辑态导致布局位移）没锚定，兜底会用上一次点击的 last_cursor / 残留
/// cross_sel 当锚点，选区就落到错误的块。修复后：新按下先清旧 cross_sel，
/// 且渲染态锚定用 press_origin + 容差命中，必定锁到被按下的块。
#[test]
fn drag_after_click_anchors_at_pressed_block_not_stale_cursor() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();

    // 跑一帧，返回 (文本, 文本pos, galley矩形) 供定位
    let locate = |doc: &mut Document,
                  state: &mut EditorState,
                  images: &mut ImageCache,
                  actions: &mut Vec<crate::render::Action>| {
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
                    ui, doc, state, &theme, &c, images, None, &[], false, false, false, true, 1,
                    actions,
                );
            });
        });
        out.textures_delta.clear();
        let mut v = Vec::new();
        for cs in &out.shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                v.push((t.galley.text().to_string(), t.pos, t.galley.rect));
            }
        }
        v
    };

    let doc_src = "# 标题一\n\n> 引用块内容在这里有一些文字\n\n普通段落带有 `inline_code` 和 **粗体** 还有更多文字用于拖动测试\n\n最后一段普通文字内容";
    let mut doc = Document::new(doc_src, None);
    let mut state = EditorState::default();
    state.active = 0;
    let mut images = ImageCache::new();
    let mut actions = Vec::new();

    // 1) 先点击引用块，让 last_cursor / active 落在引用块（污染源）
    let texts = locate(&mut doc, &mut state, &mut images, &mut actions);
    let quote_p = texts
        .iter()
        .find(|(t, _, _)| t.contains("引用块内容"))
        .map(|(_, p, r)| egui::pos2(p.x + 5.0, p.y + r.center().y))
        .expect("引用块文本");
    run_editor_frame(
        &ctx, &mut doc, &mut state, &theme, &mut images, &mut actions,
        vec![
            egui::Event::PointerMoved(quote_p),
            egui::Event::PointerButton { pos: quote_p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        ],
    );
    run_editor_frame(
        &ctx, &mut doc, &mut state, &theme, &mut images, &mut actions,
        vec![egui::Event::PointerButton { pos: quote_p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
    );
    for _ in 0..2 {
        run_editor_frame(&ctx, &mut doc, &mut state, &theme, &mut images, &mut actions, vec![]);
    }
    assert_eq!(state.active, 2, "点击后应激活引用块（污染源就位）");

    // 2) 布局已因引用块进入编辑态而改变，重新取标题坐标。
    let texts = locate(&mut doc, &mut state, &mut images, &mut actions);
    let (code_x, code_abs_top) = texts
        .iter()
        .find(|(t, _, _)| t.contains("普通段落带有"))
        .map(|(_, p, r)| (p.x, p.y + r.top()))
        .expect("行内代码段落文本");
    let last_p = texts
        .iter()
        .find(|(t, _, _)| t.contains("最后一段"))
        .map(|(_, p, r)| egui::pos2(p.x + 5.0, p.y + r.center().y))
        .expect("末段文本");

    // 从行内代码段落文字上缘稍微往上 2px（块间距内，旧严格命中会漏）按下。
    let press = egui::pos2(code_x + 5.0, code_abs_top - 2.0);
    actions.clear();
    run_editor_frame(
        &ctx, &mut doc, &mut state, &theme, &mut images, &mut actions,
        vec![
            egui::Event::PointerMoved(press),
            egui::Event::PointerButton { pos: press, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        ],
    );
    actions.clear();
    // 慢速向下拖到末段
    let steps = 10;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let p = egui::pos2(press.x + (last_p.x - press.x) * t, press.y + (last_p.y - press.y) * t);
        run_editor_frame(&ctx, &mut doc, &mut state, &theme, &mut images, &mut actions, vec![egui::Event::PointerMoved(p)]);
        actions.clear();
    }
    run_editor_frame(
        &ctx, &mut doc, &mut state, &theme, &mut images, &mut actions,
        vec![egui::Event::PointerButton { pos: last_p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }],
    );

    let sel = state.cross_sel.expect("拖动应建立选区");
    assert_eq!(
        sel.0, 4,
        "锚点应落在被按下的行内代码段落(块4)，而不是上次点击的引用块(2)；实际 {sel:?}"
    );
    assert!(sel.2 > sel.0, "终点应在该段落下方的块；实际 {sel:?}");
}
