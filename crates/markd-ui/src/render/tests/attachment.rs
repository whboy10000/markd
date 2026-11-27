use super::*;

/// 只含一个链接的段落（附件卡片的触发形态）。
fn link_para(label: &str, url: &str) -> Block {
    Block::Paragraph {
        content: vec![Inline::Link {
            label: vec![Inline::Text(label.to_string())],
            url: url.to_string(),
        }],
        source: format!("[{label}]({url})").into(),
    }
}

fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

fn find_text(shapes: &[egui::epaint::ClippedShape], want: &str) -> Option<egui::Pos2> {
    shapes.iter().find_map(|cs| match &cs.shape {
        egui::Shape::Text(t) if t.galley.text() == want => {
            Some(t.pos + t.galley.rect.center().to_vec2())
        }
        _ => None,
    })
}

/// 临时目录里放一个已知字节数的 PDF，卡片走真实 `stat`，大小文案才是定值。
fn fake_doc_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "markd-att-card-{}-{}",
        std::process::id(),
        // 同一进程里多个用例各用一份，互不覆盖
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(dir.join("files")).unwrap();
    std::fs::write(dir.join("files/年报 2026.pdf"), vec![7u8; 1536]).unwrap();
    dir
}

/// 渲染一帧附件卡片，然后按 `at` 打一次点击；返回末帧的动作、形状与卡片矩形。
fn file_frame(
    block: &Block,
    at: Option<egui::Pos2>,
    dir: &std::path::Path,
) -> (Vec<Action>, Vec<egui::epaint::ClippedShape>, egui::Rect) {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let ctx = egui::Context::default();
    let mut images = ImageCache::new();
    let mut rect = egui::Rect::NOTHING;
    let press = |down: bool| -> Vec<egui::Event> {
        let p = at.unwrap();
        let mut ev = vec![egui::Event::PointerMoved(p)];
        ev.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        });
        ev
    };
    let mut actions: Vec<Action> = Vec::new();
    let mut run = |events: Vec<egui::Event>| -> Vec<egui::epaint::ClippedShape> {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let c = ui.ctx().clone();
                    let mut env = RenderEnv {
                        theme: &theme,
                        ctx: &c,
                        images: &mut images,
                        base_dir: Some(dir),
                        outline: &outline,
                        actions: &mut actions,
                        max_width: 700.0,
                        cross_sel: None,
                        folded: &folded,
                        group_tab: 0,
                    };
                    rect = render_block(ui, 0, block, &mut env, false).rect;
                });
            },
        );
        out.textures_delta.clear();
        out.shapes
    };
    let shapes = run(vec![]);
    let shapes = if at.is_some() {
        let _ = run(press(true));
        run(press(false))
    } else {
        shapes
    };
    (actions, shapes, rect)
}

/// 单个附件链接画成文件卡片：类型徽标、真实文件名、大小与来源都在，链接原文可见。
#[test]
fn single_attachment_link_renders_file_card() {
    let dir = fake_doc_dir();
    let block = link_para("年报 2026.pdf", "files/%E5%B9%B4%E6%8A%A5%202026.pdf");
    let (actions, shapes, _) = file_frame(&block, None, &dir);
    let t = texts(&shapes);
    assert!(t.iter().any(|s| s == "附件 · PDF"), "应有类型徽标：{t:?}");
    assert!(
        t.iter().any(|s| s == "年报 2026.pdf"),
        "链接文字就是卡片标题：{t:?}"
    );
    // 1536 字节 → 1.5 KB，且标明是从文档目录里找到的本机文件
    assert!(
        t.iter()
            .any(|s| s == "PDF 文档 · 1.5 KB · 本地文件"),
        "状态行应给出类型·大小·来源：{t:?}"
    );
    assert!(
        t.iter().any(|s| s == "files/%E5%B9%B4%E6%8A%A5%202026.pdf"),
        "卡片上应能看到地址原文：{t:?}"
    );
    assert!(actions.is_empty(), "只渲染不产生动作：{actions:?}");
}

