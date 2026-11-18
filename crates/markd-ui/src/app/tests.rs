//! 应用层单元测试。

use super::*;

/// 拖动缩放：只给目标图片写入 `{width=N}`，其余图片属性不动；
/// 混着正文的段落不处理（重建会把文字弄丢）。
#[test]
fn resize_image_source_rewrites_width_attr() {
    let doc = Document::new("![a](imgs/a.png)\n![b](imgs/b.png){width=120}", None);
    let src = resize_image_source(&doc, 0, 1, 300).expect("纯图片段落应可重建");
    assert_eq!(src, "![a](imgs/a.png)\n![b](imgs/b.png){width=300}");
    assert!(
        resize_image_source(&doc, 0, 9, 300).is_none(),
        "序号越界不处理"
    );
    let mixed = Document::new("文字 ![a](a.png)", None);
    assert!(
        resize_image_source(&mixed, 0, 0, 300).is_none(),
        "混着正文的段落不应重建源码"
    );
}

/// 加粗按钮：源码用标准 Markdown 的 ** 标记（即时渲染态由 inline_job
/// 隐藏标记、显示粗体），不再写 <strong>。
#[test]
fn bold_button_writes_markdown_stars() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("加粗这段文字", None);
    app.editor.last_sel = Some((0, 2, 6)); // 选中「这段文字」
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.source(), "加粗**这段文字**");
}

/// 跨块拖选：鼠标选中某块的文字后点加粗，只加粗选中的文字，
/// 不碰光标所在的活动块（两者往往不是同一行）。
#[test]
fn bold_applies_to_cross_sel_not_active_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("光标行\n\n第二行选中文字结尾\n\n第三行", None);
    app.editor.active = 0; // 光标在第 0 块
    // 鼠标拖选的是第 2 块的「选中文字」（字符 3..7）
    app.editor.cross_sel = Some((2, 3, 2, 7));
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.blocks[0].source(), "光标行", "光标所在行不应被加粗");
    assert_eq!(app.doc.blocks[2].source(), "第二行**选中文字**结尾");
}

/// 跨多块选区：首块取选中片段、中间块整块、尾块取片段。
#[test]
fn bold_cross_sel_spans_multiple_blocks() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("甲甲乙丙丁\n\n第二块全文\n\n戊己庚辛", None);
    app.editor.active = 0;
    // 选区：块0 的 2..5（乙丙丁） → 块4 的 0..2（戊己）；0/4 是块下标
    app.editor.cross_sel = Some((0, 2, 4, 2));
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.blocks[0].source(), "甲甲**乙丙丁**");
    assert_eq!(app.doc.blocks[4].source(), "**戊己**庚辛");
}

/// 再次对已加粗的选区点加粗 → 取消加粗。
#[test]
fn bold_cross_sel_toggles_off() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("前缀粗体后缀", None);
    app.editor.cross_sel = Some((0, 2, 0, 4));
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.source(), "前缀**粗体**后缀");
    // 渲染态仍显示纯文本「前缀粗体后缀」，再次选中「粗体」应取消加粗
    app.editor.cross_sel = Some((0, 2, 0, 4));
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.source(), "前缀粗体后缀");
}

/// 无跨块选区时，加粗仍作用于活动块（原有行为不变）。
#[test]
fn bold_without_cross_sel_uses_active_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("正文一段", None);
    app.editor.last_sel = Some((0, 2, 4));
    app.execute(Command::Bold, &ctx);
    assert_eq!(app.doc.source(), "正文**一段**");
}

/// 行内代码按钮：跨块拖选一段文字后点「行内代码」，应只包裹选中文字，
/// 光标所在的活动块不受影响（与加粗共用 wrap 逻辑）。
#[test]
fn inline_code_applies_to_cross_sel() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("光标行\n\n第二行选中文字结尾", None);
    app.editor.active = 0;
    app.editor.cross_sel = Some((2, 3, 2, 7)); // 选中块2的「选中文字」
    app.execute(Command::Code, &ctx);
    assert_eq!(app.doc.blocks[0].source(), "光标行", "光标行不应被改动");
    assert_eq!(app.doc.blocks[2].source(), "第二行`选中文字`结尾");
}

/// 点「有序列表」「无序列表」按钮：把当前段落转成列表项、不新增块。
#[test]
fn list_buttons_convert_current_block() {
    let ctx = egui::Context::default();
    for (cmd, want_marker) in [(Command::Ordered, "1."), (Command::Bullet, "-")] {
        let mut app = test_app();
        app.doc = Document::new("列表内容", None);
        app.execute(cmd.clone(), &ctx);
        assert_eq!(app.doc.blocks.len(), 1, "不应新增块");
        assert!(
            matches!(&app.doc.blocks[0], crate::ast::Block::ListItem { item, .. } if item.marker == want_marker),
            "{:?} 应转成标记 {want_marker} 的列表项: {:?}",
            &cmd,
            app.doc.blocks[0]
        );
    }
}

/// 点「任务列表」按钮：把光标所在段落转成任务项，保留文字、不新增块。
#[test]
fn task_button_converts_current_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    app.doc = Document::new("写文档", None);
    app.execute(Command::Task, &ctx);
    assert_eq!(app.doc.blocks.len(), 1, "不应新增块，只转换当前块");
    assert!(
        matches!(&app.doc.blocks[0], crate::ast::Block::ListItem { item, .. } if item.checked == Some(false)),
        "当前段应转成任务项: {:?}",
        app.doc.blocks[0]
    );
    assert_eq!(app.doc.source(), "- [ ] 写文档");
}

/// 即时渲染态：inline_job 把 **...** 的内容渲染为粗体、标记隐藏。
#[test]
fn inline_job_renders_stars_as_bold() {
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let job = crate::render::inline_job("**粗体**", &theme, 16.0, theme.text, 400.0, None, None);
    assert!(
            job.sections.iter().any(|sec| {
                matches!(&sec.format.font_id.family, egui::FontFamily::Name(n) if n.as_ref() == "markd-bold")
            }),
            "双星号内容应以粗体字体渲染: {job:?}"
        );
}

/// 自动保存判定：开关 / 有修改 / 已落盘 三个条件缺一不可。
#[test]
fn autosave_requires_enabled_dirty_and_path() {
    assert!(should_autosave(true, true, true));
    assert!(!should_autosave(false, true, true));
    assert!(!should_autosave(true, false, true), "没改过不该写盘");
    assert!(
        !should_autosave(true, true, false),
        "未命名文档没有路径，不该自动写盘"
    );
}

/// 间隔判定：未到点不存、到点存；间隔下限 5 秒；时钟回拨不触发。
#[test]
fn autosave_interval_gate() {
    assert!(!autosave_due(19, 0, 20));
    assert!(autosave_due(20, 0, 20));
    assert!(autosave_due(1000, 20, 20));
    // 配置被改成 0 也至少等 5 秒，否则等于每帧写盘
    assert!(!autosave_due(4, 0, 0));
    assert!(autosave_due(5, 0, 0));
    assert!(!autosave_due(5, 100, 20), "时钟回拨时不该误触发");
}

/// 退出拦截：只有「有未保存修改 + 真的在关闭 + 未选择不保存」才弹确认框。
#[test]
fn exit_confirm_gate() {
    assert!(needs_exit_confirm(false, true, 1));
    assert!(
        !needs_exit_confirm(true, true, 1),
        "已选过「不保存」应直接放行"
    );
    assert!(!needs_exit_confirm(false, false, 1), "没点关闭不该弹窗");
    assert!(!needs_exit_confirm(false, true, 0), "没有未保存修改不弹窗");
}

