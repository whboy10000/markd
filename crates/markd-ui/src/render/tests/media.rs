use super::*;

/// 图片预览：大图压到视口高度的六成以内，小图保持原尺寸不放大。
#[test]
fn image_preview_is_capped_to_viewport() {
    let theme = Theme::get(ThemeKind::Light);
    let dir = std::env::temp_dir().join(format!("markd-img-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(200, 1200, image::Rgba([200, 30, 30, 255]))
        .save(dir.join("tall.png"))
        .unwrap();
    image::RgbaImage::from_pixel(50, 40, image::Rgba([30, 30, 200, 255]))
        .save(dir.join("small.png"))
        .unwrap();
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut heights = [0.0f32; 2];
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let c = ui.ctx().clone();
                let mut env = RenderEnv {
                    theme: &theme,
                    ctx: &c,
                    images: &mut images,
                    base_dir: Some(&dir),
                    outline: &outline,
                    actions: &mut actions,
                    max_width: 700.0,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                for (i, url) in ["tall.png", "small.png"].iter().enumerate() {
                    let block = Block::Paragraph {
                        content: vec![Inline::Image {
                            alt: "图".to_string(),
                            url: url.to_string(),
                            width: None,
                        }],
                        source: format!("![图]({url})"),
                    };
                    heights[i] = render_block(ui, i, &block, &mut env, false).rect.height();
                }
            });
        },
    );
    out.textures_delta.clear();
    assert!(heights[0] > 300.0, "大图应有一定高度：{}", heights[0]);
    assert!(
        heights[0] <= 361.0,
        "大图预览应压到视口高度的六成以内，实际 {}",
        heights[0]
    );
    assert!(
        (heights[1] - 40.0).abs() < 1.5,
        "小图应保持原尺寸不放大，实际 {}",
        heights[1]
    );
    let _ = std::fs::remove_dir_all(&dir);
}
