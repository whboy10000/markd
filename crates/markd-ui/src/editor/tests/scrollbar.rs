//! 编辑区滚动条：0.5 cm 实条、与正文之间的分隔线、光标位置标记。
//!
//! 判据全部取自本帧真实画出来的形状（轨道矩形、两条线段），不去读 egui 内部状态：
//! 宽度写没写进去、分隔线有没有贴住正文右缘、标记跟不跟着光标走，都只看像素。

use super::*;

#[derive(Debug)]
struct Seg {
    a: egui::Pos2,
    b: egui::Pos2,
    width: f32,
    color: egui::Color32,
}

/// 本帧画出的线段：分隔线与光标标记都是 `line_segment`。
fn segments(shapes: &[egui::epaint::ClippedShape]) -> Vec<Seg> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::LineSegment { points, stroke } => Some(Seg {
                a: points[0],
                b: points[1],
                width: stroke.width,
                color: stroke.color,
            }),
            _ => None,
        })
        .collect()
}

/// 本帧画出的实心矩形：滚动条的轨道底色与滑块都是 `rect_filled`。
fn filled_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Rect> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.stroke.width == 0.0 => Some(r.rect),
            _ => None,
        })
        .collect()
}

/// 滚动条轨道：贴在编辑区右缘、宽 18.9 pt（0.5 cm）的那个矩形。
///
/// 宽度这里写死像素数而不是引用常量：常量改了但没落到绘制上，这条得红。
fn track(shapes: &[egui::epaint::ClippedShape]) -> Option<egui::Rect> {
    filled_rects(shapes)
        .into_iter()
        .find(|r| (r.width() - 18.9).abs() < 0.01 && r.right() > 780.0)
}

/// 这条带子里画出来的矩形（几何、底色、圆角）：贯穿全高的是轨道，矮一截的是滑块。
fn strip_paint(
    shapes: &[egui::epaint::ClippedShape],
) -> Vec<(egui::Rect, egui::Color32, egui::CornerRadius)> {
    shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r)
                if r.stroke.width == 0.0
                    && (r.rect.width() - 18.9).abs() < 0.05
                    && r.rect.right() > 780.0 =>
            {
                Some((r.rect, r.fill, r.corner_radius))
            }
            _ => None,
        })
        .collect()
}

/// 带子里「最后一层见得到」的东西。滑块是产品代码盖在 egui 那张上面的，
/// 所以同一个几何会有两层：下面那张几何与圆角都必须与上面那张一致，否则边上露底。
struct Strip {
    /// 轨道见得到的底色（滑块没盖住的那段）
    track: egui::Color32,
    /// 滑块见得到的底色
    thumb: egui::Color32,
    /// 真正见得到的那张滑块
    thumb_rect: egui::Rect,
    thumb_radius: egui::CornerRadius,
    /// 垫在它下面、egui 自己画的那张（文档不满一屏时只有一层，就没有这一项）
    under: Option<(egui::Rect, egui::CornerRadius)>,
}

/// 从本帧的绘制顺序里还原这条带子。
fn strip(shapes: &[egui::epaint::ClippedShape]) -> Strip {
    let layers = strip_paint(shapes);
    assert!(
        !layers.is_empty(),
        "这条带子里一个矩形都没画出来（轨道与滑块都不见）"
    );
    let h = |r: &(egui::Rect, egui::Color32, egui::CornerRadius)| r.0.height();
    let tallest = layers.iter().map(h).fold(f32::MIN, f32::max);
    let shortest = layers.iter().map(h).fold(f32::MAX, f32::min);
    let top = layers
        .iter()
        .rev()
        .find(|l| (h(l) - shortest).abs() < 0.01)
        .expect("按高度取滑块层");
    let under = if (tallest - shortest).abs() < 0.01 {
        None
    } else {
        layers
            .iter()
            .find(|l| (h(l) - shortest).abs() < 0.01)
            .map(|l| (l.0, l.2))
    };
    Strip {
        track: layers
            .iter()
            .rev()
            .find(|l| (h(l) - tallest).abs() < 0.01)
            .expect("按高度取轨道层")
            .1,
        thumb: top.1,
        thumb_rect: top.0,
        thumb_radius: top.2,
        under,
    }
}

