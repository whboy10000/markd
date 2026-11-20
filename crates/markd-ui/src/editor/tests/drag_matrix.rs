//! 「真拖一遍」验收矩阵：跨块拖拽必须在各块类型（含行内代码、加粗、链接、
//! 标题、引用、列表、任务项、代码块、表格、公式）上都成立，并且选中的文字
//! 必须与指针落点逐字相等——只选中一部分同样算失败。
//!
//! 与 `selection.rs` 的互补点：那里直接喂固定坐标；这里从渲染结果里取「某块
//! 第 k 个字符插入点」的真实屏幕坐标，逐帧模拟鼠标，因此对版式位移（换行、
//! 行高、标记露出、块间距）敏感，和用户实际操作一致。

use super::*;

/// 跑一帧，返回这一帧画出的全部文字形状（落点 + galley），按绘制顺序。
fn one_frame(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    events: Vec<egui::Event>,
) -> Vec<(egui::Pos2, std::sync::Arc<egui::Galley>)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let _ = show_editor(
                ui, doc, state, theme, &c, images, None, &[], false, false, false, false, 1,
                &mut actions,
            );
        });
    });
    out.textures_delta.clear();
    out.shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::Shape::Text(t) => Some((t.pos, t.galley.clone())),
            _ => None,
        })
        .collect()
}

/// 该块可见文本里最长的一段连续非空白词，用来在 galley 里认出这块。
fn needle(doc: &Document, idx: usize) -> Option<String> {
    let plain = doc.blocks[idx].plain();
    plain
        .split_whitespace()
        .map(|w| w.chars().take(12).collect::<String>())
        .filter(|w| !w.is_empty())
        .max_by_key(|w| w.chars().count())
}

/// galley 内第 ch 个插入点的屏幕坐标。
fn char_pos(origin: egui::Pos2, g: &egui::Galley, ch: usize) -> egui::Pos2 {
    let r = g.pos_from_cursor(egui::text::CCursor::new(ch));
    origin + r.center().to_vec2()
}

/// 跑一帧并返回这一帧产生的动作（勾选、激活、拖选上报等），用于验证
/// 「行带扩到圆点/复选框那片空白」不会抢走可点击控件的事件。
fn one_frame_actions(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    events: Vec<egui::Event>,
) -> Vec<crate::render::Action> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let _ = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let _ = show_editor(
                ui,
                doc,
                state,
                theme,
                &c,
                images,
                None,
                &[],
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
    });
    actions
}

/// 两点之间按 `step` 像素分步，保证 egui 判定为拖动。
fn path(a: egui::Pos2, b: egui::Pos2, step: f32) -> Vec<egui::Pos2> {
    let steps = ((b - a).length().ceil() / step).max(1.0) as usize;
    (1..=steps)
        .map(|k| {
            let t = k as f32 / steps as f32;
            egui::pos2(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
        })
        .collect()
}

/// 跑一帧，返回这一帧画出的全部「选区底色」矩形（按颜色筛出，描边宽度 0，
/// 正是 `selection_shapes` / 叠色那两种产出的形状）。
/// 文本块用不透明的 `theme.selection`；代码块、表格叠在内容之上的是同色 alpha=110。
fn one_frame_rects_color(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    events: Vec<egui::Event>,
    color: egui::Color32,
) -> Vec<egui::Rect> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| {
            let c = ui.ctx().clone();
            let mut actions: Vec<crate::render::Action> = Vec::new();
            let _ = show_editor(
                ui, doc, state, theme, &c, images, None, &[], false, false, false, false, 1,
                &mut actions,
            );
        });
    });
    // 选区底色是整包 `Shape::Vec` 回填到预留槽位的，要递归展开才看得到里面的矩形。
    fn walk(shape: &egui::Shape, color: egui::Color32, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(r) if r.fill == color && r.stroke.width == 0.0 => out.push(r.rect),
            egui::Shape::Vec(v) => {
                for s in v {
                    walk(s, color, out);
                }
            }
            _ => {}
        }
    }
    let mut rects = Vec::new();
    for cs in &out.shapes {
        walk(&cs.shape, color, &mut rects);
    }
    rects
}

/// 文本块那套不透明选区底色。
fn one_frame_rects(
    ctx: &egui::Context,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    events: Vec<egui::Event>,
) -> Vec<egui::Rect> {
    one_frame_rects_color(ctx, doc, state, theme, images, events, theme.selection)
}

struct Rig {
    ctx: egui::Context,
    theme: Theme,
    images: ImageCache,
}

impl Rig {
    fn new() -> Self {
        Rig {
            ctx: egui::Context::default(),
            theme: Theme::get(crate::theme::ThemeKind::Light),
            images: ImageCache::new(),
        }
    }

    fn frame(&mut self, doc: &mut Document, state: &mut EditorState, events: Vec<egui::Event>) {
        one_frame(&self.ctx, doc, state, &self.theme, &mut self.images, events);
    }

