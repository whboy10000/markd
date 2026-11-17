//! 行内文本排版：样式 span、链接、选区高亮与双击选词。

use super::*;

mod job;
mod link;
mod widget;

pub(crate) use self::widget::*;
pub use self::{job::*, link::*};
