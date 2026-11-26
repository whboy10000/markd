use super::*;
use crate::video::{VideoInfo, VideoSource};

fn fence(lang: &str, code: &str) -> Block {
    Block::Code {
        lang: lang.to_string(),
        code: code.to_string(),
        source: format!("```{lang}\n{code}\n```").into(),
        quote: 0,
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

/// 临时目录里放一张「封面」，卡片走本地解码，单测不碰网络。
fn fake_cover_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "markd-video-{}-{}",
        std::process::id(),
        // 同一进程里多个用例各用一份，互不覆盖
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(320, 180, image::Rgba([10, 120, 200, 255]))
        .save(dir.join("cover.png"))
        .unwrap();
    dir
}

/// 渲染一帧视频围栏，然后按 `at` 打一次点击；返回末帧的动作与形状。
fn video_frame(
    block: &Block,
    at: Option<egui::Pos2>,
    dir: &std::path::Path,
    prime: bool,
) -> (Vec<Action>, Vec<egui::epaint::ClippedShape>, egui::Rect) {
    let theme = Theme::get(ThemeKind::Light);
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let ctx = egui::Context::default();
    let mut images = ImageCache::new();
    if prime {
        images.prime_video(
            &VideoSource::Ep {
                ep_id: "406221".into(),
            },
            Some(VideoInfo {
                title: "康熙来了 2007·史上最嗜赌女星，扑克牌不离手".into(),
                cover: "cover.png".into(),
                duration: 2605,
            }),
        );
    }
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
    let shapes = if let Some(p) = at {
        let _ = p;
        let _ = run(press(true));
        run(press(false))
    } else {
        shapes
    };
    (actions, shapes, rect)
}

/// ```video 围栏画成播放器卡片：标题、时长角标、B 站标记都在，代码块功能栏不再出现。
#[test]
fn video_fence_renders_as_player_card() {
    let dir = fake_cover_dir();
    let block = fence("video", "https://www.bilibili.com/bangumi/play/ep406221");
    let (actions, shapes, rect) = video_frame(&block, None, &dir, true);
    let t = texts(&shapes);
    assert!(
        t.iter().any(|s| s == "视频 · bilibili"),
        "应有视频标记：{t:?}"
    );
    assert!(
        t.iter().any(|s| s.starts_with("康熙来了 2007")),
        "应显示接口返回的标题：{t:?}"
    );
    assert!(
        t.iter().any(|s| s == "43:25"),
        "番剧毫秒时长应显示为 43:25：{t:?}"
    );
    assert!(
        t.iter()
            .any(|s| s == "https://www.bilibili.com/bangumi/play/ep406221"),
        "卡片上应能看到链接原文：{t:?}"
    );
    // 卡片不是代码块：不该出现代码块的语言下拉标签与「折叠」按钮。
    assert!(
        !t.iter().any(|s| s == "折叠"),
        "视频卡片不该复用代码块功能栏：{t:?}"
    );
    assert!(rect.is_positive(), "卡片应占一个正矩形");
    assert!(actions.is_empty(), "只渲染不产生动作：{actions:?}");
}

/// 点封面 = 用系统浏览器打开 B 站播放页；不进入编辑态。
#[test]
fn clicking_poster_opens_bilibili_page() {
    let dir = fake_cover_dir();
    let block = fence("video", "https://www.bilibili.com/bangumi/play/ep406221");
    // 第一帧量出卡片矩形，点击落在封面区（卡片上部）。
    let (_, shapes, rect) = video_frame(&block, None, &dir, true);
    let poster_y = rect.top() + 60.0;
    let (actions, _, _) = video_frame(
        &block,
        Some(egui::pos2(rect.center().x, poster_y)),
        &dir,
        true,
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::OpenLink(u) if u == "https://www.bilibili.com/bangumi/play/ep406221")),
        "点封面应打开 B 站播放页：{actions:?}"
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Jump(_) | Action::Caret(..))),
        "点封面不应把本块切进编辑态：{actions:?}"
    );
    let _ = &shapes;
}

