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

// ==================== 围栏块行首退格 ====================
// 规则：` ``` ` 围栏是块语义本身，退格不许把它拆掉。
// 普通文本块（段落/标题/列表）行首退格仍照常并回上一块。

/// 键盘帧驱动器：把第 `idx` 块设为活动块、光标落在字符 `caret`，之后逐帧注入按键。
/// 同一个 Context 用到底，等价于用户真实连续按键。
struct Keys {
    ctx: egui::Context,
    theme: Theme,
    images: ImageCache,
    actions: Vec<crate::render::Action>,
}

impl Keys {
    fn new(doc: &mut Document, state: &mut EditorState, idx: usize, caret: usize) -> Self {
        let mut me = Self {
            ctx: egui::Context::default(),
            theme: Theme::get(crate::theme::ThemeKind::Light),
            images: ImageCache::new(),
            actions: Vec::new(),
        };
        state.active = idx;
        state.focus_req = Some(idx);
        state.caret_req = Some((idx, caret));
        state.last_cursor = caret;
        me.frame(doc, state, vec![]);
        me
    }

    fn frame(&mut self, doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>) {
        run_editor_frame(
            &self.ctx,
            doc,
            state,
            &self.theme,
            &mut self.images,
            &mut self.actions,
            events,
        );
    }

    fn key(&mut self, doc: &mut Document, state: &mut EditorState, key: egui::Key) {
        self.frame(
            doc,
            state,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
    }

    fn type_str(&mut self, doc: &mut Document, state: &mut EditorState, text: &str) {
        self.frame(doc, state, vec![egui::Event::Text(text.into())]);
    }
}

const CODE_SRC: &str = "para\n\n```rust\nlet a = 1;\nlet b = 2;\n```";

fn is_code(doc: &Document, i: usize) -> bool {
    matches!(doc.blocks.get(i), Some(Block::Code { .. }))
}

/// 代码块第一行行首按退格：光标原地不动，代码块原样保留。
/// 以前这一按会把围栏连同代码一起并回上一块，整块代码当场掉成普通段落。
#[test]
fn backspace_at_code_block_start_keeps_block_and_caret() {
    let mut doc = Document::new(CODE_SRC, None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    for _ in 0..2 {
        keys.key(&mut doc, &mut state, egui::Key::Backspace);
    }
    assert_eq!(doc.source(), CODE_SRC, "退格不该改动代码块一个字节");
    assert_eq!(doc.blocks.len(), 3, "代码块不该被并回上一块");
    assert!(is_code(&doc, idx), "活动块还得是代码块");
    assert_eq!(state.active, idx, "活动块不该跳走");
    assert_eq!(state.last_cursor, 0, "光标应停在第一行第一个字符");
}

/// 删掉代码块里最后一个字符：留一个空代码块，不许塌成空段落。
/// 删到空那一帧同样满足「行首退格」条件，写回必须补回围栏。
#[test]
fn deleting_last_char_in_code_block_keeps_fence() {
    let mut doc = Document::new("para\n\n```rust\nx\n```", None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 1);
    keys.key(&mut doc, &mut state, egui::Key::Backspace);
    assert_eq!(doc.blocks.len(), 3, "只剩一个字符时删掉它不该并块");
    assert!(is_code(&doc, idx), "删空代码块后要留空围栏，实际 {:?}", doc.source());
    assert_eq!(doc.source(), "para\n\n```rust\n\n```");
    assert_eq!(state.active, idx, "活动块该留在代码块里继续写");
    assert_eq!(state.last_cursor, 0, "光标应停在这块唯一的空行行首");
}

/// 空代码块行首退格：整份文档逐字节不变（不写回、不标脏、不多一步撤销）。
#[test]
fn backspace_in_empty_code_block_changes_nothing() {
    let src = "para\n\n```rust\n```";
    let mut doc = Document::new(src, None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    keys.key(&mut doc, &mut state, egui::Key::Backspace);
    assert_eq!(doc.source(), src, "空代码块里按退格不该写回任何改动");
    assert!(is_code(&doc, idx));
}

/// 空代码块里打一个空格（缩进的第一个字符）：围栏必须留着。
/// 写回空源码会让代码块在敲下第一个空格那一帧就变成普通段落。
#[test]
fn typing_space_in_empty_code_block_keeps_fence() {
    let mut doc = Document::new("para\n\n```rust\n```", None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    keys.type_str(&mut doc, &mut state, " ");
    assert_eq!(doc.source(), "para\n\n```rust\n \n```", "空格要作为代码留着");
    assert!(
        matches!(&doc.blocks[idx], Block::Code { code, .. } if code == " "),
        "实际 {:?}",
        doc.blocks[idx].source()
    );
}

/// 图表块同理：行首退格不许把 mermaid 围栏并回上一段。
#[test]
fn backspace_at_diagram_start_keeps_fence() {
    let src = "para\n\n```mermaid\ngraph TD;\nA-->B;\n```";
    let mut doc = Document::new(src, None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    keys.key(&mut doc, &mut state, egui::Key::Backspace);
    assert_eq!(doc.source(), src, "图表块行首退格不该改文档");
    assert!(
        matches!(&doc.blocks[idx], Block::Diagram { .. }),
        "图表块不该被并成段落：{:?}",
        doc.blocks[idx].source()
    );
}

/// 代码组同理：行首退格不许把整组围栏塞进上一段。
/// 这条规则原先写在 merge_into_previous_block 里，现在统一到调用点的 is_fenced 判定。
#[test]
fn backspace_at_code_group_start_keeps_group() {
    let src = r###"para

````code-group
```java
int a = 1;
```
```python
a = 1
```
````"###;
    let mut doc = Document::new(src, None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    assert!(
        matches!(&doc.blocks[idx], Block::CodeGroup { .. }),
        "夹具应是代码组，实际 {:?}",
        doc.blocks[idx].source()
    );
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    keys.key(&mut doc, &mut state, egui::Key::Backspace);
    assert_eq!(doc.source(), src, "代码组行首退格不该改文档");
    assert!(
        matches!(&doc.blocks[idx], Block::CodeGroup { .. }),
        "代码组不该被并回上一段：{:?}",
        doc.blocks[idx].source()
    );
}

/// 退格不再并块之后，围栏块仍要有退出办法：空代码块连按两次回车变回普通段落
/// （块头「删除」按钮是另一条，见 app 层删除确认窗）。
#[test]
fn enter_twice_in_empty_code_block_exits_to_paragraph() {
    let mut doc = Document::new("para\n\n```rust\n```", None);
    let mut state = EditorState::default();
    let idx = doc.blocks.len() - 1;
    let mut keys = Keys::new(&mut doc, &mut state, idx, 0);
    for _ in 0..2 {
        keys.key(&mut doc, &mut state, egui::Key::Enter);
    }
    assert!(
        !is_code(&doc, state.active),
        "空代码块连按两次回车后应退出代码格式，实际 {:?}",
        doc.blocks[state.active].source()
    );
    assert!(
        !doc.blocks.iter().any(|b| matches!(b, Block::Code { .. })),
        "退出后不该还留着代码块：{:?}",
        doc.source()
    );
}
