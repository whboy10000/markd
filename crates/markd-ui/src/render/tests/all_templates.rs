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
        "Git 图" => &["init", "feat: login", "v1.0"],
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
        let first = tpl
            .lines()
            .nth(1)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let (texts, height) = render_texts(&doc.blocks[0]);
        assert!(height > 40.0, "{label} 应画出有高度的图形，实际 {height}");
        if texts
            .iter()
            .any(|t| t.trim().to_lowercase() == first)
        {
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
