use super::*;

/// 新建空白文档后，无论点多少次、点哪块空白，光标都应回到第一行并保持闪烁。
/// 之前只在块自身矩形内处理点击，点正文下方没有任何控件命中，焦点就丢了。
#[allow(deprecated)]
#[test]
fn blank_click_refocuses_first_line() {
    let mut doc = Document::new("", None);
    let mut state = EditorState::default();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    // 第一帧建立布局
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    // 正文下方、左右页边距、页面中部：反复点，每次都必须落回第一行。
    for (i, pos) in [
        egui::pos2(400.0, 800.0),
        egui::pos2(20.0, 400.0),
        egui::pos2(780.0, 400.0),
        egui::pos2(400.0, 300.0),
        egui::pos2(400.0, 800.0),
    ]
    .into_iter()
    .enumerate()
    {
        click_blank(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            pos,
        );
        assert_eq!(state.active, 0, "第 {} 次点空白后应回到第一个块", i + 1);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new(("block_edit", 1, 0))),
            "第 {} 次点空白后第一行的 TextEdit 必须持有键盘焦点",
            i + 1
        );
        assert_eq!(
            state.last_cursor,
            0,
            "第 {} 次点空白后光标应停在第一行行首",
            i + 1
        );
    }
}

/// 真实面板结构下点空白：焦点必须回到第一行（复现用户报告的 bug）。
#[test]
fn blank_click_refocuses_in_app_like_layout() {
    let mut doc = Document::new("", None);
    let mut state = EditorState::default();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    run_app_like_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );
    let pos = egui::pos2(600.0, 500.0);
    run_app_like_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
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
    run_app_like_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    // 给「请求焦点 + 设置光标」两帧的落地时间
    for _ in 0..2 {
        run_app_like_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            vec![],
        );
    }
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new(("block_edit", 1, 0))),
        "真实面板结构下点空白也必须把焦点还给第一行"
    );
}

/// 多块文档：无论点正文下方还是页边空白，光标都落到最后一行（最后一块末尾）。
#[allow(deprecated)]
#[test]
fn blank_click_below_content_focuses_last_block_end() {
    let mut doc = Document::new("a\n\nb", None);
    let mut state = EditorState::default();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
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

    // 点正文下方、点正文左侧的空白，结果都一样：光标落在最后一行（最后一块末尾）。
    let last = doc.blocks.len() - 1;
    for pos in [egui::pos2(400.0, 800.0), egui::pos2(20.0, 300.0)] {
        click_blank(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            pos,
        );
        assert_eq!(state.active, last, "点空白 {pos:?} 应回到最后一块");
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new(("block_edit", 1, last))),
            "点空白 {pos:?} 后最后一块的 TextEdit 必须持有键盘焦点"
        );
        assert_eq!(
            state.last_cursor, 1,
            "点空白 {pos:?} 光标应停在最后一块末尾"
        );
    }
}

