//! 视口裁剪（虚拟化）的回归测试。
//!
//! 主绘制循环只排版视口内的块，于是三件事必须成立，且都不由排版表自己证明：
//! - 滚开的块不参与排版（帧成本与文档大小脱钩）；
//! - 点击命中「像素所在的那一块」，坐标不因裁剪整体偏移；
//! - 活动块被滚出视口后仍收得到键盘输入（不吞键）。
//!
//! 因此这里判据一律取自本帧真实的文字形状：先找到某个块画在了哪里，再点那里、
//! 再看落到哪一块上。`support::run_editor_frame` 不带滚动量也不返回形状，
//! 所以自带一个帧驱动。

use super::*;

struct View {
    doc: Document,
    state: EditorState,
    theme: Theme,
    images: ImageCache,
    actions: Vec<crate::render::Action>,
    ctx: egui::Context,
    line_numbers: bool,
    /// 帧序号：喂给 `RawInput::time`，让 egui 的滚轮动量按真实时间衰减。
    frame_no: u64,
}

/// 每条一个列表项：列表项各占一块，块下标与条目号一一对应，断言不用换算。
fn list_doc(items: usize, tail: impl Fn(usize) -> String) -> String {
    let mut s = String::new();
    for i in 0..items {
        s.push_str(&format!("- 条目 {i}：{}\n", tail(i)));
    }
    s
}

impl View {
    fn new(text: &str) -> Self {
        let ctx = egui::Context::default();
        markd_platform::fonts::install(&ctx);
        Self {
            doc: Document::new(text, None),
            state: EditorState::default(),
            theme: Theme::get(crate::theme::ThemeKind::Light),
            images: ImageCache::new(),
            actions: Vec::new(),
            ctx,
            line_numbers: false,
            frame_no: 0,
        }
    }

    /// 跑一帧，返回本帧所有文字及其屏幕矩形。
    /// `scroll` 是滚轮增量（点），正数把内容往下推、也就是往文档开头滚。
    fn frame(&mut self, events: Vec<egui::Event>, scroll: f32) -> Vec<(String, egui::Rect)> {
        self.frame_sized(events, scroll, 800.0, 800.0)
    }

    fn frame_sized(
        &mut self,
        mut events: Vec<egui::Event>,
        scroll: f32,
        width: f32,
        height: f32,
    ) -> Vec<(String, egui::Rect)> {
        if scroll != 0.0 {
            // 滚轮只作用于「指针所在的滚动区」，所以先把指针放到编辑区中间。
            events.insert(0, egui::Event::PointerMoved(egui::pos2(width * 0.5, height * 0.5)));
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, scroll),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Default::default(),
            });
        }
        let Self {
            doc,
            state,
            theme,
            images,
            actions,
            ctx,
            line_numbers,
            frame_no,
        } = self;
        *frame_no += 1;
        let show_line_numbers = *line_numbers;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            // 时间必须往前走，否则 egui 的滚轮动量/衰减停在同一时刻不生效。
            time: Some(*frame_no as f64 / 60.0),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| {
                let _ = show_editor(
                    ui,
                    doc,
                    state,
                    theme,
                    ctx,
                    images,
                    None,
                    &[],
                    false,
                    false,
                    false,
                    show_line_numbers,
                    1,
                    actions,
                );
            });
        });
        let texts = shape_texts(&out.shapes);
        // 无头环境没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
        out.textures_delta.clear();
        texts
    }

    /// 滚到文档结尾 / 开头：给一个足够大的滚轮量，再由 ScrollArea 夹到边界。
    fn scroll_to_end(&mut self) {
        for _ in 0..4 {
            self.frame(vec![], -1.0e7);
        }
    }

    fn scroll_to_top(&mut self) {
        for _ in 0..4 {
            self.frame(vec![], 1.0e7);
        }
    }

    /// 用 app 自己的跳转通道（大纲 / TOC 走的就是这条）把某块带到视口上缘。
    /// `scroll_to_rect` 在 egui 里是带缓动的，所以要跑到它真的停下来为止；
    /// 返回停稳之后的文字矩形，供「点哪里就是哪一块」的断言使用。
    fn jump(&mut self, idx: usize) -> Vec<(String, egui::Rect)> {
        let needle = format!("条目 {idx}");
        self.jump_to(idx, &needle)
    }

    fn jump_to(&mut self, idx: usize, needle: &str) -> Vec<(String, egui::Rect)> {
        self.state.active = idx;
        self.state.focus_req = Some(idx);
        self.state.scroll_req = Some(idx);
        self.state.scroll_top = true;
        let mut texts = Vec::new();
        for _ in 0..120 {
            texts = self.frame(vec![], 0.0);
            if rect_of(&texts, needle).is_some() {
                break;
            }
        }
        let mut prev = rect_of(&texts, needle);
        if prev.is_none() {
            panic!("跳转到块 {idx} 后始终看不到 {needle}");
        }
        // 刚「看见」目标行时动画通常还没停：实测第一帧它在 y=288，停稳在 y=56，
        // 差九行。这时按第一帧的矩形去点，命中的是别的块。跑到矩形不再移动为止。
        for _ in 0..120 {
            texts = self.frame(vec![], 0.0);
            let cur = rect_of(&texts, needle);
            let settled = match (prev, cur) {
                (Some(a), Some(b)) => (a.center() - b.center()).length() < 0.5,
                _ => false,
            };
            prev = cur;
            if settled {
                return texts;
            }
        }
        panic!("跳转到块 {idx} 后 {needle} 的矩形始终停不下来");
    }

    /// 在给定位置按下、松开，再给「请求焦点 + 落光标」两帧时间。
    fn click(&mut self, pos: egui::Pos2) {
        self.frame(
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            0.0,
        );
        self.frame(
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            0.0,
        );
        for _ in 0..2 {
            self.frame(vec![], 0.0);
        }
    }
}

