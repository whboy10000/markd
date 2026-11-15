//! 平台相关集成：原生菜单栏、系统字体。
//! 本层只依赖 `markd-core`，不依赖 UI 层，避免 crate 间循环依赖。

pub mod fonts;
pub mod native_menu;
