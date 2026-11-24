//! 应用层单元测试。

use super::*;

/// 标题栏 + 工具条占据窗口顶部；正文从这条线以下开始。
const TOP_BARS_HEIGHT: f32 = TITLEBAR_HEIGHT + TOOLBAR_HEIGHT;
const EDITOR_TOP: f32 = TOP_BARS_HEIGHT + 5.0;
/// 下半区盒子按钮的起始位置，用来把功能区选项卡文字排除在工具按钮之外。
const TOOLBOX_AREA_TOP: f32 = TITLEBAR_HEIGHT + TOOLBAR_RESERVED_HEIGHT + 3.0;

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
        stats: WritingStats::new(stats_cache.words),
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
        system_dark: false,
        menu_guard: false,
        workspace_dialog: false,
        quick_cache_dir: String::new(),
        quick_cache: Vec::new(),
        menu_queue: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        terminal: Terminal::new(std::env::temp_dir()),
        settings: Default::default(),
        home: true,
        toolbar_tab: ToolbarTab::Start,
        toolbar_collapsed: false,
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
    run_real_frame_ext_with_width(ctx, app, frame, events, 1280.0)
}

/// 同上，但可指定窗口宽度：盒子多到 1280 放不下时，用更宽的窗口验证整组居中。
fn run_real_frame_ext_with_width(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    events: Vec<egui::Event>,
    width: f32,
) -> (Vec<egui::epaint::ClippedShape>, String) {
    use eframe::App as _;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 800.0),
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
    run_real_frame_stable_ext_with_width(ctx, app, frame, events, 1280.0)
}

/// 同上，但可指定窗口宽度；点击类测试在宽窗口里才能碰到所有盒子。
fn run_real_frame_stable_ext_with_width(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    events: Vec<egui::Event>,
    width: f32,
) -> (Vec<egui::epaint::ClippedShape>, String) {
    use eframe::App as _;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 800.0),
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
        // 标题栏 + 3.2 cm 工具条共 TOP_BARS_HEIGHT；正文从 EDITOR_TOP 开始。
        .filter(|t| t.pos.y > EDITOR_TOP)
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
        // 工具条加高到 3.2 cm 后，130 已落在工具条里，正文上沿改成 175。
        (3, egui::pos2(600.0, 175.0)),
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
                (p.y > EDITOR_TOP).then_some(p)
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
        Some(egui::Id::unique(("block_edit", tab_key, 2))),
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
            .filter(|p| p.y > EDITOR_TOP)
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
        .filter(|p| p.y > EDITOR_TOP)
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .expect("未渲染标题");
    // 从标题文字开头按下（保证锚点落在标题内），拖到段落最后一个字上。
    let start = heading + egui::vec2(2.0, 7.0);
    let (para, size) = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "下面是一段文字" && t.pos.y > EDITOR_TOP => {
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
    let (src, _) = find(&|t| t.contains("加粗") && t.contains('*'), EDITOR_TOP);
    let (para, size) = find(&|t| t.starts_with("下面一段文字"), EDITOR_TOP);
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
                .find(|p| p.y > EDITOR_TOP)
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

/// 表格里拖选：经过哪几格就选哪几格，Cmd+C 复制的正是这几格。
/// 之前一拖就把整张表变成跨块选区，想复制一行都不行（用户反馈）。
/// 想连上下文一起选走仍然可以：把指针拖出表格边界，整张表才进跨块选区。
#[test]
fn drag_inside_table_selects_cells_and_copies_them() {
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

    // 只在表格里拖：从「甲」到「乙」= 第二行那两格。
    let start = find("甲") + egui::vec2(3.0, 3.0);
    let end = find("乙") + egui::vec2(3.0, 3.0);
    drag_select(&ctx, &mut app, &mut frame, start, end);
    assert_eq!(
        app.editor.table_sel,
        Some((0, (1, 0), (1, 1))),
        "拖过的两个单元格应成为表格选区（行列含表头行）"
    );
    assert!(
        app.editor.cross_sel.is_none(),
        "表内拖选不该把整张表变成跨块选区：{:?}",
        app.editor.cross_sel
    );
    let (_, copied) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Copy]);
    assert_eq!(
        copied, "甲\t乙",
        "Cmd+C 应只复制选中的单元格，实际 {copied:?}"
    );
    // 剪切不该删掉表格内容，只复制。
    let (_, cut) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![egui::Event::Cut]);
    assert_eq!(cut, "甲\t乙", "Cmd+X 应退化成复制，实际 {cut:?}");
    assert!(
        app.doc.source().contains("甲") && app.doc.source().contains("乙"),
        "表格单元格选区不该改动文档：{:?}",
        app.doc.source()
    );

    // 拖出表格边界：整张表进跨块选区（要连着上下文一起选时靠它）。
    app.editor.table_sel = None;
    let out = find("乙") + egui::vec2(3.0, 200.0);
    drag_select(&ctx, &mut app, &mut frame, start, out);
    let len = app.doc.blocks[0].plain().chars().count();
    assert_eq!(
        app.editor.cross_sel,
        Some((0, 0, 0, len)),
        "拖出表格后应把整张表交给跨块选区"
    );
    assert!(app.editor.table_sel.is_none(), "两套选区不能同时存在");
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
        Some(egui::Id::unique(("block_edit", 0, 1))),
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
        Some(egui::Id::unique(("block_edit", 0, 1))),
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
        Some(egui::Id::unique(("block_edit", 0, 1))),
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
    // 语言下拉有 460+ 行，靠滚动定位某一行太脆：先往搜索框塞关键词，
    // 列表只剩命中项。过滤串存在下拉自己的临时内存里，键与 language_combo 一致。
    ctx.memory_mut(|m| {
        m.data.insert_temp(
            egui::Id::unique(("code_group_lang", 1)).with("filter"),
            "flutter".to_string(),
        )
    });
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

    // 搜「flutter」后列表里只剩这一行（别名各自成项），点它就把围栏写成 ```flutter。
    // 弹层里的搜索框也渲染同样的关键词文字，列表项在它之后绘制，所以取最后一个匹配。
    let find_last = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().rev().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    };
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let c = find_last(&shapes, "flutter").expect("弹层里应有 flutter 这一行");
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
            assert_eq!(items[0].lang, "flutter", "java 标签应改成 flutter：{items:?}");
            assert_eq!(items[1].lang, "python", "其它标签不受影响：{items:?}");
        }
        other => panic!("代码组应保留：{other:?}"),
    }
    assert!(
        app.doc.blocks[1].source().contains("```flutter"),
        "源码围栏应更新"
    );
    // flutter 是 dart 的别名写法：围栏里是 flutter，高亮仍按 dart 走
    assert_eq!(
        crate::theme::lang_family("flutter"),
        crate::theme::lang_family("dart"),
        "别名写法的高亮应与规范名等价"
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

/// 取编辑区里（y > min_y）第一处包含 `key` 的文字形状，返回 (落点, galley)。
fn editor_galley<'a>(
    shapes: &'a [egui::epaint::ClippedShape],
    key: &str,
    min_y: f32,
) -> (egui::Pos2, &'a egui::Galley) {
    shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text().contains(key) && t.pos.y > min_y => {
                Some((t.pos, &*t.galley))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("编辑区里未渲染「{key}」"))
}

/// galley 里 `key` 首字符的下标。
fn char_index_of(g: &egui::Galley, key: &str) -> usize {
    let text = g.text();
    let byte = text.find(key).unwrap_or_else(|| panic!("galley 里没有「{key}」"));
    text[..byte].chars().count()
}

/// 第 ch 个插入点的屏幕坐标。
fn point_at(shapes: &[egui::epaint::ClippedShape], key: &str, min_y: f32, ch: usize) -> egui::Pos2 {
    let (origin, g) = editor_galley(shapes, key, min_y);
    let r = g.pos_from_cursor(egui::text::CCursor::new(ch.min(g.text().chars().count())));
    origin + r.center().to_vec2()
}

/// 按 `key` 首字符偏移 k 个字符处的插入点（用来把指针精确落在某个词上）。
fn point_of_key(shapes: &[egui::epaint::ClippedShape], key: &str, min_y: f32, k: usize) -> egui::Pos2 {
    let (origin, g) = editor_galley(shapes, key, min_y);
    let idx = char_index_of(g, key) + k;
    let r = g.pos_from_cursor(egui::text::CCursor::new(idx.min(g.text().chars().count())));
    origin + r.center().to_vec2()
}

/// 逐帧细粒度拖动（每 8px 一步），比 `drag_select` 的三点式更接近真实鼠标。
fn drag_select_fine(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    start: egui::Pos2,
    end: egui::Pos2,
) {
    run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(start)]);
    run_real_frame(ctx, app, frame, vec![pointer_button(start, true)]);
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let steps = ((dx.hypot(dy) / 8.0).ceil() as usize).max(1);
    for k in 1..=steps {
        let t = k as f32 / steps as f32;
        run_real_frame(
            ctx,
            app,
            frame,
            vec![egui::Event::PointerMoved(egui::pos2(
                start.x + dx * t,
                start.y + dy * t,
            ))],
        );
    }
    run_real_frame(ctx, app, frame, vec![pointer_button(end, false)]);
}

/// 用户反馈①：段落里有行内代码时拖不动。活动块含 `code_block`，
/// 从段首拖到段末，选中文字必须等于整段可见文本。
#[test]
fn drag_inside_active_paragraph_with_inline_code_selects_exact() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("前缀 `code_block` 后缀文字内容\n\n下面一段文字", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let (_, g) = editor_galley(&shapes, "code_block", EDITOR_TOP);
    let n = g.text().chars().count();
    let start = point_at(&shapes, "code_block", EDITOR_TOP, 0);
    let end = point_at(&shapes, "code_block", EDITOR_TOP, n);
    drag_select_fine(&ctx, &mut app, &mut frame, start, end);
    let sel = app.editor.cross_sel.expect("活动块内含行内代码应能拖选出选区");
    let got = crate::editor::cross_sel_text(&app.doc, Some(sel)).unwrap_or_default();
    let want = app.doc.blocks[0].plain();
    assert_eq!(
        got.replace(' ', "").replace('_', ""),
        want.replace(' ', "").replace('_', ""),
        "拖到段末应选中整段可见文字，实际选区 {sel:?}（{got:?} vs {want:?}）"
    );
}

/// 用户反馈②：上面是标题、下面是含行内代码的段落，从标题拖进段落要选全。
#[test]
fn drag_heading_into_paragraph_with_inline_code_selects_exact() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(
        "# 标题文字\n\n段落里有 `inline_code` 后面的话也要一起选中\n\n第三段",
        None,
    );
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = point_at(&shapes, "标题文字", EDITOR_TOP, 0);
    let end = {
        let (_, g) = editor_galley(&shapes, "inline_code", EDITOR_TOP);
        point_at(&shapes, "inline_code", EDITOR_TOP, g.text().chars().count())
    };
    drag_select_fine(&ctx, &mut app, &mut frame, start, end);
    let sel = app.editor.cross_sel.expect("标题→含行内代码段落应建立选区");
    let got = crate::editor::cross_sel_text(&app.doc, Some(sel)).unwrap_or_default();
    let want = app.doc.blocks[2].plain();
    assert!(
        got.replace(' ', "").contains(&want.replace(' ', "")),
        "段落应整段选中，实际选区 {sel:?} 文本 {got:?}，应为 {want:?}"
    );
    assert!(got.contains("标题文字"), "标题文字应在选区内，实际 {got:?}");
}