/// 造一个标签：原内容 src，改成 edited 后处于脏状态；path 为 None 表示未命名。
fn dirty_tab(src: &str, edited: &str, path: Option<PathBuf>) -> Tab {
    let mut doc = Document::new(src, path.clone());
    doc.edit_block(0, edited);
    assert!(doc.dirty);
    Tab {
        uid: 0,
        path,
        seq: 1,
        state: Some(TabState {
            doc,
            editor: EditorState::default(),
            outline: Vec::new(),
            stats: Default::default(),
        }),
    }
}

/// 造一个干净的（没改过）标签。
fn clean_tab(src: &str, path: PathBuf) -> Tab {
    Tab {
        uid: 0,
        path: Some(path.clone()),
        seq: 1,
        state: Some(TabState {
            doc: Document::new(src, Some(path)),
            editor: EditorState::default(),
            outline: Vec::new(),
            stats: Default::default(),
        }),
    }
}

/// 退出批量保存：已命名的脏标签原地写回，未命名的写到刚选的路径；
/// 没有修改的标签不碰磁盘；写完后脏标记清掉。
#[test]
fn write_dirty_tabs_writes_only_dirty_ones() {
    let dir = std::env::temp_dir().join(format!("markd-exit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.md");
    let b = dir.join("b.md");
    let c = dir.join("untouched.md");
    let mut tabs = vec![
        dirty_tab("旧 A", "新 A", Some(a.clone())),
        dirty_tab("旧 B", "新 B", None),
        clean_tab("干净", c.clone()),
    ];
    let paths = vec![Some(a.clone()), Some(b.clone()), Some(c.clone())];
    let written = write_dirty_tabs(&mut tabs, &paths).unwrap();
    assert_eq!(written, vec![a.clone(), b.clone()]);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "新 A");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "新 B");
    assert!(!c.exists(), "没改过的标签不该被写盘");
    assert!(!tabs[0].state.as_ref().unwrap().doc.dirty);
    assert!(!tabs[1].state.as_ref().unwrap().doc.dirty);
    assert_eq!(tabs[1].path, Some(b), "未命名标签保存后应记住路径");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 没拿到路径（用户取消）的标签保持脏状态，下一次还得提示保存。
#[test]
fn write_dirty_tabs_skips_tabs_without_path() {
    let mut tabs = vec![dirty_tab("旧 B", "新 B", None)];
    let written = write_dirty_tabs(&mut tabs, &[None]).unwrap();
    assert!(written.is_empty());
    assert!(
        tabs[0].state.as_ref().unwrap().doc.dirty,
        "没有路径时不该清掉脏标记"
    );
}

/// 大纲显隐开关：点击隐藏、再点显示，且状态可往返。
#[test]
fn outline_toggle_roundtrip() {
    // 初始隐藏（文件树）→ 点击 → 显示大纲
    let (show, tab) = outline_toggle(false);
    assert!(show);
    assert_eq!(tab, SideTab::Outline);

    // 再点击 → 隐藏（回到文件树）
    let (show2, tab2) = outline_toggle(show && tab == SideTab::Outline);
    assert!(!show2);
    assert_eq!(tab2, SideTab::Files);

    // 第三次点击 → 又显示
    let (show3, tab3) = outline_toggle(show2 && tab2 == SideTab::Outline);
    assert!(show3);
    assert_eq!(tab3, SideTab::Outline);
}

/// 验证「文件 / 大纲」入口的四种点击组合：展开、收起、互切。
#[test]
fn sidebar_click_toggles_and_switches() {
    // 1) 收起状态点「文件」→ 展开文件树
    let (show, tab, outline) = sidebar_click(false, SideTab::Outline, SideTab::Files);
    assert!(show, "应收起->展开");
    assert_eq!(tab, SideTab::Files);
    assert!(!outline);

    // 2) 正显示文件树再点「文件」→ 收起
    let (show, tab, outline) = sidebar_click(true, SideTab::Files, SideTab::Files);
    assert!(!show, "再点当前页签应收起侧栏");
    assert_eq!(tab, SideTab::Files);
    assert!(!outline);

    // 3) 正显示文件树点「大纲」→ 切换到大纲
    let (show, tab, outline) = sidebar_click(true, SideTab::Files, SideTab::Outline);
    assert!(show, "切到另一页签应保持展开");
    assert_eq!(tab, SideTab::Outline);
    assert!(outline);

    // 4) 正显示大纲再点「大纲」→ 收起
    let (show, tab, outline) = sidebar_click(true, SideTab::Outline, SideTab::Outline);
    assert!(!show, "再点当前页签应收起侧栏");
    assert_eq!(tab, SideTab::Outline);
    assert!(outline);

    // 5) 收起状态点「大纲」→ 展开大纲
    let (show, tab, outline) = sidebar_click(false, SideTab::Files, SideTab::Outline);
    assert!(show);
    assert_eq!(tab, SideTab::Outline);
    assert!(outline);
}

/// 构造一个最小化的真实 MarkdApp：不装 macOS 原生菜单、不读用户配置和
/// 快照目录，其余字段与 MarkdApp::new 相同，用来在测试里驱动
/// `eframe::App::ui` 这条真实的完整帧路径（含终端面板 / read_only 逻辑）。
fn test_app() -> MarkdApp {
    let mut config = Config::default();
    // 首次启动默认隐藏工具条；测试需要点击工具条按钮，这里显式打开。
    config.show_toolbar = true;
    let mut theme = Theme::get(config.theme);
    theme.body = config.font_size;
    theme.mono = (config.font_size * 0.84).max(11.0);
    let doc = Document::new("", None);
    let stats_cache = doc.stats();
    MarkdApp {
        stats: WritingStats::new(&doc),
        stats_cache,
        stats_at: std::time::Instant::now(),
        doc,
        config,
        theme,
        editor: EditorState::default(),
        images: crate::render::ImageCache::new(),
        icons: crate::icons::IconCache::default(),
        root: None,
        tree: Vec::new(),
        expanded: HashSet::new(),
        outline: Vec::new(),
        side_tab: SideTab::Files,
        outline_folded: HashSet::new(),
        menu: None,
        workspace_input: String::new(),
        palette: Palette::default(),
        search: SearchPanel::default(),
        find: FindPanel::default(),
        graph: GraphPanel::default(),
        snapshots: SnapshotStore {
            items: Vec::new(),
            open: false,
        },
        status: String::new(),
        toast: None,
        about: false,
        shortcuts: false,
        table_dialog: false,
        table_rows: 3,
        table_cols: 3,
        link_dialog: Default::default(),
        link_docs: HashMap::new(),
        link_rx: None,
        last_autosave: now_secs(),
        exit_dialog: false,
        code_delete_confirm: None,
        table_delete_confirm: None,
        image_uploads: Vec::new(),
        force_quit: false,
        theme_dirty: false,
        menu_guard: false,
        workspace_dialog: false,
        quick_cache_dir: String::new(),
        quick_cache: Vec::new(),
        menu_queue: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        terminal: Terminal::new(std::env::temp_dir()),
        settings: Default::default(),
        home: true,
        tabs: Vec::new(),
        active: 0,
        untitled_seq: 0,
        next_uid: 1,
        frame_count: 0,
        ctx_menu: None,
        file_clipboard: None,
        rename_dialog: None,
        delete_confirm: None,
        new_dialog: None,
    }
}

/// 跑一帧真实 app：1280×800 窗口，所有面板 + 编辑区完整渲染。
fn run_real_frame(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    events: Vec<egui::Event>,
) {
    let _ = run_real_frame_ext(ctx, app, frame, events);
}