    /// 块的 (落点, galley)。渲染态与编辑态都取得到。
    fn galley(
        &mut self,
        doc: &mut Document,
        state: &mut EditorState,
        idx: usize,
    ) -> Option<(egui::Pos2, std::sync::Arc<egui::Galley>)> {
        let n = needle(doc, idx)?;
        let shapes = one_frame(&self.ctx, doc, state, &self.theme, &mut self.images, vec![]);
        shapes.into_iter().find(|(_, g)| g.text().contains(&n))
    }

    /// 块内第 ch 个字符（galley 下标，超界则钳到段末）的屏幕坐标，
    /// 连同它对应的「所见文本」下标——编辑态 galley 里含 0.1px 隐藏标记，
    /// 两者不同，必须换算后才能和选区坐标对齐。
    fn aim(
        &mut self,
        doc: &mut Document,
        state: &mut EditorState,
        idx: usize,
        ch: usize,
    ) -> Option<(egui::Pos2, usize)> {
        let (o, g) = self.galley(doc, state, idx)?;
        let ch = ch.min(g.text().chars().count());
        Some((char_pos(o, &g, ch), crate::render::plain_char_index(&g, ch)))
    }

    /// 依次经过若干航点拖动：按下 → 逐点分步移动 → 松开。
    fn drag(
        &mut self,
        doc: &mut Document,
        state: &mut EditorState,
        way: &[egui::Pos2],
    ) -> Option<(usize, usize, usize, usize)> {
        let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let first = way[0];
        self.frame(doc, state, vec![egui::Event::PointerMoved(first), btn(first, true)]);
        let mut last = first;
        for seg in way.windows(2) {
            for p in path(seg[0], seg[1], 6.0) {
                self.frame(doc, state, vec![egui::Event::PointerMoved(p)]);
                last = p;
            }
        }
        self.frame(doc, state, vec![btn(last, false)]);
        state.cross_sel
    }

    /// 跑一帧并返回这一帧产生的动作。
    fn actions(
        &mut self,
        doc: &mut Document,
        state: &mut EditorState,
        events: Vec<egui::Event>,
    ) -> Vec<crate::render::Action> {
        one_frame_actions(
            &self.ctx,
            doc,
            state,
            &self.theme,
            &mut self.images,
            events,
        )
    }

    /// 当前选区在屏幕上铺了哪些底色矩形。
    fn sel_rects(&mut self, doc: &mut Document, state: &mut EditorState) -> Vec<egui::Rect> {
        one_frame_rects(
            &self.ctx,
            doc,
            state,
            &self.theme,
            &mut self.images,
            vec![],
        )
    }

    /// 指定颜色的选区底色矩形（代码块/表格叠在内容之上的是半透明那种）。
    fn sel_rects_color(
        &mut self,
        doc: &mut Document,
        state: &mut EditorState,
        color: egui::Color32,
    ) -> Vec<egui::Rect> {
        one_frame_rects_color(
            &self.ctx,
            doc,
            state,
            &self.theme,
            &mut self.images,
            vec![],
            color,
        )
    }

    /// 先点一下目标块并让焦点落地，复现「用户先点进段落、再拖动」的前置状态。
    fn click_into(&mut self, doc: &mut Document, state: &mut EditorState, idx: usize) -> bool {
        let (o, g) = self.galley(doc, state, idx).expect("块可定位");
        let mid = char_pos(o, &g, g.text().chars().count() / 2);
        let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.frame(doc, state, vec![egui::Event::PointerMoved(mid), btn(mid, true)]);
        self.frame(doc, state, vec![btn(mid, false)]);
        for _ in 0..3 {
            self.frame(doc, state, vec![]);
        }
        self.ctx.memory(|m| m.focused().is_some())
    }
}

/// 归一化选区：起点在前。
fn norm(sel: (usize, usize, usize, usize)) -> (usize, usize, usize, usize) {
    let (a, ac, b, bc) = sel;
    if (a, ac) <= (b, bc) {
        (a, ac, b, bc)
    } else {
        (b, bc, a, ac)
    }
}

