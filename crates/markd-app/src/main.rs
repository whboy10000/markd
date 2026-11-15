//! Markd —— 纯 Rust Markdown 编辑器入口。

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1).map(PathBuf::from);
    #[allow(unused_mut)]
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 600.0])
        .with_min_inner_size([720.0, 480.0])
        .with_title("Markd");
    // 让窗口内容延伸到标题栏下方，菜单栏这一行即充当自定义标题栏，
    // 系统红绿灯浮在最左侧、快捷按钮紧随其右。
    #[cfg(target_os = "macos")]
    {
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Markd",
        options,
        Box::new(move |cc| {
            markd_platform::fonts::install(&cc.egui_ctx);
            Ok(Box::new(markd_ui::app::MarkdApp::new(cc, initial.clone())) as Box<dyn eframe::App>)
        }),
    )
}