/// 用户反馈③：按下点正好落在行内代码的字符上，再往下拖到下一段末尾。
#[test]
fn drag_started_on_inline_code_chip_selects_next_block() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段带 `some_code` 的文字内容\n\n第二段普通文字内容", None);
    app.home = false;
    app.editor.active = 2;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = point_of_key(&shapes, "some_code", EDITOR_TOP, 3);
    let end = {
        let (_, g) = editor_galley(&shapes, "第二段普通文字", EDITOR_TOP);
        point_at(&shapes, "第二段普通文字", EDITOR_TOP, g.text().chars().count())
    };
    drag_select_fine(&ctx, &mut app, &mut frame, start, end);
    let sel = app.editor.cross_sel.expect("从行内代码片上起拖应建立选区");
    let got = crate::editor::cross_sel_text(&app.doc, Some(sel)).unwrap_or_default();
    assert!(
        got.contains("第二段普通文字内容"),
        "终点段应整段选中，实际选区 {sel:?} 文本 {got:?}"
    );
    assert!(
        got.contains("some_code") || got.contains("的文字内容"),
        "起点段代码片之后的文字应进入选区，实际 {got:?}"
    );
}

/// 反向拖选终点落在行内代码上：终点字符要按「所见文本」换算，不能偏。
#[test]
fn drag_ending_on_inline_code_maps_to_visible_index() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("第一段普通文字内容\n\n第二段带 `some_code` 的文字内容", None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = point_at(&shapes, "第一段普通文字", EDITOR_TOP, 0);
    // 终点精确落在代码片中间（渲染态 galley 无隐藏标记，字符下标即可见下标）
    let end = point_of_key(&shapes, "some_code", EDITOR_TOP, 4);
    drag_select_fine(&ctx, &mut app, &mut frame, start, end);
    let sel = app.editor.cross_sel.expect("拖到行内代码处应建立选区");
    assert_eq!(sel.0.max(sel.2), 2, "终点块应是含行内代码的段落，实际 {sel:?}");
    let plain2 = app.doc.blocks[2].plain();
    let ec = if sel.2 == 2 { sel.3 } else { sel.1 };
    let upto: String = plain2.chars().take(ec.min(plain2.chars().count())).collect();
    assert!(
        upto.contains("第二段带") && upto.contains("some"),
        "终点应落在代码片内部（可见文本口径），实际 ec={ec} 前缀 {upto:?}"
    );
    assert!(!upto.contains('_'), "终点不应越过代码片，实际 {upto:?}");
}

/// 拖选路径形态。
#[derive(Copy, Clone, Debug)]
enum PathStyle {
    Fine,   // 每 8px 一步
    Coarse, // 按下 → 中点 → 终点（旧三点式）
    Fast,   // 两大跳（真实快速甩鼠标）
    LShaped, // 先竖直再水平（用户斜着拖出块外）
    LeftMargin, // 走左侧留白（x 比文字左缘更偏左）
    RightBlank, // 终点落在文字右侧空白
}

/// 按形态执行一次拖动。
fn drag_with_style(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    start: egui::Pos2,
    end: egui::Pos2,
    style: PathStyle,
) {
    let move_to = |ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2| {
        run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(p)]);
    };
    run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(start)]);
    run_real_frame(ctx, app, frame, vec![pointer_button(start, true)]);
    let put = |ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2| {
        move_to(ctx, app, frame, p);
    };
    match style {
        PathStyle::Fine => {
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            let steps = ((dx.hypot(dy) / 8.0).ceil() as usize).max(1);
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                put(ctx, app, frame, egui::pos2(start.x + dx * t, start.y + dy * t));
            }
        }
        PathStyle::Coarse => {
            put(ctx, app, frame, egui::pos2(start.x, (start.y + end.y) / 2.0));
            put(ctx, app, frame, end);
        }
        PathStyle::Fast => {
            put(ctx, app, frame, egui::pos2(start.x + (end.x - start.x) * 0.6, start.y));
            put(ctx, app, frame, end);
        }
        PathStyle::LShaped => {
            put(ctx, app, frame, egui::pos2(start.x, end.y));
            put(ctx, app, frame, end);
        }
        PathStyle::LeftMargin => {
            let left = egui::pos2(start.x - 24.0, (start.y + end.y) * 0.5);
            put(ctx, app, frame, left);
            put(ctx, app, frame, egui::pos2(left.x, end.y));
            put(ctx, app, frame, end);
        }
        PathStyle::RightBlank => {
            put(ctx, app, frame, egui::pos2(end.x + 120.0, end.y));
        }
    }
    let last = match style {
        PathStyle::RightBlank => egui::pos2(end.x + 120.0, end.y),
        _ => end,
    };
    run_real_frame(ctx, app, frame, vec![pointer_button(last, false)]);
    for _ in 0..2 {
        run_real_frame(ctx, app, frame, vec![]);
    }
}

/// 穷举扫描：各种块类型 × 各种路径形态 × 各种活动块位置，
/// 把「拖不出选区」或「只选中一部分」的组合全部列出来。
#[test]
fn drag_path_sweep_finds_no_stuck_selections() {
    let docs = [
        ("纯段落+标题", "# 标题文字在这里\n\n下面是一段普通文字，用来做拖拽的终点区域。\n\n第三段文字内容。"),
        ("行内代码段落", "第一段带 `some_code` 的文字内容在这里\n\n第二段带 `other_code` 的文字内容也在这里\n\n第三段收尾"),
        ("加粗与链接", "**粗体文字** 与 [链接文字](https://e.com) 的段落内容\n\n后面一段普通文字内容用来当终点"),
        ("标题+列表", "# 列表之上的标题\n\n- 列表项一内容\n- 列表项二内容\n\n列表之后的段落。"),
        ("引用与代码块", "> 引用文字内容在这里。\n\n```rust\nfn main() {}\n```\n\n代码块之后的段落文字。"),
        ("行内公式", "前 $x^2 + y^2$ 后的段落文字内容\n\n第二段普通文字内容在这里。"),
        ("表格与任务", "| 甲列 | 乙列 |\n|:--|--:|\n| 一 | 二 |\n\n- [ ] 任务项内容\n- [x] 已完成项\n\n表格之后的段落。"),
    ];
    let styles = [
        PathStyle::Fine,
        PathStyle::Coarse,
        PathStyle::Fast,
        PathStyle::LShaped,
        PathStyle::LeftMargin,
        PathStyle::RightBlank,
    ];
    let mut bad: Vec<String> = Vec::new();
    for (name, src) in docs {
        for style in styles {
            for active in [0usize, 2, 4] {
                let ctx = egui::Context::default();
                let mut app = test_app();
                let mut frame = eframe::Frame::_new_kittest();
                app.doc = Document::new(src, None);
                app.home = false;
                app.editor.active = active.min(app.doc.blocks.len() - 1);
                for _ in 0..3 {
                    run_real_frame(&ctx, &mut app, &mut frame, vec![]);
                }
                let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
                // 起点取第一个有文字的块首字，终点取最后一个有文字的块末字之后。
                let firsts = ["标题文字在这里", "第一段带", "粗体文字", "列表之上的标题", "引用文字内容", "前 ", "甲列"];
                let lasts = ["第三段文字内容", "第三段收尾", "后面一段普通文字", "列表之后的段落", "代码块之后的段落", "第二段普通文字", "表格之后的段落"];
                let key0 = firsts.iter().copied().find(|k| shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t) if t.galley.text().contains(k) && t.pos.y > EDITOR_TOP)));
                let key1 = lasts.iter().copied().find(|k| shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t) if t.galley.text().contains(k) && t.pos.y > EDITOR_TOP)));
                let (Some(k0), Some(k1)) = (key0, key1) else {
                    bad.push(format!("{name}｜{style:?}｜active={active}：起点/终点文字没渲染出来"));
                    continue;
                };
                let start = point_at(&shapes, k0, EDITOR_TOP, 0);
                let (_, g) = editor_galley(&shapes, k1, EDITOR_TOP);
                let end = point_at(&shapes, k1, EDITOR_TOP, g.text().chars().count());
                drag_with_style(&ctx, &mut app, &mut frame, start, end, style);
                let Some(sel) = app.editor.cross_sel else {
                    bad.push(format!("{name}｜{style:?}｜active={active}：拖不出选区"));
                    continue;
                };
                let got = crate::editor::cross_sel_text(&app.doc, Some(sel)).unwrap_or_default();
                let want = crate::editor::cross_sel_text(&app.doc, Some((0, 0, app.doc.blocks.len() - 1, usize::MAX)))
                    .unwrap_or_default();
                let sg: String = got.chars().filter(|c| !c.is_whitespace()).collect();
                let sw: String = want.chars().filter(|c| !c.is_whitespace()).collect();
                if sg != sw {
                    // 报出第一段没进选区的内容，便于定位是哪一块漏了。
                    let mut tail = String::new();
                    for w in sw.split_inclusive(|c: char| c == '。' || c == '\n') {
                        if !sg.contains(&w[..w.len().min(6)]) {
                            tail.push_str(w);
                        }
                    }
                    bad.push(format!(
                        "{name}｜{style:?}｜active={active}：只选中一部分（选区 {sel:?}）漏：{}",
                        if tail.is_empty() {
                            "（内容顺序或末尾差异）".to_string()
                        } else {
                            tail
                        }
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "拖选扫描发现 {} 个未通过组合：\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// 诊断：表格起拖 + L 形路径，逐帧打印锚点与选区。
#[test]
fn diag_table_lshaped_anchor() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(
        "| 甲列 | 乙列 |\n|:--|--:|\n| 一 | 二 |\n\n- [ ] 任务项内容\n- [x] 已完成项\n\n表格之后的段落。",
        None,
    );
    app.home = false;
    app.editor.active = 2;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = point_at(&shapes, "甲列", EDITOR_TOP, 0);
    let (_, g) = editor_galley(&shapes, "表格之后的段落", EDITOR_TOP);
    let end = point_at(&shapes, "表格之后的段落", EDITOR_TOP, g.text().chars().count());
    println!("诊断 start={start:?} end={end:?} 块数={}", app.doc.blocks.len());
    for i in 0..app.doc.blocks.len() {
        println!("  块 {i}: plain={:?}", app.doc.blocks[i].plain());
    }
    run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(start)]);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(start, true)]);
    println!("  按下后 cross_sel={:?} press_caret={:?}", app.editor.cross_sel, app.editor.press_caret);
    let mid = egui::pos2(start.x, end.y);
    run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(mid)]);
    println!("  竖直到 {mid:?} 后 cross_sel={:?} press_caret={:?}", app.editor.cross_sel, app.editor.press_caret);
    run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(end)]);
    println!("  水平到终点后 cross_sel={:?}", app.editor.cross_sel);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(end, false)]);
    println!("  松开后 cross_sel={:?}", app.editor.cross_sel);
    println!("  文本 {:?}", crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel));
}

/// 拖到「块内第 k 个字符」与「下一块首字」这类精确落点，选中文字必须逐字相等。
fn sweep_expect(doc: &Document, sel: Option<(usize, usize, usize, usize)>) -> String {
    let Some(sel) = sel else { return String::new() };
    crate::editor::cross_sel_text(doc, Some(sel)).unwrap_or_default()
}