/// 卡片右上角「编辑」进编辑态、「删除」走删除确认，与代码块同一套动作。
#[test]
fn video_card_buttons_jump_and_delete() {
    let dir = fake_cover_dir();
    let block = fence("video", "https://www.bilibili.com/bangumi/play/ep406221");
    let (_, shapes, _) = video_frame(&block, None, &dir, true);
    let edit = find_text(&shapes, "编辑").expect("应有「编辑」按钮");
    let (actions, _, _) = video_frame(&block, Some(edit), &dir, true);
    assert!(
        actions.iter().any(|a| matches!(a, Action::Jump(0))),
        "点「编辑」应让本块进入编辑态：{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::OpenLink(_))),
        "点「编辑」不应顺手打开浏览器：{actions:?}"
    );

    let del = find_text(&shapes, "删除").expect("应有「删除」按钮");
    let (actions, _, _) = video_frame(&block, Some(del), &dir, true);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::AskDeleteBlock(0))),
        "点「删除」应请求删除确认：{actions:?}"
    );
}

/// 认不出链接的 ```video（比如手写说明）仍按代码块显示，链接原文可见。
#[test]
fn unparseable_video_fence_stays_code() {
    let dir = fake_cover_dir();
    let block = fence("video", "这里写视频说明，不是链接");
    let (actions, shapes, _) = video_frame(&block, None, &dir, false);
    let t = texts(&shapes);
    assert!(
        !t.iter().any(|s| s == "视频 · bilibili"),
        "解析失败不应画卡片：{t:?}"
    );
    assert!(
        t.iter().any(|s| s.contains("这里写视频说明")),
        "内容应按代码原样显示：{t:?}"
    );
    assert!(actions.is_empty(), "{actions:?}");
}

/// 普通代码块里贴 B 站链接（注释、示例）不能被当成视频。
#[test]
fn bilibili_url_in_plain_code_stays_code() {
    let dir = fake_cover_dir();
    let code = "// https://www.bilibili.com/video/BV1GJ411x7h7\nfn main() {}";
    let block = fence("rust", code);
    let (actions, shapes, _) = video_frame(&block, None, &dir, false);
    let t = texts(&shapes);
    assert!(
        !t.iter().any(|s| s == "视频 · bilibili"),
        "rust 围栏不是视频：{t:?}"
    );
    assert!(actions.is_empty(), "{actions:?}");
}

/// 卡片限宽并在正文列居中：铺满 700 列宽的 16:9 有一面墙那么高，压得正文看不见。
#[test]
fn video_card_is_capped_and_centered() {
    let dir = fake_cover_dir();
    let block = fence("video", "https://www.bilibili.com/bangumi/play/ep406221");
    let (_, _, rect) = video_frame(&block, None, &dir, true);
    // 面板内边距 12×2 之外就是封面宽（harness 的 max_width = 700，超过上限）。
    let outer_max = crate::render::VIDEO_CARD_MAX_WIDTH + 24.0;
    assert!(
        rect.width() <= outer_max + 0.5,
        "卡片宽度应被限到 {outer_max}，实际 {}",
        rect.width()
    );
    assert!(
        rect.width() >= outer_max - 0.5,
        "限宽后不该再窄：{}",
        rect.width()
    );
    // 900 宽的窗口里 CentralPanel 左右对称，居中即卡片中线落在 450。
    assert!(
        (rect.center().x - 450.0).abs() < 2.0,
        "卡片应居中，实际中线 {}",
        rect.center().x
    );
    // 封面 420 宽 → 高 236；整卡（含功能栏与标题）应明显低于旧的一屏墙。
    assert!(
        rect.height() < 400.0,
        "整卡高度应 <400，实际 {}",
        rect.height()
    );
}
