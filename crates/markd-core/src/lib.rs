//! Author: aurelianus <aurelianus@126.com>

//! 数据与逻辑层：Markdown 解析、文档模型、主题、配置、导出与命令定义。
//! 该层不依赖 eframe/窗口，可被 UI、平台集成等上层 crate 复用。

pub mod ast;
pub mod attachment;
pub mod command;
pub mod document;
pub mod export;
pub mod image_host;
pub mod theme;
pub mod video;
pub mod workspace;
