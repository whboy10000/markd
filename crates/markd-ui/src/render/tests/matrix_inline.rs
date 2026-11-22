use super::*;

/// 形状的包围盒（递归摊平 `Shape::Vec`）。
fn shape_bounds(s: &egui::Shape) -> Option<egui::Rect> {
    match s {
        egui::Shape::Mesh(m) => Some(m.calc_bounds()),
        egui::Shape::Rect(r) => Some(r.rect),
        egui::Shape::Vec(v) => {
            let mut it = v.iter().filter_map(shape_bounds);
            let first = it.next()?;
            Some(it.fold(first, |a, r| a.union(r)))
        }
        _ => None,
    }
}

fn flatten<'a>(s: &'a egui::Shape, acc: &mut Vec<&'a egui::Shape>) {
    match s {
        egui::Shape::Vec(v) => {
            for x in v {
                flatten(x, acc);
            }
        }
        other => acc.push(other),
    }
}

/// 渲染若干块，返回这一帧真正画出的全部形状（已摊平 `Shape::Vec`）。
fn render_blocks(blocks: &[Block], max_width: f32) -> Vec<egui::Shape> {
    let theme = Theme::get(ThemeKind::Light);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0));
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
                let c = ui.ctx().clone();
                let mut env = RenderEnv {
                    theme: &theme,
                    ctx: &c,
                    images: &mut images,
                    base_dir: None,
                    outline: &outline,
                    actions: &mut actions,
                    max_width,
                    cross_sel: None,
                    folded: &folded,
                    group_tab: 0,
                };
                for (i, b) in blocks.iter().enumerate() {
                    let _ = render_block(ui, i, b, &mut env, false);
                }
            });
        },
    );
    out.textures_delta.clear();
    let refs: Vec<&egui::Shape> = out.shapes.iter().map(|s| &s.shape).collect();
    let mut flat: Vec<&egui::Shape> = Vec::new();
    for s in &refs {
        flatten(s, &mut flat);
    }
    flat.into_iter().cloned().collect()
}

fn paragraph(src: &str) -> Block {
    Block::Paragraph {
        content: crate::ast::parse_inline(src),
        source: src.to_string().into(),
    }
}

/// 行内行列式与一段文字混排时，既不能压到上下行，也不能压到同一行的文字上。
///
/// 针对两个真实缺陷：
/// 1. 占位只用一个字符加 `extra_letter_spacing`，而 epaint 0.37 只在「同段非首
///    字」之前加这个间距——单字符占位段等于一毫米宽度都没撑开，后面的正文直接从
///    公式底下压过来（实测同行「的行」三字落在公式 x 区间内）。
/// 2. 占位段的行高不随公式增高，公式底部溢出行底约 7px，压住下一行。
#[test]
fn inline_matrix_does_not_overlap_surrounding_text() {
    let src = "设矩阵 $\\begin{vmatrix}a&b&c\\\\d&e&f\\\\g&h&i\\end{vmatrix}$ 的行列式记作 D，\
               后面再跟一段足够长的中文文字，让这一行必然发生换行，这样就能看到公式会不会\
               压到上一行或者下一行的文字上面去。";
    let shapes = render_blocks(&[paragraph(src)], 520.0);

    let mut galley: Option<&egui::Galley> = None;
    let mut origin = Pos2::ZERO;
    for s in &shapes {
        if let egui::Shape::Text(t) = s
            && t.galley.text().contains('\u{E000}')
        {
            galley = Some(&t.galley);
            origin = t.pos;
            break;
        }
    }
    let galley = galley.expect("应找到含公式占位字符的 galley");
    assert!(
        galley.rows.len() >= 2,
        "这段文字应换行，否则测不到上下行：{} 行",
        galley.rows.len()
    );

    let math = shapes
        .iter()
        .filter(|s| matches!(s, egui::Shape::Mesh(_)))
        .filter_map(shape_bounds)
        .reduce(|a, r| a.union(r))
        .expect("应画出公式形状");

    let marker_row = galley
        .rows
        .iter()
        .position(|r| r.text().contains('\u{E000}'))
        .expect("占位字符应在某一行");
    let row = &galley.rows[marker_row];
    let row_top = origin.y + row.pos.y;
    let row_bot = row_top + row.row.size.y;

    // ① 竖向：公式必须完整待在属于它的那一行带里。
    assert!(
        math.top() >= row_top - 0.5 && math.bottom() <= row_bot + 0.5,
        "公式越出所在行：行带 [{row_top:.1},{row_bot:.1}]，公式 [{:.1},{:.1}]",
        math.top(),
        math.bottom()
    );
    for (i, r) in galley.rows.iter().enumerate() {
        if i == marker_row {
            continue;
        }
        let t = origin.y + r.pos.y;
        let b = t + r.row.size.y;
        assert!(
            math.bottom() <= t + 0.5 || math.top() >= b - 0.5,
            "公式侵占第 {i} 行的文字带 [{t:.1},{b:.1}]：公式 [{:.1},{:.1}]",
            math.top(),
            math.bottom()
        );
    }

    // ② 横向：同一行里，占位之外的任何字形都不能落进公式的水平区间。
    for g in &row.glyphs {
        if g.chr == '\u{E000}' {
            continue;
        }
        let x0 = origin.x + row.pos.x + g.pos.x;
        let x1 = x0 + g.advance_width;
        assert!(
            x1 <= math.left() + 0.5 || x0 >= math.right() - 0.5,
            "公式盖住了同行的字 {:?}：字 x=[{x0:.1},{x1:.1}]，公式 x=[{:.1},{:.1}]",
            g.chr,
            math.left(),
            math.right()
        );
    }

    // ③ 可读性：旧实现把三阶行列式压到 1.45em（23.2px），这里要求保持可读尺寸。
    assert!(
        math.height() >= 16.0 * 2.0,
        "行列式被压得太小：{:.1}",
        math.height()
    );
}