/// 同上，但额外返回 (全部绘制形状, 系统剪贴板收到的文本)：
/// 跨块选区的手动链路验证需要抓块坐标和剪贴板内容。
fn run_real_frame_ext(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    events: Vec<egui::Event>,
) -> (Vec<egui::epaint::ClippedShape>, String) {
    use eframe::App as _;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        let mut root = egui::Ui::new(
            root.ctx().clone(),
            egui::Id::unique("markd_test_root"),
            egui::UiBuilder::new(),
        );
        app.ui(&mut root, frame);
    });
    out.textures_delta.clear();
    // 0.37 的剪贴板写入走 platform_output.commands 的 CopyText 命令
    let copied = out
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (out.shapes, copied)
}

/// 与 `run_real_frame_ext` 相同，但 root Ui 的 Id 稳定。
/// 点击类测试必须用它：自动 Id 每帧变化会让 egui 认为按下和松开不是同一个控件。
fn run_real_frame_stable_ext(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    events: Vec<egui::Event>,
) -> (Vec<egui::epaint::ClippedShape>, String) {
    use eframe::App as _;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        let mut root = egui::Ui::new(
            root.ctx().clone(),
            egui::Id::new("markd_test_root"),
            egui::UiBuilder::new(),
        );
        app.ui(&mut root, frame);
    });
    out.textures_delta.clear();
    let copied = out
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default();
    (out.shapes, copied)
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// 取文档编辑区里一段已渲染文字的可见中心，作为鼠标点击落点。
///
/// 两个跨平台陷阱：
/// 1. 非 macOS（CI Linux）窗口内会渲染 VS Code 风格菜单栏，「段落」既是菜单
///    分类名又是文档文字；`shapes` 按绘制顺序排列，菜单在编辑区之前，直接
///    `find_map` 会命中菜单按钮、点开菜单而不是点文档段落（macOS 用原生菜单
///    栏、窗口内无此文字，所以本地一直通过）。因此只取 y 在编辑区内的文字。
/// 2. 不能用「文字左上角 + 固定偏移」：不同平台字体度量不同，固定 y 偏移会
///    落到块间空隙、被判成「点空白」。用 `galley.rect` 中心跨平台都落在文字
///    交互矩形内。
fn text_center(shapes: &[egui::epaint::ClippedShape], name: &str) -> Option<egui::Pos2> {
    shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t),
            _ => None,
        })
        // 菜单栏/工具条/标签栏都在窗口顶部约 80px 内，文档正文在其下方。
        .filter(|t| t.pos.y > 80.0)
        .last()
        .map(|t| t.pos + t.galley.rect.center().to_vec2())
}

/// 手动链路验证：拖选跨块 → Cmd+C 复制剪贴板 → 退格合并 → Cmd+Z 撤销。
#[test]
fn cross_block_manual_flow() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    // 抓"第二段"/"第三段"的文本锚点（块文字左上角）
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> egui::Pos2 {
        for cs in shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                if t.galley.text() == name {
                    return t.pos;
                }
            }
        }
        panic!("{name} 未渲染");
    };
    let p2 = find(&shapes, "第二段");
    let p4 = find(&shapes, "第三段");
    println!(
        "[坐标] 第二段@({:.1},{:.1}) 第三段@({:.1},{:.1})",
        p2.x, p2.y, p4.x, p4.y
    );
    // 拖选点：起点在块 2 文字开头（字符 0），终点在块 4 行尾空白（钳到末尾）
    let start = egui::pos2(p2.x + 3.0, p2.y + 9.0);
    let end = egui::pos2(p4.x + 300.0, p4.y + 9.0);

    // 拖选：按下 → 移动 → 多帧保持（drag_started 晚一帧生效）→ 抬起
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(end)],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(end)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(end, false)]);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let sel = app.editor.cross_sel.expect("拖选后应建立跨块选区");
    let (lo, hi) = (sel.0.min(sel.2), sel.0.max(sel.2));
    assert_eq!((lo, hi), (2, 4), "选区应覆盖第二段到第三段，实际 {sel:?}");

    // Cmd+C：egui-winit 会把它转成 Event::Copy（不产生 Key 事件），
    // 剪贴板应收到两段文本。
    let (_, copied) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Copy]);
    assert_eq!(copied, "第二段\n\n第三段", "Cmd+C 应把跨块选区复制进剪贴板");

    // 退格：删除选区（跨块合并）
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let src = app.doc.source();
    assert!(
        !src.contains("第二段") && !src.contains("第三段"),
        "退格应删除选中的两段，实际 {src:?}"
    );
    assert!(src.contains("第一段"), "未选中的第一段必须保留：{src:?}");

    // Cmd+Z：撤销恢复
    let cmd_z = egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            command: true,
            ..Default::default()
        },
    };
    let mods_cmd = egui::Event::ModifiersChanged(egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..Default::default()
    });
    run_real_frame(&ctx, &mut app, &mut frame, vec![mods_cmd, cmd_z]);
    assert_eq!(
        app.doc.source(),
        "第一段\n\n第二段\n\n第三段",
        "Cmd+Z 应恢复被删除的两段"
    );
}

/// Cmd+X：跨块选区应先复制进剪贴板，再从文档里删除。
#[test]
fn cross_block_cut_shortcut_copies_and_deletes() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段\n\n第二段\n\n第三段", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    // 直接给一个跨块选区：第二段(2) 到第三段(4) 全选。
    app.editor.cross_sel = Some((2, 0, 4, 3));

    // egui-winit 把 Cmd+X 转成 Event::Cut（不产生 Key 事件）。
    let (_, copied) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Cut]);
    assert_eq!(copied, "第二段\n\n第三段", "Cmd+X 应把跨块选区复制进剪贴板");
    assert!(app.editor.cross_sel.is_none(), "剪切后选区应清除");
    let src = app.doc.source();
    assert!(
        !src.contains("第二段") && !src.contains("第三段"),
        "Cmd+X 应删除选中的两段，实际 {src:?}"
    );
    assert!(src.contains("第一段"), "未选中的第一段必须保留：{src:?}");
}

/// 真实 app 完整帧路径：新建文档后点编辑区空白，光标必须一直在第一行。
/// 这是用户报告 bug 的原始场景；驱动的是 `MarkdApp::ui`，不是仿真面板。
#[test]
fn real_app_blank_click_after_new_document() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();

    // 首页一帧，然后走「+ 新建」的真实路径
    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    app.new_document();
    // 让 focus_req=Some(0) 落地：此时光标应已在第一行（用户确认的初始状态）
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let tab_key = app.tabs[app.active].uid;
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::unique(("block_edit", tab_key, 0))),
        "新建文档后光标应默认在第一行"
    );

    // 反复点编辑区空白，每次点完光标都必须还在第一行：
    // 正文下方 / 左侧页边距 / 正文上方贴着的空白，全部覆盖。
    for (round, blank) in [
        (1, egui::pos2(600.0, 400.0)),
        (2, egui::pos2(240.0, 300.0)),
        (3, egui::pos2(600.0, 130.0)),
    ] {
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![
                egui::Event::PointerMoved(blank),
                pointer_button(blank, true),
            ],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![pointer_button(blank, false)],
        );
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::unique(("block_edit", tab_key, 0))),
            "第 {round} 次点空白后光标必须仍在第一行"
        );
        assert_eq!(
            app.editor.active, 0,
            "第 {round} 次点空白后活动块应为第一块"
        );
        assert_eq!(
            app.editor.last_cursor, 0,
            "第 {round} 次点空白后光标应在行首"
        );
    }
}

