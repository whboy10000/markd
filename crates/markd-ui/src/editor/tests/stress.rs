//! 大文档压力测试：10 万字 → 1000 万字规模阶梯。
//!
//! 默认跳过，不拖慢 `cargo test`。手工开启：
//! `MARKD_STRESS=1 MARKD_STRESS_TARGET=10000000 cargo test --release -p markd-ui stress -- --nocapture --test-threads=1`
//!
//! 输出为 `##S <标签> <指标> <值>` 行，便于脚本汇总。每帧耗时同时给出 egui 布局
//! （`run_ui`）与三角化（`tessellate`）两段，因为真实掉帧可能来自任一段。

use super::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_TARGET: usize = 1_000_000;

fn enabled() -> bool {
    std::env::var_os("MARKD_STRESS").is_some()
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.replace('_', "").parse::<usize>().ok())
        .unwrap_or(default)
}

/// 本进程物理内存（MB）。macOS/Linux 的 `ps -o rss=` 单位是 KB。
fn rss_mb() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<f64>()
                .ok()
        })
        .unwrap_or(0.0)
        / 1024.0
}

fn stress_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/stress");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn log(tag: &str, metric: &str, value: impl std::fmt::Display) {
    println!("##S {tag} {metric} {value}");
}

/// 规模标签：不足 100 万字用 k，避免 3 万字与 6 万字都显示成 0.0M/0.1M 而混淆。
fn mtag(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else {
        format!("{}k", n / 1000)
    }
}

fn avg(d: Duration, n: usize) -> Duration {
    Duration::from_secs_f64(d.as_secs_f64() / n.max(1) as f64)
}

fn ms(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1000.0)
}

// ---------------------------------------------------------------- 语料生成

const PARA: &str = "这是一段用于压力测试的正文，覆盖 **加粗**、*斜体*、`行内代码`、[站内链接](other.md)、[外部链接](https://example.com/a/b?x=1) 与中文标点，长度控制在两行左右以复现真实排版的换行成本。";

fn table_block(i: usize) -> String {
    let mut s = String::new();
    s.push_str("| 指标 | 数值 | 单位 | 备注 |\n| --- | --- | --- | --- |\n");
    for r in 0..5 {
        s.push_str(&format!(
            "| 行 {i}-{r} | {} | MB | 说明文字，表格块走多行 TextEdit。{r} |\n",
            r * 131
        ));
    }
    s.push('\n');
    s
}

fn code_block(i: usize) -> String {
    format!(
        "```rust\nfn case_{i}() {{\n    let v = vec![1, 2, 3, 4];\n    for x in &v {{\n        println!(\"{{}}\", x);\n    }}\n}}\n```\n\n"
    )
}

fn math_block(i: usize) -> String {
    format!(
        "```math\n\\frac{{\\partial u}}{{\\partial t}} + \\sum_{{i={i}}}^{{N}} \\nabla^2 \\phi_i = \\int_0^1 f(x)\\,dx\n```\n\n"
    )
}

fn quote_block(i: usize) -> String {
    format!(
        "> 引用第一行 {i}：验证引用块左侧竖线的连续绘制与缩进。\n> 引用第二行：跨行引用需要保留 source 无损往返。\n\n"
    )
}

fn list_block(i: usize) -> String {
    let mut s = String::new();
    for k in 0..4 {
        s.push_str(&format!(
            "- 列表项 {i}-{k}：混合 `代码`、**强调** 与 [链接](x.md)。\n"
        ));
    }
    s.push('\n');
    s
}

fn task_block(i: usize) -> String {
    let mut s = String::new();
    for k in 0..4 {
        s.push_str(&format!(
            "- [{}] 任务 {i}-{k}：复选框是可交互控件，成本高于纯文本。\n",
            if k % 2 == 0 { 'x' } else { ' ' }
        ));
    }
    s.push('\n');
    s
}

fn ordered_block(i: usize) -> String {
    let mut s = String::new();
    for k in 1..=4 {
        s.push_str(&format!("{k}. 有序项 {i}-{k}：编号前缀解析与对齐。\n"));
    }
    s.push('\n');
    s
}

