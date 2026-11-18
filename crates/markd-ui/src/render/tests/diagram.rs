use super::*;

/// 时序图解析：participant 声明、四类箭头、Note、title。
/// 重点回归两个解析 bug："-->>" 的第一个 '-' 属于发送方（箭头匹配必须
/// 先长后短，否则发送方被切成 "D-"）；Note 的参与者名区分大小写。
#[test]
fn parse_sequence_basics() {
    let code = "sequenceDiagram\n  title 下单流程\n  participant U as 用户\n  participant S as 服务端\n  participant D as 数据库\n  U->>S: 提交订单\n  S->>D: 写入\n  D-->>S: 成功\n  S-->>U: 返回结果\n  Note over S,D: 事务保证\n  U->>U: 刷新页面\n  Note right of U: 备注";
    let (parts, items, title) = parse_sequence(code);
    assert_eq!(title, "下单流程");
    // 只有 U/S/D 三个参与者，不允许凭空多出 "D-"/"S-"/小写 "s"/"d"
    let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["U", "S", "D"], "参与者应恰为三个: {:?}", ids);
    assert_eq!(parts[0].label, "用户");
    let mut msgs = items
        .iter()
        .filter(|i| matches!(i, SeqItem::Message { .. }));
    let notes = items
        .iter()
        .filter(|i| matches!(i, SeqItem::Note { .. }))
        .count();
    assert_eq!(notes, 2, "两条 Note 都要解析出来");
    // 第一条：U->>S 实线
    if let Some(SeqItem::Message {
        from,
        to,
        label,
        dashed,
        cross,
    }) = msgs.next()
    {
        assert_eq!((*from, *to), (0, 1));
        assert_eq!(label, "提交订单");
        assert!(!dashed && !cross);
    } else {
        panic!("第一条应为消息");
    }
    // D-->>S：虚线，方向 D→S
    if let Some(SeqItem::Message {
        from, to, dashed, ..
    }) = items.iter().nth(2)
    {
        assert_eq!((*from, *to), (2, 1), "D-->>S 应为 D 指向 S");
        assert!(dashed, "-->> 应识别为虚线");
    }
    // Note over S,D 覆盖 1..2 区间
    if let Some(SeqItem::Note { over, label }) =
        items.iter().find(|i| matches!(i, SeqItem::Note { .. }))
    {
        assert_eq!(*over, (1, 2), "Note over S,D 应覆盖下标 1..2");
        assert_eq!(label, "事务保证");
    }
    // 自环消息 from == to
    assert!(
        items
            .iter()
            .any(|i| matches!(i, SeqItem::Message { from, to, .. } if from == to)),
        "U->>U 应解析出自环消息"
    );
}

/// 「流程图」按钮插入的模板：节点、菱形形状、连线标签、TD 竖排方向都要解析出来。
#[test]
fn parse_flowchart_template() {
    let code = "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]";
    let (nodes, edges, horizontal) = parse_flow(code);
    assert!(!horizontal, "TD 应为竖排方向");
    assert_eq!(nodes.len(), 3, "三个节点都要解析出来: {:?}", nodes);
    assert_eq!(nodes[0], ("A".into(), "开始".into(), 0_u8), "矩形节点");
    assert_eq!(nodes[1], ("B".into(), "判断".into(), 2), "决策节点应为菱形");
    assert_eq!(nodes[2], ("C".into(), "结束".into(), 0), "矩形节点");
    assert_eq!(edges.len(), 2, "两条连线: {:?}", edges);
    assert_eq!(edges[0], ("A".into(), "B".into(), String::new()));
    assert_eq!(edges[1], ("B".into(), "C".into(), "是".into()), "连线标签");
}

/// 分层布局的依据：入度为零的节点在第 0 层，后继节点逐层加深（最长路径）。
/// draw_flow 按这个层级决定行号（TD）/列号（LR），方向不再失效。
#[test]
fn layered_depth_follows_edges() {
    // A --> B --> C：竖排时应该三行，从上到下 A、B、C
    let code = "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]";
    let (nodes, edges, _) = parse_flow(code);
    let mut layer: HashMap<String, usize> =
        nodes.iter().map(|(id, _, _)| (id.clone(), 0)).collect();
    for _ in 0..nodes.len() {
        let mut changed = false;
        for (a, b, _) in &edges {
            let la = layer.get(a.as_str()).copied();
            if let Some(lb) = layer.get_mut(b.as_str()) {
                let la = la.unwrap_or(0);
                if la + 1 > *lb {
                    *lb = la + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    assert_eq!(layer["A"], 0);
    assert_eq!(layer["B"], 1);
    assert_eq!(layer["C"], 2, "TD 应自上而下分层");
}

/// 「图表」按钮插入的饼图模板：标题和数值都要解析出来，不含中文冒号坑。
#[test]
fn parse_pie_template() {
    let code = "pie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31\n  \"分类三\" : 27";
    let (title, data) = parse_pie(code);
    assert_eq!(title, "示例图表");
    assert_eq!(data.len(), 3, "三条数据: {:?}", data);
    assert_eq!(data[0], ("分类一".into(), 42.0));
    assert_eq!(data[2], ("分类三".into(), 27.0));
    // 中文标签后紧跟半角冒号（无空格）的写法也不能出错
    let (_, data) = parse_pie("pie\n\"A\":8\n\"B\":2");
    assert_eq!(data, vec![("A".into(), 8.0), ("B".into(), 2.0)]);
}

/// LR 方向（横向）解析 + 孤立 %% 注释行不产生节点。
#[test]
fn parse_flow_direction_and_comments() {
    let (nodes, _, horizontal) = parse_flow("graph LR\n  A[甲] --> B[乙]\n  %% 注释");
    assert!(horizontal, "LR 应为横排方向");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].1, "甲");
}

/// 流程图/饼图图形必须可点击：注入真实的按下+松开事件，clicked() 必须触发。
/// 这是「点击图形重新进入编辑态」的前提——此前是 Sense::hover，点击永远无效，
/// 导致插入流程图后无法再次编辑（回归测试）。
#[test]
fn diagram_graphic_is_clickable() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    for code in [
        "flowchart TD\n  A[开始] --> B{判断}\n  B -->|是| C[结束]",
        "pie title 示例图表\n  \"分类一\" : 42\n  \"分类二\" : 31",
    ] {
        let is_pie = code
            .lines()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.trim().starts_with("pie"));
        let draw = |ui: &mut egui::Ui| -> egui::Response {
            if is_pie {
                draw_pie(ui, code, &theme)
            } else {
                draw_flow(ui, code, &theme)
            }
        };
        let run = |events: Vec<egui::Event>, clicked: &mut bool, rect: &mut egui::Rect| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let r = draw(ui);
                        *clicked = r.clicked();
                        *rect = r.rect;
                    });
                },
            );
            out.textures_delta.clear();
        };
        // 第一帧：渲染图形并记录矩形
        let mut rect = egui::Rect::NOTHING;
        let mut clicked = false;
        run(vec![], &mut clicked, &mut rect);
        assert!(!rect.is_negative(), "图形应有实际尺寸: {code}");
        // 图形中心注入按下与松开。必须放进 RawInput 的 events：
        // `input_mut` 写入的是当前帧输入，下一次 begin_pass 会被新输入覆盖。
        let pos = rect.center();
        run(
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            &mut clicked,
            &mut rect,
        );
        run(
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
            &mut clicked,
            &mut rect,
        );
        assert!(
            clicked,
            "点击图形必须触发 clicked()（否则插入后无法再次编辑）: {code}"
        );
    }
}