/// 点文件名 = 预览这个附件（本机的交给系统程序，远程的后台下载）。
#[test]
fn clicking_file_name_previews_attachment() {
    let dir = fake_doc_dir();
    let block = link_para("年报 2026.pdf", "files/%E5%B9%B4%E6%8A%A5%202026.pdf");
    let (_, shapes, _) = file_frame(&block, None, &dir);
    let name = find_text(&shapes, "年报 2026.pdf").expect("应有文件名一行");
    let (actions, _, _) = file_frame(&block, Some(name), &dir);
    assert!(
        actions.iter().any(
            |a| matches!(a, Action::PreviewFile(u) if u == "files/%E5%B9%B4%E6%8A%A5%202026.pdf")
        ),
        "点文件名应请求预览：{actions:?}"
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Jump(_) | Action::Caret(..))),
        "点文件名不应把本块切进编辑态：{actions:?}"
    );
}

/// 「新标签页」交地址给系统，「编辑」进编辑态，「删除」走删除确认。
#[test]
fn card_buttons_open_tab_edit_and_delete() {
    let dir = fake_doc_dir();
    let block = link_para("年报 2026.pdf", "files/%E5%B9%B4%E6%8A%A5%202026.pdf");
    let (_, shapes, _) = file_frame(&block, None, &dir);
    let abs = dir.join("files/年报 2026.pdf");
    let want = crate::attachment::file_href(&abs.to_string_lossy());

    let tab = find_text(&shapes, "新标签页").expect("应有「新标签页」按钮");
    let (actions, _, _) = file_frame(&block, Some(tab), &dir);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::OpenLink(u) if *u == want)),
        "本地附件的新标签页应是 file:// 绝对地址 {want}：{actions:?}"
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::PreviewFile(_))),
        "点「新标签页」不该顺带触发下载：{actions:?}"
    );

    let edit = find_text(&shapes, "编辑").expect("应有「编辑」按钮");
    let (actions, _, _) = file_frame(&block, Some(edit), &dir);
    assert!(
        actions.iter().any(|a| matches!(a, Action::Jump(0))),
        "点「编辑」应让本块进入编辑态：{actions:?}"
    );

    let del = find_text(&shapes, "删除").expect("应有「删除」按钮");
    let (actions, _, _) = file_frame(&block, Some(del), &dir);
    assert!(
        actions.iter().any(|a| matches!(a, Action::AskDeleteBlock(0))),
        "点「删除」应请求删除确认：{actions:?}"
    );
}

/// 句中夹着的文件链接仍是普通链接：一句话里塞一张卡片会打断阅读。
/// 链接放在段首，这样「只判首元素、不判段落只有一个元素」的写法会立刻露馅。
#[test]
fn inline_file_link_stays_plain_text() {
    let dir = fake_doc_dir();
    let block = Block::Paragraph {
        content: vec![
            Inline::Link {
                label: vec![Inline::Text("季度报告.pdf".to_string())],
                url: "files/%E5%B9%B4%E6%8A%A5%202026.pdf".to_string(),
            },
            Inline::Text(" 已附上，请查收".to_string()),
        ],
        source: "[季度报告.pdf](files/x.pdf) 已附上，请查收".into(),
    };
    let (actions, shapes, _) = file_frame(&block, None, &dir);
    let t = texts(&shapes);
    assert!(
        !t.iter().any(|s| s.starts_with("附件 ·")),
        "夹在句中的链接不该出卡片：{t:?}"
    );
    assert!(
        t.iter().any(|s| s.contains("已附上，请查收")),
        "正文应照常显示：{t:?}"
    );
    assert!(actions.is_empty(), "{actions:?}");
}