/// 长文档 + 多行内标记混排：逐行、逐块拖选都不能漏。
#[test]
fn drag_sweep_multiline_and_marked_text_is_exact() {
    let long = "这一段足够长会自然换行到第二行甚至第三行，中间还夹着 `some_code`、**粗体文字**、\
                [一个链接](https://example.com) 以及行内公式 $x_i^2 + y_i^2$，后面继续补充内容：\
                水准测量在每个测段都要做前后视距差检核，超过限值就要重测；这里再写一些文字，\
                保证排版一定会折到第二行、第三行上去，这样拖选跨行时的端点才检验得出来。";
    let src = format!(
        "# 混排标题在上方\n\n{long}\n\n{long}\n\n> 引用同样带 `代码` 与 **粗体**，也要能整段拖选出来。\n\n\
         - 列表项里带 `代码` 的长文字，用来检查列表项作为端点时选区是否落全内容。\n- 第二个列表项文字。\n\n\
         结束段。"
    );
    let styles = [
        PathStyle::Fine,
        PathStyle::Coarse,
        PathStyle::Fast,
        PathStyle::LShaped,
        PathStyle::LeftMargin,
        PathStyle::RightBlank,
    ];
    let mut bad: Vec<String> = Vec::new();
    for style in styles {
        for active in [0usize, 2, 4, 8] {
            let ctx = egui::Context::default();
            let mut app = test_app();
            let mut frame = eframe::Frame::_new_kittest();
            app.doc = Document::new(&src, None);
            app.home = false;
            app.editor.active = active.min(app.doc.blocks.len() - 1);
            for _ in 0..3 {
                run_real_frame(&ctx, &mut app, &mut frame, vec![]);
            }
            let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
            let Some((_, g)) = shapes.iter().find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text().contains("some_code") && t.pos.y > EDITOR_TOP => {
                    Some((t.pos, &*t.galley))
                }
                _ => None,
            }) else {
                bad.push(format!("{style:?}｜active={active}：没渲染出混排段落"));
                continue;
            };
            let rows = g.rows.len();
            assert!(rows >= 2, "混排段落应换行，实际 {rows} 行（用例前提不成立）");
            // 目标：从本段首字拖到「结束段」的最后一个字之后。
            let end_key = "结束段。";
            let (_, ge) = editor_galley(&shapes, end_key, EDITOR_TOP);
            // 起点取「混排段落第一个字」，与下面 want 从该块整块起算保持一致。
            let start = point_at(&shapes, "some_code", EDITOR_TOP, 0);
            let end = point_at(&shapes, end_key, EDITOR_TOP, ge.text().chars().count());
            drag_with_style(&ctx, &mut app, &mut frame, start, end, style);
            let got = sweep_expect(&app.doc, app.editor.cross_sel);
            let want = {
                let n = app.doc.blocks.len() - 1;
                let start_block = app
                    .doc
                    .blocks
                    .iter()
                    .position(|b| b.plain().contains("some_code"))
                    .expect("混排段落存在");
                let mut v: Vec<String> = Vec::new();
                for i in start_block..=n {
                    v.push(app.doc.blocks[i].plain());
                }
                v.join("\n\n")
            };
            let sg: String = got.chars().filter(|c| !c.is_whitespace()).collect();
            let sw: String = want.chars().filter(|c| !c.is_whitespace()).collect();
            if sg != sw {
                bad.push(format!(
                    "{style:?}｜active={active}｜起点段 {rows} 行：漏字（选区 {:?}）",
                    app.editor.cross_sel
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "混排长文拖选未通过：\n{}",
        bad.join("\n")
    );
}

/// 长文档（内容高于视口）：拖到视口之外要能自动滚动并一路选到底部块。
#[test]
fn drag_beyond_viewport_autoscrolls_to_bottom_block() {
    let mut src = String::new();
    for i in 0..40 {
        src.push_str(&format!("第 {i} 段文字内容，带 `code_{i}` 的行内代码也一并排进去。\n\n"));
    }
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(&src, None);
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = point_of_key(&shapes, "code_0", EDITOR_TOP, 0);
    // 终点在视口外：按下后先滚一帧再落到视口底缘之外，靠越界自动滚动延伸。
    let below = egui::pos2(start.x + 40.0, 1400.0);
    run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(start)]);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(start, true)]);
    for k in 1..=20 {
        let p = egui::pos2(start.x + 40.0, start.y + (below.y - start.y) * k as f32 / 20.0);
        run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(p)]);
    }
    // 停在视口之外持续滚动，直到内容不再移动（最多 120 帧）
    let mut last_sel_end = app.editor.cross_sel.map_or(0, |s| s.2);
    let mut still = 0;
    for _ in 0..120 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(below)]);
        let end = app.editor.cross_sel.map_or(0, |s| s.2.max(s.0));
        if end == last_sel_end {
            still += 1;
            if still >= 6 {
                break;
            }
        } else {
            still = 0;
            last_sel_end = end;
        }
    }
    let last = app.doc.blocks.len() - 1;
    // 松手前指针仍在视口外：按最后一次落点松手
    let p = ctx.input(|i| i.pointer.interact_pos()).unwrap_or(below);
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, false)]);
    let sel = app.editor.cross_sel.unwrap_or((0, 0, 0, 0));
    assert!(
        sel.0.min(sel.2) == 0 && sel.0.max(sel.2) >= last - 2,
        "拖到视口外应自动滚动并选到文档末尾附近，实际选区 {sel:?}（末块 {last}）"
    );
    let got = sweep_expect(&app.doc, app.editor.cross_sel);
    assert!(
        got.contains("第 30 段") || got.contains("第 39 段"),
        "选区应包含视口外的段落文字，实际长度 {}",
        got.chars().count()
    );
}

/// 网格扫查用的文档集合：块类型尽量铺满（标题、段落、行内代码、加粗、链接、
/// 行内公式、图片、脚注、列表、任务项、有序列表、嵌套引用、代码块、表格、块级公式）。
const GRID_DOCS: [&str; 5] = [
    "# 网格扫查用的标题\n\n\
     带 `inline_code` 的段落文字内容，用来检查代码片附近的落点。\n\n\
     > 引用一行带 `code` 的文字内容。\n\n\
     - 列表项一内容带 `code`\n- 列表项二内容\n\n\
     ```rust\nfn grid() {}\n```\n\n\
     | 网格甲 | 网格乙 |\n|:--|--:|\n| 一 | 二 |\n\n\
     收尾段落文字内容在这里。",
    "# 带标记的标题\n\n\
     先看 `alpha_code` 与 **粗体文字** 再加 [一个链接](https://example.com) 后面的中文内容，\
     段落要足够长才会折行，第二行同样夹着 `beta_code` 与 **另一段粗体**，用来逐点验证归属。\n\n\
     前 $x_i^2 + y_i^2$ 后的行内公式段落文字内容。\n\n\
     ![配图说明](pics/notexist-9f3a.png)\n\n\
     尾注句子[^n1]与下一段。\n\n[^n1]: 网格用例的脚注内容文字。\n\n\
     收尾段落文字内容在这里。",
    "# 列表阶梯标题\n\n\
     - 任务项一内容带 `code`\n- [x] 已完成任务项内容\n\n\
     1. 有序项一内容\n2. 有序项二内容\n\n\
     > 引用第一层文字内容带 `code`。\n\n>> 引用第二层文字内容。\n\n\
     ```python\nprint('网格代码块内容')\n```\n\n\
     | 甲 | 乙 | 丙 |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n\n$$\nx^2+y^2=z^2\n$$\n\n\
     收尾段落文字内容在这里。",
    "# 段落与换行\n\n\
     一段非常长的中文正文，用来把段落撑到两三行，中间还要夹一个 `code_in_long_paragraph`\
     让代码片出现在行中与行首的位置上，看看拖选会不会漏掉后半截内容，因此继续补充文字\
     直到必然折行为止，最后一行只剩几个字。\n\n另一段普通文字内容。",
    // 用户截图那种多层嵌套列表：缩进越深、圆点与文字之间空隙越宽，
    // 左侧 -24px 采样必须照样锚定与延伸。
    "- 有哪些大模型应用开发框架呢\n  \
     - 截止到2025年7月26日，GitHub统计数据：\n    \
     - LangChain：这些工具里出现最早、最成熟的，适合复杂任务分解和单智能体应用\n      \
     - Llamalndex：专注于高效的索引和检索，适合 RAG 场景。\n        \
     - DeepSeek：注意不是Meta开发的，功能略少于LangChain，但核心功能都有。\n  \
     - [ ] 任务条目带 `code`，复选框左边也属于本行\n\
     收尾段落文字内容在这里。",
];

/// 指针网格扫查：在正文的每一个 y（含块内留白、块间空档、左侧缩进、右侧空白）
/// 落点都必须 ① 建立选区，② 终点块 = 产品本帧真实矩形里该行的归属块，
/// ③ 锚点与终点之间的所有内容块必须整块进选区（不允许「只选中一部分」）。
#[test]
fn drag_endpoint_grid_covers_every_pointer_row() {
    for src in GRID_DOCS {
        grid_sweep(src);
    }
}