/// 一个「组」= 若干相邻块。`kind` 决定块类型配比。
fn group(i: usize, kind: &str) -> String {
    let mut s = String::new();
    match kind {
        // 纯段落：结构最简，用来分离「块数量」与「块复杂度」两个变量
        "flat" => {
            s.push_str(PARA);
            s.push_str("\n\n");
        }
        "code" => {
            s.push_str(&code_block(i));
            s.push_str(&code_block(i + 1));
        }
        "table" => {
            s.push_str(&table_block(i));
            s.push_str(&table_block(i + 1));
        }
        // 公式为主：每帧都要重新栅格化或细分字形轮廓
        "math" => {
            s.push_str(&math_block(i));
            s.push_str(&math_block(i + 1));
        }
        "heavy" => {
            s.push_str(&math_block(i));
            s.push_str(&table_block(i));
            s.push_str(&code_block(i));
            s.push_str("```mermaid\ngraph LR\n  A[开始] --> B{判断}\n  B -->|是| C[结束]\n```\n\n");
        }
        // 日常文档配比：段落四成，标题/列表/任务/引用/代码/表格/公式均衡出现
        _ => match i % 10 {
            0 => {
                s.push_str(&format!("## 第 {i} 节 · 压力测试标题\n\n"));
                s.push_str(PARA);
                s.push_str("\n\n");
            }
            1 => s.push_str(&list_block(i)),
            2 => {
                s.push_str(PARA);
                s.push_str("\n\n");
            }
            3 => s.push_str(&task_block(i)),
            4 => {
                s.push_str(&quote_block(i));
                s.push_str(PARA);
                s.push_str("\n\n");
            }
            5 => s.push_str(&code_block(i)),
            6 => s.push_str(&table_block(i)),
            7 => s.push_str(&ordered_block(i)),
            8 => {
                s.push_str("行内公式 $E=mc^2$ 与普通文字混排，还有 `代码` 与 ![图片](missing.png) 的图文混排。\n\n");
                s.push_str(PARA);
                s.push_str("\n\n");
            }
            _ => {
                s.push_str(PARA);
                s.push_str("\n\n---\n\n");
            }
        },
    }
    s
}

/// 生成不少于 `target` 字符的语料。字符数按增量累加，避免每轮重扫全文退化成 O(n²)。
fn corpus(kind: &str, target: usize) -> (String, usize) {
    let mut text = String::from("# 压力测试文档\n\n");
    let mut chars = text.chars().count();
    let mut i = 0usize;
    while chars < target {
        let g = group(i, kind);
        chars += g.chars().count();
        text.push_str(&g);
        i += 1;
    }
    (text, chars)
}

fn write_fixture(kind: &str, chars: usize, text: &str) -> PathBuf {
    let p = stress_dir().join(format!("{kind}-{chars}.md"));
    let _ = std::fs::write(&p, text);
    p
}

// ---------------------------------------------------------------- 帧测量

#[allow(clippy::too_many_arguments)]
fn frame_once(
    ctx: &egui::Context,
    w: f32,
    h: f32,
    doc: &mut Document,
    state: &mut EditorState,
    theme: &Theme,
    images: &mut ImageCache,
    outline: &[crate::document::OutlineItem],
    actions: &mut Vec<crate::render::Action>,
    source_mode: bool,
) -> (Duration, usize, usize, Duration) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, h),
        )),
        ..Default::default()
    };
    let t = Instant::now();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(8))
            .show(root, |ui| {
                let _ = show_editor(
                    ui,
                    doc,
                    state,
                    theme,
                    ctx,
                    images,
                    None,
                    outline,
                    source_mode,
                    false,
                    false,
                    false,
                    1,
                    actions,
                );
            });
    });
    let layout = t.elapsed();
    let shapes = out.shapes.len();
    let ppf = out.pixels_per_point;
    let t2 = Instant::now();
    let prims = ctx.tessellate(std::mem::take(&mut out.shapes), ppf);
    let tess = t2.elapsed();
    let verts: usize = prims
        .iter()
        .map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
            egui::epaint::Primitive::Callback(_) => 0,
        })
        .sum();
    // 无头环境没有渲染器消费纹理增删，必须显式清掉，否则析构会 panic。
    out.textures_delta.clear();
    (layout, shapes, verts, tess)
}

/// 帧宽：默认 1280，可用 MARKD_STRESS_W 收窄到真实正文宽度（窗口 1000 − 侧栏 220）。
fn frame_w() -> f32 {
    std::env::var("MARKD_STRESS_W")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1280.0)
}

