//! Markd —— 纯 Rust Markdown 编辑器入口。

use std::path::PathBuf;

/// 把图标编进二进制：Windows / Linux 的任务栏图标直接用它，
/// macOS 的 Dock 图标仍只认 .app 包里的 AppIcon.icns（系统不支持从可执行文件取图标）。
fn app_icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icon.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let img = img.to_rgba8();
            let (width, height) = img.dimensions();
            egui::IconData {
                rgba: img.into_raw(),
                width,
                height,
            }
        }
        Err(_) => egui::IconData {
            rgba: Vec::new(),
            width: 0,
            height: 0,
        },
    }
}

fn main() -> eframe::Result<()> {
    // 打包成 .app 后没有终端，panic 会直接闪退、看不出原因。
    // 挂个钩子把信息与调用栈写进 ~/.markd/panic.log，方便事后排查。
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let dir = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".markd");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("panic.log"))
        {
            let _ = writeln!(f, "---- {info}");
            let _ = writeln!(f, "{}", std::backtrace::Backtrace::force_capture());
        }
    }));
    let initial = std::env::args().nth(1).map(PathBuf::from);
    #[allow(unused_mut)]
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 600.0])
        .with_min_inner_size([720.0, 480.0])
        .with_icon(app_icon())
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