/// 常见行列式 / 矩阵 / 分段函数都要撑得住：任何一条都不能越出所在行带。
#[test]
fn every_tall_inline_formula_stays_inside_its_line() {
    for tex in [
        "\\begin{vmatrix}1&2\\\\3&4\\end{vmatrix}",
        "\\begin{pmatrix}1&2\\\\3&4\\end{pmatrix}",
        "\\begin{bmatrix}a&b&c\\\\d&e&f\\\\g&h&i\\end{bmatrix}",
        "\\begin{cases}x=1\\\\y=2\\end{cases}",
        "\\frac{\\frac{a}{b}}{\\frac{c}{d}}",
        "\\int_{0}^{\\infty} e^{-x^2}\\,dx",
    ] {
        let src = format!("前面文字 ${tex}$ 后面文字继续加长一定会换行到这里来再往下走一行看看。");
        let shapes = render_blocks(&[paragraph(&src)], 300.0);
        let mut galley: Option<&egui::Galley> = None;
        let mut origin = Pos2::ZERO;
        for s in &shapes {
            if let egui::Shape::Text(t) = s
                && t.galley.text().contains('\u{E000}')
            {
                galley = Some(&t.galley);
                origin = t.pos;
                break;
            }
        }
        let galley = galley.expect("应含占位字符");
        let math = shapes
            .iter()
            .filter(|s| matches!(s, egui::Shape::Mesh(_)))
            .filter_map(shape_bounds)
            .reduce(|a, r| a.union(r))
            .unwrap_or_else(|| panic!("{tex} 应画出公式形状"));
        let i = galley
            .rows
            .iter()
            .position(|r| r.text().contains('\u{E000}'))
            .expect("占位字符应在某一行");
        let row = &galley.rows[i];
        let top = origin.y + row.pos.y;
        let bot = top + row.row.size.y;
        assert!(
            math.top() >= top - 0.5 && math.bottom() <= bot + 0.5,
            "[{tex}] 越出所在行：行带 [{top:.1},{bot:.1}]，公式 [{:.1},{:.1}]",
            math.top(),
            math.bottom()
        );
        for g in &row.glyphs {
            if g.chr == '\u{E000}' {
                continue;
            }
            let x0 = origin.x + row.pos.x + g.pos.x;
            let x1 = x0 + g.advance_width;
            assert!(
                x1 <= math.left() + 0.5 || x0 >= math.right() - 0.5,
                "[{tex}] 公式盖住同行字 {:?}：字 [{x0:.1},{x1:.1}]，公式 [{:.1},{:.1}]",
                g.chr,
                math.left(),
                math.right()
            );
        }
    }
}

/// 短小行内公式不该撑大行高（否则整段行距被一条 `x^2` 带松）。
#[test]
fn inline_short_formula_keeps_natural_line_height() {
    let plain = render_blocks(&[paragraph("普通文字普通文字普通文字")], 520.0)
        .into_iter()
        .find_map(|s| match s {
            egui::Shape::Text(t) if t.galley.text().contains('普') => {
                Some(t.galley.rows[0].row.size.y)
            }
            _ => None,
        })
        .expect("应渲染普通段落");
    let with_math = render_blocks(&[paragraph("文字 $x^2 + y^2$ 文字")], 520.0)
        .into_iter()
        .find_map(|s| match s {
            egui::Shape::Text(t) if t.galley.text().contains('\u{E000}') => {
                Some(t.galley.rows[0].row.size.y)
            }
            _ => None,
        })
        .expect("应渲染含行内公式的段落");
    assert!(
        (with_math - plain).abs() < 1.0,
        "短公式不该改变行高：无公式 {plain:.1}，有公式 {with_math:.1}"
    );
}