/// 一整套无头运行时：真实字体、指定缩放、与 app 相同的窗口尺寸。
struct Rig {
    doc: Document,
    state: EditorState,
    outline: Vec<crate::document::OutlineItem>,
    ctx: egui::Context,
    theme: Theme,
    images: ImageCache,
    actions: Vec<crate::render::Action>,
    blocks: usize,
}

impl Rig {
    fn new(text: &str, ppf: f32) -> Self {
        let doc = Document::new(text, None);
        let outline = doc.outline();
        let blocks = doc.blocks.len();
        let ctx = egui::Context::default();
        markd_platform::fonts::install(&ctx);
        ctx.set_pixels_per_point(ppf);
        Self {
            doc,
            state: EditorState::default(),
            outline,
            ctx,
            theme: Theme::get(crate::theme::ThemeKind::Light),
            images: ImageCache::new(),
            actions: Vec::new(),
            blocks,
        }
    }

    fn frame(&mut self, source_mode: bool) -> (Duration, usize, usize, Duration) {
        let Self {
            doc,
            state,
            outline,
            ctx,
            theme,
            images,
            actions,
            ..
        } = self;
        frame_once(
            ctx,
            frame_w(),
            800.0,
            doc,
            state,
            theme,
            images,
            outline,
            actions,
            source_mode,
        )
    }
}

// ---------------------------------------------------------------- 用例