/// 单击正文块就应进入编辑并获得焦点，不能要求点第二次。
#[test]
fn real_app_single_click_focuses_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段\n\n第二段", None);
    app.home = false;
    for _ in 0..3 {
        let _ = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    }

    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    let pos = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "第二段" => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > 80.0).then_some(p)
            }
            _ => None,
        })
        .expect("第二段未渲染");

    let _ = run_real_frame_stable_ext(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(pos)],
    );
    let _ = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![pointer_button(pos, true)]);
    let _ = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![pointer_button(pos, false)]);
    for _ in 0..2 {
        let _ = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    }

    assert_eq!(app.editor.active, 2, "单击后活动块应切到第二段");
    let tab_key = app.tabs.get(app.active).map(|t| t.uid).unwrap_or(0);
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new(("block_edit", tab_key, 2))),
        "单击一次后第二段就必须持有焦点（光标闪烁）"
    );
}

/// 空白处右键应记录菜单位置并保持打开；点别处后关闭。
#[test]
fn real_app_blank_secondary_click_opens_context_menu() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段\n\n第二段", None);
    app.home = false;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    let blank = egui::pos2(600.0, 500.0);
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(blank),
            egui::Event::PointerButton {
                pos: blank,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    // egui 的 clicked/secondary_clicked 在松开时触发：按下帧只登记指针。
    assert!(
        app.editor.context_menu.is_none(),
        "按下右键不应提前打开菜单"
    );

    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerButton {
            pos: blank,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        app.editor.context_menu.is_some(),
        "松开右键后应打开上下文菜单"
    );

    // Area 首帧是 sizing pass（不可见），下一帧才真正绘制。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    fn collect_texts<'a>(shape: &'a egui::Shape, out: &mut Vec<&'a str>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text()),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_texts(s, out);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for cs in &shapes {
        collect_texts(&cs.shape, &mut texts);
    }
    for label in ["剪切", "拷贝", "粘贴", "段落", "格式", "插入"] {
        assert!(
            texts.contains(&label),
            "右键菜单应渲染「{label}」，实际：{texts:?}"
        );
    }
    assert!(
        app.editor.context_menu.is_some(),
        "没有点击外部时菜单应保持打开"
    );

    // 点「段落」→ 子菜单出现 → 点「一级标题」→ 当前块变成 H1。
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> egui::Pos2 {
        shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == name => {
                    Some(t.pos + t.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            // 跳过菜单栏/标签栏里的同名文字，只取正文区域。
            .filter(|p| p.y > 80.0)
            .last()
            .unwrap_or_else(|| panic!("未渲染：{name}"))
    };
    let paragraph = find(&shapes, "段落");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![pointer_button(paragraph, true)],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![pointer_button(paragraph, false)],
    );
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let h1 = find(&shapes, "一级标题");
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(h1, true)]);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(h1, false)]);
    assert!(
        matches!(
            app.doc.blocks.first(),
            Some(crate::ast::Block::Heading { level: 1, .. })
        ),
        "点击「一级标题」应把当前块变成一级标题"
    );

    // 再验证一次「在正文块上右键」：块矩形通常占满整行，不应因此失效。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let body = find(&shapes, "第一段");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(body),
            egui::Event::PointerButton {
                pos: body,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerButton {
            pos: body,
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(
        app.editor.context_menu.is_some(),
        "在正文块上右键也应稳定打开菜单"
    );
}

/// 代码块编辑态换语言必须一次点击完成：
/// 光标在代码块内（TextEdit 持有焦点）→ 点一次语言下拉（弹层打开）→
/// 点一次语言项（语言改掉、弹层收起）。这是用户报告的「需要点两次」bug 的原始场景。
#[test]
fn real_app_code_lang_combo_one_click() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();

    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    app.new_document();
    // 把第一块做成代码块并聚焦，让 TextEdit 持有焦点（= 光标在代码块里）
    app.doc.edit_block(0, "```rust\nfn main() {}\n```");
    app.editor.active = 0;
    app.editor.focus_req = Some(0);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let tab_key = app.tabs[app.active].uid;
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::unique(("block_edit", tab_key, 0))),
        "前置条件：光标应在代码块的编辑框里"
    );

    // 从测试钩子里拿到语言下拉按钮的矩形与弹层 id。
    // 只取本测试线程写入的记录：COMBO_TRACE 是全局的，并行跑别的
    // 渲染测试时也会往里 push，取 last() 会偶尔拿到别人的矩形（偶发失败）。
    let me = std::thread::current().id();
    let (rect, popup_id, ..) = *crate::render::COMBO_TRACE
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e.4 == me)
        .expect("代码块编辑态应渲染语言下拉");
    crate::render::COMBO_TRACE.lock().unwrap().clear();

    // 单击一次语言下拉（按下/松开分两帧），弹层必须打开
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(rect.center()),
            pointer_button(rect.center(), true),
        ],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![pointer_button(rect.center(), false)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    assert!(
        egui::Popup::is_id_open(&ctx, popup_id),
        "一次点击后语言下拉弹层应打开"
    );

    // 弹层里点第一个语言项（列表 A→Z，「纯文本」排最前）
    let popup_rect = ctx
        .read_response(popup_id)
        .map(|r| r.rect)
        .expect("弹层应已渲染出响应矩形");
    let item_pos = egui::pos2(popup_rect.left() + 30.0, popup_rect.top() + 45.0);
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(item_pos),
            pointer_button(item_pos, true),
        ],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![pointer_button(item_pos, false)],
    );
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    // 语言必须已改掉（rust → 其它），且一次点击就生效
    let lang = match app.doc.blocks.first() {
        Some(crate::ast::Block::Code { lang, .. }) => lang.clone(),
        other => panic!("第一块应仍是代码块，实际 {other:?}"),
    };
    assert_ne!(lang, "rust", "点一次语言项后语言应已改变");
    assert!(
        !egui::Popup::is_id_open(&ctx, popup_id),
        "选完语言后弹层应收起"
    );
}

/// 从 start 拖到 end（中间经过两段之间的空档），然后松开鼠标。
fn drag_select(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    start: egui::Pos2,
    end: egui::Pos2,
) {
    run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(start)]);
    run_real_frame(ctx, app, frame, vec![pointer_button(start, true)]);
    let mid = egui::pos2(start.x, (start.y + end.y) / 2.0);
    run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(mid)]);
    run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(end)]);
    run_real_frame(ctx, app, frame, vec![pointer_button(end, false)]);
}

/// 回归：上面一段文字、下面一个列表，从文字拖到列表下方的空白处时，
/// 选区必须覆盖到最后一个列表项（以前指针离开块矩形后端点不再更新，
/// 表现为「拖到底也选不全」）。
#[test]
fn drag_paragraph_to_below_list_selects_all() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("上面这一段文字\n\n- 列表项一\n- 列表项二\n- 列表项三", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |name: &str| -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("未渲染：{name}"))
    };
    let start = find("上面这一段文字") + egui::vec2(2.0, 7.0);
    // 落在最后一个列表项下方：这里没有任何块的矩形，正好覆盖兜底逻辑。
    let end = find("列表项三") + egui::vec2(200.0, 40.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);
    assert_eq!(
        crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel).as_deref(),
        Some("上面这一段文字\n\n列表项一\n\n列表项二\n\n列表项三"),
        "选区应覆盖整个列表，实际 {:?}",
        app.editor.cross_sel
    );
}