fn grid_sweep(src: &str) {
    let offsets_x = [-24.0f32, 0.0, 60.0, 240.0, 520.0];
    let mut bad: Vec<String> = Vec::new();
    let mut samples = 0usize;
    for dx in offsets_x {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();
        app.doc = Document::new(src, None);
        app.home = false;
        app.editor.active = 0;
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let first_key = {
            let k: String = app.doc.blocks[0]
                .plain()
                .chars()
                .filter(|c| !c.is_whitespace())
                .take(8)
                .collect();
            if k.is_empty() {
                app.doc
                    .blocks
                    .iter()
                    .find_map(|b| {
                        let k: String = b
                            .plain()
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .take(8)
                            .collect();
                        (!k.is_empty()).then_some(k)
                    })
                    .expect("文档有可读块")
            } else {
                k
            }
        };
        let anchor = point_at(&shapes, &first_key, EDITOR_TOP, 0);
        // 本帧真实块矩形（含空白行——空行本身也是可选中、可停留的一行）。
        let rects: Vec<(usize, egui::Rect)> = app
            .editor
            .last_block_rects
            .iter()
            .map(|(i, r)| {
                (
                    *i,
                    egui::Rect::from_min_max(egui::pos2(r.0, r.1), egui::pos2(r.2, r.3)),
                )
            })
            .collect();
        assert!(
            rects.len() >= 4,
            "可见块太少，网格前提不成立：{}",
            rects.len()
        );
        let lo = rects.iter().map(|(_, r)| r.top()).fold(f32::INFINITY, f32::min) - 4.0;
        let hi = rects.iter().map(|(_, r)| r.bottom()).fold(0.0f32, f32::max) + 4.0;
        let mut y = lo;
        while y <= hi {
            samples += 1;
            let px = anchor.x + dx;
            let expect = rects
                .iter()
                .find(|(_, r)| r.contains(egui::pos2(px, y)))
                .map(|(i, _)| *i)
                .unwrap_or_else(|| {
                    rects
                        .iter()
                        .min_by_key(|(_, r)| {
                            let d = if y < r.top() {
                                r.top() - y
                            } else if y > r.bottom() {
                                y - r.bottom()
                            } else {
                                0.0
                            };
                            (d * 1000.0) as i32
                        })
                        .map(|(i, _)| *i)
                        .unwrap_or(0)
                });
            run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(anchor)]);
            run_real_frame(
                &ctx,
                &mut app,
                &mut frame,
                vec![pointer_button(anchor, true)],
            );
            // 先横着走 40px 保证被判定成拖动，再走向目标落点。
            let warm = egui::pos2(anchor.x + 40.0, anchor.y);
            run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(warm)]);
            let mut cur = warm;
            for k in 1..=8 {
                let t = k as f32 / 8.0;
                cur = egui::pos2(warm.x + (px - warm.x) * t, warm.y + (y - warm.y) * t);
                run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(cur)]);
            }
            run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(cur, false)]);
            let Some(sel) = app.editor.cross_sel else {
                bad.push(format!(
                    "文档「{}」dx={dx:>6.0} y={y:>6.1}：没有选区（期望块 {expect}）",
                    &src[..src.chars().take(10).collect::<Vec<_>>().len().min(30)]
                ));
                y += 12.0;
                continue;
            };
            let end_block = sel.0.max(sel.2);
            let start_block = sel.0.min(sel.2);
            if start_block != 0 {
                bad.push(format!("dx={dx:>6.0} y={y:>6.1}：锚点块 {start_block} 应为 0（{sel:?}）"));
            }
            if end_block != expect {
                bad.push(format!(
                    "dx={dx:>6.0} y={y:>6.1}：终点块 {end_block} ≠ 归属块 {expect}（{sel:?}）"
                ));
            }
            // 中间所有内容块必须整块进选区。
            let got: String = crate::editor::cross_sel_text(&app.doc, Some(sel))
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            for i in (start_block + 1)..end_block {
                let Some(b) = app.doc.blocks.get(i) else { continue };
                if matches!(b, crate::ast::Block::Blank { .. }) {
                    continue;
                }
                let key: String = b
                    .plain()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .take(6)
                    .collect();
                if !key.is_empty() && !got.contains(&key) {
                    bad.push(format!(
                        "dx={dx:>6.0} y={y:>6.1}：中间块 {i}「{key}」整块漏选（{sel:?}）"
                    ));
                }
            }
            app.editor.cross_sel = None;
            y += 12.0;
        }
    }
    println!("网格扫查（{} 字文档）共采样 {samples} 个落点，失败 {} 个", src.chars().count(), bad.len());
    assert!(
        bad.is_empty(),
        "网格扫查失败 {} 个落点（前 24 个）：\n{}",
        bad.len(),
        bad.iter().take(24).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// 行内代码 / 加粗 / 链接混排的活动段落：把指针逐点放到每个字符上，
/// 松手后的终点必须与该点在最终版式下对应的可见字符一致（允许 ±1 字符）。
/// 这一条专治「有行内代码就只选中半截」：光标进入标记内部会露出 `**`/反引号，
/// 版式当场位移，端点算错就表现为漏选。
#[test]
fn drag_endpoint_char_matches_pointer_over_marked_text() {
    let src = "先看 `alpha_code` 与 **粗体文字** 再加 [一个链接](https://example.com) 后面的中文内容，\
               段落要足够长才会折行，第二行同样夹着 `beta_code` 与 **另一段粗体**，用来逐点验证终点字符。\n\n\
               收尾段落。";
    for (tag, active) in [("活动块（编辑态）", 0usize), ("渲染态（活动块在别处）", 2usize)] {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();
        app.doc = Document::new(src, None);
        app.home = false;
        app.editor.active = active;
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let key = "alpha_code";
        let (_, g) = editor_galley(&shapes, key, EDITOR_TOP);
        let n = g.text().chars().count();
        let anchor = point_at(&shapes, key, EDITOR_TOP, 0);
        let plain: String = app.doc.blocks[0].plain();
        let mut bad: Vec<String> = Vec::new();
        // 逐字符取样（步长 3 个字符，兼顾覆盖与耗时）。
        let mut ch = 2usize;
        while ch < n {
            let target = point_at(&shapes, key, EDITOR_TOP, ch);
            run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(anchor)]);
            run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(anchor, true)]);
            let warm = egui::pos2(anchor.x + 30.0, anchor.y);
            run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(warm)]);
            let mut cur = warm;
            let steps = (((target - warm).length() / 10.0).ceil() as usize).max(1);
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                cur = egui::pos2(warm.x + (target.x - warm.x) * t, warm.y + (target.y - warm.y) * t);
                run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(cur)]);
            }
            run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(cur, false)]);
            let Some(sel) = app.editor.cross_sel else {
                bad.push(format!("{tag} 字符 {ch}：没有选区"));
                ch += 3;
                continue;
            };
            // 以「松手后的实际版式」为基准换算期望字符。
            let (shapes2, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
            let (o2, g2) = editor_galley(&shapes2, key, EDITOR_TOP);
            let expect_buf = g2
                .cursor_from_pos(cur - o2)
                .index
                .0
                .min(g2.text().chars().count());
            let expect = crate::render::plain_char_index(g2, expect_buf).min(plain.chars().count());
            let got = if sel.2 == 0 { sel.3 } else { sel.1 };
            if got.abs_diff(expect) > 1 {
                bad.push(format!(
                    "{tag} 取样字符 {ch}（p={cur:?}）：终点字符 {got} ≠ 落点 {expect}"
                ));
            }
            app.editor.cross_sel = None;
            ch += 3;
        }
        assert!(
            bad.is_empty(),
            "{tag}：逐点拖选终点未通过 {} 处：\n{}",
            bad.len(),
            bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
        );
    }
}

fn kind_of(b: &crate::ast::Block) -> &'static str {
    use crate::ast::Block as B;
    match b {
        B::Heading { .. } => "Heading",
        B::Paragraph { .. } => "Paragraph",
        B::Quote { .. } => "Quote",
        B::ListItem { .. } => "ListItem",
        B::Code { .. } => "Code",
        B::Table { .. } => "Table",
        B::Math { .. } => "Math",
        B::Footnote { .. } => "Footnote",
        B::Blank { .. } => "Blank",
        _ => "其它",
    }
}

/// 逐块起拖：文档里每一种块都当作拖选的起点按下，向下拖到最后一块，
/// 锚点必须锁在被按下的块上（专治「在表格/代码块/引用/任务项上按下拖不动」）。
#[test]
fn every_block_type_can_start_a_drag() {
    let src = "# 起点扫查标题\n\n\
               起点扫查段落带 `code_a` 文字内容。\n\n\
               > 起点扫查引用文字内容。\n\n\
               - 起点扫查列表项内容带 `code_b`\n\
               - [ ] 起点扫查任务项内容\n\n\
               1. 起点扫查有序项内容\n\n\
               ```rust\nfn grid_start() {}\n```\n\n\
               | 起点甲 | 起点乙 |\n|:--|--:|\n| 丙 | 丁 |\n\n\
               $$\nx^2+y^2=z^2\n$$\n\n\
               尾注句子[^gn]与收尾段落。\n\n[^gn]: 起点扫查脚注内容文字。";
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(src, None);
    app.home = false;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    // 工具条加高后视口里可见的块会随焦点变化：块进入编辑态后高度会变，
    // 所以每一轮都要重新量矩形和文字位置，不能用一开始缓存的那一套。
    let indices: Vec<usize> = app.editor.last_block_rects.iter().map(|(i, _)| *i).collect();
    let mut bad: Vec<String> = Vec::new();
    for i in indices {
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let rects: Vec<(usize, egui::Rect)> = app
            .editor
            .last_block_rects
            .iter()
            .map(|(i, r)| {
                (
                    *i,
                    egui::Rect::from_min_max(egui::pos2(r.0, r.1), egui::pos2(r.2, r.3)),
                )
            })
            .collect();
        let Some((_, r)) = rects.iter().find(|(j, _)| *j == i).copied() else {
            continue;
        };
        let (last, last_rect) = *rects.last().expect("有块矩形");
        if matches!(app.doc.blocks[i], crate::ast::Block::Blank { .. }) || i == last {
            continue;
        }
        // 按下点：块内文字优先，没有文字的块用矩形中心。
        let key: String = app.doc.blocks[i]
            .plain()
            .split_whitespace()
            .max_by_key(|w| w.chars().count())
            .unwrap_or("")
            .chars()
            .take(8)
            .collect();
        let press = match (key.is_empty(), shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text().contains(&key) && t.pos.y > EDITOR_TOP => Some(t.pos),
            _ => None,
        })) {
            (false, Some(p)) => egui::pos2(p.x + 4.0, p.y + 7.0),
            _ => r.center(),
        };
        let end = egui::pos2(last_rect.center().x, last_rect.bottom() - 3.0);
        app.editor.cross_sel = None;
        run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(press)]);
        run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(press, true)]);
        let mut cur = press;
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            cur = egui::pos2(press.x + (end.x - press.x) * t, press.y + (end.y - press.y) * t);
            run_real_frame(&ctx, &mut app, &mut frame, vec![egui::Event::PointerMoved(cur)]);
        }
        run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(cur, false)]);
        let Some(sel) = app.editor.cross_sel else {
            bad.push(format!("块 {i} 起拖：没有建立选区（按下点 {press:?}）"));
            continue;
        };
        let anchor = sel.0.min(sel.2);
        if anchor != i {
            bad.push(format!(
                "块 {i}（{}）起拖：锚点应锁在本块，实际 {anchor}（{sel:?}，按下点 {press:?}）",
                kind_of(&app.doc.blocks[i])
            ));
            continue;
        }
        if sel.0.max(sel.2) < last {
            bad.push(format!("块 {i} 起拖：终点没到最后一块 {last}（{sel:?}）"));
        }
    }
    assert!(
        bad.is_empty(),
        "逐块起拖未通过 {} 处：\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// 模态对话框要开在视口正中，而不是 egui 级联出来的左上角。
/// 走真实帧：「插入表格」由命令弹出、「插入链接」由链接工具弹出。
/// 对话框外框用它的三处文字（标题 +「插入」+「取消」）的并集近似；
/// 菜单栏的「插入」在窗口顶部 80px 内，按 y 排除，标题取最后一个同名文字。
#[test]
fn modal_dialogs_open_at_viewport_center() {
    let openers: Vec<(&str, fn(&mut MarkdApp))> = vec![
        ("插入表格", |a| a.table_dialog = true),
        ("插入链接", |a| a.link_dialog.open = true),
    ];
    for (title, open) in openers {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let mut frame = eframe::Frame::_new_kittest();
        app.doc = Document::new("段落", None);
        app.home = false;
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        open(&mut app);
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let (s, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
            shapes = s;
        }
        let mut box_rect = egui::Rect::NOTHING;
        let mut found: Vec<String> = Vec::new();
        for cs in shapes.iter().rev() {
            let egui::Shape::Text(t) = &cs.shape else { continue };
            let text = t.galley.text().to_string();
            let in_body = t.pos.y >= EDITOR_TOP;
            let take = if text == title {
                // 只取最靠后的一个（弹层在菜单之后绘制）
                found.iter().all(|s| s != "标题")
            } else {
                in_body && matches!(text.as_str(), "插入" | "取消") && found.len() < 3
            };
            if take {
                box_rect = box_rect.union(t.galley.rect.translate(t.pos.to_vec2()));
                found.push(if text == title { "标题".to_string() } else { text });
            }
        }
        assert_eq!(found.len(), 3, "{title} 应渲染出标题与两个按钮，实际 {found:?}");
        let area = ctx.content_rect();
        let off = box_rect.center().to_vec2() - area.center().to_vec2();
        assert!(
            off.x.abs() < 80.0 && off.y.abs() < 80.0,
            "{title} 应居中弹出：对话框外框 {box_rect:?}（中心 {:.0}, {:.0}），内容区 {area:?}（中心 {:.0}, {:.0}），偏移 ({:.0}, {:.0})",
            box_rect.center().x,
            box_rect.center().y,
            area.center().x,
            area.center().y,
            off.x,
            off.y
        );
    }
}

/// 代码块 galley 里第 ch 个插入点的屏幕坐标（用于把鼠标落点对到具体字符）。
fn code_char_pos(shapes: &[egui::epaint::ClippedShape], marker: &str, ch: usize) -> egui::Pos2 {
    let t = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text().contains(marker) => Some(t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("未渲染代码正文：{marker}"));
    let r = t.galley.pos_from_cursor(egui::text::CCursor::new(ch));
    t.pos + r.center().to_vec2()
}

/// 打开一个「段落 + 4 行 Rust 代码块 + 段落」的真实 app，返回 (ctx, app, frame)。
fn code_doc_app() -> (egui::Context, MarkdApp, eframe::Frame) {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new(
        "para above\n\n```rust\nlet aaa = 1;\nlet bbb = 2;\nlet ccc = 3;\nlet ddd = 4;\n```\n\npara below",
        None,
    );
    app.home = false;
    app.editor.active = 0;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    (ctx, app, frame)
}

/// 在指定位置单击一次（移动 → 按下 → 松开 → 三帧落地）。
fn click_at(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, pos: egui::Pos2) {
    let mk = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run_real_frame(
        ctx,
        app,
        frame,
        vec![egui::Event::PointerMoved(pos), mk(true)],
    );
    run_real_frame(ctx, app, frame, vec![mk(false)]);
    for _ in 0..3 {
        run_real_frame(ctx, app, frame, vec![]);
    }
}

/// 那份文档的代码正文（块 2）：每行 12 字符 + 换行，共 4 行。
const CODE_LEN: usize = 51;
/// 第二/三行「let xxx = 」里 x 之前的那个插入点下标。
const LINE2: usize = 17;
const LINE3: usize = 30;

/// 逐帧经过若干航点的拖拽：按下 → 每段按 6px 分步移动 → 松开。
/// 真实拖拽的鼠标事件比这密得多，`drag_select` 只有 3 帧，验证不到
/// 「锚点建立之后端点还能继续跟着指针」这一段。
fn drag_path(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    way: &[egui::Pos2],
) {
    let mk = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let first = way[0];
    run_real_frame(
        ctx,
        app,
        frame,
        vec![egui::Event::PointerMoved(first), mk(first, true)],
    );
    let mut last = first;
    for seg in way.windows(2) {
        let steps = ((seg[1] - seg[0]).length() / 6.0).ceil().max(1.0) as usize;
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            last = egui::pos2(
                seg[0].x + (seg[1].x - seg[0].x) * t,
                seg[0].y + (seg[1].y - seg[0].y) * t,
            );
            run_real_frame(ctx, app, frame, vec![egui::Event::PointerMoved(last)]);
        }
    }
    run_real_frame(ctx, app, frame, vec![mk(last, false)]);
}

