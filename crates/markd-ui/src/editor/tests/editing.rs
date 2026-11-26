use super::*;

/// 原地编辑：标题只编辑文字、代码块只编辑代码，写回时再补回 Markdown 标记。
#[test]
fn edit_kind_rebuilds_markdown() {
    assert_eq!(EditKind::Heading(2).rebuild("标题"), "## 标题");

    let src = EditKind::Code {
        lang: "rust".to_string(),
        quote: 0,
    }
    .rebuild("fn main() {}");
    assert_eq!(src, "```rust\nfn main() {}\n```");
    let blocks = crate::ast::parse(&src);
    assert!(
        matches!(&blocks[0], Block::Code { lang, code, .. } if lang == "rust" && code == "fn main() {}"),
        "代码块写回后应能还原成同一段代码"
    );

    // 多行代码同样可往返
    let src = EditKind::Code {
        lang: String::new(),
        quote: 0,
    }
    .rebuild("a\nb");
    let blocks = crate::ast::parse(&src);
    assert!(matches!(&blocks[0], Block::Code { code, .. } if code == "a\nb"));

    // 末尾空行（缓冲以 \n 结尾）也要能往返：在末行回车产生的空行不丢失
    let src = EditKind::Code {
        lang: "rust".to_string(),
        quote: 0,
    }
    .rebuild("a\n");
    let blocks = crate::ast::parse(&src);
    assert!(
        matches!(&blocks[0], Block::Code { code, .. } if code == "a\n"),
        "{src}"
    );
    let src = EditKind::Code {
        lang: "rust".to_string(),
        quote: 0,
    }
    .rebuild("\n");
    let blocks = crate::ast::parse(&src);
    assert!(
        matches!(&blocks[0], Block::Code { code, .. } if code == "\n"),
        "{src}"
    );
}

/// 引用块进入编辑态时隐藏 `>`，保留行内 Markdown；写回时补回相同层级。
#[test]
fn quote_edit_hides_marker_and_roundtrips() {
    let doc = Document::new("> 引用 **加粗** 文字", None);
    let b = &doc.blocks[0];
    assert!(
        matches!(EditKind::of(b), EditKind::Quote { level: 1 }),
        "引用应识别为 Quote 编辑类型"
    );
    assert_eq!(block_buffer(b), "引用 **加粗** 文字", "编辑缓冲不应含 `>`");
    assert_eq!(
        rebuild_source(b, &block_buffer(b)),
        "> 引用 **加粗** 文字",
        "写回应补回 `>` 且不丢失行内格式"
    );

    // 多级引用：连续 `>` 的层级要原样保留
    let doc = Document::new(">> 嵌套引用", None);
    let b = &doc.blocks[0];
    assert!(matches!(EditKind::of(b), EditKind::Quote { level: 2 }));
    assert_eq!(block_buffer(b), "嵌套引用");
    assert_eq!(rebuild_source(b, "嵌套引用"), ">> 嵌套引用");
}

/// 引用按钮是开关：已是引用的块再点一次变回普通段落，普通段落点一次变引用。
#[test]
fn quote_command_toggles_quote_off() {
    let mut doc = Document::new("引用文字", None);
    let mut state = EditorState::default();
    state.active = 0;
    assert!(matches!(doc.blocks[0], Block::Paragraph { .. }));

    apply_prefix(&mut doc, &mut state, "> ");
    assert!(
        matches!(doc.blocks[0], Block::Quote { .. }),
        "第一次点击应加上引用：{}",
        doc.blocks[0].source()
    );

    apply_prefix(&mut doc, &mut state, "> ");
    assert!(
        matches!(doc.blocks[0], Block::Paragraph { .. }),
        "再次点击应还原成普通段落：{}",
        doc.blocks[0].source()
    );
    assert_eq!(doc.blocks[0].source(), "引用文字", "取消引用不应丢文字");
}

