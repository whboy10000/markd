//! 回归：Mermaid 各模板都必须画成图形，而不是退回「逐行代码文本」。

use super::*;

/// 「时间线」模板必须走 timeline 渲染器：画布上出现标题、时间段名与事件文字，
/// 且不再出现首行 `timeline`（回退分支会把源码原样逐行打印）。
#[test]
fn timeline_template_is_rendered_not_raw_code() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let tpl = "```mermaid\ntimeline\n    title 产品迭代\n    section 2024\n        v0.1 : 原型上线\n        v0.2 : 支持 Markdown\n    section 2025\n        v1.0 : 双向链接\n        v1.2 : 插件系统\n```";
    let doc = crate::document::Document::new(tpl, None);
    assert_eq!(doc.blocks.len(), 1, "模板应解析为单块");
    let block = doc.blocks[0].clone();
    assert!(
        matches!(block, Block::Diagram { .. }),
        "时间线应解析为图表块，实际 {block:?}"
    );

    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let mut env = RenderEnv {
                    theme: &theme,
                    ctx: &ctx,
                    images: &mut images,
                    base_dir: None,
                    outline: &outline,
                    actions: &mut actions,
                    max_width: 700.0,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                let _ = render_block(ui, 0, &block, &mut env, false);
            });
        },
    );
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(ts) => Some(ts.galley.text().to_string()),
            _ => None,
        })
        .collect();
    out.textures_delta.clear();
    assert!(
        !texts.iter().any(|t| t.trim() == "timeline"),
        "不应退回逐行代码文本，实际文字：{texts:?}"
    );
    for need in ["产品迭代", "2024", "v0.1", "原型上线"] {
        assert!(
            texts.iter().any(|t| t.contains(need)),
            "图形里应出现 {need}，实际文字：{texts:?}"
        );
    }
}