/// 块级行列式独占一行：前后段落都不能与它重叠。
#[test]
fn block_matrix_between_paragraphs_does_not_overlap() {
    let blocks = vec![
        paragraph("上段：行列式如下所示。"),
        Block::Math {
            tex: "\\begin{vmatrix}a&b&c\\\\d&e&f\\\\g&h&i\\end{vmatrix}".to_string(),
            source: "$$\\begin{vmatrix}a&b&c\\\\d&e&f\\\\g&h&i\\end{vmatrix}$$".to_string().into(),
        },
        paragraph("下段：按第一行展开。"),
    ];
    let shapes = render_blocks(&blocks, 520.0);
    let texts: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Text(t) => Some(egui::Rect::from_min_size(t.pos, t.galley.size())),
            _ => None,
        })
        .collect();
    let math = shapes
        .iter()
        .filter(|s| matches!(s, egui::Shape::Mesh(_)))
        .filter_map(shape_bounds)
        .reduce(|a, r| a.union(r))
        .expect("块级公式应画出形状");
    assert!(
        math.height() > 20.0,
        "块级行列式高度异常：{}",
        math.height()
    );
    for (i, t) in texts.iter().enumerate() {
        let separated = math.bottom() <= t.top() + 0.5
            || math.top() >= t.bottom() - 0.5
            || math.right() <= t.left() + 0.5
            || math.left() >= t.right() - 0.5;
        assert!(
            separated,
            "块级公式与第 {i} 段文字重叠：公式 {math:?} 文字 {t:?}"
        );
    }
}

/// 矢量快照（人工核对排版观感用）：
/// `MARKD_MATH_SVG=/tmp/x.svg cargo test --release -p markd-ui svg_snapshot -- --nocapture`
/// 未设置该环境变量时直接跳过，不影响常规测试。
#[test]
fn svg_snapshot_of_inline_matrix() {
    let Some(path) = std::env::var_os("MARKD_MATH_SVG") else {
        return;
    };
    let src = "设矩阵 $\\begin{vmatrix}a&b&c\\\\d&e&f\\\\g&h&i\\end{vmatrix}$ 的行列式记作 D，\
               后面再跟一段足够长的中文文字让这一行必然换行到这里来看整体行距。";
    let shapes = render_blocks(&[paragraph(src)], 420.0);
    let mut svg = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="520" height="320"><rect width="520" height="320" fill="#ffffff"/>"##,
    );
    for s in &shapes {
        match s {
            egui::Shape::Text(t) => {
                for row in &t.galley.rows {
                    svg.push_str(&format!(
                        r##"<rect x="{:.1}" y="{:.1}" width="420" height="{:.1}" fill="none" stroke="#e6e6e6"/>"##,
                        t.pos.x + row.pos.x,
                        t.pos.y + row.pos.y,
                        row.row.size.y
                    ));
                    for g in &row.glyphs {
                        if !g.chr.is_printable_marker() {
                            continue;
                        }
                        let x = t.pos.x + row.pos.x + g.pos.x;
                        let y = t.pos.y + row.pos.y + g.pos.y;
                        svg.push_str(&format!(
                            r##"<text x="{x:.1}" y="{y:.1}" font-size="16" fill="#262628" font-family="sans-serif">{}</text>"##,
                            g.chr
                        ));
                    }
                }
            }
            egui::Shape::Mesh(m) => {
                let mut i = 0;
                while i + 2 < m.indices.len() {
                    let p: Vec<egui::Pos2> = (0..3)
                        .map(|k| m.vertices[m.indices[i + k] as usize].pos)
                        .collect();
                    let c = m.vertices[m.indices[i] as usize].color;
                    svg.push_str(&format!(
                        r##"<path d="M {:.1} {:.1} L {:.1} {:.1} L {:.1} {:.1} Z" fill="rgb({},{},{})" fill-opacity="{:.2}"/>"##,
                        p[0].x, p[0].y, p[1].x, p[1].y, p[2].x, p[2].y,
                        c.r(), c.g(), c.b(), c.a() as f64 / 255.0
                    ));
                    i += 3;
                }
            }
            egui::Shape::Rect(r) => svg.push_str(&format!(
                r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="rgb({},{},{})"/>"##,
                r.rect.left(),
                r.rect.top(),
                r.rect.width(),
                r.rect.height(),
                r.fill.r(),
                r.fill.g(),
                r.fill.b()
            )),
            _ => {}
        }
    }
    svg.push_str("</svg>");
    std::fs::write(&path, svg).expect("写 SVG 失败");
    println!("##V 已写出 {}", PathBuf::from(&path).display());
}