/// 打开流水线各环节：读盘 → 解析 → 大纲/统计/回写 → 导出 → 搜索 → 撤销快照。
/// 不涉及 GUI，1000 万字也在秒级，可放心跑最大规模。
#[test]
fn stress_pipeline() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let kind = std::env::var("MARKD_STRESS_KIND").unwrap_or_else(|_| "mixed".into());
    let ttag = format!("{kind}-{}", mtag(target));

    let (text, chars) = corpus(&kind, target);
    let path = write_fixture(&kind, chars, &text);
    log(&ttag, "chars", chars);
    log(&ttag, "bytes", text.len());

    // 1) 读盘（workspace::read 是整读）
    let t = Instant::now();
    let read = std::fs::read_to_string(&path).expect("read fixture");
    log(&ttag, "read_ms", ms(t.elapsed()));
    drop(read);
    log(&ttag, "rss_base_mb", format!("{:.0}", rss_mb()));

    // 2) 解析
    let t = Instant::now();
    let parsed = crate::ast::parse(&text);
    let parse_d = t.elapsed();
    log(&ttag, "parse_ms", ms(parse_d));
    log(&ttag, "blocks", parsed.len());
    log(
        &ttag,
        "parse_chars_per_s",
        format!("{:.0}", chars as f64 / parse_d.as_secs_f64()),
    );
    log(&ttag, "rss_after_parse_mb", format!("{:.0}", rss_mb()));

    // 块类型分布：证明语料确实是混合结构，而不是退化成单一段落
    let mut dist: std::collections::BTreeMap<&str, usize> = Default::default();
    {
        let probe = parsed;
        for b in probe.iter() {
            let name = match b {
                Block::Heading { .. } => "heading",
                Block::Paragraph { .. } => "para",
                Block::Quote { .. } => "quote",
                Block::ListItem { .. } => "list",
                Block::Code { .. } => "code",
                Block::CodeGroup { .. } => "codegroup",
                Block::Table { .. } => "table",
                Block::Rule { .. } => "rule",
                Block::Math { .. } => "math",
                Block::Diagram { .. } => "diagram",
                Block::FrontMatter { .. } => "frontmatter",
                Block::Footnote { .. } => "footnote",
                Block::Toc { .. } => "toc",
                Block::Blank { .. } => "blank",
            };
            *dist.entry(name).or_default() += 1;
        }
    }
    let dist_s = dist
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    log(&ttag, "block_dist", dist_s);
    drop(dist);

    let mut doc = Document::new(&text, Some(path.clone()));

    // 3) 打开文档时的派生数据
    let t = Instant::now();
    let outline_len = doc.outline().len();
    log(&ttag, "outline_ms", ms(t.elapsed()));
    log(&ttag, "outline_items", outline_len);
    let t = Instant::now();
    let stats = doc.stats();
    log(&ttag, "stats_ms", ms(t.elapsed()));
    log(&ttag, "stats_words", stats.words);

    // 4) 无损往返 + 落盘（每按一次 Cmd+S 的成本）
    let t = Instant::now();
    let src = doc.source();
    log(&ttag, "source_ms", ms(t.elapsed()));
    log(
        &ttag,
        "roundtrip_lossless",
        if src == text { "yes" } else { "NO" },
    );
    let t = Instant::now();
    std::fs::write(stress_dir().join(format!("save-{ttag}.md")), &src).expect("save");
    log(&ttag, "save_ms", ms(t.elapsed()));
    drop(src);

    // 5) 导出 HTML
    let t = Instant::now();
    let html = crate::export::to_html("stress", &text, false);
    log(&ttag, "export_html_ms", ms(t.elapsed()));
    log(&ttag, "export_html_bytes", html.len());
    drop(html);

    // 6) 查找：走真实实现（分片并行 + 命中上限）。查一个文中不存在的词，
    //    逼它扫完整篇 —— 这才是「搜不到」时用户等的那一帧。
    let re = regex::Regex::new("压力不存在词xy").unwrap();
    let t = Instant::now();
    let par = crate::find::scan_hits(&doc.blocks, &re);
    log(&ttag, "find_scan_ms", ms(t.elapsed()));
    let t = Instant::now();
    let ser = crate::find::scan_hits_with(&doc.blocks, &re, 1);
    log(&ttag, "find_scan_serial_ms", ms(t.elapsed()));
    assert_eq!(par, ser, "并行与串行结果必须逐条一致");
    log(&ttag, "find_hits", par.len());

    // 7) 撤销记录成本：record() 只闭合编辑段，栈里存块级差异，不再复制全文
    let t = Instant::now();
    doc.record();
    log(&ttag, "undo_record_one_ms", ms(t.elapsed()));
    let before = rss_mb();
    for k in 0..9 {
        // 每轮先改出一个真正不同的版本，保证这步是真实编辑而不是空记录
        doc.edit_block(k, &format!("撤销快照压力测试 {k} {}", chars));
        doc.record();
    }
    let after = rss_mb();
    log(&ttag, "undo_record_9_mb", format!("{:.2}", after - before));
    log(&ttag, "undo_stack_kb", format!("{:.1}", doc.undo_bytes() as f64 / 1024.0));
    log(&ttag, "undo_depth", doc.undo_depth());
    log(
        &ttag,
        "undo_cap_200_mb",
        format!("{:.2}", (after - before) / 9.0 * 200.0),
    );
    log(&ttag, "rss_after_undo_mb", format!("{after:.0}"));

    // 8) 撤销/重做：整篇重新解析
    let t = Instant::now();
    let ok = doc.undo();
    log(&ttag, "undo_ms", ms(t.elapsed()));
    log(&ttag, "undo_ok", ok);
    let t = Instant::now();
    let ok = doc.redo();
    log(&ttag, "redo_ms", ms(t.elapsed()));
    log(&ttag, "redo_ok", ok);
}

/// 撤销栈内存增长：栈里存的是「被改动那几块的源码」，不是整篇副本，
/// 所以斜率应当只与编辑量有关，与文档大小无关。这里同时量 RSS 与栈内字节。
#[test]
fn stress_undo_growth() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let steps = env_usize("MARKD_STRESS_UNDO", 60);
    let ttag = format!("undo-{}", mtag(target));
    let (text, chars) = corpus("mixed", target);
    let mut doc = Document::new(&text, None);
    let blocks = doc.blocks.len();
    log(&ttag, "source_bytes", text.len());
    log(&ttag, "rss_step0_mb", format!("{:.0}", rss_mb()));
    for k in 1..=steps {
        doc.edit_block(k % blocks, &format!("撤销快照 {k} / {chars}"));
        doc.record();
        if k % 10 == 0 {
            log(
                &ttag,
                &format!("rss_step{k}_mb"),
                format!("{:.0}", rss_mb()),
            );
        }
    }
    log(&ttag, "rss_end_mb", format!("{:.0}", rss_mb()));
    log(&ttag, "records", steps);
    log(&ttag, "undo_depth", doc.undo_depth());
    log(
        &ttag,
        "undo_stack_kb",
        format!("{:.1}", doc.undo_bytes() as f64 / 1024.0),
    );
    // 撤干净必须一字不差回到原文：这是「按编辑量增长」没写错下标的证明。
    let t = Instant::now();
    for i in 0..steps {
        assert!(doc.undo(), "第 {i} 步撤销失败");
    }
    log(&ttag, "undo_all_ms", ms(t.elapsed()));
    log(
        &ttag,
        "undo_all_lossless",
        if doc.source() == text { "yes" } else { "NO" },
    );
}