/// 按选区端点算出「本应选中」的文本，口径与 `cross_sel_text` 一致
/// （表格/公式这类非文本块作为端点时整块计入；代码块正文按字符可映射，精确截断）。
fn expected_text(doc: &Document, sel: (usize, usize, usize, usize)) -> String {
    let (s, sc, e, ec) = norm(sel);
    let mut out: Vec<String> = Vec::new();
    for i in s..=e {
        let Some(b) = doc.blocks.get(i) else { continue };
        let plain = b.plain();
        let len = plain.chars().count();
        let atomic = (i == s || i == e)
            && !crate::editor::selection::text_selection_block(b)
            && !crate::editor::selection::line_selectable_block(b);
        let a0 = if i == s && !atomic { sc.min(len) } else { 0 };
        let a1 = if i == e && !atomic { ec.min(len) } else { len };
        if a0 < a1 {
            out.push(plain.chars().skip(a0).take(a1 - a0).collect());
        }
    }
    out.join("\n\n")
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 判据：①选区存在；②覆盖块区间正确；③选中文字与指针落点逐字相等。
fn verify(
    doc: &Document,
    sel: Option<(usize, usize, usize, usize)>,
    want_blocks: (usize, usize),
    tag: &str,
) -> Result<(usize, usize), String> {
    let Some(sel) = sel else {
        return Err(format!("{tag}：没有建立任何选区（cross_sel = None）"));
    };
    let n = norm(sel);
    if (n.0, n.2) != want_blocks {
        return Err(format!(
            "{tag}：覆盖块区间 {}..={}，应为 {}..={}（选区 {n:?}）",
            n.0,
            n.2,
            want_blocks.0,
            want_blocks.1
        ));
    }
    let got = crate::editor::selection::cross_sel_text(doc, Some(sel)).unwrap_or_default();
    let want = expected_text(doc, sel);
    if squash(&got) != squash(&want) {
        return Err(format!(
            "{tag}：选中文字与落点不符（选区 {n:?}）。\n    应选「{}」\n    实选「{}」",
            squash(&want),
            squash(&got)
        ));
    }
    Ok((n.1, n.3))
}

/// 活动块（编辑态、含行内代码）：从段首拖到段中若干档位，终点必须逐字准确。
#[test]
fn active_paragraph_with_inline_code_drag_endpoints_are_exact() {
    let src = "先看 `inline_code` 这段，后面接足够长的中文让它必然换行到第二行去，\
               第二行的末尾才是我要选到的地方，继续再补一句更长一些的话凑够两行有余量，\
               再加第三行内容确保排版一定会折行，这样拖到最后一个字的时候才能验证终点落全。\n\n另一段。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut state = EditorState::default();
    let focused = rig.click_into(&mut doc, &mut state, 0);
    assert!(focused, "点进块后应取得键盘焦点（前置状态与真实应用一致）");
    let gchars = rig
        .galley(&mut doc, &mut state, 0)
        .map(|(_, g)| g.text().chars().count())
        .expect("块 0 可定位");
    let mut fails: Vec<String> = Vec::new();
    for frac in [0.25f32, 0.5, 0.75, 1.0] {
        let target = ((gchars as f32) * frac).round() as usize;
        let st = &mut state;
        let a = rig.aim(&mut doc, st, 0, target).expect("目标可定位");
        let s0 = rig.aim(&mut doc, st, 0, 0).expect("段首可定位");
        let sel = rig.drag(&mut doc, st, &[s0.0, a.0]);
        match verify(&doc, sel, (0, 0), &format!("活动块（focused）拖到 {frac:.2} 处，plain 终点 {}", a.1)) {
            Ok((_, ec)) => {
                if ec != a.1 {
                    fails.push(format!(
                        "拖到 {frac:.2}：终点字符 {ec} ≠ 落点 {}",
                        a.1
                    ));
                }
            }
            Err(e) => fails.push(e),
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

/// 渲染态（活动块是另一块）含行内代码段落：同样逐档校验终点。
#[test]
fn rendered_paragraph_with_inline_code_drag_endpoints_are_exact() {
    let src = "第一段是活动块，光标留在这里，长度也够长一些以便换行看看效果差别。\n\n\
               第二段含 `inline_code` 行内代码，后面接足够长的中文让它必然换行到第二行去，\
               还要继续加长，好让拖选跨越两行以上，最后一个字之后应能作为终点。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut state = EditorState::default();
    state.active = 0;
    let gchars = rig
        .galley(&mut doc, &mut state, 2)
        .map(|(_, g)| g.text().chars().count())
        .expect("块 2 可定位");
    let mut fails: Vec<String> = Vec::new();
    for frac in [0.3f32, 0.6, 1.0] {
        let target = ((gchars as f32) * frac).round() as usize;
        let a = rig.aim(&mut doc, &mut state, 2, target).expect("目标可定位");
        let s0 = rig.aim(&mut doc, &mut state, 0, 0).expect("起点可定位");
        let sel = rig.drag(&mut doc, &mut state, &[s0.0, a.0]);
        match verify(&doc, sel, (0, 2), &format!("渲染态块 2 拖到 {frac:.1} 处")) {
            Ok((_, ec)) if ec != a.1 => fails.push(format!(
                "拖到 {frac:.1}：终点字符 {ec} ≠ 落点 {}",
                a.1
            )),
            Ok(_) => {}
            Err(e) => fails.push(e),
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

/// 标题在上、段落在下：双向拖选，选中文字逐字相等。
#[test]
fn heading_then_paragraph_drag_is_exact_both_ways() {
    let src = "# 标题在上，标题行高比正文大，拖动经过它的上下留白时端点不能丢\n\n\
               标题下面的段落，足够长会换行，用来检查从标题拖进来时会不会只选中半截，\
               这里再补一句确保它跨到第二行末尾。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut fails: Vec<String> = Vec::new();
    for (tag, from, to) in [("标题→段落末", 0usize, 2usize), ("段落→标题首", 2, 0)] {
        let mut st = EditorState::default();
        let a = rig.aim(&mut doc, &mut st, from, 0).expect("起点");
        let n = rig
            .galley(&mut doc, &mut st, to)
            .map(|(_, g)| g.text().chars().count())
            .expect("终点块");
        let b = rig.aim(&mut doc, &mut st, to, n).expect("终点");
        let sel = rig.drag(&mut doc, &mut st, &[a.0, b.0]);
        if let Err(e) = verify(&doc, sel, (0, 2), tag) {
            fails.push(e);
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

/// 富文档相邻块两两拖选：每一对的选中文字都要逐字相等。
#[test]
fn adjacent_block_pairs_drag_is_exact() {
    let src = "# 标题甲\n\n普通段落乙的文字内容，这里要足够长以便换行看看第二行的效果如何。\n\n\
               含 `行内代码` 的段落丙，指针会从代码片的底色上经过。\n\n\
               **粗体** 与 [链接](https://e.com) 的段落丁。\n\n> 引用文字戊。\n\n\
               - 列表项己一\n- 列表项己二\n\n1. 有序项庚\n\n- [ ] 任务项辛\n\n\
               ```rust\nfn main() {}\n```\n\n| 表甲 | 表乙 |\n|:--|--:|\n| 一 | 二 |\n\n\
               $$\nx^2+y^2=z^2\n$$\n\n段落壬结尾，最后一个字之后应能作为终点。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let grabbable: Vec<usize> = (0..doc.blocks.len())
        .filter(|i| rig.galley(&mut doc, &mut EditorState::default(), *i).is_some())
        .collect();
    let mut fails: Vec<String> = Vec::new();
    for w in grabbable.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut st = EditorState::default();
        let pa = rig.aim(&mut doc, &mut st, a, 0).expect("起点");
        let n = rig
            .galley(&mut doc, &mut st, b)
            .map(|(_, g)| g.text().chars().count())
            .expect("终点块");
        let pb = rig.aim(&mut doc, &mut st, b, n).expect("终点");
        let sel = rig.drag(&mut doc, &mut st, &[pa.0, pb.0]);
        let want = (a.min(b), a.max(b));
        if let Err(e) = verify(&doc, sel, want, &format!("块 {a}「{}」→ 块 {b}「{}」", needle(&doc, a).unwrap_or_default(), needle(&doc, b).unwrap_or_default())) {
            fails.push(e);
        }
    }
    assert!(
        fails.is_empty(),
        "相邻块拖选未通过：\n{}",
        fails.join("\n")
    );
}

/// 跨过代码块 / 表格 / 公式这类原子块：整块必须进入选区。
#[test]
fn drag_across_atomic_blocks_includes_them_whole() {
    let src = "上面一段文字，作为拖选起点，够长会换行一下看看效果如何。\n\n\
               ```python\nprint('原子代码块')\n```\n\n| 甲列 | 乙列 |\n|:--|--:|\n| 丙 | 丁 |\n\n\
               $$\n\\int_0^1 x\\,dx\n$$\n\n下面一段文字，作为拖选终点，最后一个字之后要能选全。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let last = doc.blocks.len() - 1;
    let a = rig.aim(&mut doc, &mut st, 0, 0).expect("起点");
    let n = rig
        .galley(&mut doc, &mut st, last)
        .map(|(_, g)| g.text().chars().count())
        .expect("终点块");
    let b = rig.aim(&mut doc, &mut st, last, n).expect("终点");
    let sel = rig.drag(&mut doc, &mut st, &[a.0, b.0]);
    let copied = squash(
        &crate::editor::selection::cross_sel_text(&doc, sel).unwrap_or_default(),
    );
    for key in [
        "上面一段文字",
        "print",
        "原子代码块",
        "甲列",
        "乙列",
        "丙",
        "丁",
        "下面一段文字",
    ] {
        assert!(
            copied.contains(key),
            "跨过原子块的拖选漏了「{key}」，实选「{copied}」（选区 {sel:?}）"
        );
    }
    verify(&doc, sel, (0, last), "跨过代码块/表格/公式的拖选").unwrap_or_else(|e| panic!("{e}"));
}

/// 代码块里拖选：按下点落在代码正文里就按字符精确锚定，只选中拖过的那两行。
/// 之前代码块是「原子块」，一拖就把整块所有代码一起选中（用户反馈的 bug）。
#[test]
fn drag_inside_code_block_selects_lines_not_whole_block() {
    let src =
        "前言一段\n\n```python\nprint('第一行')\nx = 2\nprint('第三行')\nlast = 4\n```\n\n后记一段";
    let mut doc = Document::new(src, None);
    // 前言(0) / 空行(1) / 代码块(2) / 空行(3) / 后记(4)；active=0 → 代码块是渲染态。
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let code = doc.blocks[2].plain();
    // 第 k 行行首在代码正文里的字符下标。
    let at = |k: usize| -> usize {
        code.split('\n')
            .take(k)
            .map(|l| l.chars().count() + 1)
            .sum()
    };
    let a = rig
        .aim(&mut doc, &mut st, 2, at(1))
        .expect("第二行行首可定位");
    let b = rig
        .aim(&mut doc, &mut st, 2, at(3))
        .expect("第四行行首可定位");
    let sel = rig
        .drag(&mut doc, &mut st, &[a.0, b.0])
        .expect("代码块内拖选应建立选区");
    assert_eq!(
        norm(sel),
        (2, at(1), 2, at(3)),
        "选区应正好是从第二行行首拖到第四行行首"
    );
    let copied = crate::editor::selection::cross_sel_text(&doc, Some(norm(sel))).unwrap();
    assert_eq!(
        copied,
        code.chars()
            .skip(at(1))
            .take(at(3) - at(1))
            .collect::<String>(),
        "只该复制拖过的那两行，实选 {copied:?}"
    );
    assert!(
        copied.contains("x = 2") && copied.contains("print('第三行')"),
        "拖过的两行都该在选区里，实选 {copied:?}"
    );
    assert!(
        !copied.contains("last") && !copied.contains("第一行"),
        "没拖到的行不该被选进来，实选 {copied:?}"
    );

    // 高亮也只铺那两行：最后一行的文字上不该有选区色。
    let hi = rig.sel_rects_color(
        &mut doc,
        &mut st,
        crate::render::selection_overlay(&rig.theme),
    );
    let last_line = rig
        .galley(&mut doc, &mut st, 2)
        .map(|(o, g)| {
            let n = g.text().chars().count();
            egui::pos2(
                o.x + 6.0,
                o.y + g
                    .pos_from_cursor(egui::text::CCursor::new(n - 1))
                    .center()
                    .y,
            )
        })
        .expect("末行可定位");
    assert!(
        !hi.iter().any(|r| r.contains(last_line)),
        "没拖到的最后一行不该被高亮：底色 {hi:?}"
    );

    // 按退格删掉这段精确选区：只有选中的两行消失，其余代码原样保留。
    crate::editor::selection::apply_cross_sel_edit(&mut doc, &mut st, None);
    let after = doc.blocks[2].plain();
    assert_eq!(
        after.trim_end_matches('\n'),
        "print('第一行')\nlast = 4",
        "删除应严格只作用于选中的那两行，实际 {after:?}"
    );
}

/// 从活动代码块里往外拖：锚点必须落在按下那个字符。
/// 代码块的多行缓冲里换行符也计入所见文本，早先按「跳过换行」的下标换算，
/// 锚点会整体往上挪几行，表现为「从第三行拖出去却把第一行也带走」。
#[test]
fn drag_out_of_active_code_block_anchors_at_pressed_char() {
    let src = "```text\nline one aaa\nline two bbb\nline three ccc\n```\n\n后面独立的一段文字";
    let mut doc = Document::new(src, None);
    // 代码块(0) / 空行(1) / 段落(2)
    let mut rig = Rig::new();
    let mut st = EditorState {
        active: 2,
        ..Default::default()
    };
    assert!(rig.click_into(&mut doc, &mut st, 0), "点进代码块应拿到焦点");
    for _ in 0..2 {
        rig.frame(&mut doc, &mut st, vec![]);
    }
    assert!(matches!(
        doc.blocks.get(st.active),
        Some(Block::Code { .. })
    ));
    let code = doc.blocks[0].plain();
    let at = |k: usize| -> usize {
        code.split('\n')
            .take(k)
            .map(|l| l.chars().count() + 1)
            .sum()
    };
    let a = rig
        .aim(&mut doc, &mut st, 0, at(2))
        .expect("第三行行首可定位");
    let n = rig
        .galley(&mut doc, &mut st, 2)
        .map(|(_, g)| g.text().chars().count())
        .expect("段落可定位");
    let b = rig.aim(&mut doc, &mut st, 2, n).expect("段落末尾可定位");
    let sel = rig
        .drag(&mut doc, &mut st, &[a.0, b.0])
        .expect("从代码块拖到段落应建立跨块选区");
    let n = norm(sel);
    assert_eq!(
        (n.0, n.1),
        (0, at(2)),
        "锚点应落在按下的第三行行首（字符 {}），实际选区 {n:?}",
        at(2)
    );
    let copied = crate::editor::selection::cross_sel_text(&doc, Some(n)).unwrap();
    assert!(
        copied.starts_with("line three ccc"),
        "复制文本应从按下的那行开始，实选 {copied:?}"
    );
}

/// 跟着指针回到表格。否则端点停在「刚才那个已上报过精确字符的段落」，复制结果
/// 会带上用户没有选中的内容。
#[test]
fn drag_back_from_overshoot_clamps_endpoint_to_pointer_block() {
    let src = "起点段落零，先让指针从它下面经过。\n\n\
               中间段落一，指针从这里起拖。\n\n\
               | 表头甲 | 表头乙 |\n|:--|--:|\n| 单元一 | 单元二 |\n\n\
               尾部段落三，指针先拖过这里，再退回上面的表格。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let s0 = rig.aim(&mut doc, &mut st, 2, 0).expect("起拖点");
    let tail = rig
        .galley(&mut doc, &mut st, 6)
        .map(|(_, g)| g.text().chars().count())
        .expect("尾块可定位");
    let far = rig.aim(&mut doc, &mut st, 6, tail).expect("overshoot 落点");
    let tbl = rig.galley(&mut doc, &mut st, 4).expect("表格可定位");
    let back = egui::pos2(tbl.0.x + 10.0, tbl.0.y + tbl.1.rect.height() * 0.5);
    let sel = rig.drag(&mut doc, &mut st, &[s0.0, far.0, back]);
    let n = norm(sel.expect("退回表格后仍应有选区"));
    assert_eq!(
        (n.0, n.2),
        (2, 4),
        "端点应跟随指针回到表格块 4，实际选区 {n:?}"
    );
    let copied = crate::editor::selection::cross_sel_text(&doc, sel).unwrap_or_default();
    assert!(
        !squash(&copied).contains("尾部段落三"),
        "端点在表格，复制里不该有表格之后的段落，实选「{}」",
        squash(&copied)
    );
}

/// 嵌套列表整段拖选（用户截图场景）：三层缩进的每个条目、每一行文字都必须
/// 完整进入选区——条目之间断开、缩进越深越选不上，都算失败。
#[test]
fn nested_list_drag_selects_every_item_completely() {
    let src = "- 有哪些大模型应用开发框架呢\n  \
               - 截止到2025年7月26日，GitHub统计数据：\n    \
               - LangChain：这些工具里出现最早、最成熟的，适合复杂任务分解和单智能体应用\n    \
               - Llamalndex：专注于高效的索引和检索，适合 RAG 场景。\n  \
               - 但是主要的核心功能都是有的，有待进一步成熟\n\
               - SemanticKernel：也称为sk，微软推出的，对于C#同学来说，那就是5颗星";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let last = doc.blocks.len() - 1;
    let a = rig.aim(&mut doc, &mut st, 0, 0).expect("首条目可定位");
    let n = rig
        .galley(&mut doc, &mut st, last)
        .map(|(_, g)| g.text().chars().count())
        .expect("末条目可定位");
    let b = rig.aim(&mut doc, &mut st, last, n).expect("末条目末尾可定位");
    let sel = rig.drag(&mut doc, &mut st, &[a.0, b.0]);
    let copied = squash(
        &crate::editor::selection::cross_sel_text(&doc, sel).unwrap_or_default(),
    );
    let mut missing: Vec<String> = Vec::new();
    for (i, blk) in doc.blocks.iter().enumerate() {
        let want = squash(&blk.plain());
        if !want.is_empty() && !copied.contains(&want) {
            missing.push(format!("条目 {i}「{}」未完整选中", blk.plain()));
        }
    }
    assert!(
        missing.is_empty(),
        "嵌套列表拖选漏项（选区 {:?}）：\n{}\n实选「{}」",
        sel,
        missing.join("\n"),
        copied
    );
}

/// 指针落在「文字之外」的行带里也必须能拖选（用户截图场景）：嵌套列表的圆点、
/// 任务复选框、缩进那片空白都属于这一行——在那片空白里按下要能锚定本条目，
/// 缩进越深越不能失效。终点用行末字符，避免和「指针在行首」的语义混在一起。
#[test]
fn drag_started_in_bullet_and_indent_margin_anchors() {
    let src = "- 顶层条目甲，按下点在它的圆点空白里\n  \
               - 二级条目乙，指针经过这里\n    \
               - 三级条目丙，指针经过这里\n  \
               - [ ] 任务条目丁，复选框左边同样是本行\n- 尾条目戊，不该被选中";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let mut fails: Vec<String> = Vec::new();
    // 行带内的按下点：文字左缘往左 4/10/16px（圆点、复选框、缩进都在这段里）。
    for back in [4.0f32, 10.0, 16.0] {
        let g0 = rig.galley(&mut doc, &mut st, 0).expect("条目 0 可定位");
        let a = egui::pos2(g0.0.x - back, g0.0.y + g0.1.rect.height() * 0.5);
        let n3 = rig
            .galley(&mut doc, &mut st, 3)
            .map(|(_, g)| g.text().chars().count())
            .expect("条目 3 可定位");
        let b = rig.aim(&mut doc, &mut st, 3, n3).expect("条目 3 末尾可定位");
        let sel = rig.drag(&mut doc, &mut st, &[a, b.0]);
        match sel {
            None => fails.push(format!("左移 {back:.0}px 按下起拖：完全没有选区")),
            Some(s) => {
                let n = norm(s);
                if (n.0, n.2) != (0, 3) {
                    fails.push(format!(
                        "左移 {back:.0}px 按下起拖：覆盖块 {}..={}，应为 0..=3（选区 {n:?}）",
                        n.0, n.2
                    ));
                }
                let copied = squash(
                    &crate::editor::selection::cross_sel_text(&doc, Some(s))
                        .unwrap_or_default(),
                );
                for i in 0..=3 {
                    let want = squash(&doc.blocks[i].plain());
                    if !want.is_empty() && !copied.contains(&want) {
                        fails.push(format!(
                            "左移 {back:.0}px：条目 {i}「{}」没被完整选中",
                            doc.blocks[i].plain()
                        ));
                    }
                }
                if copied.contains(&squash(&doc.blocks[4].plain())) {
                    fails.push(format!("左移 {back:.0}px：终点之后的条目 4 也被选进来了"));
                }
            }
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
}

/// 拖进上一行的行带（圆点/缩进那片空白）时，端点必须落到该行的第一个字符，
/// 不能停在更靠后的块上——表现为「拖到某一行就是选不上」。
#[test]
fn drag_into_bullet_margin_extends_to_line_start() {
    let src = "- 顶层条目甲，从这里起拖\n  \
               - 二级条目乙，中间经过\n    \
               - 三级条目丙，终点落在它的圆点空白里\n- 尾条目丁";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let a = rig.aim(&mut doc, &mut st, 0, 0).expect("条目 0 起点");
    let g2 = rig.galley(&mut doc, &mut st, 2).expect("条目 2 可定位");
    let b = egui::pos2(g2.0.x - 10.0, g2.0.y + g2.1.rect.height() * 0.5);
    let sel = rig.drag(&mut doc, &mut st, &[a.0, b]);
    let n = norm(sel.expect("拖进圆点空白应建立选区"));
    assert_eq!(
        (n.0, n.2),
        (0, 2),
        "端点应跟随指针进入条目 2，实际选区 {n:?}"
    );
    assert_eq!(n.3, 0, "指针在条目 2 行首左侧，终点应落在该行第 0 个字符，实际 {}", n.3);
    let copied = squash(
        &crate::editor::selection::cross_sel_text(&doc, sel).unwrap_or_default(),
    );
    for i in 0..=1 {
        let want = squash(&doc.blocks[i].plain());
        assert!(
            copied.contains(&want),
            "条目 {i}「{}」应被完整选中，实选「{copied}」",
            doc.blocks[i].plain()
        );
    }
    assert!(
        !copied.contains(&squash(&doc.blocks[2].plain())),
        "终点在条目 2 行首，不该把条目 2 的文字也选进来"
    );
}

/// 高亮必须铺满整行带：圆点/缩进那片空白、行尾右侧空白都要有底色，
/// 否则用户看到的是断成一截一截，误以为「拖不动、没选中」。
#[test]
fn selection_highlight_paints_bullet_indent_and_line_tail() {
    // 截图那种混合形态：三层缩进、任务复选框、行内代码，逐条都要铺满行带。
    let src = "- 顶层条目甲，整行都要有底色\n  \
               - 二级条目乙，整行都要有底色\n    \
               - 三级条目丙，带 `inline_code` 也要整行有底色\n  \
               - [ ] 任务条目丁，复选框那片也要有底色\n- 尾条目戊，不该有底色";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let g0 = rig.galley(&mut doc, &mut st, 0).expect("条目 0 可定位");
    let a = egui::pos2(g0.0.x - 10.0, g0.0.y + g0.1.rect.height() * 0.5);
    let g3 = rig.galley(&mut doc, &mut st, 3).expect("条目 3 可定位");
    let n3 = g3.1.text().chars().count();
    let b = rig.aim(&mut doc, &mut st, 3, n3).expect("条目 3 末尾可定位");
    let sel = rig.drag(&mut doc, &mut st, &[a, b.0]);
    assert!(sel.is_some(), "行带内起拖应建立选区");
    let rects = rig.sel_rects(&mut doc, &mut st);
    let mut fails: Vec<String> = Vec::new();
    for i in 0..=3 {
        let (o, g) = rig.galley(&mut doc, &mut st, i).expect("条目可定位");
        let mid_y = o.y + g.rect.height() * 0.5;
        let Some(r) = rects.iter().find(|r| r.contains(egui::pos2(o.x + 2.0, mid_y))) else {
            fails.push(format!("条目 {i} 这一行没有任何选区底色矩形"));
            continue;
        };
        if r.left() > o.x - 5.0 {
            fails.push(format!(
                "条目 {i} 底色没铺到圆点/缩进区：底色左缘 {:.1}，文字左缘 {:.1}",
                r.left(),
                o.x
            ));
        }
        if r.right() < o.x + g.rect.width() {
            fails.push(format!(
                "条目 {i} 底色没铺到行尾：底色右缘 {:.1}，文字右缘 {:.1}",
                r.right(),
                o.x + g.rect.width()
            ));
        }
    }
    assert!(fails.is_empty(), "\n{}", fails.join("\n"));
    // 反面：终点之后的条目不能有底色，否则就是「铺过头」，选区边界不可信。
    let (o4, g4) = rig.galley(&mut doc, &mut st, 4).expect("尾条目可定位");
    let mid4 = o4.y + g4.rect.height() * 0.5;
    assert!(
        !rects
            .iter()
            .any(|r| r.contains(egui::pos2(o4.x + 2.0, mid4))),
        "终点之后的尾条目 4 不该有选区底色"
    );
}

/// 行带向左扩到「复选框那片空白」后，点复选框仍然只能切换勾选，不能被文字行
/// 抢走点击、顺带把条目切进编辑态（用户会看到「想勾一下，结果源码露出来了」）。
#[test]
fn click_task_checkbox_toggles_without_activating_edit() {
    let src = "- [ ] 任务条目甲，点在复选框上\n- [ ] 任务条目乙";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let (o, g) = rig.galley(&mut doc, &mut st, 0).expect("条目 0 可定位");
    // 复选框在文字左缘再往左约 24..5px 这段里，取其中 12px 处。
    let p = egui::pos2(o.x - 12.0, o.y + g.rect.height() * 0.5);
    let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let mut acts = rig.actions(
        &mut doc,
        &mut st,
        vec![egui::Event::PointerMoved(p), btn(p, true)],
    );
    acts.extend(rig.actions(&mut doc, &mut st, vec![btn(p, false)]));
    let toggled = acts.iter().any(|a| matches!(a, crate::render::Action::ToggleTask(0)));
    let activated = acts
        .iter()
        .any(|a| matches!(a, crate::render::Action::Caret(0, _)));
    assert!(toggled, "点复选框应产生 ToggleTask，实际动作 {acts:?}");
    assert!(
        !activated,
        "点复选框不该把条目 0 切进编辑态（产生了 Caret 动作），实际动作 {acts:?}"
    );
}

/// 渲染态（非活动块）的任务项，点复选框应一次就勾选、且不切进编辑态。
/// 与上面那个测试互补：上面 active=0 点的是编辑态复选框，这里让块 0 处于渲染态。
/// 行带的拖/点命中区若盖住复选框，会吞掉点击或抢走它产生 Caret，
/// 表现就是「要点两下才能勾选」——这条测试锁住这个回归。
#[test]
fn rendered_task_checkbox_toggles_in_one_click() {
    let src = "- [ ] 任务条目甲，点在复选框上\n- [ ] 任务条目乙";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    st.active = 1; // 块 0 处于渲染态
    let (o, g) = rig.galley(&mut doc, &mut st, 0).expect("条目 0 可定位");
    let p = egui::pos2(o.x - 12.0, o.y + g.rect.height() * 0.5);
    let btn = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let mut acts = rig.actions(
        &mut doc,
        &mut st,
        vec![egui::Event::PointerMoved(p), btn(p, true)],
    );
    acts.extend(rig.actions(&mut doc, &mut st, vec![btn(p, false)]));
    let toggled = acts
        .iter()
        .any(|a| matches!(a, crate::render::Action::ToggleTask(0)));
    let activated = acts
        .iter()
        .any(|a| matches!(a, crate::render::Action::Caret(0, _)));
    assert!(toggled, "渲染态点复选框应一次产生 ToggleTask，实际动作 {acts:?}");
    assert!(
        !activated,
        "点复选框不该把条目 0 切进编辑态（产生了 Caret 动作），实际动作 {acts:?}"
    );
}

/// 块间留白里按下再拖动：必须能锚定并延伸到落点。
#[test]
fn press_in_gap_then_drag_anchors() {
    let src = "# 标题在上\n\n下面的段落，从标题与段落之间的空白里按下再拖到本段末尾。\n\n第三段。";
    let mut doc = Document::new(src, None);
    let mut rig = Rig::new();
    let mut st = EditorState::default();
    let h = rig.galley(&mut doc, &mut st, 0).expect("标题可定位");
    let p = rig.aim(&mut doc, &mut st, 2, usize::MAX).expect("段落末可定位");
    let h_end = char_pos(h.0, &h.1, h.1.text().chars().count());
    let gap = egui::pos2(h.0.x + 10.0, (h_end.y + p.0.y) * 0.5);
    let sel = rig.drag(&mut doc, &mut st, &[gap, p.0]);
    verify(&doc, sel, (0, 2), "块间留白按下起拖").unwrap_or_else(|e| panic!("{e}"));
}