/// 回归：光标停在标题（活动 TextEdit）里，从标题拖到下方段落，
/// 跨块选区必须从标题里的按下字符一直覆盖到段落末端。
#[test]
fn drag_heading_to_paragraph_selects_all() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("# 标题\n\n下面是一段文字", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    // 活动标题的 TextEdit 只显示「标题」两个字（编辑态隐藏 `# ` 标记）；
    // 工具条/大纲/侧栏里也有同名文字，只取编辑区里（y 在工具条下方）那一个。
    let heading = shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "标题" => Some(t.pos),
            _ => None,
        })
        .filter(|p| p.y > 70.0)
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .expect("未渲染标题");
    // 从标题文字开头按下（保证锚点落在标题内），拖到段落最后一个字上。
    let start = heading + egui::vec2(2.0, 7.0);
    let (para, size) = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "下面是一段文字" && t.pos.y > 140.0 => {
                Some((t.pos, t.galley.size()))
            }
            _ => None,
        })
        .expect("未渲染段落");
    let end = para + egui::vec2(size.x - 4.0, 7.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);

    let text = crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel).unwrap_or_default();
    assert!(
        text.contains("下面是一段文字"),
        "从标题往下拖必须选中段落，实际选区 {:?}",
        app.editor.cross_sel
    );
    assert!(
        text.contains("标题"),
        "从标题按下时锚点必须落在标题内，实际选区 {:?}",
        app.editor.cross_sel
    );
}

/// 回归：活动段落里含 Markdown 标记（`**`）时，从可见文字处起拖，
/// 锚点必须换算到「所见文本」坐标，不能因隐藏标记而少选/漏选。
#[test]
fn drag_from_active_bold_paragraph_selects_visible_text() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("前缀**加粗**后缀\n\n下面一段文字", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |pred: &dyn Fn(&str) -> bool, min_y: f32| -> (egui::Pos2, egui::Vec2) {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if pred(t.galley.text()) && t.pos.y > min_y => {
                    Some((t.pos, t.galley.size()))
                }
                _ => None,
            })
            .expect("未渲染目标文字")
    };
    // 编辑态缓冲区仍是原文（`**` 被排成透明字），文字形状里带标记。
    let (src, _) = find(&|t| t.contains("加粗") && t.contains('*'), 70.0);
    let (para, size) = find(&|t| t.starts_with("下面一段文字"), 120.0);
    let start = src + egui::vec2(2.0, 7.0);
    let end = para + egui::vec2(size.x - 4.0, 7.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);

    let sel = app.editor.cross_sel.expect("应建立跨块选区");
    assert_eq!(
        (sel.0, sel.2),
        (0, 2),
        "锚点/终点块应为 0 和 2，实际 {sel:?}"
    );
    assert!(sel.1 <= 2, "锚点应按所见文本换算，实际 {sel:?}");
    let text = crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel).unwrap_or_default();
    assert!(text.contains("前缀"), "选区应从按下处开始，实际 {sel:?}");
    assert!(
        text.contains("下面一段文字"),
        "选区应覆盖下一段，实际 {sel:?}"
    );
}

/// 回归：反向从列表下方往上拖到段落，中间经过块间空档，也要选全。
#[test]
fn drag_list_up_to_paragraph_selects_all() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("上面这一段文字\n\n- 列表项一\n- 列表项二", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |name: &str| -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("未渲染：{name}"))
    };
    let start = find("列表项二") + egui::vec2(60.0, 7.0);
    let end = find("上面这一段文字") + egui::vec2(2.0, 7.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);
    assert_eq!(
        crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel).as_deref(),
        Some("上面这一段文字\n\n列表项一\n\n列表项二"),
        "反向拖选也应覆盖全部内容，实际 {:?}",
        app.editor.cross_sel
    );
}

/// 行内公式必须用原生形状渲染，而不是退化成 `$x^2$` 文本。
#[test]
fn real_app_inline_math_draws_texture() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("前 $x^2 + y^2$ 后\n\n第二段", None);
    app.home = false;
    // 第一个块处于编辑态时显示源码，不会走富文本渲染；把焦点放到第二段。
    app.editor.active = 2;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    fn has_native_math(shape: &egui::Shape) -> bool {
        match shape {
            // 原生公式是用默认白色纹理的实心 Mesh（区别于贴图 Mesh）。
            egui::Shape::Mesh(mesh) => {
                mesh.texture_id == egui::TextureId::default() && !mesh.vertices.is_empty()
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(has_native_math),
            _ => false,
        }
    }
    assert!(
        shapes.iter().any(|cs| has_native_math(&cs.shape)),
        "行内公式没有用原生形状绘制"
    );
    let text = |needle: &str| {
        shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Text(t) => t.galley.text().contains(needle),
            _ => false,
        })
    };
    assert!(text("前") && text("后"), "公式两侧的文字应正常保留");
    assert!(!text("x^2") && !text("$"), "公式源码不应作为文本出现");
}

/// 块级公式 `$$...$$` 也必须渲染成居中的 RaTeX 原生形状。
#[test]
fn real_app_display_math_draws_texture() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("$$\nE = mc^2\n$$\n\n第二段", None);
    app.home = false;
    app.editor.active = 2;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    fn has_native_math(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Mesh(mesh) => {
                mesh.texture_id == egui::TextureId::default() && !mesh.vertices.is_empty()
            }
            egui::Shape::Rect(r) => r.fill != egui::Color32::TRANSPARENT,
            egui::Shape::Vec(shapes) => shapes.iter().any(has_native_math),
            _ => false,
        }
    }
    assert!(
        shapes.iter().any(|cs| has_native_math(&cs.shape)),
        "块级公式没有用原生形状绘制"
    );
    let text = |needle: &str| {
        shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Text(t) => t.galley.text().contains(needle),
            _ => false,
        })
    };
    assert!(
        !text("E = mc^2") && !text("$$"),
        "块级公式源码不应作为文本出现"
    );
}

/// 回归：多列表格必须把所有列都排进版心，不能只显示第一列。
#[test]
fn real_app_table_shows_all_columns() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(
        "| A1 | B1 | C1 | D1 |\n| --- | --- | --- | --- |\n| A2 | B2 | C2 | D2 |",
        None,
    );
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let mut xs: Vec<f32> = ["A1", "B1", "C1", "D1"]
        .iter()
        .map(|name| {
            shapes
                .iter()
                .filter_map(|cs| match &cs.shape {
                    egui::Shape::Text(t) if t.galley.text() == *name => Some(t.pos),
                    _ => None,
                })
                .find(|p| p.y > 80.0)
                .unwrap_or_else(|| panic!("未渲染：{name}"))
                .x
        })
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs.len(), 4, "四个表头都应渲染");
    for pair in xs.windows(2) {
        assert!(
            pair[1] - pair[0] < 400.0,
            "列间距过大，后面的列会被挤出屏幕：{xs:?}"
        );
    }
}

/// 回归：光标在表格第一格时，按住鼠标拖到其它单元格，应把整张表格选入
/// 跨块选区（之后 Cmd+C 或表格工具条的「复制」都能直接复制整表）。
#[test]
fn drag_inside_table_selects_whole_table() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("| 名称 | 说明 |\n| --- | --- |\n| 甲 | 乙 |", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |name: &str| -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("未渲染：{name}"))
    };
    let start = find("名称") + egui::vec2(3.0, 3.0);
    let end = find("乙") + egui::vec2(3.0, 3.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);

    let len = app.doc.blocks[0].plain().chars().count();
    assert_eq!(
        app.editor.cross_sel,
        Some((0, 0, 0, len)),
        "拖动经过其它单元格后应选中整张表格"
    );
    let text = crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel)
        .expect("整表选区应有可复制文本");
    assert!(
        text.contains("名称") && text.contains("甲") && text.contains("乙"),
        "复制文本：{text}"
    );
}

