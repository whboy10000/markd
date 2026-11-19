//! Author: aurelianus <aurelianus@126.com>

//! 集成终端：基于 PTY 的交互式 shell（VS Code 风格）。
//!
//! `portable-pty` 启动一个持久 shell，`vte` 把它的 ANSI 字节流解析成字符网格，
//! 键盘输入直接写回 PTY。输出经 `mpsc` 回传，UI 帧 `poll()` 时更新，线程用
//! `ctx.request_repaint()` 唤醒界面。
//!
//! 子模块：`screen` 维护字符网格与 ANSI 解析，`session` 负责 PTY 会话与面板绘制，
//! `keys` 把按键映射成写入 PTY 的字节序列。

use crate::theme::Theme;
use egui::{self, Color32, FontFamily, FontId, RichText, ScrollArea, Sense};
use portable_pty::{Child, CommandBuilder, MasterPty, PtyPair, PtySize, native_pty_system};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

mod keys;
mod screen;
mod session;
#[cfg(test)]
mod tests;

use self::keys::*;
use self::screen::*;
pub use self::session::*;

const MAX_SCROLLBACK: usize = 400;