/// 跨块拖选后点引用：选中的引用块整体还原，空行保持原样。
#[test]
fn quote_command_toggles_off_across_selection() {
    let mut doc = Document::new("> 甲\n\n> 乙", None);
    let mut state = EditorState::default();
    state.cross_sel = Some((0, 0, 2, 1));

    apply_prefix(&mut doc, &mut state, "> ");
    assert!(
        doc.blocks.iter().all(|b| !matches!(b, Block::Quote { .. })),
        "选区内的引用块应全部还原：{:?}",
        doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
    );
    assert!(state.cross_sel.is_none(), "处理完应清空跨块选区");

    // 选区里混有普通段落时仍然统一加引用，不做「取消」。
    let mut doc = Document::new("> 甲\n\n乙", None);
    let mut state = EditorState::default();
    state.cross_sel = Some((0, 0, 2, 1));
    apply_prefix(&mut doc, &mut state, "> ");
    assert!(
        doc.blocks
            .iter()
            .filter(|b| !b.source().trim().is_empty())
            .all(|b| matches!(b, Block::Quote { .. })),
        "混选时应统一加引用：{:?}",
        doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
    );

    // 选区里包含「引用内代码块」：取消引用要把代码块每行的 `>` 都去掉，
    // 变成普通代码块，而不是只剥掉第一行前缀把代码弄坏。
    let mut doc = Document::new("> 说明\n> ```rust\n> let a = 1;\n> ```", None);
    let mut state = EditorState::default();
    state.cross_sel = Some((0, 0, 1, 0));
    apply_prefix(&mut doc, &mut state, "> ");
    assert!(matches!(doc.blocks[0], Block::Paragraph { .. }));
    assert!(matches!(
        &doc.blocks[1],
        Block::Code { quote: 0, code, .. } if code == "let a = 1;"
    ));
}

/// 引用里的代码块：编辑缓冲只含代码，写回时逐行补回 `>`，往返不丢引用。
#[test]
fn quoted_code_edit_roundtrips() {
    let doc = Document::new("> ```rust\n> let a = 1;\n> let b = 2;\n> ```", None);
    let b = &doc.blocks[0];
    assert!(matches!(EditKind::of(b), EditKind::Code { quote: 1, .. }));
    assert_eq!(block_buffer(b), "let a = 1;\nlet b = 2;");
    let src = rebuild_source(b, &block_buffer(b));
    assert_eq!(src, "> ```rust\n> let a = 1;\n> let b = 2;\n> ```");
    let re = crate::ast::parse(&src);
    assert!(matches!(
        &re[0],
        Block::Code { quote: 1, code, .. } if code == "let a = 1;\nlet b = 2;"
    ));
}

/// 在引用内代码块里打字：写回源码时每行仍带 `>`，不会把代码块挤出引用。
#[test]
fn typing_in_quoted_code_stays_quoted() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
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
    let mut doc = Document::new("> ```rust\n> fn main() {}\n> ```", None);
    let mut state = EditorState {
        active: 0,
        focus_req: Some(0),
        last_cursor: 12,
        ..Default::default()
    };
    // 第一帧把焦点交给代码块的 TextEdit，第二帧输入字符。
    run(&mut doc, &mut state, vec![]);
    run(&mut doc, &mut state, vec![egui::Event::Text("x".into())]);
    let src = doc.blocks[0].source().to_string();
    assert!(
        src.starts_with("> ```rust\n> "),
        "代码块应仍在引用内：{src}"
    );
    assert!(src.contains('x'), "输入的字符应进入引用内代码块：{src}");
    assert!(src.ends_with("> ```"), "闭合围栏应保留引用前缀：{src}");
}