/// 两色的最大通道差：低于这个量就当作「压在一起分不开」。
fn channel_delta(a: egui::Color32, b: egui::Color32) -> i16 {
    let (ar, ag, ab, _) = a.to_tuple();
    let (br, bg, bb, _) = b.to_tuple();
    let d = |x: u8, y: u8| (x as i16 - y as i16).abs();
    d(ar, br).max(d(ag, bg)).max(d(ab, bb))
}

/// 两张矩形是否严丝合缝（0.01 pt 内）。
fn same_rect(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min.x - b.min.x).abs() < 0.01
        && (a.min.y - b.min.y).abs() < 0.01
        && (a.max.x - b.max.x).abs() < 0.01
        && (a.max.y - b.max.y).abs() < 0.01
}

/// 分隔线：贴着轨道左缘的那条竖线。
fn divider(shapes: &[egui::epaint::ClippedShape], t: egui::Rect) -> Option<Seg> {
    segments(shapes)
        .into_iter()
        .find(|s| (s.a.x - s.b.x).abs() < 0.01 && (s.a.x - t.left()).abs() < 0.01)
}

/// 光标位置标记：横在轨道里的那条短横线。
fn marker(shapes: &[egui::epaint::ClippedShape], t: egui::Rect) -> Option<Seg> {
    segments(shapes).into_iter().find(|s| {
        (s.a.y - s.b.y).abs() < 0.01
            && s.a.x > t.left()
            && s.b.x < t.right()
            && (s.width - SCROLLBAR_MARKER).abs() < 0.01
    })
}

/// 驱动编辑器跑若干帧，留下最后一帧的全部形状。
///
/// `time` 必须逐帧前进：滚动条的显形是动画，时钟冻住时源码模式的条永远长不出来。
struct Bar {
    ctx: egui::Context,
    doc: Document,
    state: EditorState,
    theme: Theme,
    images: ImageCache,
    actions: Vec<crate::render::Action>,
    shapes: Vec<egui::epaint::ClippedShape>,
    frame_no: u64,
    source_mode: bool,
}

impl Bar {
    fn new(text: &str, active: usize) -> Self {
        Self::build(text, active, false)
    }

    fn source(text: &str, active: usize) -> Self {
        Self::build(text, active, true)
    }

    /// 一帧不跑：第一帧画成什么样也要能量到（滚动条显形动画就在头几帧）。
    fn cold(text: &str, active: usize, source_mode: bool) -> Self {
        Self::themed(text, active, source_mode, crate::theme::ThemeKind::Light)
    }

    /// 指定主题：底色跟着主题走，所以浅色/深色两边的配色都要能量到。
    fn themed(
        text: &str,
        active: usize,
        source_mode: bool,
        kind: crate::theme::ThemeKind,
    ) -> Self {
        let ctx = egui::Context::default();
        markd_platform::fonts::install(&ctx);
        Self {
            ctx,
            doc: Document::new(text, None),
            state: EditorState {
                active,
                ..Default::default()
            },
            theme: Theme::get(kind),
            images: ImageCache::new(),
            actions: Vec::new(),
            shapes: Vec::new(),
            frame_no: 0,
            source_mode,
        }
    }

    fn build(text: &str, active: usize, source_mode: bool) -> Self {
        let mut me = Self::cold(text, active, source_mode);
        me.idle(3);
        me
    }

    fn idle(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        let Self {
            ctx,
            doc,
            state,
            theme,
            images,
            actions,
            shapes,
            frame_no,
            source_mode,
        } = self;
        *frame_no += 1;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 800.0),
            )),
            time: Some(*frame_no as f64 / 60.0),
            events,
            ..Default::default()
        };
        let source = *source_mode;
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
                    source,
                    false,
                    false,
                    false,
                    1,
                    actions,
                );
            });
        });
        *shapes = out.shapes.clone();
        out.textures_delta.clear();
    }

    /// 按下 + 松开 + 两帧落地，与 `support::click_blank` 同一套节奏。
    fn click(&mut self, pos: egui::Pos2) {
        self.frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        self.frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        self.idle(2);
    }
}

fn list_doc(items: usize) -> String {
    (0..items).map(|i| format!("- 条目 {i}\n")).collect()
}

