use super::*;

/// 10 万字文档的帧率验证：模拟滚动到不同位置，测量每帧耗时。
#[test]
fn bench_100k_doc_scroll() {
    use std::time::Instant;
    let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
    let mut text = String::new();
    for i in 0..470 {
        if i % 20 == 0 {
            text.push_str(&format!("## 标题 {i}\n\n"));
        }
        text.push_str(&para);
        text.push_str("\n\n");
    }
    println!("[100k] chars = {}", text.chars().count());

    let mut doc = Document::new(&text, None);
    let t = Instant::now();
    let _ = crate::ast::parse(&text);
    println!("[100k] parse = {:?}", t.elapsed());

    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut state = EditorState::default();
    let outline = doc.outline();
    let total = doc.blocks.len();
    println!("[100k] blocks = {total}");

    // 模拟滚动：每帧跳到文档的不同位置（会强制渲染一批全新的块）。
    let mut worst = std::time::Duration::ZERO;
    let mut sum = std::time::Duration::ZERO;
    let mut n = 0;
    for frac in 0..20 {
        let target = (total.saturating_sub(1) * frac) / 20;
        state.active = target;
        state.scroll_req = Some(target);
        let t = Instant::now();
        frame(1200.0, |ui| {
            let ctx = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &ctx,
                &mut images,
                None,
                &outline,
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
        let e = t.elapsed();
        sum += e;
        worst = worst.max(e);
        n += 1;
        if frac % 5 == 0 {
            println!("[100k] jump to block {target}/{total} = {e:?}");
        }
    }
    println!("[100k] avg frame = {:?}, worst = {:?}", sum / n, worst);

    // 连续输入：每帧改一个字，测编辑回写耗时
    let t = Instant::now();
    for k in 0..20 {
        let src = format!("连续输入测试 {k}");
        doc.edit_block(5, &src);
    }
    println!("[100k] 20 次 edit_block = {:?}", t.elapsed());
}

/// 50 万字（中文）整篇打开、滚动浏览与连续输入的实测。
#[test]
fn bench_500k_doc() {
    use std::time::Instant;
    let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
    let per = para.chars().count() + 2;
    let n = 500_000 / per + 1;
    let mut text = String::new();
    for i in 0..n {
        if i % 20 == 0 {
            text.push_str(&format!(
                "## 标题 {i}

"
            ));
        }
        text.push_str(&para);
        text.push_str(
            "

",
        );
    }
    let chars = text.chars().count();
    println!("[500k] chars = {chars}");

    let t = Instant::now();
    let blocks = crate::ast::parse(&text);
    println!(
        "[500k] 打开时解析 = {:?}, blocks = {}",
        t.elapsed(),
        blocks.len()
    );

    let mut doc = Document::new(&text, None);
    let t = Instant::now();
    let outline = doc.outline();
    println!("[500k] 大纲 = {:?}", t.elapsed());
    let t = Instant::now();
    let _ = doc.stats();
    println!("[500k] 统计 = {:?}", t.elapsed());

    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut state = EditorState::default();
    let total = doc.blocks.len();

    // 冷启动第一帧（缓存全空）：打开文档那一瞬间的成本
    let ctx = egui::Context::default();
    let t = Instant::now();
    frame_warm(
        &ctx,
        1200.0,
        900.0,
        &mut doc,
        &mut state,
        &theme,
        &mut images,
        &outline,
        &mut actions,
    );
    println!("[500k] 首帧（冷缓存）= {:?}", t.elapsed());

    // 热缓存下的滚动浏览：真实使用时的每帧成本
    let mut worst = std::time::Duration::ZERO;
    let mut sum = std::time::Duration::ZERO;
    let mut cnt = 0;
    for frac in 0..20 {
        let target = (total.saturating_sub(1) * frac) / 20;
        state.active = target;
        state.scroll_req = Some(target);
        let t = Instant::now();
        frame_warm(
            &ctx,
            1200.0,
            900.0,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &outline,
            &mut actions,
        );
        let e = t.elapsed();
        sum += e;
        worst = worst.max(e);
        cnt += 1;
    }
    println!(
        "[500k] 热缓存滚动 20 帧：avg = {:?}, worst = {:?}",
        sum / cnt,
        worst
    );

    // 连续输入：改一个字 + 重绘一帧，测真实打字的手感
    state.active = total / 2;
    state.scroll_req = None;
    let t = Instant::now();
    let mut worst_edit = std::time::Duration::ZERO;
    let mut sum_edit = std::time::Duration::ZERO;
    for k in 0..30 {
        doc.edit_block(total / 2, &format!("连续输入测试 {k}"));
        let t1 = Instant::now();
        frame_warm(
            &ctx,
            1200.0,
            900.0,
            &mut doc,
            &mut state,
            &theme,
            &mut images,
            &outline,
            &mut actions,
        );
        let e = t1.elapsed();
        sum_edit += e;
        worst_edit = worst_edit.max(e);
    }
    let _ = t.elapsed();
    println!(
        "[500k] 每键一帧 30 次：avg = {:?}, worst = {:?}",
        sum_edit / 30,
        worst_edit
    );

    let t = Instant::now();
    doc.edit_block(total / 2, &format!("最终 {}", chars));
    println!("[500k] 单次 edit_block = {:?}", t.elapsed());
}

/// 性能基准（打印耗时）：10 万字级别的文档，看各环节开销。
#[test]
fn bench_large_doc() {
    use std::time::Instant;
    let para = "这是一段用于性能测试的正文，包含 **加粗**、*斜体*、`行内代码` 和 [链接](https://example.com) 以及中文标点。".repeat(3);
    let mut text = String::new();
    for i in 0..300 {
        if i % 20 == 0 {
            text.push_str(&format!("## 标题 {i}\n\n"));
        }
        text.push_str(&para);
        text.push_str("\n\n");
    }
    println!("[bench] chars = {}", text.chars().count());

    let t = Instant::now();
    let blocks = crate::ast::parse(&text);
    println!(
        "[bench] parse = {:?}, blocks = {}",
        t.elapsed(),
        blocks.len()
    );

    let mut doc = Document::new(&text, None);
    let t = Instant::now();
    let _ = doc.stats();
    println!("[bench] stats = {:?}", t.elapsed());
    let t = Instant::now();
    let _ = doc.outline();
    println!("[bench] outline = {:?}", t.elapsed());

    let t = Instant::now();
    doc.edit_block(5, "改了一下的段落");
    println!("[bench] edit_block = {:?}", t.elapsed());

    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let mut images = ImageCache::new();
    let mut actions: Vec<crate::render::Action> = Vec::new();
    let mut state = EditorState::default();
    let outline = doc.outline();
    for i in 0..4 {
        let t = Instant::now();
        frame(1000.0, |ui| {
            let ctx = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &ctx,
                &mut images,
                None,
                &outline,
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
        println!("[bench] editor frame {i} = {:?}", t.elapsed());
    }
    // 视口只有 300pt 时，只要渲染少数几个块，用来对比每个块的成本。
    for i in 0..3 {
        let t = Instant::now();
        frame_sized(1000.0, 300.0, |ui| {
            let ctx = ui.ctx().clone();
            let _ = show_editor(
                ui,
                &mut doc,
                &mut state,
                &theme,
                &ctx,
                &mut images,
                None,
                &outline,
                false,
                false,
                false,
                false,
                1,
                &mut actions,
            );
        });
        println!("[bench] small-viewport frame {i} = {:?}", t.elapsed());
    }
}
