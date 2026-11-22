//! 回归：Mermaid 下拉里的每一个模板都必须画成图形，不能退回逐行代码文本。

use super::*;

/// 渲染一个块，返回 (画布上的文字, 图形整体高度)。
fn render_texts(block: &Block) -> (Vec<String>, f32) {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut height = 0.0;
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
                    max_width: 800.0,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                let resp = render_block(ui, 0, block, &mut env, false);
                height = resp.rect.height();
            });
        },
    );
    let texts = out
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(ts) => Some(ts.galley.text().to_string()),
            _ => None,
        })
        .collect();
    out.textures_delta.clear();
    (texts, height)
}

/// 每个模板里必须真的画出来的关键文字，防止「没回退但画了张空白图」。
fn expected_texts(label: &str) -> &'static [&'static str] {
    match label {
        "流程图" => &["开始", "判断", "终止"],
        "时序图" => &["用户", "点击登录", "返回 Token"],
        "类图" => &["Animal", "makeSound()", "Dog"],
        "状态图" => &["Idle", "Running", "启动"],
        "ER 图" => &["CUSTOMER", "created_at", "下单"],
        "甘特图" => &["项目排期", "需求分析", "预发布验证"],
        "饼图" => &["浏览器市场份额", "Chrome"],
        "旅程图" => &["用户购物旅程", "浏览首页", "支付成功"],
        "Git 图" => &["a3f82c1", "d4e8f3a", "b9d7e21", "feature"],
        "思维导图" => &["知识管理", "采集", "网页剪藏"],
        "时间线" => &["产品迭代", "2024", "原型上线"],
        "象限图" => &["功能优先级", "核心编辑", "优先做"],
        "XY 图表" => &["月度活跃用户", "1月", "1200"],
        "雷达图" => &["能力评估", "编程", "Alice"],
        "桑基图" => &["流量来源", "注册", "官网"],
        "矩形树图" => &["磁盘占用", "项目 A", "420"],
        "C4 上下文图" => &["系统上下文图", "Markdown 编辑器", "用户"],
        "块图" => &["客户端", "API 网关", "DB"],
        "需求图" => &["loginReq", "用户可以使用账号密码登录", "satisfies"],
        "数据包图" => &["TCP 头部", "Source Port", "Checksum"],
        "看板" => &["Todo", "编写 Mermaid 模板", "Done"],
        "泳道图" => &["订单处理泳道图", "提交订单", "扣款"],
        "Cynefin 图" => &["决策域", "Clear", "Confused"],
        "铁路图" => &["简单表达式", "identifier", "let"],
        _ => &[],
    }
}

/// 遍历 Mermaid 下拉的全部模板：首行关键字若被原样画出来，说明走了代码回退分支。
#[test]
fn all_mermaid_templates_render_graphics() {
    let mut missing: Vec<&str> = Vec::new();
    for (label, tpl) in crate::app::menu::MERMAID_TEMPLATES {
        let doc = crate::document::Document::new(tpl, None);
        assert_eq!(doc.blocks.len(), 1, "{label} 应解析为单块");
        let first = tpl.lines().nth(1).unwrap_or_default().trim().to_lowercase();
        let (texts, height) = render_texts(&doc.blocks[0]);
        assert!(height > 40.0, "{label} 应画出有高度的图形，实际 {height}");
        if texts.iter().any(|t| t.trim().to_lowercase() == first) {
            missing.push(label);
        }
        for need in expected_texts(label) {
            assert!(
                texts.iter().any(|t| t.contains(need)),
                "{label} 的图形里应出现 {need:?}，实际文字：{texts:?}"
            );
        }
    }
    assert!(
        missing.is_empty(),
        "以下图类型仍回退为代码文本，需要实现渲染：{missing:?}"
    );
}

/// Git 图矢量快照（人工核对连线几何用）：
/// `MARKD_GIT_SVG=/tmp/git.svg cargo test -p markd-ui git_svg_snapshot -- --nocapture`
/// 未设置环境变量时跳过，不影响常规测试。
#[test]
fn git_svg_snapshot() {
    let Some(path) = std::env::var_os("MARKD_GIT_SVG") else {
        return;
    };
    let (_, tpl) = crate::app::menu::MERMAID_TEMPLATES
        .iter()
        .find(|(l, _)| *l == "Git 图")
        .unwrap();
    let doc = crate::document::Document::new(tpl, None);
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let outline: Vec<OutlineItem> = Vec::new();
    let folded: HashSet<usize> = HashSet::new();
    let mut images = ImageCache::new();
    let mut actions: Vec<Action> = Vec::new();
    let mut h = 0.0f32;
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
                    max_width: 800.0,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                h = render_block(ui, 0, &doc.blocks[0], &mut env, false)
                    .rect
                    .height();
            });
        },
    );
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="820" height="{:.0}"><rect width="820" height="{:.0}" fill="#ffffff"/>"##,
        h + 20.0,
        h + 20.0
    );
    for cs in &out.shapes {
        match &cs.shape {
            egui::Shape::LineSegment { points, stroke } => {
                svg.push_str(&format!(
                    r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="rgb({},{},{})" stroke-opacity="{:.2}" stroke-width="{:.1}"/>"##,
                    points[0].x, points[0].y, points[1].x, points[1].y,
                    stroke.color.r(), stroke.color.g(), stroke.color.b(),
                    stroke.color.a() as f64 / 255.0, stroke.width
                ));
            }
            egui::Shape::Circle(c) => {
                let s = &c.stroke;
                svg.push_str(&format!(
                    r##"<circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="rgb({},{},{})" stroke="rgb({},{},{})" stroke-width="{:.1}"/>"##,
                    c.center.x, c.center.y, c.radius,
                    c.fill.r(), c.fill.g(), c.fill.b(),
                    s.color.r(), s.color.g(), s.color.b(), s.width
                ));
            }
            egui::Shape::Rect(r) => {
                let c = r.fill;
                if c.a() > 0 {
                    svg.push_str(&format!(
                        r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="5" fill="rgb({},{},{})"/>"##,
                        r.rect.left(), r.rect.top(), r.rect.width(), r.rect.height(),
                        c.r(), c.g(), c.b()
                    ));
                }
            }
            egui::Shape::Text(t) => {
                let deg = t.angle * 180.0 / std::f32::consts::PI;
                let color = t.override_text_color.unwrap_or(t.fallback_color);
                let text = t.galley.text().replace('&', "&amp;").replace('<', "&lt;");
                svg.push_str(&format!(
                    r##"<text x="{:.1}" y="{:.1}" font-size="13" fill="rgb({},{},{})" transform="rotate({deg:.1} {:.1} {:.1})">{}</text>"##,
                    t.pos.x, t.pos.y + 11.0, color.r(), color.g(), color.b(),
                    t.pos.x, t.pos.y, text
                ));
            }
            _ => {}
        }
    }
    svg.push_str("</svg>");
    std::fs::write(&path, svg).expect("写 SVG 失败");
    out.textures_delta.clear();
    println!("##V 已写出 {}", std::path::PathBuf::from(&path).display());
}