/// 回归：按下点落在代码面板之外（正文右边距）再拖进代码文字，终点要跟着
/// 指针落到字符上。此前终点只在按下点也落在代码区时才上报，锚点被兜底定成块首
/// 后就再也不更新，表现成「拖了半天一个字符也没选中」。
#[test]
fn drag_starting_outside_code_panel_follows_pointer() {
    let (ctx, mut app, mut frame) = code_doc_app();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let panel = app
        .editor
        .last_block_rects
        .iter()
        .find(|(i, _)| *i == 2)
        .map(|(_, r)| *r)
        .expect("代码块矩形");
    let start = egui::pos2(panel.2 + 6.0, code_char_pos(&shapes, "ccc", LINE3).y);
    let end = code_char_pos(&shapes, "bbb", LINE2);
    drag_path(&ctx, &mut app, &mut frame, &[start, end]);
    let (a, ac, b, bc) = app
        .editor
        .cross_sel
        .unwrap_or_else(|| panic!("代码块外起拖、拖进代码后应建立选区"));
    assert_eq!((a, b), (2, 2), "选区应只在代码块内，实际 {a}→{b}");
    let hi = ac.max(bc);
    assert!(
        (hi as isize - LINE2 as isize).abs() <= 2,
        "终点该跟到指针那一步的字符 {LINE2}，实际 ({ac}, {bc}) / 整块 0..{CODE_LEN}"
    );
}

/// 回归：往上拖经过代码块功能栏时，端点要停在最后落到的那个字符，
/// 不能因为指针落在「整块矩形内但没有文字」的位置就跳成块尾——
/// 那正是「往上拖，底下几行却全被选中」。
#[test]
fn drag_up_through_code_header_does_not_jump_to_block_end() {
    let (ctx, mut app, mut frame) = code_doc_app();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let panel = app
        .editor
        .last_block_rects
        .iter()
        .find(|(i, _)| *i == 2)
        .map(|(_, r)| *r)
        .expect("代码块矩形");
    let start = code_char_pos(&shapes, "ccc", LINE3);
    // 功能栏正中：在面板里、代码文字上方。
    let header = egui::pos2(
        start.x,
        (panel.1 + code_char_pos(&shapes, "aaa", 0).y) * 0.5,
    );
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(start)],
    );
    let mk = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run_real_frame(&ctx, &mut app, &mut frame, vec![mk(start, true)]);
    for t in 1..=6 {
        let f = t as f32 / 6.0;
        let p = egui::pos2(
            start.x + (header.x - start.x) * f,
            start.y + (header.y - start.y) * f,
        );
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::PointerMoved(p)],
        );
    }
    run_real_frame(&ctx, &mut app, &mut frame, vec![mk(header, false)]);
    let (a, ac, b, bc) = app
        .editor
        .cross_sel
        .unwrap_or_else(|| panic!("代码块内往上拖应建立选区"));
    assert_eq!((a, b), (2, 2), "选区应只在代码块内");
    let (lo, hi) = (ac.min(bc), ac.max(bc));
    assert!(
        hi < CODE_LEN,
        "终点跳到块尾了（往上拖却选中下面所有行）：实际 ({lo}, {hi}) / {CODE_LEN}"
    );
}

/// 回归：锚点在代码块里，往下拖出该块（经过块间空白落到下一段文字）时，
/// 选区必须一直延伸到指针处。「锚点在本块就跳过整块端点」只该在指针仍
/// 留在该块矩形内时生效，不能把跨块延伸一起削掉。
#[test]
fn drag_from_code_block_down_across_blocks_selects_through() {
    let (ctx, mut app, mut frame) = code_doc_app();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let start = code_char_pos(&shapes, "ccc", LINE3);
    let end = code_char_pos(&shapes, "para below", 5);
    drag_path(&ctx, &mut app, &mut frame, &[start, end]);
    let (a, ac, b, bc) = app.editor.cross_sel.expect("跨块拖选应建立选区");
    assert_eq!(
        (a, b),
        (2, 4),
        "选区应从代码块延伸到后面的段落，实际 {a}→{b}"
    );
    assert!(ac > 0, "锚点该落在按下处的字符上，实际 {ac}");
    assert_eq!(bc, 5, "终点应落在后面段落里指针那一步的字符，实际 {bc}");
    let copied = crate::editor::cross_sel_text(&app.doc, app.editor.cross_sel).unwrap_or_default();
    for key in ["ccc", "ddd", "para"] {
        assert!(
            copied.contains(key),
            "拖过的内容漏了「{key}」，实际 {copied:?}"
        );
    }
    assert!(
        !copied.contains("aaa"),
        "没拖到的第一行不该进来，实际 {copied:?}"
    );
}

/// 点渲染态代码块的某个字符：光标要落在那里，不能跳到整段代码末尾。
/// 以前代码块不报 `Action::Caret`（只有文字块报），实测点第一行行首后
/// `last_cursor` 是 51（缓冲末尾），紧跟着按退格删掉的是最后一行的字符。
#[test]
fn clicking_code_block_char_places_caret_there() {
    let (ctx, mut app, mut frame) = code_doc_app();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    // 代码 galley 的下标与缓冲下标一致：CCursor(35) 就是第三行第 5 个字符。
    let p = code_char_pos(&shapes, "let ccc", LINE3 + 5);
    click_at(&ctx, &mut app, &mut frame, p);
    assert_eq!(app.editor.active, 2, "点代码块应进入该块编辑态");
    assert_eq!(
        app.editor.last_cursor,
        LINE3 + 5,
        "光标应落在第三行第 5 个字符（缓冲下标 {}），实际 {}",
        LINE3 + 5,
        app.editor.last_cursor
    );
}

/// 用户报的现场：光标在代码块第一行第一个字符，按退格不该退出代码块。
/// 真 app 帧驱动——先点第一行行首（光标落点由上一条测试钉住），再按退格。
#[test]
fn backspace_at_first_char_of_code_block_keeps_it_code() {
    let (ctx, mut app, mut frame) = code_doc_app();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let p = code_char_pos(&shapes, "let aaa", 0);
    click_at(&ctx, &mut app, &mut frame, p);
    assert_eq!(app.editor.last_cursor, 0, "夹具：光标应在第一行第一个字符");

    let before = app.doc.source();
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

    assert_eq!(app.doc.source(), before, "退格不该改动文档一个字节");
    assert_eq!(app.doc.blocks.len(), 5, "代码块不该被并回上一块");
    assert!(
        matches!(app.doc.blocks[2], crate::ast::Block::Code { .. }),
        "第 2 块还得是代码块：{:?}",
        app.doc.blocks[2].source()
    );
    assert_eq!(app.editor.active, 2, "活动块不该跳走");
    assert_eq!(app.editor.last_cursor, 0, "光标应原地不动");
}

/// 工具条上「行内公式 / 公式块 / 代码块公式」三个平铺按钮收成一个「公式」下拉：
/// 未展开时工具条上没有那三个标签，展开后底下正好三项，点哪一项就执行哪个插入。
#[test]
fn toolbar_formula_dropdown_holds_three_items() {
    let labels = ["行内公式", "公式块", "代码块公式"];
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("正文一段", None);
    app.home = false;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }

    // 文字形状 → 它的屏幕中心；`top` 只认工具条那一条（标题栏之下、工具条底之上）。
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
    };
    let top = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        // 不能先 find 再过滤：上方功能区选项卡也叫「插入」，会先命中但被过滤掉，
        // 后面的工具盒子就没机会被看到了。这里对每个同文字形状逐个判断 y。
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    };
    let click =
        |app: &mut MarkdApp, ctx: &egui::Context, frame: &mut eframe::Frame, p: egui::Pos2| {
            run_real_frame(
                ctx,
                app,
                frame,
                vec![egui::Event::PointerMoved(p), pointer_button(p, true)],
            );
            run_real_frame(ctx, app, frame, vec![pointer_button(p, false)]);
            for _ in 0..2 {
                run_real_frame(ctx, app, frame, vec![]);
            }
        };

    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    for label in labels {
        assert!(
            top(&shapes, label).is_none(),
            "工具条不该再平铺「{label}」，它要收进「公式」下拉"
        );
    }
    let btn = top(&shapes, "公式").expect("工具条上要有「公式」下拉");

    // 展开：三项都必须在弹出的菜单里。
    click(&mut app, &ctx, &mut frame, btn);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let items: Vec<egui::Pos2> = labels
        .iter()
        .filter_map(|l| find(&shapes, l))
        .filter(|p| p.y > btn.y)
        .collect();
    assert_eq!(
        items.len(),
        labels.len(),
        "「公式」下拉展开后应有三项，实际找到 {} 项",
        items.len()
    );

    // 点第三项「代码块公式」：真的插入 ```math 块，菜单随后收起。
    let math_code = find(&shapes, "代码块公式").expect("代码块公式项");
    click(&mut app, &ctx, &mut frame, math_code);
    assert!(
        app.doc.source().contains("```math"),
        "点「代码块公式」应插入公式代码块，实际 {:?}",
        app.doc.source()
    );
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(
        find(&shapes, "代码块公式").is_none(),
        "选完一项后下拉要收起，不该还挂着菜单项"
    );

    // 再展开点第一项「行内公式」：插入 $...$。
    let btn = top(&shapes, "公式").expect("工具条上要有「公式」下拉");
    click(&mut app, &ctx, &mut frame, btn);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let inline = find(&shapes, "行内公式").expect("行内公式项");
    click(&mut app, &ctx, &mut frame, inline);
    assert!(
        app.doc.source().contains("$E = mc^2$"),
        "点「行内公式」应插入行内公式，实际 {:?}",
        app.doc.source()
    );
}

