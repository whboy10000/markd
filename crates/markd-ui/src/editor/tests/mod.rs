//! 编辑器测试，按主题拆分到子模块。

use self::support::*;
use super::*;

mod bench;
mod drag_matrix;
mod commands;
mod editing;
mod focus;
mod folds;
mod html_embed;
mod layout;
mod line_break;
mod probes;
mod selection;
mod selection_text;
mod stress;
mod support;
mod quote_bar;
mod scrollbar;
mod tables;
mod table_writeback;
mod typing;
mod viewport;