/// 行内公式的占位段必须贴着公式自己的墨迹：既不能压到相邻正文，也不能把相邻
/// 正文顶开一大截。
///
/// 回归缺陷：占位宽用 `Fonts::glyph_width` 量私用区字符，而它对缺字形直接返回
/// `0`（真正排版走的是 `.notdef` 豆腐块，约 0.5em）。于是占位个数按「一像素一个
/// 字符」算、每个再按豆腐块宽推进，`$x^2$` 的占位实测 144px 而墨迹只有 13.6px：
/// 公式两侧凭空多出大段空白，光标一离开这段（切到渲染态）整段就被顶成好几行。
#[test]
fn inline_math_placeholder_tracks_the_formula_ink() {
    let theme = Theme::get(ThemeKind::Light);
    for tex in [
        "x^2",
        "\\frac{a}{b}",
        "\\sqrt{a^2+b^2}",
        "\\alpha+\\beta=\\gamma",
        "\\int_{0}^{1} x\\,dx",
    ] {
        let src = format!("前面文字 ${tex}$ 后面文字");
        let shapes = render_blocks(&[paragraph(&src)], 420.0);
        let galley = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text().contains('\u{E000}') => {
                    Some(t.galley.clone())
                }
                _ => None,
            })
            .expect("应找到含公式占位字符的 galley");
        assert_eq!(galley.rows.len(), 1, "[{tex}] 一条短公式不该把这段顶成多行");

        let origin = shapes
            .iter()
            .find_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text().contains('\u{E000}') => Some(t.pos),
                _ => None,
            })
            .expect("galley 的绘制原点");
        let row = &galley.rows[0];
        let is_marker = |c: char| (c as u32) >= 0xE000 && (c as u32) <= 0xE0FF;
        let last_marker = row
            .glyphs
            .iter()
            .rposition(|g| is_marker(g.chr))
            .expect("占位字符应在行内");
        let prev_right = row.glyphs[..row.glyphs.iter().position(|g| is_marker(g.chr)).unwrap()]
            .iter()
            .filter(|g| !g.chr.is_whitespace())
            .map(|g| origin.x + row.pos.x + g.pos.x + g.advance_width)
            .fold(f32::NEG_INFINITY, f32::max);
        let next_left = row.glyphs[last_marker + 1..]
            .iter()
            .filter(|g| !g.chr.is_whitespace())
            .map(|g| origin.x + row.pos.x + g.pos.x)
            .fold(f32::INFINITY, f32::min);

        // 公式画出来的墨迹（字形网格 + 分数线/根号这类细横线矩形），只取落在
        // 这一行、且在这一行占位段附近的，避免把面板底色之类的形状算进来。
        let row_top = origin.y + row.pos.y;
        let row_bot = row_top + row.row.size.y;
        let ink = shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.rect.height() <= 3.0 => Some(r.rect),
                _ => shape_bounds(s),
            })
            .filter(|r| {
                r.center().y >= row_top
                    && r.center().y <= row_bot
                    && r.left() > prev_right - 40.0
                    && r.right() < next_left + 40.0
            })
            .reduce(|a, r| a.union(r))
            .expect("应画出公式形状");

        assert!(
            ink.left() >= prev_right - 0.5,
            "[{tex}] 公式压到左边的字上：墨迹左缘 {:.1} < 前字右缘 {:.1}",
            ink.left(),
            prev_right
        );
        assert!(
            ink.right() <= next_left + 0.5,
            "[{tex}] 公式压到右边的字上：墨迹右缘 {:.1} > 后字左缘 {:.1}",
            ink.right(),
            next_left
        );
        // 相邻两字之间的距离只该是「源文里的两个空格 + 公式自己 + 每侧 1px 呼吸位」。
        // 空格在这个字号下约 4px，留出 6px 容差；旧的 144px 占位在这里会直接爆。
        let slack = (next_left - prev_right - ink.width()).max(0.0);
        assert!(
            slack <= 6.0 + 2.0 * 4.0,
            "[{tex}] 公式与正文之间空了 {slack:.1}px，占位段比墨迹宽太多"
        );
        // 上下文的字号一致性：公式按正文字号排，不该比正文明显高出一截。
        let line_h = row.row.size.y;
        assert!(
            line_h <= theme.body * 1.6,
            "[{tex}] 行高被撑到 {line_h:.1}（正文 {}）",
            theme.body
        );
    }
}

/// 占位字符与空白不画，其它照常。
trait PrintableMarker {
    fn is_printable_marker(&self) -> bool;
}
impl PrintableMarker for char {
    fn is_printable_marker(&self) -> bool {
        let c = *self;
        !c.is_whitespace() && !(c >= '\u{E000}' && c <= '\u{E0FF}')
    }
}