/// 混合模式（默认视图）的帧成本：首帧、静置帧、滚动跳帧、每键一帧。
#[test]
fn stress_frames() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let kind = std::env::var("MARKD_STRESS_KIND").unwrap_or_else(|_| "mixed".into());
    let ppf = std::env::var("MARKD_STRESS_PPF")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(2.0);
    let ttag = format!("{kind}-{}-ppf{ppf}", mtag(target));

    let (text, _chars) = corpus(&kind, target);
    let mut rig = Rig::new(&text, ppf);
    let total = rig.blocks;
    log(&ttag, "frames_blocks", total);
    log(&ttag, "rss_after_parse_mb", format!("{:.0}", rss_mb()));

    // 首帧（冷字体图集 + 冷 galley 缓存）= 打开文档到看见内容的等待
    let t = Instant::now();
    let (layout, _, _, _) = rig.frame(false);
    log(&ttag, "first_frame_ms", ms(t.elapsed()));
    log(&ttag, "first_layout_ms", ms(layout));
    log(
        &ttag,
        "rss_after_first_frame_mb",
        format!("{:.0}", rss_mb()),
    );

    // 静置热帧：内容不变时每次重绘的固定开销
    let mut worst = Duration::ZERO;
    let mut sum = Duration::ZERO;
    let mut last = (0usize, 0usize, Duration::ZERO);
    for _ in 0..5 {
        let (layout, shapes, verts, tess) = rig.frame(false);
        sum += layout;
        worst = worst.max(layout);
        last = (shapes, verts, tess);
    }
    log(&ttag, "idle_frame_avg_ms", ms(avg(sum, 5)));
    log(&ttag, "idle_frame_worst_ms", ms(worst));
    log(&ttag, "shapes_per_frame", last.0);
    log(&ttag, "verts_per_frame", last.1);
    log(&ttag, "tessellate_ms", ms(last.2));
    log(&ttag, "rss_hot_mb", format!("{:.0}", rss_mb()));

    // 滚动跳帧：跳到 20 个位置，模拟拖动滚动条
    let mut worst = Duration::ZERO;
    let mut sum = Duration::ZERO;
    for k in 0..20 {
        let b = (total.saturating_sub(1) * k) / 20;
        rig.state.active = b;
        rig.state.scroll_req = Some(b);
        let (layout, _, _, _) = rig.frame(false);
        sum += layout;
        worst = worst.max(layout);
    }
    log(&ttag, "scroll_frame_avg_ms", ms(avg(sum, 20)));
    log(&ttag, "scroll_frame_worst_ms", ms(worst));

    // 每键一帧：编辑一个块 + 重绘，打字手感的真实成本
    rig.state.active = total / 2;
    rig.state.scroll_req = None;
    let mut worst = Duration::ZERO;
    let mut sum_edit = Duration::ZERO;
    let mut sum_frame = Duration::ZERO;
    for k in 0..10 {
        let t = Instant::now();
        rig.doc.edit_block(total / 2, &format!("连续输入测试 {k}"));
        let e = t.elapsed();
        let (layout, _, _, _) = rig.frame(false);
        sum_edit += e;
        sum_frame += layout;
        worst = worst.max(e + layout);
    }
    log(&ttag, "edit_block_avg_ms", ms(avg(sum_edit, 10)));
    log(&ttag, "keypress_frame_avg_ms", ms(avg(sum_frame, 10)));
    log(&ttag, "keypress_worst_ms", ms(worst));
    log(&ttag, "rss_end_mb", format!("{:.0}", rss_mb()));
}

/// 同一规模下换用源码模式（整篇一个 TextEdit）的帧成本。
#[test]
fn stress_source_mode() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let ttag = format!("source-{}", mtag(target));
    let (text, _chars) = corpus("mixed", target);
    let mut rig = Rig::new(&text, 2.0);
    log(&ttag, "blocks", rig.blocks);
    for _ in 0..3 {
        let (layout, shapes, verts, tess) = rig.frame(true);
        log(&ttag, "frame_ms", ms(layout));
        log(&ttag, "shapes", shapes);
        log(&ttag, "verts", verts);
        log(&ttag, "tess_ms", ms(tess));
    }
    log(&ttag, "rss_mb", format!("{:.0}", rss_mb()));
}