/// 回归：点表格工具条的「删除表格」必须先弹确认窗；取消不删，确认才删。
#[test]
fn table_delete_button_asks_for_confirmation() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("| 名称 | 说明 |\n| --- | --- |\n| 甲 | 乙 |", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos + egui::vec2(3.0, 5.0)),
            _ => None,
        })
    };
    let click =
        |ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2| {
            run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(p)]);
            run_real_frame(ctx, app, frame, vec![pointer_button(p, true)]);
            run_real_frame(ctx, app, frame, vec![pointer_button(p, false)]);
            run_real_frame(ctx, app, frame, vec![]);
        };

    // 1) 点「删除表格」→ 弹出确认窗，表格还在。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let del = find(&shapes, "删除表格").expect("编辑态表格应有「删除表格」按钮");
    click(&ctx, &mut app, &mut frame, del);
    assert_eq!(app.table_delete_confirm, Some(0), "应进入待确认状态");
    assert!(
        matches!(app.doc.blocks[0], crate::ast::Block::Table { .. }),
        "弹窗阶段不能删除表格"
    );
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(find(&shapes, "确定删除").is_some(), "应显示确认弹窗");

    // 2) 取消：表格保留。
    let cancel = find(&shapes, "取消").expect("确认窗应有取消按钮");
    click(&ctx, &mut app, &mut frame, cancel);
    assert_eq!(app.table_delete_confirm, None);
    assert!(
        matches!(app.doc.blocks[0], crate::ast::Block::Table { .. }),
        "取消后表格应保留"
    );

    // 3) 再次点击并确认：表格变成空段落。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let del = find(&shapes, "删除表格").expect("删除表格按钮");
    click(&ctx, &mut app, &mut frame, del);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let confirm = find(&shapes, "确定删除").expect("确认按钮");
    click(&ctx, &mut app, &mut frame, confirm);
    assert_eq!(app.table_delete_confirm, None);
    assert!(
        !matches!(app.doc.blocks[0], crate::ast::Block::Table { .. }),
        "确认后表格应被删除：{:?}",
        app.doc.blocks[0].source()
    );
}

/// 回归：在引用里插入代码块后，点代码块的功能栏下方留白/分隔线也要能开始写代码。
/// 以前这类点击会被当成「点空白」，把光标甩到文档末尾，看起来像代码块不能编辑。
#[test]
fn click_quoted_code_padding_focuses_code_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("> 第一行\n> 第二行", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeBlock, &ctx);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 1, "插入后代码块应为活动块");
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let combo = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "纯文本" => Some(t.pos),
            _ => None,
        })
        .expect("代码块应显示语言下拉");
    // 功能栏下方 25px：面板内、代码文本行上方的留白/分隔线区域。
    let p = combo + egui::vec2(60.0, 25.0);
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(p)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, true)]);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, false)]);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 1, "点代码块留白不应把光标甩到别的块");
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new(("block_edit", 0, 1))),
        "点代码块留白应聚焦代码编辑框"
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("x".into())],
    );
    let blocks: Vec<String> = app
        .doc
        .blocks
        .iter()
        .map(|b| b.source().to_string())
        .collect();
    assert!(blocks[1].contains('x'), "输入应进入代码块：{blocks:?}");
    assert!(!blocks[2].contains('x'), "不应写进后面的引用行：{blocks:?}");
}

/// 回归：插入空代码块 → 点别处让它变成渲染态 → 再点代码区空白处，
/// 必须进入编辑并能输入。以前空代码块只把文字 label 当点击区，
/// 空白处命不中，active/focus 都不变，看起来就是「代码块不能输入」。
#[test]
fn clicking_rendered_empty_code_block_enters_edit() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("上面段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeBlock, &ctx);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos + egui::vec2(3.0, 5.0)),
            _ => None,
        })
    };
    // 点回上面的段落，让代码块退出编辑态（变成渲染态）。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let para = find(&shapes, "上面段落").expect("段落");
    for ev in [
        vec![egui::Event::PointerMoved(para)],
        vec![pointer_button(para, true)],
        vec![pointer_button(para, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 0);

    // 渲染态代码块的代码区留白（功能栏下方偏左）：点它应重新进入编辑。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let copy = find(&shapes, "复制").expect("渲染态复制按钮");
    let p = copy + egui::vec2(-120.0, 30.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 1, "点空代码块应进入编辑");
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new(("block_edit", 0, 1))),
        "应聚焦代码编辑框"
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("x".into())],
    );
    let blocks: Vec<String> = app
        .doc
        .blocks
        .iter()
        .map(|b| b.source().to_string())
        .collect();
    assert!(blocks[1].contains('x'), "输入应进入代码块：{blocks:?}");
}

/// 回归：拖选正文后按工具条上的按钮（按住、轻微移动），选区必须保持不变。
/// 以前工具条坐标会被钳进视口，点「引用」时选区终点被改到工具条正下方
/// 那一行的字符，看起来就是「选区突然变成一大片」。
#[test]
fn toolbar_press_does_not_change_selection() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段文字\n\n第二段文字", None);
    app.home = false;
    app.editor.active = 1;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    // 在第一段（渲染态）里拖选一段文字。
    let text = find(&shapes, "第一段文字").expect("第一段");
    let p1 = text + egui::vec2(2.0, 8.0);
    let p2 = text + egui::vec2(40.0, 8.0);
    for ev in [
        vec![egui::Event::PointerMoved(p1)],
        vec![pointer_button(p1, true)],
        vec![egui::Event::PointerMoved(p2)],
        vec![pointer_button(p2, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let selected = app.editor.cross_sel.expect("应当有拖选选区");
    assert!(selected.3 > selected.1, "拖选不能是空选区：{selected:?}");

    // 按在工具条「引用」按钮上并轻微移动：选区不能被改写。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let quote = find(&shapes, "引用").expect("引用按钮");
    let p = quote + egui::vec2(8.0, 5.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![egui::Event::PointerMoved(p + egui::vec2(10.0, 0.0))],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(
        app.editor.cross_sel,
        Some(selected),
        "按工具条不能改变正文选区"
    );
}

/// 回归：刚插入空代码块、还没输入任何字时，鼠标移开再点代码区空白处
/// 也要能继续输入（不能因为「点击区只是文字宽度」而点不中）。
#[test]
fn moving_away_and_reclicking_empty_code_block_keeps_editing() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeBlock, &ctx);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    // 鼠标离开代码块（只移动，不点击）。
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(egui::pos2(900.0, 500.0))],
    );
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let combo = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "纯文本" => Some(t.pos),
            _ => None,
        })
        .expect("代码块语言下拉");
    // 语言下拉右侧下方的代码区留白。
    let p = combo + egui::vec2(100.0, 45.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 1);
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new(("block_edit", 0, 1))),
        "点回代码区应重新聚焦"
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("x".into())],
    );
    let blocks: Vec<String> = app
        .doc
        .blocks
        .iter()
        .map(|b| b.source().to_string())
        .collect();
    assert!(blocks[1].contains('x'), "输入应进入代码块：{blocks:?}");
}

