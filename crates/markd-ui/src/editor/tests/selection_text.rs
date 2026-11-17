use super::*;

/// 跨块选区复制文本：首尾块取片段、中间块取全文、块间空行连接。
#[test]
fn cross_sel_text_joins_fragments() {
    let doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    // 块 2 "第二段" chars 1..3 = "二段"；块 4 "第三段" chars 0..2 = "第三"
    let text = cross_sel_text(&doc, Some((2, 1, 4, 2))).expect("选区应产生文本");
    assert_eq!(text, "二段\n\n第三");
    // 反向（从尾拖回头）
    let text = cross_sel_text(&doc, Some((4, 2, 2, 1))).expect("反向选区应产生相同文本");
    assert_eq!(text, "二段\n\n第三");
    assert!(cross_sel_text(&doc, None).is_none(), "无选区无文本");
}

/// 代码块这类非文本块是原子的：作为选区端点时整块计入复制文本，
/// 不按拖选时打上的「块首/块尾」端点字符截断。
#[test]
fn cross_sel_text_includes_whole_code_block() {
    let doc = Document::new("alpha\n\n```rust\nfn main() {}\n```\n\ngamma", None);
    // alpha(0) / 空行(1) / 代码块(2) / 空行(3) / gamma(4)
    let code_len = doc.blocks[2].plain().chars().count();
    let text = cross_sel_text(&doc, Some((0, 0, 2, code_len))).expect("向下选区应有文本");
    assert_eq!(text, "alpha\n\nfn main() {}", "向下拖到代码块应整块计入");
    // 向上拖：终点字符是 0（块首标记），仍应整块计入。
    let text = cross_sel_text(&doc, Some((4, 6, 2, 0))).expect("向上选区应有文本");
    assert_eq!(text, "fn main() {}\n\ngamma", "向上拖到代码块也应整块计入");
}

/// 空段落行首退格并入上一块（代码块）时，光标应落在代码正文最后一行末尾，
/// 而不是被钳制到中间行。此前用整段源码长度算 cursor，含围栏导致偏到中间。
#[test]
fn backspace_into_code_block_lands_at_end() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let ctx = egui::Context::default();
    let mut doc = Document::new("```rust\nfn a() {\n    let x = 1;\n}\n```\n", None);
    let code_body = "fn a() {\n    let x = 1;\n}";
    let mut state = EditorState::default();
    state.active = doc.blocks.len() - 1; // 末尾空段落
    state.last_cursor = 0;
    run_editor_frame(
        &ctx,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &mut actions,
        vec![],
    );

    // 空段落行首退格
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
    assert!(
        matches!(doc.blocks.get(state.active), Some(Block::Code { .. })),
        "退格后活动块应是代码块: {:?}",
        doc.blocks.get(state.active)
    );
    assert_eq!(
        state.last_cursor,
        code_body.chars().count(),
        "光标应落在代码正文末尾（{}），实际 {}",
        code_body.chars().count(),
        state.last_cursor
    );
}