fn shape_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for s in shapes {
        if let egui::Shape::Text(t) = &s.shape {
            out.push((
                t.galley.text().to_string(),
                egui::Rect::from_min_size(t.pos, t.galley.size()),
            ));
        }
    }
    out
}

fn rect_of(texts: &[(String, egui::Rect)], needle: &str) -> Option<egui::Rect> {
    texts
        .iter()
        .find(|(t, _)| t.contains(needle))
        .map(|(_, r)| *r)
}

/// 视口外的块完全不排版；但内容总高仍覆盖整篇，滚到底能看见最后一块。
#[test]
fn culled_blocks_are_never_measured() {
    let n = 1200;
    let mut v = View::new(&list_doc(n, |_| "正文".to_string()));
    for _ in 0..3 {
        v.frame(vec![], 0.0);
    }
    let blocks = v.doc.blocks.len();
    assert!(
        blocks >= n && blocks <= n + 2,
        "每个列表项各占一块，末尾最多多一个空块：{blocks} vs {n}"
    );
    let measured = (0..blocks).filter(|&i| v.state.layout.raw_height(i) > 0.0).count();
    assert!(
        measured > 0 && measured < blocks / 4,
        "只该排版视口附近那几十块，实测 {measured}/{blocks}"
    );
    assert!(v.state.layout.raw_height(blocks - 1) == 0.0, "末尾块不该被排版");

    // 滚动范围必须按整篇给：一路向下滚要能真的摸到最后一项。
    v.scroll_to_end();
    let texts = v.frame(vec![], 0.0);
    assert!(
        rect_of(&texts, "条目 1199").is_some(),
        "滚到底应看到最后一块"
    );
    assert!(rect_of(&texts, "条目 0").is_none(), "滚到底后首块应已离开视口");
    // 滚到底之后末尾块才被排版，且不需要动前面那些块的高。
    assert!(
        v.state.layout.raw_height(blocks - 1) > 0.0,
        "滚到底后末尾块应完成实测"
    );
    let end_measured = (0..blocks).filter(|&i| v.state.layout.raw_height(i) > 0.0).count();
    assert!(
        end_measured < blocks / 2,
        "滚到底也只该排版尾部附近，实测 {end_measured}/{blocks}"
    );

    // 回到开头，开头块的高要还是当初实测的那个数（缓存没被整篇作废）。
    let first_h = v.state.layout.raw_height(0);
    v.scroll_to_top();
    v.frame(vec![], 0.0);
    assert_eq!(v.state.layout.raw_height(0), first_h, "滚回开头不该重测首块");
    let texts = v.frame(vec![], 0.0);
    assert!(rect_of(&texts, "条目 0").is_some(), "滚回开头应看到首块");
}

/// 跳到远处后，点击命中「眼睛看到的那一块」——判据取自真实文字矩形，
/// 与排版表无关：坐标若因裁剪整体偏移，点中的就是别的块。
#[test]
fn click_lands_on_the_block_that_is_actually_painted() {
    let mut v = View::new(&list_doc(1200, |_| "正文".to_string()));
    for _ in 0..3 {
        v.frame(vec![], 0.0);
    }
    let texts = v.jump(1000);
    // 故意点非活动块：点活动块「还是自己」证明不了坐标对齐。
    let target = rect_of(&texts, "条目 1002").expect("跳转后应看到 1002 项");
    v.click(target.center());
    assert_eq!(
        v.state.active, 1002,
        "点击应激活指针下的块，实际激活 {}",
        v.state.active
    );

    // 往上几项也一样，排除「只有跳转落点那一块碰巧对齐」。
    let texts = v.frame(vec![], 0.0);
    let upper = rect_of(&texts, "条目 1001").expect("1001 项应在视口内");
    v.click(upper.center());
    assert_eq!(v.state.active, 1001);
}