/// 宽度是 0.5 cm：仓库口径 1 cm ≈ 37.8 pt（与 `app` 里标题栏尺寸的注释同一换算）。
/// 定值写死，谁改口径就得连带改这条断言，不许悄悄滑回 egui 默认的细条。
#[test]
fn scrollbar_width_is_half_a_centimetre() {
    assert_eq!(SCROLLBAR_WIDTH, 0.5 * 37.8);
    assert_eq!(SCROLLBAR_WIDTH, 18.9);
    assert_eq!(SCROLLBAR_DIVIDER, 1.0);
    assert_eq!(SCROLLBAR_MARKER, 2.0);
    assert_eq!(SCROLLBAR_MARKER_HALO, 4.0);
}

/// 长文档：轨道真的按 0.5 cm 画出来，而且从第一帧起宽度不变。
///
/// 默认 `VisibleWhenNeeded` 会把滚动条的出现做成宽度动画，正文可用宽度逐帧变窄，
/// 排版表按宽度指纹整篇作废重测，落在那几帧的跳转请求就再也到不了位。
/// 常显（`AlwaysVisible`）消掉这个动画，所以这里逐帧量轨道宽度。
#[test]
fn track_is_wide_and_never_changes_width() {
    let mut b = Bar::cold(&list_doc(400), 0, false);
    let mut widths = Vec::new();
    for no in 1..=8 {
        b.frame(vec![]);
        let t = track(&b.shapes)
            .unwrap_or_else(|| panic!("第 {no} 帧就该画出 18.9 宽的轨道，实测 {widths:?}"));
        assert!(
            (t.width() - SCROLLBAR_WIDTH).abs() < 0.01,
            "第 {no} 帧轨道宽 {}，和 {} 对不上",
            t.width(),
            SCROLLBAR_WIDTH
        );
        widths.push(t.width());
    }
}

/// 短文档也保留这条带子：常显才有稳定的版心，不会因多打一行字整篇左右跳动。
#[test]
fn strip_survives_a_short_document() {
    let b = Bar::new("只有一行。\n", 0);
    assert!(track(&b.shapes).is_some(), "内容不满一屏时轨道也该画出来");
}

/// 分隔线：竖在轨道左缘、贯穿整条轨道，且正文一个字都不许压到轨道底下。
#[test]
fn divider_separates_text_from_track() {
    let b = Bar::new(&list_doc(400), 0);
    let t = track(&b.shapes).expect("长文档应画出轨道");
    let d = divider(&b.shapes, t).expect("轨道左缘应有分隔线");
    assert!(
        (d.width - SCROLLBAR_DIVIDER).abs() < 0.01,
        "分隔线宽 {}",
        d.width
    );
    assert!(
        (d.a.y - t.top()).abs() < 0.01 && (d.b.y - t.bottom()).abs() < 0.01,
        "分隔线该贯穿整条轨道，实际 {}..{}",
        d.a.y,
        d.b.y
    );
    // 正文右缘必须让开这条带子：浮动条是盖在文字上的，实条不是。
    for c in &b.shapes {
        if let egui::Shape::Text(text) = &c.shape {
            let right = text.pos.x + text.galley.size().x;
            assert!(
                right <= t.left() + 0.01,
                "文字 {:?} 压到了轨道上（右缘 {right} > 轨道左缘 {}）",
                text.galley.text(),
                t.left()
            );
        }
    }
}

/// 光标标记跟着活动块走：文档开头的在上半，结尾的在下半。
/// 映射写反（或活动块滚出视口时算错）就会翻过来，这条立刻红。
#[test]
fn marker_follows_the_caret_down_the_document() {
    let text = list_doc(400);
    let top = Bar::new(&text, 0);
    let t = track(&top.shapes).expect("长文档应画出轨道");
    let m_top = marker(&top.shapes, t).expect("轨道上应有光标标记");
    assert!(m_top.a.y - t.top() < 12.0, "首块的光标标记该在轨道顶端");

    let last = top.doc.blocks.len() - 1;
    let bottom = Bar::new(&text, last);
    let t2 = track(&bottom.shapes).expect("长文档应画出轨道");
    let m_bottom = marker(&bottom.shapes, t2).expect("轨道上应有光标标记");
    assert!(
        t2.bottom() - m_bottom.a.y < 12.0,
        "末块（{last}）的光标标记该在轨道底端，实际 y={}",
        m_bottom.a.y
    );
    assert!(m_top.a.y < m_bottom.a.y);
    // 标记要整条落在轨道里，不能画到正文上。
    assert!(m_bottom.a.x > t2.left() && m_bottom.b.x < t2.right());
}