/// 回归：点击引用进入编辑态，左侧竖线必须停在原地——横坐标、线宽、线高
/// 三者与渲染态逐像素相同（以前编辑态按 `TextEdit` 的响应矩形画线，
/// 那个矩形比文字本身高 1px，点一下竖线就长大一截，表现为「边框动了一下」）。
#[test]
fn quote_bar_geometry_is_identical_in_both_states() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("> 甲乙丙丁\n\n正文段落", None);
    app.home = false;
    app.editor.active = 2;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let bar = |app: &mut MarkdApp, frame: &mut eframe::Frame| -> (f32, f32, f32) {
        let (shapes, _) = run_real_frame_ext(&ctx, app, frame, vec![]);
        let rects: Vec<_> = shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Rect(r) if r.fill == app.theme.accent && r.rect.width() <= 4.0 => {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        let b = rects.first().expect("引用块要画左侧竖线");
        assert_eq!(rects.len(), 1, "只该有一条竖线，实际 {rects:?}");
        (b.min.x, b.width(), b.height())
    };
    let before = bar(&mut app, &mut frame);

    let p = {
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text().contains("甲乙丙丁") => {
                    Some(t.pos + t.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .expect("引用正文")
    };
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::PointerMoved(p), pointer_button(p, true)],
    );
    run_real_frame(&ctx, &mut app, &mut frame, vec![pointer_button(p, false)]);
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert_eq!(app.editor.active, 0, "夹具：引用块要已成为活动块（编辑态）");
    let after = bar(&mut app, &mut frame);
    assert_eq!(
        after.0, before.0,
        "点击后竖线横向位置变了：{before:?} -> {after:?}"
    );
    assert_eq!(
        (after.1, after.2),
        (before.1, before.2),
        "点击后竖线尺寸变了（线宽/线高）：{before:?} -> {after:?}"
    );
    // 块高也必须一致，否则下方内容会整体位移。
    assert_eq!(
        app.editor.layout.raw_height(0),
        29.0,
        "编辑态引用块的排版高度要与渲染态一致"
    );
}

/// 工具条「代码」下拉：行内代码 / 代码块 / 代码组 三项收在一起，
/// 工具条下半区不平铺这三项；原「插入」下拉已展开成独立盒子。
#[test]
fn toolbar_code_dropdown_holds_three_items() {
    let labels = ["行内代码", "代码块", "代码组"];
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("正文一段", None);
    app.home = false;
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    let find = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
    };
    let top = |shapes: &[egui::epaint::ClippedShape], name: &str| -> Option<egui::Pos2> {
        // 不能先 find 再过滤：上方功能区选项卡也叫「插入」，会先命中但被过滤掉，
        // 后面的工具盒子就没机会被看到了。这里对每个同文字形状逐个判断 y。
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    };
    let click =
        |app: &mut MarkdApp, ctx: &egui::Context, frame: &mut eframe::Frame, p: egui::Pos2| {
            run_real_frame(
                ctx,
                app,
                frame,
                vec![egui::Event::PointerMoved(p), pointer_button(p, true)],
            );
            run_real_frame(ctx, app, frame, vec![pointer_button(p, false)]);
            for _ in 0..2 {
                run_real_frame(ctx, app, frame, vec![]);
            }
        };

    // 工具条上只该有「代码」这一个入口。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    for label in labels {
        assert!(
            top(&shapes, label).is_none(),
            "工具条不该再平铺「{label}」，它要收进「代码」下拉"
        );
    }
    let btn = top(&shapes, "代码").expect("工具条上要有「代码」下拉");

    // 展开：三项齐全。
    click(&mut app, &ctx, &mut frame, btn);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let missing: Vec<&str> = labels
        .iter()
        .copied()
        .filter(|l| find(&shapes, l).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "「代码」下拉展开后三项都要在，缺失 {missing:?}"
    );

    // 点「代码组」：真的插入代码组围栏。
    let group = find(&shapes, "代码组").expect("代码组项");
    click(&mut app, &ctx, &mut frame, group);
    assert!(
        app.doc.source().contains("````code-group"),
        "点「代码组」应插入代码组，实际 {:?}",
        app.doc.source()
    );

    // 原「插入」下拉已展开成独立盒子，下半区不再有汇总的「插入」入口；
    // 代码三项也仍只收在「代码」下拉里，不平铺到工具条。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(
        top(&shapes, "插入").is_none(),
        "下半区不该再有「插入」下拉按钮"
    );
    for label in labels {
        assert!(
            top(&shapes, label).is_none(),
            "「{label}」仍应收在「代码」下拉里，不该平铺到工具条"
        );
    }
}

/// 跑一帧并指定系统深浅档（测「跟随系统」用；普通帧脚手架不带这个字段）。
fn run_frame_os_theme(
    ctx: &egui::Context,
    app: &mut MarkdApp,
    frame: &mut eframe::Frame,
    os: Option<egui::Theme>,
) {
    use eframe::App as _;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        system_theme: os,
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
}

/// 跟随系统：系统深浅档一变，配色立刻跟着换；关掉跟随后系统档不再插手。
#[test]
fn theme_follow_system_tracks_the_os_theme() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.config.theme = ThemeKind::Light;
    app.config.theme_follow_system = true;
    app.rebuild_theme();
    assert_eq!(app.theme.kind, ThemeKind::Light, "没收到系统档时按浅色兜底");

    run_frame_os_theme(&ctx, &mut app, &mut frame, Some(egui::Theme::Dark));
    assert_eq!(app.theme.kind, ThemeKind::Dark, "系统转深色应立刻换配色");
    run_frame_os_theme(&ctx, &mut app, &mut frame, Some(egui::Theme::Light));
    assert_eq!(app.theme.kind, ThemeKind::Light, "系统转回浅色也要跟着换");

    app.config.theme_follow_system = false;
    app.rebuild_theme();
    run_frame_os_theme(&ctx, &mut app, &mut frame, Some(egui::Theme::Dark));
    assert_eq!(
        app.theme.kind,
        ThemeKind::Light,
        "不跟随时系统档不该插手手动选择"
    );
}

/// 源码模式里最长的一个 galley 的行高（= 字号 × 行距倍数）。
fn source_row_height(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame) -> f32 {
    let (shapes, _) = run_real_frame_ext(ctx, app, frame, vec![]);
    let galley = shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) => Some(t.galley.clone()),
            _ => None,
        })
        .max_by_key(|g| g.rows.len())
        .expect("源码模式应渲染出整篇文本");
    assert!(
        galley.rows.len() >= 4,
        "至少要四行才测得出行距，实际 {} 行",
        galley.rows.len()
    );
    galley.rows.iter().map(|r| r.size.y).fold(0.0_f32, f32::max)
}

/// 「源码模式行距」这个设置项要真的改动渲染出来的行高，不是只存进配置。
#[test]
fn line_height_setting_changes_source_mode_row_height() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    app.doc = Document::new("甲\n乙\n丙\n丁", None);
    app.home = false;
    app.execute(Command::ToggleSource, &ctx);
    assert!(app.config.source_mode, "夹具应已切到源码模式");

    app.config.line_height = 1.0;
    app.apply_config(&ctx);
    let tight = source_row_height(&ctx, &mut app, &mut frame);
    app.config.line_height = 2.0;
    app.apply_config(&ctx);
    let loose = source_row_height(&ctx, &mut app, &mut frame);
    assert!(
        loose > tight * 1.4,
        "行距倍数从 1.0 调到 2.0，渲染行高没跟着变大：{tight} → {loose}"
    );
}

/// 标题栏锁定约 1 cm 高，且文档名/图标要在这一行里垂直居中——
/// 不能因为加高就把内容顶到上缘，跟 macOS 红绿灯错开。
#[test]
fn titlebar_is_one_centimeter_tall_and_centered() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let bar = shapes.first().expect("至少要画出标题栏背景");
    assert!(
        (bar.clip_rect.height() - TITLEBAR_HEIGHT).abs() < 0.01,
        "标题栏高度应是 {TITLEBAR_HEIGHT} 逻辑点（约 1 cm），实际 {}",
        bar.clip_rect.height()
    );
    let title = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "首页" => Some(t),
            _ => None,
        })
        .expect("标题栏上的文档名");
    let title_mid = title.pos.y + title.galley.rect.height() / 2.0;
    assert!(
        (title_mid - TITLEBAR_HEIGHT / 2.0).abs() < 2.0,
        "标题文字应在标题栏垂直居中，实际文字中点 {title_mid}"
    );
}

/// 标签栏整行移除后，标题栏上的「首页 / + / ×」必须真的能切换、新建、关闭文档。
#[test]
fn titlebar_buttons_switch_new_and_close_documents() {
    fn button_pos(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t)
                    if t.galley.text() == text && t.pos.y < TITLEBAR_HEIGHT =>
                {
                    Some(t.pos + t.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("标题栏上应有「{text}」按钮"))
    }
    fn click_button(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        p: egui::Pos2,
    ) {
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
    }
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();

    // 首页 → 新建：应开出一个标签并离开首页。
    assert!(app.home && app.tabs.is_empty());
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let plus = button_pos(&shapes, "+");
    click_button(&ctx, &mut app, &mut frame, plus);
    assert!(!app.home, "点标题栏 + 应离开首页");
    assert_eq!(app.tabs.len(), 1, "点标题栏 + 应新建标签");

    // 新建 → 关闭：最后一个标签关掉后回首页。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let close = button_pos(&shapes, "×");
    click_button(&ctx, &mut app, &mut frame, close);
    assert!(
        app.home && app.tabs.is_empty(),
        "点 × 关掉最后一个标签应回首页"
    );

    // 再新建 → 首页按钮：只回首页，标签仍留着。
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let plus = button_pos(&shapes, "+");
    click_button(&ctx, &mut app, &mut frame, plus);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let home = button_pos(&shapes, "首页");
    click_button(&ctx, &mut app, &mut frame, home);
    assert!(app.home, "点标题栏「首页」应进入首页");
    assert_eq!(app.tabs.len(), 1, "回首页不应关掉已有标签");
}

/// 格式工具条总高 3.2 cm，上 0.7 cm 留白、下 2.5 cm 放按钮，中间一条横线。
#[test]
fn toolbar_is_split_by_a_divider_after_reserved_space() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);

    let toolbar = shapes
        .iter()
        .find(|cs| (cs.clip_rect.min.y - TITLEBAR_HEIGHT).abs() < 0.01)
        .expect("标题栏下方应有工具条面板");
    assert!(
        (toolbar.clip_rect.height() - TOOLBAR_HEIGHT).abs() < 0.01,
        "工具条应是 {TOOLBAR_HEIGHT} 逻辑点（约 3.2 cm），实际 {}",
        toolbar.clip_rect.height()
    );

    let divider_y = TITLEBAR_HEIGHT + TOOLBAR_RESERVED_HEIGHT;
    let divider = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::LineSegment { points, .. }
                if (points[0].y - divider_y).abs() < 3.0
                    && points[1].x - points[0].x > 1000.0 =>
            {
                Some(points)
            }
            _ => None,
        })
        .expect("留白区与按钮区之间应有横线");
    assert!(
        divider[0].x < 1.0 && divider[1].x > 1270.0,
        "横线应横跨整条工具条，实际 {divider:?}"
    );

    let upper_labels: Vec<&str> = shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.pos.y > TITLEBAR_HEIGHT && t.pos.y < divider_y + 3.0 => {
                Some(t.galley.text())
            }
            _ => None,
        })
        .collect();
    for label in ["开始", "插入", "页面样式", "高级", "视图", "工具", "AI"] {
        assert!(
            upper_labels.contains(&label),
            "上半区应有「{label}」选项卡，实际 {upper_labels:?}"
        );
    }
    assert!(
        upper_labels
            .iter()
            .all(|l| ["开始", "插入", "页面样式", "高级", "视图", "工具", "AI"].contains(l)),
        "上半区只该有七个选项卡文字，实际 {upper_labels:?}"
    );
}

