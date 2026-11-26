//! Author: aurelianus <aurelianus@126.com>

//! egui 界面层。
//!
//! 这里用 `pub use` 把 `markd-core` / `markd-platform` 的模块挂到本 crate 根，
//! 因此各模块内仍可沿用 `crate::ast`、`crate::theme`、`crate::native_menu` 等路径，
//! 拆分 workspace 时不需要改动每个文件的引用。

pub use markd_core::{ast, command, document, export, image_host, theme, video, workspace};
pub use markd_platform::native_menu;

pub mod app;
pub(crate) mod clipboard;
pub mod editor;
pub mod features;
pub mod find;
pub mod icons;
pub mod image_insert;
pub mod render;
pub mod settings;
pub mod sidebar;
pub mod terminal;

/// 打开文档的分阶段耗时探针，`MARKD_TRACE_OPEN=1` 时打到 stderr。
/// 用来定位「大文档开起来卡在哪一步」，平时只有一次环境变量判断的开销。
#[macro_export]
macro_rules! trace_open {
    ($tag:literal, $elapsed:expr) => {
        if std::env::var_os("MARKD_TRACE_OPEN").is_some() {
            eprintln!("OPEN {} {:.1}ms", $tag, $elapsed.as_secs_f64() * 1000.0);
        }
    };
}