/// 源码模式每帧成本的构成：`source_mode.rs` 里那几趟全文扫描分开计时。
/// 帧本身只有一个总数，看不出该先砍哪一刀。
#[test]
fn stress_source_parts() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let kind = std::env::var("MARKD_STRESS_KIND").unwrap_or_else(|_| "mixed".into());
    let ttag = format!("{kind}-sourceparts-{}", mtag(target));
    let (text, _chars) = corpus(&kind, target);
    let mut rig = Rig::new(&text, 2.0);
    let theme = &rig.theme;

    // 1) 整篇源码字符串：每帧从块重建一遍。
    let t = Instant::now();
    let src = rig.doc.source();
    log(&ttag, "doc_source_ms", ms(t.elapsed()));

    // 2) 每个源码行首字符下标（行号与光标行判定都要用它）。
    let t = Instant::now();
    let mut starts = vec![0usize];
    let mut cnt = 0usize;
    for c in src.chars() {
        if c == '\n' {
            starts.push(cnt + 1);
        }
        cnt += 1;
    }
    log(&ttag, "line_starts_ms", ms(t.elapsed()));
    log(&ttag, "source_lines", starts.len());

    // 3) 最长行宽度估算（横向滚动范围，逐字符扫全文）。
    let t = Instant::now();
    let content_w = src
        .lines()
        .map(|l| {
            let w: f32 = l
                .chars()
                .map(|c| if c.is_ascii() { 0.62 } else { 1.25 })
                .sum();
            w * theme.mono
        })
        .fold(0.0f32, f32::max);
    log(&ttag, "content_w_ms", ms(t.elapsed()));
    log(&ttag, "content_w", format!("{content_w:.0}"));

    // 4) 整篇一个 LayoutJob 的排版：TextEdit 每帧都要重排一遍。
    let layout = |ctx: &egui::Context| {
        let mut job = egui::text::LayoutJob::default();
        let mut fmt = theme.mono_fmt();
        fmt.line_height = Some(theme.mono * 1.4);
        job.append(&src, 0.0, fmt);
        job.wrap.max_width = f32::INFINITY;
        ctx.fonts_mut(|f| f.layout_job(job))
    };
    let ctx = egui::Context::default();
    markd_platform::fonts::install(&ctx);
    // fonts 要跑过一帧才存在（Context::fonts_mut 在此之前是 None）。
    ctx.run_ui(egui::RawInput::default(), |_| {});
    let t = Instant::now();
    let cold = layout(&ctx);
    log(&ttag, "galley_cold_ms", ms(t.elapsed()));
    let t = Instant::now();
    let galley = layout(&ctx);
    log(&ttag, "galley_warm_ms", ms(t.elapsed()));
    log(&ttag, "galley_rows", galley.rows.len());
    log(&ttag, "src_mb", format!("{:.1}", src.len() as f64 / 1e6));
    let _ = cold.rows.len();

    // 5) 行号栏：再遍历一遍 galley 的每一行。
    let t = Instant::now();
    let mut per_line: Vec<Option<f32>> = vec![None; starts.len() + 1];
    let mut acc = 0usize;
    for row in &galley.rows {
        let start_char = acc;
        acc += row.char_count_including_newline().0;
        let line_no = match starts.binary_search(&start_char) {
            Ok(i) => i + 1,
            Err(i) => i.min(starts.len()),
        };
        if per_line.get(line_no).map_or(false, |s| s.is_none()) {
            per_line[line_no] = Some(row.pos.y + row.row.size.y * 0.5);
        }
    }
    log(&ttag, "gutter_scan_ms", ms(t.elapsed()));
    // 行几何是否均匀：源码模式的行号/光标行底色能否用「行下标 × 行高」直接算。
    log(&ttag, "row0_y", format!("{:.3}", galley.rows[0].pos.y));
    log(&ttag, "row_dy", format!("{:.3}", galley.rows[9].pos.y - galley.rows[8].pos.y));
    log(&ttag, "row_h", format!("{:.3}", galley.rows[8].row.size.y));
    log(&ttag, "want_dy", format!("{:.3}", theme.mono * 1.4));

    // 6) 改一个字符：源码模式写回走整篇重解析，这是每键的成本下界。
    let mut edited = src.clone();
    // 行首是安全的字节边界：换行符之后插入一个字符，等价于「敲一个键」。
    let at = src.find('\n').unwrap() + 1;
    edited.insert(at, 'x');
    let rss_before_undo = rss_mb();
    let t = Instant::now();
    rig.doc.set_source(&edited);
    log(&ttag, "set_source_ms", ms(t.elapsed()));
    // 连打 9 个字（同一撤销组，模拟源码模式连续输入）：撤销栈该只按编辑量增长。
    // 旧实现每键存两份整篇，1000 万字下 10 键就是 ~370MB。
    for i in 0..9u8 {
        let cur = rig.doc.source();
        let at = cur.find('\n').unwrap() + 1 + i as usize;
        let mut e = cur;
        e.insert(at.min(e.len() - 1), 'y');
        rig.doc.set_source(&e);
    }
    log(
        &ttag,
        "undo10_stack_kb",
        format!("{:.1}", rig.doc.undo_bytes() as f64 / 1024.0),
    );
    log(&ttag, "undo10_depth", rig.doc.undo_depth());
    log(
        &ttag,
        "undo10_rss_mb",
        format!("{:.0}", rss_mb() - rss_before_undo),
    );
    let t = Instant::now();
    let ok = rig.doc.undo();
    log(&ttag, "undo_ms", ms(t.elapsed()));
    log(&ttag, "undo_lossless", ok && rig.doc.source() == src);
    log(&ttag, "rss_end_mb", format!("{:.0}", rss_mb()));
}