/// 下半区每个功能都是独立盒子：1.5 cm × 1.7 cm，上半图标、下半文字。
#[test]
fn toolbar_buttons_are_independent_icon_text_boxes() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let boxes: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - TOOLBOX_WIDTH).abs() < 0.5
                    && (r.rect.height() - TOOLBOX_HEIGHT).abs() < 0.5 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    assert!(boxes.len() >= 20, "每个功能应占一个独立盒子，实际 {}", boxes.len());
    // 撤销/重做已改成选项卡行左侧的纯图标，不该再出现在下方盒子里。
    assert!(
        !shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t)
            if (t.galley.text() == "撤销" || t.galley.text() == "重做") && t.pos.y > TOOLBOX_AREA_TOP)),
        "撤销/重做应只留符号并放在选项卡行最左边"
    );

    let label = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "加粗" => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .expect("盒子里应有「加粗」文字");
    let b = boxes
        .iter()
        .find(|b| b.contains(label))
        .expect("文字应落在某个盒子里");
    assert!(
        label.y > b.top() + TOOLBOX_HEIGHT * 0.5,
        "文字应在盒子下半区，实际 {label:?} vs {b:?}"
    );

    let icon = shapes
        .iter()
        .find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == "B" => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        })
        .expect("加粗盒子应有 B 图标");
    assert!(
        icon.y < b.top() + TOOLBOX_HEIGHT * 0.5,
        "图标应在盒子上半区，实际 {icon:?}"
    );
}

/// 无序 / 有序 / 任务列表已拆成三个独立盒子，不再收在「列表」下拉里。
#[test]
fn toolbar_list_commands_are_three_separate_boxes() {
    fn label_pos(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    }

    fn click(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2) {
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
    }

    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);

    for label in ["无序列表", "有序列表", "任务列表"] {
        assert!(
            label_pos(&shapes, label).is_some(),
            "开始页应有独立的「{label}」盒子"
        );
    }
    assert!(
        label_pos(&shapes, "列表").is_none(),
        "不应再有汇总的「列表」下拉"
    );

    for (label, want) in [
        ("无序列表", "- "),
        ("有序列表", "1. "),
        ("任务列表", "- [ ] "),
    ] {
        app.new_document();
        for _ in 0..3 {
            run_real_frame(&ctx, &mut app, &mut frame, vec![]);
        }
        run_real_frame(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::Text("甲".into())],
        );
        let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
        let p = label_pos(&shapes, label).unwrap_or_else(|| panic!("未找到「{label}」"));
        click(&ctx, &mut app, &mut frame, p);
        assert!(
            app.doc.source().starts_with(want),
            "点「{label}」应把当前段转成 {want} 前缀，实际 {:?}",
            app.doc.source()
        );
    }
}

/// 正文段落与一级到六级标题已拆成七个独立盒子，不再收在「标题」下拉里。
#[test]
fn toolbar_heading_commands_are_seven_separate_boxes() {
    const WIDTH: f32 = 1800.0;

    fn paint(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        run_real_frame_stable_ext_with_width(ctx, app, frame, events, WIDTH).0
    }

    fn label_pos(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    }

    fn click(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2) {
        for ev in [
            vec![egui::Event::PointerMoved(p)],
            vec![pointer_button(p, true)],
            vec![pointer_button(p, false)],
        ] {
            paint(ctx, app, frame, ev);
        }
        for _ in 0..2 {
            paint(ctx, app, frame, vec![]);
        }
    }

    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let shapes = paint(&ctx, &mut app, &mut frame, vec![]);

    for label in [
        "正文段落", "一级标题", "二级标题", "三级标题", "四级标题", "五级标题", "六级标题",
    ] {
        assert!(
            label_pos(&shapes, label).is_some(),
            "开始页应有独立的「{label}」盒子"
        );
    }
    assert!(
        label_pos(&shapes, "标题").is_none(),
        "不应再有汇总的「标题」下拉"
    );

    for (label, want) in [
        ("正文段落", ""),
        ("一级标题", "# "),
        ("二级标题", "## "),
        ("三级标题", "### "),
        ("四级标题", "#### "),
        ("五级标题", "##### "),
        ("六级标题", "###### "),
    ] {
        app.new_document();
        for _ in 0..3 {
            paint(&ctx, &mut app, &mut frame, vec![]);
        }
        paint(
            &ctx,
            &mut app,
            &mut frame,
            vec![egui::Event::Text("甲".into())],
        );
        let shapes = paint(&ctx, &mut app, &mut frame, vec![]);
        let p = label_pos(&shapes, label).unwrap_or_else(|| panic!("未找到「{label}」"));
        click(&ctx, &mut app, &mut frame, p);
        if want.is_empty() {
            assert_eq!(app.doc.source(), "甲", "点「{label}」应恢复正文段落");
        } else {
            assert!(
                app.doc.source().starts_with(want),
                "点「{label}」应把当前段转成 {want} 前缀，实际 {:?}",
                app.doc.source()
            );
        }
    }
}

/// 原「插入」下拉里的七项已全部展开，且只出现在「插入」选项卡里。
#[test]
fn toolbar_insert_items_are_separate_boxes() {
    fn label_pos(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    }

    fn click(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2) {
        for ev in [
            vec![egui::Event::PointerMoved(p)],
            vec![pointer_button(p, true)],
            vec![pointer_button(p, false)],
        ] {
            run_real_frame_stable_ext(ctx, app, frame, ev);
        }
        for _ in 0..2 {
            run_real_frame_stable_ext(ctx, app, frame, vec![]);
        }
    }

    let labels = ["表格", "分隔线", "图表", "流程图", "脚注", "目录", "上传图片"];

    // 开始页不该出现这些展开项，也不该再有汇总的「插入」下拉。
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    for label in labels {
        assert!(
            label_pos(&shapes, label).is_none(),
            "「{label}」应只放在插入选项卡里"
        );
    }
    assert!(
        label_pos(&shapes, "插入").is_none(),
        "下半区不该再有「插入」下拉按钮"
    );

    // 插入页：七个盒子都独立存在。
    app.toolbar_tab = ToolbarTab::Insert;
    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    for label in labels {
        assert!(
            label_pos(&shapes, label).is_some(),
            "插入页应有独立的「{label}」盒子"
        );
    }

    // 逐个真实点击可安全验证的命令（上传图片会弹系统文件框，只验按钮存在）。
    for (label, expect) in [
        ("表格", None),
        ("分隔线", Some("---")),
        ("图表", Some("```mermaid")),
        ("流程图", Some("```mermaid")),
        ("脚注", Some("[^1]:")),
        ("目录", Some("[TOC]")),
    ] {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.new_document();
        app.toolbar_tab = ToolbarTab::Insert;
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..3 {
            run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
        }
        let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
        let p = label_pos(&shapes, label).unwrap_or_else(|| panic!("未找到「{label}」"));
        click(&ctx, &mut app, &mut frame, p);
        match expect {
            None => assert!(app.table_dialog, "点「表格」应打开表格对话框"),
            Some(s) => assert!(
                app.doc.source().contains(s),
                "点「{label}」应插入含 {s:?} 的内容，实际 {:?}",
                app.doc.source()
            ),
        }
    }
}

/// 七个功能区选项卡：开始/插入/高级有按钮，其余先留白；点选项卡能来回切。
#[test]
fn toolbar_tabs_switch_between_button_areas_and_blank_placeholders() {
    fn toolbox_text(shapes: &[egui::epaint::ClippedShape], label: &str) -> bool {
        shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                let y = t.pos.y + t.galley.rect.center().y;
                y > TOOLBOX_AREA_TOP && y < TOP_BARS_HEIGHT
            }
            _ => false,
        })
    }
    fn text_center(shapes: &[egui::epaint::ClippedShape], label: &str, min_y: f32, max_y: f32) -> egui::Pos2 {
        shapes
            .iter()
            .find_map(|cs| match &cs.shape {
                egui::Shape::Text(t) if t.galley.text() == label => {
                    let p = t.pos + t.galley.rect.center().to_vec2();
                    (p.y > min_y && p.y < max_y).then_some(p)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("未找到「{label}」"))
    }
    fn click(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        p: egui::Pos2,
    ) {
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
    }
    let tab_range = (TITLEBAR_HEIGHT + 1.0, TITLEBAR_HEIGHT + TOOLBAR_RESERVED_HEIGHT);
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();

    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(app.toolbar_tab, ToolbarTab::Start);
    assert!(text_center(&shapes, "加粗", TOOLBOX_AREA_TOP, TOP_BARS_HEIGHT).x > 0.0);

    let tab = text_center(&shapes, "页面样式", tab_range.0, tab_range.1);
    click(&ctx, &mut app, &mut frame, tab);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(app.toolbar_tab, ToolbarTab::PageStyle);
    assert!(
        !shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Rect(r)
            if (r.rect.width() - TOOLBOX_WIDTH).abs() < 0.5
                && (r.rect.height() - TOOLBOX_HEIGHT).abs() < 0.5)),
        "留白选项卡不该画工具盒子"
    );

    let tab = text_center(&shapes, "插入", tab_range.0, tab_range.1);
    click(&ctx, &mut app, &mut frame, tab);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(app.toolbar_tab, ToolbarTab::Insert);
    assert!(text_center(&shapes, "图片", TOOLBOX_AREA_TOP, TOP_BARS_HEIGHT).x > 0.0);
    // 「加粗」只属于开始选项卡。
    assert!(
        !shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t)
            if t.galley.text() == "加粗" && t.pos.y > TOOLBOX_AREA_TOP)),
        "插入选项卡不该出现开始选项卡的加粗"
    );

    let tab = text_center(&shapes, "开始", tab_range.0, tab_range.1);
    click(&ctx, &mut app, &mut frame, tab);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(app.toolbar_tab, ToolbarTab::Start);
    assert!(text_center(&shapes, "加粗", TOOLBOX_AREA_TOP, TOP_BARS_HEIGHT).x > 0.0);
    // 快照 / 关系图 / 统计已迁走，开始页不再有这三个盒子。
    assert!(
        !toolbox_text(&shapes, "快照")
            && !toolbox_text(&shapes, "关系图")
            && !toolbox_text(&shapes, "统计"),
        "开始页不该再有快照 / 关系图 / 统计"
    );

    let tab = text_center(&shapes, "高级", tab_range.0, tab_range.1);
    click(&ctx, &mut app, &mut frame, tab);
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    assert_eq!(app.toolbar_tab, ToolbarTab::Advanced);
    for label in ["快照", "关系图", "统计"] {
        assert!(toolbox_text(&shapes, label), "高级页应有「{label}」盒子");
    }
    assert!(!toolbox_text(&shapes, "加粗"), "高级页不该出现开始页的加粗");
}

