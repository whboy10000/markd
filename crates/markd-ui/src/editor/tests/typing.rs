use super::*;

/// Typora 式任务列表：非空任务项回车 → 下一行继承任务标记；
/// 空任务项再回车 → 退出列表，变回普通段落。
#[test]
fn enter_task_list_inherits_then_exits() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();

    let enter = || {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    };
    let is_task =
        |b: &Block| matches!(b, Block::ListItem { item, .. } if item.checked == Some(false));

    // 从普通段落出发，先点「任务列表」转换成任务项（由 app 层 Command::Task 完成，
    // 这里用等价的 insert_task 模拟转换）。
    let mut doc = Document::new("- [ ] 写文档", None);
    let mut state = EditorState::default();
    state.active = 0;
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

    // 光标到「写文档」末尾回车 → 下一行应继承任务标记
    state.last_cursor = "写文档".chars().count();
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        enter(),
    );
    assert_eq!(doc.blocks.len(), 2, "回车应拆出新块");
    assert!(
        is_task(&doc.blocks[1]),
        "非空任务项回车，下一行应仍是任务列表: {:?}",
        doc.blocks[1]
    );

    // 现在光标在新空任务项上，再回车 → 退出列表变普通段落（空行）
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        enter(),
    );
    assert!(
        !matches!(doc.blocks.last(), Some(Block::ListItem { .. })),
        "空任务项再回车应退出列表、不再是任务项: {:?}",
        doc.blocks.last()
    );
}

/// Typora 式有序/无序列表：非空项回车继承标记，空项回车退出变普通段落。
#[test]
fn enter_list_inherits_then_exits() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let enter = || {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    };

    for (src, body, label) in [
        ("1. 第一项", "第一项", "有序"),
        ("- 第一项", "第一项", "无序"),
    ] {
        let mut doc = Document::new(src, None);
        let mut state = EditorState::default();
        state.active = 0;
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
        state.last_cursor = body.chars().count();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            enter(),
        );
        assert!(
            matches!(&doc.blocks[1], Block::ListItem { .. }),
            "{label}列表非空项回车，下一行应仍是列表: {:?}",
            doc.blocks[1]
        );
        // 空项再回车退出：不新增块（当前空项写回成普通段落）
        let n_before = doc.blocks.len();
        run_editor_frame(
            &ctx,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &mut actions,
            enter(),
        );
        assert_eq!(
            doc.blocks.len(),
            n_before,
            "{label}列表空项回车退出时不应新增块: {:?}",
            doc.blocks
        );
        assert!(
            !matches!(doc.blocks.last(), Some(Block::ListItem { .. })),
            "{label}列表空项再回车应退出: {:?}",
            doc.blocks.last()
        );
    }
}

/// 已勾选任务项回车，新项应为未勾选状态。
#[test]
fn enter_on_checked_task_inherits_unchecked() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let mut doc = Document::new("- [x] 已完成", None);
    let mut state = EditorState::default();
    state.active = 0;
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
    state.last_cursor = "已完成".chars().count();
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
    assert!(
        matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.checked == Some(false)),
        "已勾选任务项回车，新项应未勾选: {:?}",
        doc.blocks[1]
    );
}

/// 任务列表项里按回车应当换行（拆出新块）；普通段落同理。
#[test]
fn enter_splits_task_item_and_paragraph() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();

    // 任务列表：光标在「任务一」末尾回车，应产生新的一行
    let mut doc = Document::new("- [ ] 任务一\n- [ ] 任务二", None);
    let mut state = EditorState::default();
    state.active = 0;
    state.focus_req = Some(0);

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
    // 光标放到第一项文字末尾再回车
    state.last_cursor = "任务一".chars().count();
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
    assert!(
        doc.blocks.len() > 2,
        "任务列表项里回车应换行拆块，实际块数 {}",
        doc.blocks.len()
    );

    // 普通段落：光标在中间回车，拆成两段
    let mut doc = Document::new("第一行内容", None);
    let mut state = EditorState::default();
    state.active = 0;
    state.focus_req = Some(0);
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
    state.last_cursor = 3;
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
        2,
        "段落里回车应拆成两块，实际块数 {}",
        doc.blocks.len()
    );
}

/// 大纲跳转：点击标题后应把该块设为活动块，并消费滚动请求
/// （之前 scroll_req 只写不读，所以点大纲完全不会滚动）。
/// 行首按退格要把当前块并回上一块（Typora 行为）：
/// 当前块删空后再按一次，应当与上一块合并，而不是毫无反应。
#[test]
fn backspace_at_block_start_merges_previous_block() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut doc = Document::new("# 标题一\n## 标题二", None);
    assert_eq!(doc.blocks.len(), 2, "两行标题应解析成两个块");

    let mut state = EditorState::default();
    state.active = 1;
    state.focus_req = Some(1);

    let ctx = egui::Context::default();
    // 第一帧：让第二个块的 TextEdit 拿到焦点，光标落在行首
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
        ctx.memory(|m| m.focused()).is_some(),
        true,
        "编辑块应当拿到键盘焦点"
    );
    // 连续按 4 次退格：3 次删掉「标题二」，第 4 次（此时行已空）应当并回上一块
    for _ in 0..4 {
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
    }

    assert_eq!(
        doc.blocks.len(),
        1,
        "行首退格应把第二块并进第一块，实际仍有 {} 块：{:?}",
        doc.blocks.len(),
        doc.source()
    );
    assert_eq!(doc.source(), "# 标题一", "空行应被并入上一块");
}
