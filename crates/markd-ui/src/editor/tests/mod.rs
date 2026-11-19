//! 编辑器测试，按主题拆分到子模块。

use self::support::*;
use super::*;

mod bench;
mod commands;
mod editing;
mod focus;
mod folds;
mod layout;
mod probes;
mod selection;
mod selection_text;
mod stress;
mod support;
mod tables;
mod typing;
mod viewport;