/// 图片、网页、Markdown 文档链接各有自己的显示方式，不能被当成附件卡片。
#[test]
fn non_attachment_links_stay_links() {
    let dir = fake_doc_dir();
    for url in [
        "https://x.com/a.png",
        "https://www.rust-lang.org/",
        "notes/其它文档.md",
        "https://x.com/a/notatype",
    ] {
        let block = link_para("链接", url);
        let (actions, shapes, _) = file_frame(&block, None, &dir);
        let t = texts(&shapes);
        assert!(
            !t.iter().any(|s| s.starts_with("附件 ·")),
            "{url} 不该画成附件卡片：{t:?}"
        );
        assert!(actions.is_empty(), "{url}: {actions:?}");
    }
}

/// 链接文字为空时用地址末段当名字；地址解码后中文名照样显示。
#[test]
fn empty_label_falls_back_to_url_name() {
    let dir = fake_doc_dir();
    let block = link_para("", "files/%E5%B9%B4%E6%8A%A5%202026.pdf");
    let (_, shapes, _) = file_frame(&block, None, &dir);
    let t = texts(&shapes);
    assert!(
        t.iter().any(|s| s == "年报 2026.pdf"),
        "空链接文字应回退到文件名：{t:?}"
    );
}

/// 本地文件被挪走时如实说明并给出解析后的路径，不能假装能打开。
#[test]
fn missing_local_file_says_so() {
    let dir = fake_doc_dir();
    let block = link_para("没了.pdf", "files/没了.pdf");
    let (_, shapes, _) = file_frame(&block, None, &dir);
    let t = texts(&shapes);
    assert!(
        t.iter()
            .any(|s| s.starts_with("PDF 文档 · 找不到本地文件：") && s.ends_with("没了.pdf")),
        "缺文件应说明并给出路径：{t:?}"
    );
}

/// 远程附件没下过时给「下载预览」；后台下载进行中给「下载中…」并屏蔽重复点击。
#[test]
fn remote_card_offers_download_and_shows_progress() {
    let dir = fake_doc_dir();
    let url = "https://example.com/files/report.pdf";
    let block = link_para("report.pdf", url);
    let (_, shapes, _) = file_frame(&block, None, &dir);
    let t = texts(&shapes);
    assert!(
        t.iter().any(|s| s == "下载预览"),
        "未下载的远程附件应有「下载预览」：{t:?}"
    );
    assert!(
        t.iter()
            .any(|s| s == "PDF 文档 · 远程文件，点「下载预览」取到本机"),
        "远程附件应说明还没下载：{t:?}"
    );

    // 同一条链接标记成下载中：按钮换成「下载中…」，且不再产生新的下载请求。
    let mut images = ImageCache::new();
    images.begin_download(url);
    assert!(images.is_downloading(url));
    images.end_download(url);
    assert!(!images.is_downloading(url));
    images.begin_download(" https://example.com/files/report.pdf ");
    assert!(
        images.is_downloading(url),
        "首尾空白不算地址的一部分，标记要对得上"
    );
}

/// 卡片限宽并在正文列居中：一屏宽的文件条只是把边框拉长，没有可读性收益。
#[test]
fn file_card_is_capped_and_centered() {
    let dir = fake_doc_dir();
    let block = link_para("年报 2026.pdf", "files/%E5%B9%B4%E6%8A%A5%202026.pdf");
    let (_, _, rect) = file_frame(&block, None, &dir);
    // 面板内边距 12×2 之外就是卡片宽（harness 的 max_width = 700，超过上限）。
    let outer_max = crate::render::FILE_CARD_MAX_WIDTH + 24.0;
    assert!(
        (rect.width() - outer_max).abs() < 1.0,
        "卡片宽度应被限到 {outer_max}，实际 {}",
        rect.width()
    );
    // 900 宽的窗口里 CentralPanel 左右对称，居中即卡片中线落在 450。
    assert!(
        (rect.center().x - 450.0).abs() < 2.0,
        "卡片应居中，实际中线 {}",
        rect.center().x
    );
    assert!(rect.height() < 260.0, "一条文件栏不该高过一屏");
}