/// 活动块被滚出视口后仍必须留在控件树上：否则焦点挂在一个本帧没渲染的文本框上，
/// 键盘事件无人接收，用户滚开之后打的字会整段丢掉。
#[test]
fn typing_while_the_active_block_is_offscreen_is_not_swallowed() {
    let mut v = View::new(&list_doc(1200, |_| "正文".to_string()));
    for _ in 0..3 {
        v.frame(vec![], 0.0);
    }
    v.jump(900);
    let before = v.doc.blocks[900].source().to_string();
    assert!(before.contains("条目 900"), "{before}");

    // 一路向上滚回文档开头，把 900 项推出视口。
    v.scroll_to_top();
    let texts = v.frame(vec![], 0.0);
    assert!(
        rect_of(&texts, "条目 900").is_none(),
        "前提不成立：900 项仍在视口内"
    );
    assert!(
        v.state.layout.raw_height(900) > 0.0,
        "活动块即使离屏也要被排版，否则它的文本框收不到按键"
    );

    v.frame(vec![egui::Event::Text("Z".into())], 0.0);
    let after = v.doc.blocks[900].source().to_string();
    assert!(
        after.contains('Z') && after != before,
        "离屏输入被吞掉了：{after}"
    );
    assert!(after.contains("条目 900"), "只该改 900 这一块：{after}");
    for (i, b) in v.doc.blocks.iter().enumerate() {
        if i != 900 {
            assert!(!b.source().contains('Z'), "字符写错了块：{i}");
        }
    }
}

/// 窗口宽度变了块高必然变：排版表按指纹整篇作废，窄窗口下长行换行后高度重测。
#[test]
fn resizing_the_window_rebuilds_the_height_table() {
    let long = "这一行正文故意写得比较长，窗口收窄之后必须换行，用来验证高度表整篇作废重测。";
    let mut v = View::new(&list_doc(400, |_| long.to_string()));
    v.frame_sized(vec![], 0.0, 900.0, 800.0);
    v.frame_sized(vec![], 0.0, 900.0, 800.0);
    let wide_h = v.state.layout.raw_height(0);
    let wide_total = v.state.layout.total_height();
    assert!(wide_h > 0.0, "宽窗口下应已实测块高");

    v.frame_sized(vec![], 0.0, 320.0, 800.0);
    v.frame_sized(vec![], 0.0, 320.0, 800.0);
    let narrow_h = v.state.layout.raw_height(0);
    assert!(
        narrow_h > wide_h,
        "同样的段落在 320px 宽下应比 900px 高，实际 {wide_h} → {narrow_h}"
    );
    assert!(
        v.state.layout.total_height() > wide_total,
        "换行后整篇更高，滚动条不能还是原来的长度"
    );
}

fn line_labels(texts: &[(String, egui::Rect)]) -> Vec<String> {
    texts
        .iter()
        .filter(|(t, _)| t.trim().parse::<u32>().is_ok())
        .map(|(t, _)| t.trim().to_string())
        .collect()
}

/// 行号表在裁剪后仍要给出「该块在源码里的第一行」：以前每帧整篇重数，
/// 现在靠增量行数表，滚动到远处后必须和真实源码行数一致。
#[test]
fn line_numbers_match_the_source_after_culling() {
    // 每 7 项插一条续行：块行数不再是「每块一行」，前缀和算错就会立刻露馅。
    let mut text = String::new();
    for i in 0..300 {
        text.push_str(&format!("- 条目 {i}：正文\n"));
        if i % 7 == 6 {
            text.push_str("  续行\n");
        }
    }
    let mut v = View::new(&text);
    v.line_numbers = true;
    for _ in 0..3 {
        v.frame(vec![], 0.0);
    }
    assert!(
        v.state.layout.lines_missing() == 0,
        "开启行号后行数应已补齐"
    );

    for item in [8usize, 150, 299] {
        // 续行是否并入同块由解析器决定，块下标不能靠算的——直接找。
        let needle = format!("条目 {item}");
        let idx = v
            .doc
            .blocks
            .iter()
            .position(|b| b.source().contains(&needle))
            .unwrap_or_else(|| panic!("文档里没有 {needle}"));
        let texts = v.jump_to(idx, &needle);
        let expect = v.doc.blocks[..idx]
            .iter()
            .map(|b| b.source().split('\n').count())
            .sum::<usize>()
            + 1;
        assert!(
            texts
                .iter()
                .any(|(t, _)| t.trim() == expect.to_string()),
            "块 {idx}（{needle}）的行号应是 {expect}，本帧行号：{:?}",
            line_labels(&texts)
        );
    }
}