/// 不满一屏时光标在哪儿就画在哪儿：内容不满一屏，内容坐标就等于屏幕坐标。
/// 旧算法拿「内容总高」当分母，两行的文档会把标记画到轨道中间去。
#[test]
fn marker_sits_at_the_caret_when_nothing_scrolls() {
    let b = Bar::new("第一行\n\n第二行\n", 0);
    let t = track(&b.shapes).expect("短文档也该保留轨道");
    let m = marker(&b.shapes, t).expect("轨道上应有光标标记");
    let line = b
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Text(text) if text.galley.text().contains("第一行") => {
                Some((text.pos.y, text.galley.size().y))
            }
            _ => None,
        })
        .expect("本帧应画出「第一行」");
    assert!(
        m.a.y > line.0 - 1.0 && m.a.y < line.0 + line.1 + 1.0,
        "标记 y={} 应落在「第一行」那一行（{}..{}）上",
        m.a.y,
        line.0,
        line.0 + line.1
    );
}

/// 源码模式共用同一套实条与标记：标记按当前行定位。
#[test]
fn source_mode_has_the_same_track_and_marker() {
    let mut b = Bar::source(&list_doc(400), 0);
    // 源码模式的条按需要显形，等动画长满再量。
    b.idle(20);
    let t = track(&b.shapes).expect("源码模式应画出 0.5 cm 轨道");
    let m = marker(&b.shapes, t).expect("源码模式轨道上应有光标标记");
    let s = strip(&b.shapes);
    assert_eq!(
        (s.track, s.thumb),
        (b.theme.panel, b.theme.dim),
        "源码模式共用同一套主题配色"
    );
    assert!(
        m.a.y - t.top() < 12.0,
        "光标在第一行，标记 y={}",
        m.a.y
    );
}

/// 这条带子是真控件：点它算滚动，不算「点空白把光标甩到文末」；
/// 轨道左边的页边距照样算空白。两侧一起断言，漏掉任一侧的回归都能红。
#[allow(deprecated)]
#[test]
fn clicking_the_strip_is_not_a_blank_click() {
    let text = list_doc(6);
    let mut margin = Bar::new(&text, 3);
    let n = margin.doc.blocks.len();
    assert!(
        (6..=7).contains(&n),
        "前提：六个列表项各占一块（末尾可能多一块空的）：{n}"
    );
    assert_eq!(margin.state.active, 3, "前提：起点不在文末");
    // 正文下方的页边距：空白点击 → 落到最后一块并拿到焦点。
    margin.click(egui::pos2(400.0, 700.0));
    assert_eq!(margin.state.active, n - 1, "点正文下方空白应落到最后一块");
    assert_eq!(
        margin.ctx.memory(|m| m.focused()),
        Some(egui::Id::unique(("block_edit", 1, n - 1))),
        "空白点击后最后一块的 TextEdit 必须持有焦点"
    );

    // 轨道正中：不能当成空白点击，否则滚动条就点不动了。
    let mut bar = Bar::new(&text, 3);
    let t = track(&bar.shapes).expect("轨道应常显");
    bar.click(t.center());
    assert_eq!(
        bar.state.active, 3,
        "点在滚动条上不该被当成点空白（滚动条就没法拖了）"
    );
}

/// 标记落在活动块的中心：块在视口里、被滚出视口，都按排版表算，一个口径。
/// 只取块顶、丢掉块高一半，标记会往上跑 5.5 pt（实测：期望 721.22，丢掉后 714.69）。
#[test]
fn marker_sits_at_the_active_block_centre() {
    for i in [5usize, 55] {
        let mut b = Bar::new(&list_doc(60), i);
        let t = track(&b.shapes).expect("长文档应画出轨道");
        let m = marker(&b.shapes, t).expect("轨道上应有光标标记");
        let caret = b.state.layout.top(i) + b.state.layout.height_of(i) * 0.5;
        let total = b.state.layout.total_height();
        // 分母取排版表总高；产品代码用的是 egui 的内容高，多出末尾那块间距，
        // 折算到轨道上实测差 0.99 pt，所以容差放到 2 pt。
        let expect = t.top() + (caret / total).clamp(0.0, 1.0) * t.height();
        assert!(
            (m.a.y - expect).abs() < 2.0,
            "活动块 {i} 的标记 y={}，排版表算出来应是 {expect:.2}",
            m.a.y
        );
    }
}