/// 撤销/重做移出盒子后，选项卡行最左边的两个纯图标仍要能真正撤销与重做。
#[test]
fn toolbar_undo_redo_icons_in_tab_row_work() {
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    // new_document() 会同时把光标焦点放到第一块，文字事件才能进入编辑器。
    app.new_document();
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    run_real_frame(
        &ctx,
        &mut app,
        &mut frame,
        vec![egui::Event::Text("甲".into())],
    );
    for _ in 0..3 {
        run_real_frame(&ctx, &mut app, &mut frame, vec![]);
    }
    assert!(app.doc.source().contains('甲'), "前置输入应进入文档");

    let y = TITLEBAR_HEIGHT + TOOLBAR_RESERVED_HEIGHT * 0.5;
    fn click_icon(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        p: egui::Pos2,
    ) {
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
    }
    // 选项卡行最左边：10pt 留白 + 24pt 图标，两个图标相隔 4pt。
    click_icon(&ctx, &mut app, &mut frame, egui::pos2(22.0, y));
    assert!(!app.doc.source().contains('甲'), "点最左侧撤销图标应回退输入");
    click_icon(&ctx, &mut app, &mut frame, egui::pos2(50.0, y));
    assert!(app.doc.source().contains('甲'), "点第二个重做图标应恢复输入");
}

/// 选项卡行最右侧的折叠按钮：点一下收起下半工具区（工具条只剩 0.7 cm），再点展开。
#[test]
fn toolbar_collapse_button_folds_tool_area() {
    fn chevron_center(shapes: &[egui::epaint::ClippedShape]) -> egui::Pos2 {
        // chevron 是选项卡行 y 范围内最右侧的短线段（撤销/重做在左，选项卡下划线居中）。
        let pts: Vec<egui::Pos2> = shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[0].y > TITLEBAR_HEIGHT
                        && points[1].y < TITLEBAR_HEIGHT + TOOLBAR_RESERVED_HEIGHT
                        && points[0].x > 1150.0 =>
                {
                    Some(*points)
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert!(!pts.is_empty(), "选项卡行最右侧应有折叠 chevron 图标");
        let left = pts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let right = pts.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let top = pts.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let bottom = pts.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        egui::pos2((left + right) * 0.5, (top + bottom) * 0.5)
    }

    fn click_chevron(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        p: egui::Pos2,
    ) {
        for ev in [
            vec![egui::Event::PointerMoved(p)],
            vec![pointer_button(p, true)],
            vec![pointer_button(p, false)],
        ] {
            run_real_frame_stable_ext(ctx, app, frame, ev);
        }
        for _ in 0..2 {
            run_real_frame_stable_ext(ctx, app, frame, vec![]);
        }
    }

    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();

    // 初始展开：工具条 3.2 cm，下半区画出工具按钮。
    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    let toolbar = shapes
        .iter()
        .find(|cs| (cs.clip_rect.min.y - TITLEBAR_HEIGHT).abs() < 0.01)
        .expect("标题栏下方应有工具条面板");
    assert!(
        (toolbar.clip_rect.height() - TOOLBAR_HEIGHT).abs() < 0.01,
        "初始应为展开的 3.2 cm，实际 {}",
        toolbar.clip_rect.height()
    );
    assert!(!app.toolbar_collapsed);
    let p = chevron_center(&shapes);

    click_chevron(&ctx, &mut app, &mut frame, p);
    assert!(app.toolbar_collapsed, "点最右侧 chevron 应折叠工具区");

    // 折叠后：工具条只剩 0.7 cm 选项卡行，下半区不再画盒子与按钮文字。
    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    let toolbar = shapes
        .iter()
        .find(|cs| (cs.clip_rect.min.y - TITLEBAR_HEIGHT).abs() < 0.01)
        .expect("折叠后工具条面板仍在");
    assert!(
        (toolbar.clip_rect.height() - TOOLBAR_RESERVED_HEIGHT).abs() < 0.01,
        "折叠后工具条应只剩 0.7 cm，实际 {}",
        toolbar.clip_rect.height()
    );
    assert!(
        !shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t)
            if t.galley.text() == "加粗" && t.pos.y > TITLEBAR_HEIGHT)),
        "折叠后不该再画工具按钮"
    );
    assert!(
        !shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Rect(r)
            if (r.rect.width() - TOOLBOX_WIDTH).abs() < 0.5
                && (r.rect.height() - TOOLBOX_HEIGHT).abs() < 0.5)),
        "折叠后不该有工具盒子"
    );

    // 再点一下（图标换成向下 chevron）展开，工具区恢复。
    let p = chevron_center(&shapes);
    click_chevron(&ctx, &mut app, &mut frame, p);
    assert!(!app.toolbar_collapsed, "再点 chevron 应展开工具区");
    let (shapes, _) = run_real_frame_stable_ext(&ctx, &mut app, &mut frame, vec![]);
    assert!(
        shapes.iter().any(|cs| matches!(&cs.shape, egui::Shape::Text(t)
            if t.galley.text() == "加粗" && t.pos.y > TITLEBAR_HEIGHT)),
        "展开后应恢复工具按钮"
    );
}

/// 工具盒子放得下时整组居中；21 个盒子在 1280 宽窗口放不下，改为单行横向滚动。
#[test]
fn toolbar_button_boxes_are_centered() {
    fn box_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
        let rects: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Rect(r)
                    if (r.rect.width() - TOOLBOX_WIDTH).abs() < 0.5
                        && (r.rect.height() - TOOLBOX_HEIGHT).abs() < 0.5 =>
                {
                    Some(r.rect)
                }
                _ => None,
            })
            .collect();
        assert!(!rects.is_empty(), "当前选项卡应画出工具盒子");
        rects
    }
    fn center(rects: &[egui::Rect]) -> f32 {
        let left = rects.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
        let right = rects.iter().map(|r| r.right()).fold(f32::NEG_INFINITY, f32::max);
        (left + right) * 0.5
    }

    // 1280 宽：开始页 21 个盒子可能仍超出宽度；无论如何都只占一行，
    // 放不下时从左侧铺到右侧（横向滚动），放得下时整组居中。
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let rects = box_rects(&shapes);
    assert!(rects.len() <= 21, "开始页最多 21 个盒子，实际 {}", rects.len());
    let left = rects.iter().map(|r| r.left()).fold(f32::INFINITY, f32::min);
    let right = rects.iter().map(|r| r.right()).fold(f32::NEG_INFINITY, f32::max);
    if left < 20.0 {
        assert!(right > 1250.0, "溢出时可见盒子应横向铺满视口，实际 {left}..{right}");
    } else {
        assert!(
            ((left + right) * 0.5 - 640.0).abs() < 3.0,
            "放得下时应整组居中，实际 {left}..{right}"
        );
    }
    assert!(
        rects.iter().all(|r| (r.top() - rects[0].top()).abs() < 0.5),
        "下半区只够一行盒子，不能换行"
    );

    // 1800 宽：全部盒子放得下，整组应在窗口里居中。
    let ctx_wide = egui::Context::default();
    let mut app_wide = test_app();
    let mut frame_wide = eframe::Frame::_new_kittest();
    let (shapes, _) =
        run_real_frame_ext_with_width(&ctx_wide, &mut app_wide, &mut frame_wide, vec![], 1800.0);
    let rects = box_rects(&shapes);
    assert_eq!(rects.len(), 21, "宽窗口应画出开始页全部 21 个盒子，实际 {}", rects.len());
    let start_center = center(&rects);
    assert!(
        (start_center - 900.0).abs() < 3.0,
        "宽窗口里开始页盒子组应居中，实际中心 {start_center}"
    );

    // 插入页展开后有 12 个盒子，1280 宽下仍应居中。
    app.toolbar_tab = ToolbarTab::Insert;
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let insert_rects = box_rects(&shapes);
    assert_eq!(insert_rects.len(), 12, "插入页应有 12 个盒子，实际 {}", insert_rects.len());
    let insert_center = center(&insert_rects);
    assert!(
        (insert_center - 640.0).abs() < 3.0,
        "插入页盒子组也应居中，实际中心 {insert_center}"
    );

    // 高级页只有快照 / 关系图 / 统计三个盒子，1280 宽下放得下且应居中。
    app.toolbar_tab = ToolbarTab::Advanced;
    let (shapes, _) = run_real_frame_ext(&ctx, &mut app, &mut frame, vec![]);
    let advanced_rects = box_rects(&shapes);
    assert_eq!(
        advanced_rects.len(),
        3,
        "高级页应有 3 个盒子，实际 {}",
        advanced_rects.len()
    );
    let advanced_center = center(&advanced_rects);
    assert!(
        (advanced_center - 640.0).abs() < 3.0,
        "高级页盒子组也应居中，实际中心 {advanced_center}"
    );
}

/// 快照 / 关系图 / 统计已从「开始」迁到「高级」选项卡，查找和替换仍留在「开始」。
#[test]
fn toolbar_advanced_tab_holds_snapshot_graph_and_stats() {
    // 用宽窗口：开始页 21 个盒子在 1280 宽下会横向滚动，末尾「查找和替换」看不见。
    const WIDTH: f32 = 1800.0;

    fn label_pos(shapes: &[egui::epaint::ClippedShape], label: &str) -> Option<egui::Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                let p = t.pos + t.galley.rect.center().to_vec2();
                (p.y > TOOLBOX_AREA_TOP && p.y < TOP_BARS_HEIGHT).then_some(p)
            }
            _ => None,
        })
    }

    fn paint(
        ctx: &egui::Context,
        app: &mut MarkdApp,
        frame: &mut eframe::Frame,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        run_real_frame_stable_ext_with_width(ctx, app, frame, events, WIDTH).0
    }

    fn click(ctx: &egui::Context, app: &mut MarkdApp, frame: &mut eframe::Frame, p: egui::Pos2) {
        for ev in [
            vec![egui::Event::PointerMoved(p)],
            vec![pointer_button(p, true)],
            vec![pointer_button(p, false)],
        ] {
            paint(ctx, app, frame, ev);
        }
        for _ in 0..2 {
            paint(ctx, app, frame, vec![]);
        }
    }

    let moved = ["快照", "关系图", "统计"];

    // 开始页：三个迁移按钮都不再出现，「查找和替换」仍在。
    let ctx = egui::Context::default();
    let mut app = test_app();
    let mut frame = eframe::Frame::_new_kittest();
    let shapes = paint(&ctx, &mut app, &mut frame, vec![]);
    for label in moved {
        assert!(
            label_pos(&shapes, label).is_none(),
            "「{label}」不该再留在开始页"
        );
    }
    assert!(
        label_pos(&shapes, "查找和替换").is_some(),
        "「查找和替换」应继续留在开始页"
    );

    // 高级页：三个按钮都在，且不再有开始页的「查找和替换」。
    app.toolbar_tab = ToolbarTab::Advanced;
    let shapes = paint(&ctx, &mut app, &mut frame, vec![]);
    for label in moved {
        assert!(
            label_pos(&shapes, label).is_some(),
            "高级页应有独立的「{label}」盒子"
        );
    }
    assert!(
        label_pos(&shapes, "查找和替换").is_none(),
        "高级页不该有「查找和替换」"
    );

    // 真实点一下「统计」，确认盒子仍绑定正确命令（纯 UI 状态，不写盘不起线程）。
    let p = label_pos(&shapes, "统计").unwrap_or_else(|| panic!("未找到「统计」"));
    click(&ctx, &mut app, &mut frame, p);
    assert!(app.stats.open, "点高级页「统计」应打开写作统计面板");
}
