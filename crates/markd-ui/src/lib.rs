//! egui 界面层。
//!
//! 这里用 `pub use` 把 `markd-core` / `markd-platform` 的模块挂到本 crate 根，
//! 因此各模块内仍可沿用 `crate::ast`、`crate::theme`、`crate::native_menu` 等路径，
//! 拆分 workspace 时不需要改动每个文件的引用。

pub use markd_core::{ast, command, document, export, theme, workspace};
pub use markd_platform::native_menu;

pub mod app;
pub mod editor;
pub mod features;
pub mod render;
pub mod settings;
pub mod sidebar;
pub mod terminal;