/// 代码组：插入后默认 Java / Python 两个标签，切换标签后各写各的代码，
/// 两份代码互不覆盖；源码是规范的四反引号围栏格式。
#[test]
fn code_group_inserts_and_switches_tabs() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeGroup, &ctx);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    match &app.doc.blocks[1] {
        crate::ast::Block::CodeGroup { items, .. } => {
            assert_eq!(items.len(), 2, "默认两个标签");
            assert_eq!(items[0].lang, "java");
            assert_eq!(items[1].lang, "python");
        }
        other => panic!("应插入代码组：{other:?}"),
    }

    // 在 Java 标签下输入。
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("int a = 1;".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let src = app.doc.blocks[1].source().to_string();
    assert!(src.contains("int a = 1;"), "Java 代码应写回：{src}");

    // 点 Python 标签切换，再输入 Python 代码。
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let py = find(&shapes, "python").expect("代码组应有 python 标签");
    let p = py + egui::vec2(5.0, 5.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.group_tab(1), 1, "应切到 python 标签");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("a = 1".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let src = app.doc.blocks[1].source().to_string();
    assert!(src.contains("int a = 1;"), "切换后 Java 代码不能丢：{src}");
    assert!(src.contains("a = 1"), "Python 代码应写回：{src}");
    assert!(src.starts_with("````code-group"), "{src}");
    assert!(
        src.contains("```java") && src.contains("```python"),
        "{src}"
    );
}

/// 渲染态代码组：点标签只切换显示的语言，不进入编辑态。
#[test]
fn rendered_code_group_tab_switches_displayed_language() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeGroup, &ctx);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("int a = 1;".into())],
    );
    // 切换标签写入 Python 代码。
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let py = find(&shapes, "python").expect("python 标签");
    let p = py + egui::vec2(5.0, 5.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    // 等两帧让切标签后的焦点请求生效，再输入。
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.group_tab(1), 1, "应切到 python 标签");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("a = 1".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    // 点回上面的段落，让代码组进入渲染态。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let para = text_center(&shapes, "段落").expect("段落");
    for ev in [
        vec![egui::Event::PointerMoved(para)],
        vec![pointer_button(para, true)],
        vec![pointer_button(para, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 0, "代码组应变成渲染态");

    // 渲染态默认显示 Python（上次停留的标签），点 Java 切回。
    let has = |shapes: &[egui::epaint::ClippedShape], needle: &str| {
        shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Text(t) => t.galley.text().contains(needle),
            _ => false,
        })
    };
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(has(&shapes, "a = 1"), "渲染态应显示 python 代码");
    let java = find(&shapes, "java").expect("java 标签");
    let p = java + egui::vec2(5.0, 5.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.group_tab(1), 0, "点标签应切到 java");
    assert_eq!(app.editor.active, 0, "点标签不应进入编辑态");
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(has(&shapes, "int a = 1;"), "渲染态应显示 java 代码");
}

/// 代码组的按钮：复制只复制当前标签的代码；折叠/展开、删除（带确认）与代码块一致。
#[test]
fn code_group_copy_fold_and_delete_buttons() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeGroup, &ctx);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("int a = 1;".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let click =
        |ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2| {
            for ev in [
                vec![egui::Event::PointerMoved(p)],
                vec![pointer_button(p, true)],
                vec![pointer_button(p, false)],
            ] {
                run_real_frame(ctx, app, frame, ev);
            }
            for _ in 0..2 {
                run_real_frame(ctx, app, frame, vec![]);
            }
        };

    // 1) 复制：编辑态点「复制」，剪贴板只拿到当前标签（java）的代码。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let copy = find(&shapes, "复制").expect("编辑态应有复制按钮");
    let p = copy + egui::vec2(5.0, 5.0);
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(p)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, true)]);
    let (_, copied) =
        run_real_frame_ext(&ctx, &mut app, &mut frame, vec![pointer_button(p, false)]);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(copied, "int a = 1;", "复制应只复制当前标签的代码");

    // 2) 折叠：点「折叠」后块进入折叠态，按钮变成「展开」；再点展开恢复。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let fold = find(&shapes, "折叠").expect("应有折叠按钮");
    click(&ctx, &mut app, &mut frame, fold + egui::vec2(5.0, 5.0));
    assert!(app.editor.folded.contains(&1), "折叠状态应记录在代码组上");
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(find(&shapes, "已折叠").is_some() || find(&shapes, "展开").is_some());
    let unfold = find(&shapes, "展开").expect("折叠后应显示展开按钮");
    click(&ctx, &mut app, &mut frame, unfold + egui::vec2(5.0, 5.0));
    assert!(!app.editor.folded.contains(&1), "展开后应移除折叠状态");

    // 3) 删除：先取消，再确认删除。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let del = find(&shapes, "删除").expect("应有删除按钮");
    click(&ctx, &mut app, &mut frame, del + egui::vec2(5.0, 5.0));
    assert_eq!(app.code_delete_confirm, Some(1), "应弹出删除确认");
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let cancel = find(&shapes, "取消").expect("确认窗应有取消");
    click(&ctx, &mut app, &mut frame, cancel + egui::vec2(3.0, 3.0));
    assert_eq!(app.code_delete_confirm, None);
    assert!(
        matches!(app.doc.blocks[1], crate::ast::Block::CodeGroup { .. }),
        "取消后代码组应保留"
    );

    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let del = find(&shapes, "删除").expect("删除按钮");
    click(&ctx, &mut app, &mut frame, del + egui::vec2(5.0, 5.0));
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let confirm = find(&shapes, "确定删除").expect("确认按钮");
    click(&ctx, &mut app, &mut frame, confirm + egui::vec2(3.0, 3.0));
    assert_eq!(
        app.doc.blocks.len(),
        1,
        "确认后代码组应整块删除：{:?}",
        app.doc.source()
    );
}

