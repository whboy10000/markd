//! 终端屏幕模型与按键映射的单元测试。

use super::*;

fn row_text(screen: &Screen, r: usize) -> String {
    screen.grid[r]
        .iter()
        .map(|c| c.ch)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn new_screen(rows: usize, cols: usize) -> (Screen, vte::Parser) {
    (Screen::new(rows, cols), vte::Parser::new())
}

#[test]
fn prints_wraps_and_breaks_line() {
    let (mut s, mut p) = new_screen(3, 5);
    p.advance(&mut s, b"abcdefg");
    assert_eq!(row_text(&s, 0), "abcde");
    assert_eq!(row_text(&s, 1), "fg");
}

#[test]
fn carriage_return_overwrites_line() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, b"hello\rHI");
    assert_eq!(row_text(&s, 0), "HIllo");
}

#[test]
fn erase_line_and_display() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, b"abcdef\x1b[2K");
    assert_eq!(row_text(&s, 0), "");
    p.advance(&mut s, b"one\r\ntwo\r\nthree\x1b[2J");
    assert_eq!(row_text(&s, 0), "");
    assert_eq!(row_text(&s, 1), "");
}

#[test]
fn cursor_positioning_and_sgr() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, b"\x1b[2;3HZ");
    assert_eq!(s.grid[1][2].ch, 'Z');
    p.advance(&mut s, b"\x1b[31mR");
    assert_eq!(s.grid[1][3].fg, Some(ansi_color(1)));
    p.advance(&mut s, b"\x1b[38;2;1;2;3mT");
    assert_eq!(s.grid[1][4].fg, Some(Color32::from_rgb(1, 2, 3)));
    p.advance(&mut s, b"\x1b[0mN");
    assert_eq!(s.grid[1][5].fg, None);
}

#[test]
fn alt_screen_roundtrip() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, b"AAA");
    p.advance(&mut s, b"\x1b[?1049h");
    assert_eq!(row_text(&s, 0), "");
    p.advance(&mut s, b"BBB");
    assert_eq!(row_text(&s, 0), "BBB");
    p.advance(&mut s, b"\x1b[?1049l");
    assert_eq!(row_text(&s, 0), "AAA");
}

#[test]
fn scrolling_pushes_into_scrollback() {
    let (mut s, mut p) = new_screen(2, 4);
    p.advance(&mut s, b"1\r\n2\r\n3");
    assert_eq!(s.scrollback.len(), 1);
    assert_eq!(row_text(&s, 0), "2");
    assert_eq!(row_text(&s, 1), "3");
}

#[test]
fn bracketed_paste_mode_toggles() {
    let (mut s, mut p) = new_screen(2, 10);
    assert!(!s.bracketed_paste);
    p.advance(&mut s, b"\x1b[?2004h");
    assert!(s.bracketed_paste);
    p.advance(&mut s, b"\x1b[?2004l");
    assert!(!s.bracketed_paste);
}

#[test]
fn wide_chars_take_two_columns() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, "中文ab".as_bytes());
    assert_eq!(s.grid[0][0].ch, '中');
    assert_eq!(s.grid[0][1].ch, '\0');
    assert_eq!(s.grid[0][2].ch, '文');
    assert_eq!(s.grid[0][3].ch, '\0');
    assert_eq!(s.grid[0][4].ch, 'a');
    assert_eq!(s.grid[0][5].ch, 'b');
    assert_eq!(s.cur_c, 6);
}

#[test]
fn wide_char_wraps_at_last_column() {
    let (mut s, mut p) = new_screen(3, 5);
    p.advance(&mut s, "abcd中".as_bytes());
    assert_eq!(row_text(&s, 0), "abcd");
    assert_eq!(s.grid[1][0].ch, '中');
    assert_eq!(s.grid[1][1].ch, '\0');
    assert_eq!(s.cur_c, 2);
}

#[test]
fn overwriting_wide_char_clears_halves() {
    let (mut s, mut p) = new_screen(3, 10);
    p.advance(&mut s, "中文\rX".as_bytes());
    assert_eq!(s.grid[0][0].ch, 'X');
    assert_eq!(s.grid[0][1].ch, ' ');
    assert_eq!(s.grid[0][2].ch, '文');
    assert_eq!(s.grid[0][3].ch, '\0');
}