/// 短横线描两遍（光晕 + 核心）且两遍颜色不同：只画一种色，深轨道与浅滑块必有一侧看不见。
/// 轨道底色 #0A0A0A、滑块 #D7D7D7（egui 自己的实条配色），而光标几乎总落在滑块上。
#[test]
fn marker_is_outlined_so_it_contrasts_with_track_and_thumb() {
    let b = Bar::new(&list_doc(400), 55);
    let t = track(&b.shapes).expect("长文档应画出轨道");
    let core = marker(&b.shapes, t).expect("轨道上应有光标标记");
    let halo = segments(&b.shapes)
        .into_iter()
        .find(|s| {
            (s.a.y - s.b.y).abs() < 0.01
                && (s.a.x - t.left() - 3.0).abs() < 0.01
                && (s.width - 4.0).abs() < 0.01
                && (s.a.y - core.a.y).abs() < 0.01
        })
        .expect("标记该有一圈 4 pt 的光晕，少画一遍就可能在深轨道上消失");
    assert_eq!(core.color, b.theme.text, "核心用正文色");
    assert_eq!(halo.color, b.theme.bg, "光晕用纸色：深色轨道与浅色滑块都要有对比");
    assert_ne!(core.color, halo.color, "两遍同色等于只画一遍");
}

