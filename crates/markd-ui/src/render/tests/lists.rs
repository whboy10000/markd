use super::*;

/// 子列表标记要和父列表不同：无序变形状，有序变编号样式。
#[test]
fn nested_list_markers_differ_by_level() {
    // 层级换算（默认 2 空格一层）
    assert_eq!(list_level(0), 0);
    assert_eq!(list_level(1), 1);
    assert_eq!(list_level(2), 1);
    assert_eq!(list_level(3), 2);
    assert_eq!(list_level(4), 2);
    assert_eq!(list_level(5), 3);
    assert_eq!(list_level(6), 3);

    // 顶层保留源数字与后缀
    assert_eq!(ordered_list_marker("1.", 0), "1.");
    assert_eq!(ordered_list_marker("2)", 0), "2)");
    // 一层：小写字母；二层：小写罗马数字
    assert_eq!(ordered_list_marker("1.", 1), "a.");
    assert_eq!(ordered_list_marker("2)", 1), "b)");
    assert_eq!(ordered_list_marker("3.", 2), "iii.");
    // 三层循环回数字样式
    assert_eq!(ordered_list_marker("1.", 3), "1.");
    // 超过 26 的字母
    assert_eq!(ordered_list_marker("27.", 1), "aa.");
}

/// 无序列表的圆点在子层级必须换形状：实心圆 → 空心圆 → 小方块。
#[test]
fn unordered_bullet_shape_changes_with_level() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 300.0));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                for level in 0..3 {
                    draw_list_bullet(ui, level, theme.text, 10.0);
                    ui.add_space(16.0);
                }
            });
        },
    );
    out.textures_delta.clear();
    let mut filled = 0;
    let mut hollow = 0;
    let mut square = 0;
    for cs in &out.shapes {
        match &cs.shape {
            egui::Shape::Circle(c) if c.fill != Color32::TRANSPARENT => filled += 1,
            egui::Shape::Circle(c) if c.stroke.width > 0.0 => hollow += 1,
            egui::Shape::Rect(r) if r.fill == theme.text => square += 1,
            _ => {}
        }
    }
    assert_eq!(filled, 1, "顶层应是实心圆");
    assert_eq!(hollow, 1, "第二层应是空心圆");
    assert_eq!(square, 1, "第三层应是小方块");
}