/// 源码模式：点编辑框外的空白，光标跳到文档末尾（最后一行）并重新聚焦。
#[test]
fn source_mode_blank_click_focuses_document_end() {
    let mut doc = Document::new("a\nb", None);
    let mut state = EditorState::default();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let outline = doc.outline();
    let mut src_frame = |events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let outline = &outline;
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let _ = show_editor(
                    ui,
                    &mut doc,
                    &mut state,
                    &theme,
                    &ctx,
                    &mut images,
                    None,
                    outline,
                    true,
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
    src_frame(vec![]);
    let pos = egui::pos2(400.0, 800.0);
    src_frame(vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    src_frame(vec![egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    for _ in 0..2 {
        src_frame(vec![]);
    }
    let id = egui::Id::new(("source_editor", 1));
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(id),
        "源码模式点空白后源码编辑器必须持有键盘焦点"
    );
    let caret = egui::TextEdit::load_state(&ctx, id)
        .and_then(|ts| ts.cursor.char_range())
        .map(|r| r.primary.index.0);
    assert_eq!(caret, Some(3), "光标应停在文档末尾（最后一行）");
}

/// 终端持有键盘焦点时（read_only），编辑区不得处理回车：
/// 否则在终端里按回车，编辑区的段落会跟着换行。
#[test]
fn enter_is_ignored_when_read_only() {
    let mut doc = Document::new("第一段\n\n第二段", None);
    let blocks_before = doc.blocks.len();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut state = EditorState::default();
    state.active = doc.blocks.len() - 1;
    state.read_only = true;
    state.focus_req = Some(state.active);

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
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert_eq!(
        doc.blocks.len(),
        blocks_before,
        "read_only 时回车不应拆分块"
    );
}

/// 行尾空白点击链路验证：
/// 1) 编辑态直接点击行尾空白 —— TextEdit 应把光标钳到该行最后一个字符
///    之后，且持有焦点（光标闪烁的前提）；
/// 2) 渲染态点击非活动块行尾空白 —— 产生 Caret 动作进入编辑态后，
///    pending_caret 传入的超宽 x 也要钳到行末。
#[test]
fn click_row_blank_puts_caret_at_line_end_with_focus() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();

    // ---- 1) 编辑态：点 TextEdit 内文字右侧的空白 ----
    let mut doc = Document::new("短文字", None);
    let mut state = EditorState::default(); // active=0 → 块 0 编辑态
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
    // 页宽 720 居中（pad 40），文字只有三个字：点击点 (400, 20) 在
    // TextEdit 之内、文字 galley 之外的空白区。
    let pos = egui::pos2(400.0, 20.0);
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
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
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let id = egui::Id::new(("block_edit", 1, 0));
    let caret = egui::TextEdit::load_state(&ctx, id)
        .and_then(|ts| ts.cursor.char_range())
        .map(|r| r.primary.index.0);
    assert_eq!(
        caret,
        Some(3),
        "编辑态点行尾空白，光标应落在最后一个字符之后，实际 {caret:?}"
    );
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(id),
        "点击后 TextEdit 必须持有焦点（否则光标不闪烁）"
    );

    // ---- 2) 渲染态点击非活动块行尾空白 → Caret 动作 → 进入编辑 ----
    let mut doc = Document::new("短文字\n\n第二段", None);
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
    // 第三块（"第二段"）是渲染态。先模拟渲染态块点击行尾空白产生的
    // Caret 动作：app.rs 会设置 active / focus_req / pending_caret。
    // local 偏移故意给一个超出文字宽度的 x，编辑态必须钳到行末。
    let id2 = egui::Id::new(("block_edit", 1, 2));
    state.active = 2;
    state.focus_req = Some(2);
    state.pending_caret = Some(egui::vec2(400.0, 2.0));
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );
    let caret = egui::TextEdit::load_state(&ctx, id2)
        .and_then(|ts| ts.cursor.char_range())
        .map(|r| r.primary.index.0);
    assert_eq!(
        caret,
        Some(3),
        "渲染态点行尾空白进入编辑后，光标应钳到行末，实际 {caret:?}"
    );
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(id2),
        "Caret 动作后块必须获得焦点（光标闪烁）"
    );
}

#[test]
fn outline_jump_sets_active_block_and_consumes_scroll_request() {
    let mut text = String::new();
    for i in 0..40 {
        text.push_str(&format!("段落 {i}\n\n"));
    }
    text.push_str("# 目标标题\n\n结尾段落\n");

    let mut doc = Document::new(&text, None);
    let outline = doc.outline();
    let target = outline.last().expect("应有标题").block;
    assert!(target > 30, "标题应位于文档后部，实际 block={target}");

    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut state = EditorState::default();
    state.active = target;
    state.scroll_req = Some(target);

    frame(800.0, |ui| {
        let ctx = ui.ctx().clone();
        let _ = show_editor(
            ui,
            &mut doc,
            &mut state,
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

    assert_eq!(state.active, target);
    assert!(
        state.scroll_req.is_none(),
        "滚动请求应被 show_editor 消费，否则点大纲不会滚动"
    );
}