/// 代码组的语言下拉：改的是当前标签的语言，tab 标签与源码围栏同步更新。
#[test]
fn code_group_language_combo_changes_active_tab() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeGroup, &ctx);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    // 点回段落，让代码组进入渲染态（语言下拉在渲染态标题栏里）。
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let para = text_center(&shapes, "段落").expect("段落");
    for ev in [
        vec![egui::Event::PointerMoved(para)],
        vec![pointer_button(para, true)],
        vec![pointer_button(para, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 0);

    // 从测试钩子取语言下拉按钮矩形（并行测试时只取本线程的记录）。
    crate::render::COMBO_TRACE.lock().unwrap().clear();
    let _ = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let me = std::thread::current().id();
    let (rect, popup_id, ..) = *crate::render::COMBO_TRACE
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|e| e.4 == me)
        .expect("渲染态代码组应有语言下拉");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![
            egui::Event::PointerMoved(rect.center()),
            pointer_button(rect.center(), true),
        ],
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![pointer_button(rect.center(), false)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    assert!(
        egui::Popup::is_id_open(&ctx, popup_id),
        "语言下拉弹层应打开"
    );

    // 选「c」（A→Z 列表里靠前，必定可见）。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let c = find(&shapes, "c").expect("弹层里应有 c 语言");
    let p = c + egui::vec2(3.0, 3.0);
    for ev in [
        vec![egui::Event::PointerMoved(p)],
        vec![pointer_button(p, true)],
        vec![pointer_button(p, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    match &app.doc.blocks[1] {
        crate::ast::Block::CodeGroup { items, .. } => {
            assert_eq!(items[0].lang, "c", "java 标签应改成 c：{items:?}");
            assert_eq!(items[1].lang, "python", "其它标签不受影响：{items:?}");
        }
        other => panic!("代码组应保留：{other:?}"),
    }
    assert!(
        app.doc.blocks[1].source().contains("```c"),
        "源码围栏应更新"
    );
}

/// 代码组标签的增删：编辑态与渲染态都能「+」新增、「−」删除当前标签，
/// 至少保留一个；删减时其它标签的代码不受影响。
#[test]
fn code_group_add_and_remove_tabs() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("段落", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    app.execute(Command::CodeGroup, &ctx);
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("int a = 1;".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    // 工具条/标签栏也有 "+"，代码组标题栏的按钮是最靠后的那个。
    let find_last = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().rev().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let find_full = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let click =
        |ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2| {
            for ev in [
                vec![egui::Event::PointerMoved(p)],
                vec![pointer_button(p, true)],
                vec![pointer_button(p, false)],
            ] {
                run_real_frame(ctx, app, frame, ev);
            }
            for _ in 0..2 {
                run_real_frame(ctx, app, frame, vec![]);
            }
        };
    let tabs = |app: &MarkdApp| -> Vec<(String, String)> {
        match &app.doc.blocks[1] {
            crate::ast::Block::CodeGroup { items, .. } => items
                .iter()
                .map(|i| (i.lang.clone(), i.code.clone()))
                .collect(),
            other => panic!("代码组应保留：{other:?}"),
        }
    };

    // 1) 编辑态点「+」：变成三个标签，自动切到新标签并可输入。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let plus = find_last(&shapes, "+").expect("代码组应有新增标签按钮");
    click(&ctx, &mut app, &mut frame, plus + egui::vec2(5.0, 5.0));
    assert_eq!(tabs(&app).len(), 3, "应新增一个标签");
    assert_eq!(app.editor.group_tab(1), 2, "应自动切到新标签");
    assert_eq!(tabs(&app)[0].1, "int a = 1;", "原有 Java 代码不能丢");
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("print(1)".into())],
    );
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(tabs(&app)[2].1, "print(1)", "新标签可正常输入");

    // 2) 编辑态点「−」：删掉当前（第三个）标签，前两个保留。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let minus = find_full(&shapes, "−").expect("应有删除标签按钮");
    click(&ctx, &mut app, &mut frame, minus + egui::vec2(5.0, 5.0));
    let t = tabs(&app);
    assert_eq!(t.len(), 2, "应删除当前标签：{t:?}");
    assert_eq!(t[0], ("java".to_string(), "int a = 1;".to_string()));
    assert_eq!(t[1].0, "python", "其它标签保持原样：{t:?}");
    assert_eq!(app.editor.group_tab(1), 1, "删除后切到相邻标签");

    // 3) 渲染态点「+」：新增后直接进入编辑。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let para = text_center(&shapes, "段落").expect("段落");
    for ev in [
        vec![egui::Event::PointerMoved(para)],
        vec![pointer_button(para, true)],
        vec![pointer_button(para, false)],
    ] {
        run_real_frame(&ctx, &mut app, &mut frame, ev);
    }
    for _ in 0..2 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 0);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let plus = find_last(&shapes, "+").expect("渲染态代码组应有新增按钮");
    click(&ctx, &mut app, &mut frame, plus + egui::vec2(5.0, 5.0));
    assert_eq!(tabs(&app).len(), 3, "渲染态也应能新增标签");
    assert_eq!(app.editor.active, 1, "新增后应进入编辑");
    assert_eq!(app.editor.group_tab(1), 2);

    // 4) 连续「−」到只剩一个：再点不再删除（按钮已禁用）。
    for expected in [2usize, 1] {
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let minus = find_full(&shapes, "−").expect("删除标签按钮");
        click(&ctx, &mut app, &mut frame, minus + egui::vec2(5.0, 5.0));
        assert_eq!(tabs(&app).len(), expected, "应删到 {expected} 个标签");
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    if let Some(minus) = find_full(&shapes, "−") {
        click(&ctx, &mut app, &mut frame, minus + egui::vec2(5.0, 5.0));
    }
    assert_eq!(tabs(&app).len(), 1, "至少保留一个标签");
    assert_eq!(tabs(&app)[0].0, "java", "保留的应是 Java");
}
/// 回归：引用按钮点一次就生效。
/// 1) 按下到松开鼠标轻微抖动（7px）仍算点击（以前会被当成拖动，要点两次）；
/// 2) 双击引用按钮只加一次引用（第二击不再立刻取消掉第一击）。
#[test]
fn quote_button_single_click_survives_jitter_and_double_click() {
    let setup = || {
        let ctx = egui::Context::default();
        // 与打包应用保持一致：放宽点击判定（见 MarkdApp::new）。
        ctx.memory_mut(|m| {
            let o = &mut m.options.input_options;
            o.max_click_dist = 10.0;
            o.max_click_duration = 1.5;
        });
        let mut app = test_app();
        let frame = eframe::Frame::_new_kittest();
        app.doc = Document::new("第一段文字\n\n第二段文字", None);
        app.home = false;
        app.editor.active = 0;
        (ctx, app, frame)
    };
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };

    // 1) 轻微抖动的一次点击：引号应立刻加上。
    {
        let (ctx, mut app, mut frame) = setup();
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let p = find(&shapes, "引用").expect("引用按钮") + egui::vec2(5.0, 5.0);
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::PointerMoved(p)],
        );
        run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, true)]);
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::PointerMoved(p + egui::vec2(7.0, 0.0))],
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![pointer_button(p + egui::vec2(7.0, 0.0), false)],
        );
        for _ in 0..2 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        assert_eq!(
            app.doc.blocks[0].source(),
            "> 第一段文字",
            "轻微抖动的单击也应生效"
        );
    }

    // 2) 双击：第一次已经加上引用，第二击不能把它取消。
    {
        let (ctx, mut app, mut frame) = setup();
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let p = find(&shapes, "引用").expect("引用按钮") + egui::vec2(5.0, 5.0);
        for _ in 0..2 {
            run_real_frame(
                &ctx,
                &mut app,
                &mut frame,
                vec![egui::Event::PointerMoved(p)],
            );
            run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, true)]);
            run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, false)]);
        }
        for _ in 0..2 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        assert_eq!(
            app.doc.blocks[0].source(),
            "> 第一段文字",
            "双击引用按钮不应互相抵消"
        );
    }
}

/// Mermaid 下拉：覆盖全部 24 种常用图类型，每个模板都是合法的 mermaid 围栏，
/// 且插入后能解析成单个图表块（而不是普通代码块）。
#[test]
fn mermaid_templates_cover_all_types() {
    for need in [
        "流程图",
        "时序图",
        "类图",
        "状态图",
        "ER 图",
        "甘特图",
        "饼图",
        "旅程图",
        "Git 图",
        "思维导图",
        "时间线",
        "象限图",
        "XY 图表",
        "雷达图",
        "桑基图",
        "矩形树图",
        "C4 上下文图",
        "块图",
        "需求图",
        "数据包图",
        "看板",
        "泳道图",
        "Cynefin 图",
        "铁路图",
    ] {
        assert!(
            super::menu::MERMAID_TEMPLATES
                .iter()
                .any(|(l, _)| *l == need),
            "缺少图类型：{need}"
        );
    }
    for (label, tpl) in super::menu::MERMAID_TEMPLATES {
        assert!(
            tpl.starts_with("```mermaid\n") && tpl.ends_with("```"),
            "{label} 模板应是闭合的 mermaid 围栏：{tpl}"
        );
        let body = tpl
            .trim_start_matches("```mermaid\n")
            .trim_end_matches("```")
            .trim();
        let first = body.lines().next().unwrap_or_default();
        assert!(
            !first.is_empty() && !first.starts_with(' '),
            "{label} 首行应是图类型：{first:?}"
        );
        let doc = Document::new(tpl, None);
        assert_eq!(
            doc.blocks.len(),
            1,
            "{label} 应解析为单块：{:?}",
            doc.blocks
        );
        assert!(
            matches!(doc.blocks[0], crate::ast::Block::Diagram { .. }),
            "{label} 应解析为图表块，实际 {:?}",
            doc.blocks[0]
        );
    }
}