/// 「时间线」按钮插入的模板：title、section、每个时间段的事件都要解析出来。
#[test]
fn parse_timeline_template() {
    let code = "timeline\n    title 产品迭代\n    section 2024\n        v0.1 : 原型上线\n        v0.2 : 支持 Markdown\n    section 2025\n        v1.0 : 双向链接\n        v1.2 : 插件系统";
    let (title, sections) = parse_timeline(code);
    assert_eq!(title, "产品迭代");
    let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["2024", "2025"]);
    assert_eq!(sections[0].periods.len(), 2);
    assert_eq!(sections[0].periods[0].label, "v0.1");
    assert_eq!(sections[0].periods[0].events, vec!["原型上线".to_string()]);
    assert_eq!(sections[1].periods[1].events, vec!["插件系统".to_string()]);
}

/// 官方语法细节：一条时间段可带多个事件、以 `:` 开头的续行继续追加、
/// 没有事件的纯时间段保留下来、`<br>` 变成换行。
#[test]
fn parse_timeline_continuation_and_breaks() {
    let code = "timeline\n    section 阶段\n        2020 : 事件一 : 事件二\n            : 事件三\n        2021\n        2022 : 上<br>下";
    let (_, sections) = parse_timeline(code);
    assert_eq!(sections.len(), 1, "没有空段: {:?}", sections.len());
    let periods = &sections[0].periods;
    assert_eq!(periods.len(), 3, "应解析出三个时间段");
    assert_eq!(
        periods[0].events,
        vec!["事件一".to_string(), "事件二".into(), "事件三".into()],
        "续行事件应追加到上一个时间段"
    );
    assert_eq!(periods[1].label, "2021");
    assert!(periods[1].events.is_empty(), "纯时间段不带事件");
    assert_eq!(
        periods[2].events,
        vec!["上\n下".to_string()],
        "<br> 应转换为换行"
    );
}

/// 时间线必须画出来且可点击：此前落到代码回退分支，插入后看起来「不显示」。
#[test]
fn timeline_renders_and_is_clickable() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 500.0));
    let code = "timeline\n  title 产品迭代\n  section 2024\n    v0.1 : 原型上线\n    v0.2 : 支持 Markdown\n  section 2025\n    v1.0 : 双向链接";
    let draw = |ui: &mut egui::Ui| draw_timeline(ui, code, &theme);
    let run = |events: Vec<egui::Event>, clicked: &mut bool, rect: &mut egui::Rect| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let r = draw(ui);
                    *clicked = r.clicked();
                    *rect = r.rect;
                });
            },
        );
        out.textures_delta.clear();
    };
    let mut rect = egui::Rect::NOTHING;
    let mut clicked = false;
    run(vec![], &mut clicked, &mut rect);
    assert!(
        rect.height() > 60.0,
        "时间线应有实际高度（事件 + 时间段），实际 {rect:?}"
    );
    let pos = rect.center();
    run(
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
        &mut clicked,
        &mut rect,
    );
    run(
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        }],
        &mut clicked,
        &mut rect,
    );
    assert!(clicked, "点击时间线图形必须触发 clicked()");
}

#[test]
fn state_diagram_is_compact() {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
    let code = "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Running : 启动\n    Running --> Paused : 暂停\n    Paused --> Running : 继续\n    Paused --> Idle : 停止\n    Running --> Error : 异常\n    Error --> Idle : 重置\n    Idle --> [*]";
    let flow = transform_state_to_flow(code);
    let mut h = 0.0;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |root| {
            egui::CentralPanel::default().show(root, |ui| {
                h = draw_flow(ui, &flow, &theme).rect.height();
            });
        },
    );
    out.textures_delta.clear();
    assert!(h < 600.0, "状态图高度不应爆炸，实际 {h}");
}