/// 静置时谁在要求重绘：无人输入时，一帧结束后 egui 是否已被排上下一次重绘。
/// 这决定大文档会不会「打开后一直满载」。
#[test]
fn stress_repaint_causes() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", 200_000);
    let ttag = format!("repaint-{}", mtag(target));
    let (text, _chars) = corpus("mixed", target);
    let mut rig = Rig::new(&text, 2.0);
    // 与 open_file 一致：打开即把焦点给活动块，活动块渲染成带光标的 TextEdit
    rig.state.focus_req = Some(0);
    for step in 0..6 {
        let (layout, _, _, _) = rig.frame(false);
        let mut causes = rig.ctx.repaint_causes();
        let n = causes.len();
        let mut counts: std::collections::BTreeMap<(String, u32), usize> = Default::default();
        for c in causes.drain(..) {
            *counts.entry((c.file.to_string(), c.line)).or_default() += 1;
        }
        let top = counts
            .iter()
            .max_by_key(|(_, v)| **v)
            .map(|((f, l), _)| format!("{f}:{l}"))
            .unwrap_or_else(|| "-".into());
        log(&ttag, &format!("f{step}_ms"), ms(layout));
        log(
            &ttag,
            &format!("f{step}_repaint_requests"),
            format!("{n} top={top}"),
        );
        log(
            &ttag,
            &format!("f{step}_pending"),
            rig.ctx.has_requested_repaint(),
        );
    }
    // 再把活动块挪到文档中部并请求滚动，模拟点进正文之后
    let mid = rig.blocks / 2;
    rig.state.active = mid;
    rig.state.scroll_req = Some(mid);
    for step in 6..9 {
        let (layout, _, _, _) = rig.frame(false);
        let causes = rig.ctx.repaint_causes();
        let top = causes
            .first()
            .map(|c| format!("{}:{}", c.file, c.line))
            .unwrap_or_else(|| "-".into());
        log(&ttag, &format!("f{step}_ms"), ms(layout));
        log(
            &ttag,
            &format!("f{step}_repaint_requests"),
            format!("{} top={top}", causes.len()),
        );
    }
}

