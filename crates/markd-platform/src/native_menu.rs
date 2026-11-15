//! macOS 原生系统菜单栏：菜单显示在屏幕最左上角，应用获得焦点时可见。
//! 菜单项点击事件通过队列转发给 `MarkdApp`，复用与窗口菜单相同的 `Command`。
//! 非 macOS 平台为空实现，仍使用窗口内的菜单栏。

use markd_core::command::Command;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub type CommandQueue = Arc<Mutex<VecDeque<Command>>>;

#[cfg(target_os = "macos")]
pub fn install(ctx: &egui::Context) -> CommandQueue {
    use muda::{Menu, MenuEvent, MenuId, MenuItem, Submenu};
    use std::collections::HashMap;

    fn mk(map: &mut HashMap<String, Command>, id: &str, text: &str, cmd: Command) -> MenuItem {
        map.insert(id.to_string(), cmd);
        MenuItem::with_id(MenuId::new(id), text, true, None)
    }

    let queue: CommandQueue = Arc::new(Mutex::new(VecDeque::new()));
    let mut map: HashMap<String, Command> = HashMap::new();

    // macOS 把第一个子菜单作为「应用菜单」，显示在屏幕最左上角。
    let m_about = mk(&mut map, "about", "关于 Markd", Command::About);
    let m_settings = mk(&mut map, "settings", "偏好设置…", Command::Settings);
    let m_quit = mk(&mut map, "quit", "退出 Markd", Command::Quit);
    let app_menu =
        Submenu::with_items("Markd", true, &[&m_about, &m_settings, &m_quit]).unwrap();

    let m_new = mk(&mut map, "new", "新建", Command::New);
    let m_open_ws = mk(&mut map, "open_ws", "打开工作区…", Command::OpenWorkspace);
    let m_open_file = mk(&mut map, "open_file", "打开 MD 文件…", Command::OpenFileDialog);
    let m_save = mk(&mut map, "save", "保存", Command::Save);
    let m_save_as = mk(&mut map, "save_as", "另存为…", Command::SaveAs);
    let m_export = mk(&mut map, "export", "导出 HTML", Command::ExportHtml);
    let m_snapshot = mk(&mut map, "snapshot", "版本快照 · 时间机器", Command::Snapshot);
    let file_menu = Submenu::with_items(
        "文件",
        true,
        &[&m_new, &m_open_ws, &m_open_file, &m_save, &m_save_as, &m_export, &m_snapshot],
    )
    .unwrap();

    let m_undo = mk(&mut map, "undo", "撤销", Command::Undo);
    let m_redo = mk(&mut map, "redo", "重做", Command::Redo);
    let m_bold = mk(&mut map, "bold", "加粗", Command::Bold);
    let m_italic = mk(&mut map, "italic", "斜体", Command::Italic);
    let m_link = mk(&mut map, "link", "插入链接", Command::Link);
    let m_code = mk(&mut map, "code", "行内代码", Command::Code);
    let m_h1 = mk(&mut map, "h1", "一级标题", Command::H1);
    let m_h2 = mk(&mut map, "h2", "二级标题", Command::H2);
    let m_h3 = mk(&mut map, "h3", "三级标题", Command::H3);
    let m_table = mk(&mut map, "table", "插入表格", Command::Table);
    let m_math = mk(&mut map, "math", "插入数学公式", Command::Math);
    let m_toc = mk(&mut map, "toc", "插入目录 [TOC]", Command::Toc);
    let edit_menu = Submenu::with_items(
        "编辑",
        true,
        &[&m_undo, &m_redo, &m_bold, &m_italic, &m_link, &m_code, &m_h1, &m_h2, &m_h3, &m_table, &m_math, &m_toc],
    )
    .unwrap();

    let m_source = mk(&mut map, "source", "源码模式", Command::ToggleSource);
    let m_focus = mk(&mut map, "focus", "专注模式", Command::ToggleFocus);
    let m_typewriter = mk(&mut map, "typewriter", "打字机模式", Command::ToggleTypewriter);
    let m_sidebar = mk(&mut map, "sidebar", "显示/隐藏侧边栏", Command::ToggleSidebar);
    let m_outline = mk(&mut map, "outline", "显示/隐藏大纲", Command::ToggleOutline);
    let m_header = mk(&mut map, "header", "显示/隐藏侧栏标签栏", Command::ToggleSidebarHeader);
    let m_wsbar = mk(&mut map, "wsbar", "显示/隐藏工作区栏", Command::ToggleWorkspaceBar);
    let m_search = mk(&mut map, "search", "工作区全文搜索", Command::Search);
    let m_graph = mk(&mut map, "graph", "双向链接关系图", Command::Graph);
    let m_stats = mk(&mut map, "stats", "写作统计", Command::Stats);
    let m_terminal = mk(&mut map, "terminal", "切换终端面板", Command::ToggleTerminal);
    let view_menu = Submenu::with_items(
        "视图",
        true,
        &[&m_source, &m_focus, &m_typewriter, &m_sidebar, &m_outline, &m_header, &m_wsbar,
          &m_terminal, &m_search, &m_graph, &m_stats],
    )
    .unwrap();

    let m_theme = mk(&mut map, "theme", "切换下一主题", Command::ThemeNext);
    let theme_menu = Submenu::with_items("主题", true, &[&m_theme]).unwrap();

    let m_shortcuts = mk(&mut map, "shortcuts", "快捷键一览", Command::Shortcuts);
    let help_menu = Submenu::with_items("帮助", true, &[&m_shortcuts]).unwrap();

    let menu = Menu::with_items(&[
        &app_menu, &file_menu, &edit_menu, &view_menu, &theme_menu, &help_menu,
    ])
    .unwrap();
    menu.init_for_nsapp();
    // NSMenu 需常驻到应用退出，避免 Rust 侧析构后菜单消失。
    let _ = Box::leak(Box::new(menu));

    let q = queue.clone();
    let ctx2 = ctx.clone();
    std::thread::spawn(move || {
        let rx = MenuEvent::receiver();
        while let Ok(ev) = rx.recv() {
            if let Some(cmd) = map.get(ev.id.0.as_str()) {
                if let Ok(mut v) = q.lock() {
                    v.push_back(cmd.clone());
                }
            }
            ctx2.request_repaint();
        }
    });

    queue
}

#[cfg(not(target_os = "macos"))]
pub fn install(_ctx: &egui::Context) -> CommandQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}