/// 光标在引用里点「代码块」：插入的代码块仍属于引用（quote 层级），
/// 而不是把引用拆成两段、代码块跑到引用外面。
#[test]
fn insert_code_block_inside_quote_stays_quoted() {
    let mut doc = Document::new("> 引用文字", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_code_block(&mut doc, &mut state);
    assert_eq!(state.active, 1, "新代码块应成为活动块");
    match &doc.blocks[1] {
        Block::Code {
            quote, lang, code, ..
        } => {
            assert_eq!(*quote, 1);
            assert_eq!(lang, "");
            assert_eq!(code, "");
        }
        other => panic!("应插入引用内的空代码块，实际 {other:?}"),
    }
    // 引用本身还在，代码块没有把引用拆掉。
    assert!(matches!(doc.blocks[0], Block::Quote { .. }));
    // 普通段落里插入仍是普通代码块。
    let mut doc = Document::new("普通段落", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_code_block(&mut doc, &mut state);
    assert!(matches!(&doc.blocks[1], Block::Code { quote: 0, .. }));
}

/// 插入图片后光标落在图片下方的空段落：图片块不处于编辑态，
/// 这样插图完成即可看到渲染后的预览，而不是 `![](...)` 源码。
#[test]
fn insert_image_leaves_preview_rendered() {
    let mut doc = Document::new("段落", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_rendered_block(&mut doc, &mut state, "![a](imgs/a.png)");
    assert!(
        matches!(&doc.blocks[1], Block::Paragraph { source, .. } if source.as_str() == "![a](imgs/a.png)"),
        "图片应插成独立段落：{:?}",
        doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
    );
    assert!(
        matches!(&doc.blocks[2], Block::Blank { .. }),
        "图片后应补一个空段落"
    );
    assert_eq!(state.active, 2, "光标应落在图片下方的空段落");
    assert_eq!(state.focus_req, Some(2));
}

/// 点击渲染态的图片：该块进入编辑态，界面上露出 `![文件名](路径)` 源码；
/// 点击别处后图片恢复渲染。
#[test]
fn clicking_rendered_image_reveals_source() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let dir = std::env::temp_dir().join(format!("markd-img-click-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(120, 80, image::Rgba([200, 40, 40, 255]))
        .save(dir.join("a.png"))
        .unwrap();
    let path = dir.join("note.md");
    std::fs::write(&path, "").unwrap();
    let mut doc = Document::new("![a](a.png)\n\n文字", Some(path));
    let mut state = EditorState::default();
    state.active = 2;
    let ctx = egui::Context::default();
    let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
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
                    Some(&dir),
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
    // 先渲染一帧，让图片纹理与布局就绪
    run(&mut doc, &mut state, vec![]);
    let p = egui::pos2(400.0, 25.0);
    run(
        &mut doc,
        &mut state,
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
    run(
        &mut doc,
        &mut state,
        vec![egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert_eq!(state.active, 0, "点击图片应进入该块的源码编辑态");
}

/// 列表交互对齐 Typora：
/// - 回车后下一行仍是同类型列表（有序序号 +1、任务项回到未勾选）；
/// - Tab 让当前项变成子列表（缩进一级），Shift+Tab 退回上一级。
#[test]
fn list_enter_and_tab_match_typora() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();

    let key = |k: egui::Key, shift: bool| -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        }]
    };

    let run = |doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>| {
        let mut images = ImageCache::new();
        let mut actions: Vec<crate::render::Action> = Vec::new();
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

    // Tab：第二项缩进成子列表；Shift+Tab 退回。
    let mut doc = Document::new("- 父项\n- 子项", None);
    let mut state = EditorState {
        active: 1,
        focus_req: Some(1),
        tab_size: 2,
        ..Default::default()
    };
    run(&mut doc, &mut state, key(egui::Key::Tab, false));
    assert_eq!(
        doc.blocks[1].source(),
        "  - 子项",
        "Tab 应把列表项缩进为子列表"
    );
    run(&mut doc, &mut state, key(egui::Key::Tab, true));
    assert_eq!(doc.blocks[1].source(), "- 子项", "Shift+Tab 应退回上一级");

    // 无序列表回车：下一行还是无序列表。
    let mut doc = Document::new("- 父项", None);
    let mut state = EditorState {
        active: 0,
        focus_req: Some(0),
        last_cursor: 2,
        tab_size: 2,
        ..Default::default()
    };
    run(&mut doc, &mut state, key(egui::Key::Enter, false));
    assert!(
        matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.marker == "-"),
        "回车后应新建同类型的无序列表项，实际 {:?}",
        doc.blocks
    );

    // 有序列表回车：序号自动 +1。
    let mut doc = Document::new("3. 序号", None);
    let mut state = EditorState {
        active: 0,
        focus_req: Some(0),
        last_cursor: 2,
        tab_size: 2,
        ..Default::default()
    };
    run(&mut doc, &mut state, key(egui::Key::Enter, false));
    assert!(
        matches!(&doc.blocks[1], Block::ListItem { item, .. } if item.marker == "4."),
        "有序列表回车序号应 +1，实际 {:?}",
        doc.blocks
    );

    // 任务列表回车：新项保持任务列表，但回到未勾选。
    let mut doc = Document::new("- [x] 完成", None);
    let mut state = EditorState {
        active: 0,
        focus_req: Some(0),
        last_cursor: 2,
        tab_size: 2,
        ..Default::default()
    };
    run(&mut doc, &mut state, key(egui::Key::Enter, false));
    assert!(
        matches!(
            &doc.blocks[1],
            Block::ListItem { item, .. } if item.checked == Some(false)
        ),
        "任务列表回车应新建未勾选的任务项，实际 {:?}",
        doc.blocks
    );
}

/// 引用按钮作用于代码块：围栏与每一行正文都补 `>`，只生成一个引用内代码块，
/// 而不是「空代码块 + 正文段落 + 空代码块」。
#[test]
fn quote_command_on_code_block_keeps_code() {
    let mut doc = Document::new("```rust\nlet a = 1;\nlet b = 2;\n```", None);
    let mut state = EditorState::default();
    state.active = 0;
    apply_prefix(&mut doc, &mut state, "> ");
    assert_eq!(
        doc.blocks.len(),
        1,
        "代码块不应被拆散：{:?}",
        doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
    );
    assert!(
        matches!(
            &doc.blocks[0],
            Block::Code { quote: 1, lang, code, .. }
                if lang == "rust" && code == "let a = 1;\nlet b = 2;"
        ),
        "应变成引用内代码块且代码不变：{}",
        doc.blocks[0].source()
    );
    assert_eq!(state.focus_req, Some(0), "改完应把焦点还给编辑区");

    // 再点一次取消引用，代码原样保留。
    apply_prefix(&mut doc, &mut state, "> ");
    assert!(matches!(
        &doc.blocks[0],
        Block::Code { quote: 0, code, .. } if code == "let a = 1;\nlet b = 2;"
    ));
}

/// 跨块拖选里包含代码块时点引用：代码块每行都补 `>`，正文不被拆出来。
#[test]
fn quote_command_on_selected_code_block_keeps_code() {
    let mut doc = Document::new("前言\n\n```rust\nlet a = 1;\n```", None);
    let mut state = EditorState::default();
    state.cross_sel = Some((0, 0, 2, 1));
    apply_prefix(&mut doc, &mut state, "> ");
    assert!(matches!(&doc.blocks[0], Block::Quote { .. }));
    assert!(
        matches!(&doc.blocks[2], Block::Code { quote: 1, code, .. } if code == "let a = 1;"),
        "选区里的代码块应整体变成引用内代码块：{}",
        doc.blocks[2].source()
    );
}

/// 点工具条按钮会让编辑框失焦；改完块级格式后必须把焦点和光标还回去，
/// 否则用户继续打字会「没反应」。
#[test]
fn quote_command_keeps_focus_for_typing() {
    let mut doc = Document::new("文字", None);
    let mut state = EditorState::default();
    state.active = 0;
    apply_prefix(&mut doc, &mut state, "> ");
    type_after(&mut doc, &mut state, "x");
    assert_eq!(
        doc.blocks[0].source(),
        "> 文字x",
        "点引用后应能直接继续输入"
    );
}

/// 插入代码块后立刻键盘输入：内容应进入新代码块，而不是丢失。
#[test]
fn insert_code_block_allows_immediate_typing() {
    let mut doc = Document::new("文字", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_code_block(&mut doc, &mut state);
    assert_eq!(state.last_cursor, 0, "光标应落在代码正文开头");
    type_after(&mut doc, &mut state, "x");
    assert_eq!(doc.source(), "文字\n```\nx\n```");
}

/// 引用里插入代码块后立刻输入：内容进入引用内代码块。
#[test]
fn insert_code_block_in_quote_allows_immediate_typing() {
    let mut doc = Document::new("> 引用", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_code_block(&mut doc, &mut state);
    type_after(&mut doc, &mut state, "x");
    assert_eq!(doc.source(), "> 引用\n> ```\n> x\n> ```");
}

/// 跑一帧让焦点落地，再注入一个文本事件。
fn type_after(doc: &mut Document, state: &mut EditorState, text: &str) {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let ctx = egui::Context::default();
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    run_editor_frame(&ctx, doc, state, &theme, &mut images, &mut actions, vec![]);
    run_editor_frame(
        &ctx,
        doc,
        state,
        &theme,
        &mut images,
        &mut actions,
        vec![egui::Event::Text(text.into())],
    );
}

/// 刚插入的空代码块点引用：保持为一个引用内代码块，光标回位后能继续输入。
/// 回归：以前只给第一行加 `>`，会被解析成「空代码块 + 正文」，看起来像代码块坏了。
#[test]
fn quote_command_on_empty_code_block_allows_typing() {
    let mut doc = Document::new("文字", None);
    let mut state = EditorState::default();
    state.active = 0;
    insert_code_block(&mut doc, &mut state);
    apply_prefix(&mut doc, &mut state, "> ");
    assert_eq!(
        doc.blocks.len(),
        2,
        "插入的代码块应整体变成引用内代码块：{:?}",
        doc.blocks.iter().map(|b| b.source()).collect::<Vec<_>>()
    );
    assert!(matches!(
        &doc.blocks[1],
        Block::Code { quote: 1, code, .. } if code.is_empty()
    ));
    type_after(&mut doc, &mut state, "x");
    assert_eq!(doc.source(), "文字\n> ```\n> x\n> ```");
}
