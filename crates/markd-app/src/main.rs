//! Author: aurelianus <aurelianus@126.com>

//! Markd —— 纯 Rust Markdown 编辑器入口。

// Windows 发布版声明为 GUI 子系统：不分配控制台，双击/快捷方式启动只有应用窗口，
// 也不会因为控制台被关掉而杀掉应用。debug 构建仍保留终端，方便看日志。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
        // 渲染后端固定为 glow（OpenGL，走 GPU）。本 crate 关掉 eframe 默认特性只留 glow，
        // 这里再写一遍：万一以后 wgpu 被打开，也不要把公式那套自绘 Mesh 换到别的后端上。
        // GPU 上下文由 eframe 以「优先硬件加速」创建（glow_options 默认值）——不用 Required，
        // 否则没有 GL 加速的 Linux（llvmpipe / 虚拟机）会直接起不来。
        renderer: eframe::Renderer::Glow,
        // 公式与图表把字形轮廓细分成 epaint Mesh 直接上 GPU：Mesh 顶点既不 feathering
        // 也不做像素吸附（epaint 原样透传），是这套 UI 里唯一依赖 MSAA 抗锯齿的内容。
        // 默认关闭时小字号公式边缘全是锯齿，所以开 4x。
        // 注意 MSAA 作用于整个窗口帧缓冲，与 epaint 的 feathering 会叠加：公式的分数线
        // 只有 0.56 逻辑像素厚，若以矩形提交就会同时吃到 feathering 的±1px 渐隐和 MSAA
        // 的二次柔化，表现为向外突出的一圈半透明像素。render/math.rs 已在绘制期把这些
        // 线条转成对齐物理像素栅格的实心网格——边落在整像素上，MSAA 的覆盖率非 0 即 1，
        // 只剩一层抗锯齿。
        multisampling: 4,
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