/// 按下这条带子拖动，文档真的要滚：与 `clicking_the_strip_is_not_a_blank_click`
/// 配成一对——那边管「别当成点空白」，这边管「要真的当滚动条」。
#[test]
fn dragging_the_strip_scrolls_the_document() {
    let mut b = Bar::new(&list_doc(400), 0);
    let t = track(&b.shapes).expect("轨道");
    let first_visible = |b: &Bar| -> Option<usize> {
        b.shapes.iter().filter_map(|c| match &c.shape {
            egui::Shape::Text(x) => x
                .galley
                .text()
                .split("条目 ")
                .nth(1)
                .and_then(|n| n.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()),
            _ => None,
        })
        .min()
    };
    let from = t.center();
    assert_eq!(first_visible(&b), Some(0), "前提：开头在视口里");
    b.frame(vec![
        egui::Event::PointerMoved(from),
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    for k in 1..=10 {
        b.frame(vec![egui::Event::PointerMoved(egui::pos2(
            from.x,
            from.y + k as f32 * 30.0,
        ))]);
    }
    b.frame(vec![egui::Event::PointerButton {
        pos: egui::pos2(from.x, from.y + 300.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    b.idle(2);
    let after = first_visible(&b).expect("拖动后仍有可见条目");
    assert!(after > 0, "沿轨道拖 300 pt 后视口还停在开头（首块 {after}）");
    assert_eq!(b.state.active, 0, "拖滚动条不该改活动块");
}



/// 轨道与滑块的底色跟着主题走：浅色主题不能再是 egui 那套近黑轨道（用户报的「太突兀」），
/// 深色主题是另一套。取的都是本帧真实画出来的填充色，不读 egui 内部状态。
#[test]
fn strip_paint_follows_the_theme() {
    let mut light = Bar::themed(&list_doc(400), 55, false, crate::theme::ThemeKind::Light);
    light.idle(3);
    let s = strip(&light.shapes);
    let (track_l, thumb_l) = (s.track, s.thumb);
    assert_eq!(track_l, light.theme.panel, "轨道应用面板色");
    assert_eq!(thumb_l, light.theme.dim, "滑块应用次要文字色");
    assert_ne!(
        track_l,
        egui::Color32::from_gray(10),
        "浅色主题不能留 egui 的近黑轨道（#0A0A0A）"
    );

    let mut dark = Bar::themed(&list_doc(400), 55, false, crate::theme::ThemeKind::Dark);
    dark.idle(3);
    let s = strip(&dark.shapes);
    let (track_d, thumb_d) = (s.track, s.thumb);
    assert_eq!(track_d, dark.theme.panel, "深色主题的轨道也应是主题色");
    assert_eq!(thumb_d, dark.theme.dim);

    assert!(
        channel_delta(track_l, track_d) > 120,
        "两套主题的轨道必须给出色差，实测浅色 {track_l:?} / 深色 {track_d:?}"
    );
    let (_, ag, _, _) = track_l.to_tuple();
    let (_, dg, _, _) = track_d.to_tuple();
    assert!(ag > 200 && dg < 60, "浅色轨道要亮、深色轨道要暗：{track_l:?} / {track_d:?}");
}

/// 滑块在悬停/按下时要换色：`widgets.hovered/active` 没设的话就停在灰底，看不出能拖。
#[test]
fn thumb_recolors_when_hovered() {
    let mut b = Bar::new(&list_doc(400), 55);
    let before = strip(&b.shapes);
    assert_eq!(before.thumb, b.theme.dim, "前提：静止滑块是 dim 色");
    let pos = before.thumb_rect.center();
    b.frame(vec![egui::Event::PointerMoved(pos)]);
    let now = strip(&b.shapes);
    assert_eq!(
        now.thumb,
        b.theme.text,
        "指针停在滑块上（{pos:?}）滑块应换成正文色"
    );
    assert_eq!(now.track, b.theme.panel, "悬停不该改轨道底色");
}

/// 短横线在实际画出来的两种底色上都要分得开：核心压轨道、核心压滑块、光晕压滑块。
/// 只断言颜色的话改错常量测不出来，所以三个比较全用本帧真实填充色。
#[test]
fn marker_stays_visible_over_track_and_thumb() {
    let cases = [
        crate::theme::ThemeKind::Light,
        crate::theme::ThemeKind::Dark,
    ];
    for kind in cases {
        let mut b = Bar::themed(&list_doc(400), 55, false, kind);
        b.idle(3);
        let s = strip(&b.shapes);
        let (track_fill, thumb_fill) = (s.track, s.thumb);
        let t = track(&b.shapes).expect("长文档应画出轨道");
        let core = marker(&b.shapes, t).expect("轨道上应有光标标记");
        let halo = segments(&b.shapes)
            .into_iter()
            .find(|s| (s.width - 4.0).abs() < 0.01 && (s.a.y - core.a.y).abs() < 0.01)
            .expect("标记应有 4 pt 光晕");
        assert!(
            channel_delta(core.color, track_fill) >= 40,
            "{kind:?}：核心 {:?} 与轨道 {:?} 分不开",
            core.color,
            track_fill
        );
        assert!(
            channel_delta(core.color, thumb_fill) >= 40,
            "{kind:?}：核心 {:?} 与滑块 {:?} 分不开",
            core.color,
            thumb_fill
        );
        assert!(
            channel_delta(halo.color, thumb_fill) >= 40,
            "{kind:?}：光晕 {:?} 与滑块 {:?} 分不开",
            halo.color,
            thumb_fill
        );
    }
}

/// 滑块是产品代码盖上去的，必须与 egui 自己那张逐像素重合（几何 + 圆角）：
/// 差一点，边上就露出一圈 egui 的浅灰，主题色滑块变成带毛边的方块。
/// 静止与滚动之后各量一次——滚动后两者的位置都由 offset 算出，才是真正会算错的情形。
#[test]
fn painted_thumb_matches_eguis_geometry() {
    let mut b = Bar::new(&list_doc(400), 55);
    let s = strip(&b.shapes);
    let (under_rect, under_radius) = s.under.expect("长文档的滑块下面应有 egui 自己画的那张");
    assert!(
        same_rect(s.thumb_rect, under_rect),
        "盖上去的滑块 {:?} 与 egui 的滑块 {:?} 没重合",
        s.thumb_rect,
        under_rect
    );
    assert_eq!(
        s.thumb_radius, under_radius,
        "圆角不同（{:?} vs {:?}）会在四角露出底色",
        s.thumb_radius,
        under_radius
    );

    // 滚一段再量：这时两条式子的输入都变了，几何仍然要严丝合缝。
    // 只能往下拖——静止时本就在顶端，往上滚不动。
    let t = track(&b.shapes).expect("轨道");
    let from = t.center();
    b.frame(vec![egui::Event::PointerMoved(from)]);
    b.frame(vec![egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    for k in 1..=6 {
        b.frame(vec![egui::Event::PointerMoved(egui::pos2(
            from.x,
            from.y + k as f32 * 40.0,
        ))]);
    }
    let moved = strip(&b.shapes);
    assert!(
        moved.thumb_rect.top() > s.thumb_rect.top() + 1.0,
        "前提：向下拖之后滑块该真的下移，实测 {} → {}",
        s.thumb_rect.top(),
        moved.thumb_rect.top()
    );
    let (scrolled_rect, scrolled_radius) = moved
        .under
        .expect("滚动之后滑块下面仍有 egui 那张（两层都还在）");
    assert!(
        same_rect(moved.thumb_rect, scrolled_rect),
        "滚动后盖的滑块 {th:#?} 与 egui 的 {un:#?} 错位 {d:.3} pt",
        th = moved.thumb_rect,
        un = scrolled_rect,
        d = (moved.thumb_rect.top() - scrolled_rect.top()).abs()
    );
    assert_eq!(moved.thumb_radius, scrolled_radius);
}

/// 滑块的颜色借道 `widgets.*.bg_fill` 时，任务列表的复选框内芯会跟着一起变（A/B 实测：
/// 改之前 `#3C3C3C`，改之后跟着主题跑）。滑块改成自绘之后这条必须守住：
/// 谁再把主题色写进那几个共享字段，复选框立刻红。
#[test]
fn checkbox_interior_keeps_eguis_own_color() {
    let default_fill = egui::Visuals::dark().widgets.inactive.bg_fill;
    let mut b = Bar::new("- [ ] 待办任务\n\n- [x] 已完成任务\n", 0);
    b.idle(3);
    let boxes: Vec<egui::Color32> = b
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if (r.rect.width() - 19.0).abs() < 0.01 && r.stroke.width == 1.0 => {
                Some(r.fill)
            }
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2, "本帧应画出两个复选框，实测 {boxes:?}");
    for fill in &boxes {
        assert_eq!(
            *fill, default_fill,
            "复选框内芯被滚动条的配色改写了（实测 {fill:?}，egui 默认 {default_fill:?}）"
        );
    }
    // 同一条带子里滑块确实是主题色，证明「没漏色」不是自绘没生效。
    let s = strip(&b.shapes);
    assert_eq!(s.thumb, b.theme.dim);
}

/// 超长文档里 egui 会把滑块夹到最短长度（`handle_min_length`）：自绘那张要是漏了这个
/// 下限，就会比 egui 那张短一截，上下各露出一段它的浅灰。
#[test]
fn thumb_respects_eguis_min_length_on_long_docs() {
    let min = egui::Style::default().spacing.scroll.handle_min_length;
    let mut b = Bar::new(&list_doc(2000), 55);
    b.idle(2);
    let s = strip(&b.shapes);
    assert!(
        (s.thumb_rect.height() - min).abs() < 0.01,
        "2000 条的文档里滑块该被夹到 egui 的下限 {min}，实测 {}",
        s.thumb_rect.height()
    );
    let (under_rect, under_radius) = s.under.expect("滑块下面应有 egui 自己画的那张");
    assert!(
        same_rect(s.thumb_rect, under_rect),
        "夹到下限后两层没对齐：盖的 {:?} vs egui {:?}",
        s.thumb_rect,
        under_rect
    );
    assert_eq!(s.thumb_radius, under_radius);
}

/// 分隔线要盖住带子与正文之间的缝隙：轨道矩形左缘就是分隔线的 x，两者上下端齐平。
/// 这条把「分隔线的 x」和「轨道矩形的 x」绑在一起——实条把轨道矩形往左推了两点，
/// 只按 `inner.right()` 画线就会让正文与线之间露出一道缝（实测 2 pt）。
#[test]
fn divider_is_flush_with_the_strip() {
    for mut b in [
        Bar::cold(&list_doc(400), 55, false),
        Bar::cold(&list_doc(400), 55, true),
    ] {
        b.idle(20);
        let t = track(&b.shapes).expect("长文档应画出轨道");
        let d = segments(&b.shapes)
            .into_iter()
            .find(|s| (s.width - SCROLLBAR_DIVIDER).abs() < 0.01)
            .expect("应有 1 pt 分隔线");
        assert!(
            (d.a.x - t.left()).abs() < 0.01 && (d.b.x - t.left()).abs() < 0.01,
            "分隔线 x={}..{} 与轨道左缘 {} 不齐",
            d.a.x,
            d.b.x,
            t.left()
        );
        assert!(
            (d.a.y - t.top()).abs() < 0.01 && (d.b.y - t.bottom()).abs() < 0.01,
            "分隔线 {}..{} 没贯穿轨道 {}..{}",
            d.a.y,
            d.b.y,
            t.top(),
            t.bottom()
        );
    }
}