#[test]
fn maps_special_keys_to_pty_bytes() {
    let none = egui::Modifiers::NONE;
    assert_eq!(
        key_bytes(egui::Key::Enter, none, false).unwrap(),
        vec![b'\r']
    );
    assert_eq!(
        key_bytes(egui::Key::Backspace, none, false).unwrap(),
        vec![0x7f]
    );
    assert_eq!(key_bytes(egui::Key::Tab, none, false).unwrap(), vec![b'\t']);
    assert_eq!(
        key_bytes(egui::Key::ArrowUp, none, false).unwrap(),
        b"\x1b[A".to_vec()
    );
    assert_eq!(
        key_bytes(egui::Key::ArrowUp, none, true).unwrap(),
        b"\x1bOA".to_vec()
    );
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    assert_eq!(key_bytes(egui::Key::C, ctrl, false).unwrap(), vec![0x03]);
    assert_eq!(key_bytes(egui::Key::D, ctrl, false).unwrap(), vec![0x04]);
    // Ctrl+V 留给粘贴事件，不能被当成控制字符写进 PTY。
    assert!(key_bytes(egui::Key::V, ctrl, false).is_none());
    assert!(key_bytes(egui::Key::Space, none, false).is_none());
}

fn run_term_frame(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    term: &mut Terminal,
    theme: &Theme,
    screen: egui::Rect,
) {
    let input = egui::RawInput {
        screen_rect: Some(screen),
        events,
        ..Default::default()
    };
    let c = ctx.clone();
    let mut out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show(root, |ui| term.ui(ui, &c, theme));
    });
    out.textures_delta.clear();
}

/// 点击终端主体必须把键盘焦点从编辑区抢过来，否则输入仍然进入文档。
#[test]
fn clicking_body_takes_focus() {
    let ctx = egui::Context::default();
    let theme = Theme::get(crate::theme::ThemeKind::Light);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 320.0));
    let mut term = Terminal::new(std::env::temp_dir());
    term.open = true;
    term.focus_req = false;

    run_term_frame(&ctx, vec![], &mut term, &theme, screen);
    assert!(!term.has_focus(), "初始不应有焦点");

    let p = egui::pos2(450.0, 200.0);
    run_term_frame(
        &ctx,
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut term,
        &theme,
        screen,
    );
    run_term_frame(
        &ctx,
        vec![egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut term,
        &theme,
        screen,
    );
    assert!(term.has_focus(), "点击终端主体后应获得键盘焦点");
}

/// 端到端：真的起一个 PTY shell，写入命令，再从解析结果里找输出。
/// Windows 默认 shell 是 cmd，命令语法不同，这里只在类 Unix 上跑。
#[cfg(not(windows))]
#[test]
fn pty_shell_executes_command() {
    let ctx = egui::Context::default();
    let mut term = Terminal::new(std::env::temp_dir());
    term.sessions[0].start(&ctx);
    assert!(
        term.sessions[0].master.is_some(),
        "PTY 未启动：{:?}",
        term.sessions[0].error
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    term.sessions[0].write_bytes(b"echo $((123456*2))\r");
    let mut text = String::new();
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        term.poll();
        text = term.sessions[0].plain_text();
        if text.contains("246912") {
            break;
        }
    }
    term.sessions[0].write_bytes(b"exit\r");
    assert!(text.contains("246912"), "PTY 没有回显命令结果：\n{text}");
}

/// 回归：中文输出必须按宽字符落格，复制出的纯文本不能含占位符或丢字。
#[cfg(not(windows))]
#[test]
fn pty_echoes_chinese() {
    let ctx = egui::Context::default();
    let mut term = Terminal::new(std::env::temp_dir());
    term.sessions[0].start(&ctx);
    assert!(
        term.sessions[0].master.is_some(),
        "PTY 未启动：{:?}",
        term.sessions[0].error
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    term.sessions[0].write_bytes("echo 你好终端\r".as_bytes());
    let mut text = String::new();
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        term.poll();
        text = term.sessions[0].plain_text();
        if text.matches("你好终端").count() >= 2 {
            break;
        }
    }
    term.sessions[0].write_bytes(b"exit\r");
    assert!(
        text.matches("你好终端").count() >= 2,
        "PTY 中文回显/输出不完整：\n{text}"
    );
    assert!(!text.contains('\0'), "纯文本不应含宽字符占位符");
}