/// 侧栏大纲面板的帧成本。
///
/// `stress_frames` 只走编辑区（`show_editor`），面板不在它的覆盖范围内；1000 万字
/// 有 6000+ 个标题，一旦「大纲」页签开着每帧全量排版，用户就会感到「看大纲时卡」。
#[test]
fn stress_sidebar() {
    if !enabled() {
        return;
    }
    let target = env_usize("MARKD_STRESS_TARGET", DEFAULT_TARGET);
    let kind = std::env::var("MARKD_STRESS_KIND").unwrap_or_else(|_| "mixed".into());
    let ppf = std::env::var("MARKD_STRESS_PPF")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(2.0);
    let ttag = format!("{kind}-sidebar-{}", mtag(target));

    let (text, _chars) = corpus(&kind, target);
    let t = Instant::now();
    let doc = Document::new(&text, None);
    let outline = doc.outline();
    log(&ttag, "build_ms", ms(t.elapsed()));
    log(&ttag, "headings", outline.len());

    let ctx = egui::Context::default();
    markd_platform::fonts::install(&ctx);
    ctx.set_pixels_per_point(ppf);
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let folded = std::collections::HashSet::<usize>::new();

    // 与 app 一致：Panel::left(exact_size 220) 里套一个竖向 ScrollArea。
    let frame = |events: Vec<egui::Event>| -> (Duration, usize) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        let t = Instant::now();
        let mut out = ctx.run_ui(input, |root| {
            egui::Panel::left("sidebar")
                .exact_size(220.0)
                .show(root, |ui| {
                    // 大纲面板自带滚动+虚拟化，这里不再套 ScrollArea（与 app 侧栏一致）。
                    let mut actions = Vec::new();
                    let mut toggles = Vec::new();
                    crate::sidebar::outline_panel(
                        ui,
                        &outline,
                        &theme,
                        &mut actions,
                        &folded,
                        &mut toggles,
                    );
                });
        });
        let layout = t.elapsed();
        let shapes = out.shapes.len();
        out.textures_delta.clear();
        (layout, shapes)
    };

    let (cold, shapes) = frame(Vec::new());
    log(&ttag, "first_frame_ms", ms(cold));
    log(&ttag, "shapes", shapes);

    let mut sum = Duration::ZERO;
    let mut worst = Duration::ZERO;
    for _ in 0..5 {
        let (d, s) = frame(Vec::new());
        sum += d;
        worst = worst.max(d);
        log(&ttag, "idle_shapes", s);
    }
    log(&ttag, "idle_frame_avg_ms", ms(avg(sum, 5)));
    log(&ttag, "idle_frame_worst_ms", ms(worst));

    // 滚动帧：把滚轮事件投进侧栏（x<220），一次 600px。
    let wheel = |dy: f32| {
        vec![
            egui::Event::PointerMoved(egui::pos2(100.0, 400.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Default::default(),
            },
        ]
    };
    let mut sum = Duration::ZERO;
    let mut worst = Duration::ZERO;
    let n = 8;
    for _ in 0..n {
        let (d, _) = frame(wheel(-600.0));
        sum += d;
        worst = worst.max(d);
    }
    log(&ttag, "scroll_frame_avg_ms", ms(avg(sum, n)));
    log(&ttag, "scroll_frame_worst_ms", ms(worst));
    log(&ttag, "rss_end_mb", format!("{:.0}", rss_mb()));
}

/// 块类型成本对照：固定字数，逐类比较每块渲染开销。
#[test]
fn stress_block_kinds() {
    if !enabled() {
        return;
    }
    let chars_each = env_usize("MARKD_STRESS_KIND_TARGET", 100_000);
    let frames = env_usize("MARKD_STRESS_KIND_FRAMES", 4);
    for kind in ["flat", "mixed", "code", "table", "math", "heavy"] {
        let (text, chars) = corpus(kind, chars_each);
        let mut rig = Rig::new(&text, 2.0);
        let blocks = rig.blocks;
        let (cold, _, _, _) = rig.frame(false);
        let mut sum = Duration::ZERO;
        let mut last = (0usize, 0usize);
        for _ in 0..frames {
            let (layout, shapes, verts, _) = rig.frame(false);
            sum += layout;
            last = (shapes, verts);
        }
        let warm = avg(sum, frames);
        log(kind, "chars", chars);
        log(kind, "blocks", blocks);
        log(kind, "cold_frame_ms", ms(cold));
        log(kind, "warm_frame_ms", ms(warm));
        log(
            kind,
            "us_per_block",
            format!("{:.1}", warm.as_micros() as f64 / blocks as f64),
        );
        log(kind, "shapes", last.0);
        log(kind, "verts", last.1);
        log(kind, "rss_mb", format!("{:.0}", rss_mb()));
    }
}
