//! Author: aurelianus <aurelianus@126.com>

//! macOS 原生系统菜单栏：菜单显示在屏幕最左上角，应用获得焦点时可见。
//! 菜单项点击事件通过队列转发给 `MarkdApp`，复用与窗口菜单相同的 `Command`。
//! 菜单分类对齐 Typora：Markd / 文件 / 编辑 / 段落 / 格式 / 视图 / 主题 / 窗口 / 帮助。
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
    let app_menu = Submenu::with_items("Markd", true, &[&m_about, &m_settings, &m_quit]).unwrap();

    // ---------------- 文件 ----------------
    let m_new = mk(&mut map, "new", "新建文档", Command::New);
    let m_quick_open = mk(&mut map, "quick_open", "快速打开…", Command::QuickOpen);
    let m_palette = mk(&mut map, "palette", "命令面板…", Command::CommandPalette);
    let m_open_file = mk(
        &mut map,
        "open_file",
        "打开 MD 文件…",
        Command::OpenFileDialog,
    );
    let m_open_ws = mk(&mut map, "open_ws", "打开工作区…", Command::OpenWorkspace);
    let m_save = mk(&mut map, "save", "保存", Command::Save);
    let m_save_as = mk(&mut map, "save_as", "另存为…", Command::SaveAs);
    let m_export = mk(&mut map, "export", "导出 HTML", Command::ExportHtml);
    let m_snapshot = mk(
        &mut map,
        "snapshot",
        "版本快照 · 时间机器",
        Command::Snapshot,
    );
    let m_quit2 = mk(&mut map, "quit_file", "退出", Command::Quit);
    let file_menu = Submenu::with_items(
        "文件",
        true,
        &[
            &m_new,
            &m_quick_open,
            &m_palette,
            &m_open_file,
            &m_open_ws,
            &m_save,
            &m_save_as,
            &m_export,
            &m_snapshot,
            &m_quit2,
        ],
    )
    .unwrap();

    // ---------------- 编辑 ----------------
    let m_undo = mk(&mut map, "undo", "撤销", Command::Undo);
    let m_redo = mk(&mut map, "redo", "重做", Command::Redo);
    let m_find = mk(&mut map, "find", "查找 / 替换…", Command::Find);
    let m_autosave = mk(
        &mut map,
        "autosave",
        "切换自动保存",
        Command::ToggleAutoSave,
    );
    let edit_menu =
        Submenu::with_items("编辑", true, &[&m_undo, &m_redo, &m_find, &m_autosave]).unwrap();

    // ---------------- 段落（块级结构） ----------------
    let m_para = mk(&mut map, "para", "段落", Command::Paragraph);
    let m_h1 = mk(&mut map, "h1", "一级标题", Command::H1);
    let m_h2 = mk(&mut map, "h2", "二级标题", Command::H2);
    let m_h3 = mk(&mut map, "h3", "三级标题", Command::H3);
    let m_h4 = mk(&mut map, "h4", "四级标题", Command::H4);
    let m_h5 = mk(&mut map, "h5", "五级标题", Command::H5);
    let m_h6 = mk(&mut map, "h6", "六级标题", Command::H6);
    let m_quote = mk(&mut map, "quote", "引用", Command::Quote);
    let m_bullet = mk(&mut map, "bullet", "无序列表", Command::Bullet);
    let m_ordered = mk(&mut map, "ordered", "有序列表", Command::Ordered);
    let m_task = mk(&mut map, "task", "任务列表", Command::Task);
    let m_table = mk(&mut map, "table", "插入表格", Command::Table);
    let m_rule = mk(&mut map, "rule", "插入分隔线", Command::Rule);
    let m_codeblock = mk(&mut map, "codeblock", "插入代码块", Command::CodeBlock);
    let m_codegroup = mk(
        &mut map,
        "codegroup",
        "插入代码组（多语言 Tab）",
        Command::CodeGroup,
    );
    let m_math = mk(&mut map, "math", "插入公式块", Command::Math);
    let m_inline_math = mk(&mut map, "inline_math", "插入行内公式", Command::InlineMath);
    let m_mermaid = mk(&mut map, "mermaid", "插入 Mermaid 图", Command::Mermaid);
    let m_flowchart = mk(&mut map, "flowchart", "插入流程图", Command::Flowchart);
    let m_toc = mk(&mut map, "toc", "插入目录 [TOC]", Command::Toc);
    let m_footnote = mk(&mut map, "footnote", "插入脚注", Command::Footnote);
    let m_frontmatter = mk(
        &mut map,
        "frontmatter",
        "插入 YAML Front Matter",
        Command::FrontMatter,
    );
    let paragraph_menu = Submenu::with_items(
        "段落",
        true,
        &[
            &m_para,
            &m_h1,
            &m_h2,
            &m_h3,
            &m_h4,
            &m_h5,
            &m_h6,
            &m_quote,
            &m_bullet,
            &m_ordered,
            &m_task,
            &m_table,
            &m_rule,
            &m_codeblock,
            &m_codegroup,
            &m_math,
            &m_inline_math,
            &m_mermaid,
            &m_flowchart,
            &m_toc,
            &m_footnote,
            &m_frontmatter,
        ],
    )
    .unwrap();

    // ---------------- 格式（行内样式） ----------------
    let m_bold = mk(&mut map, "bold", "加粗", Command::Bold);
    let m_italic = mk(&mut map, "italic", "斜体", Command::Italic);
    let m_underline = mk(&mut map, "underline", "下划线", Command::Underline);
    let m_strike = mk(&mut map, "strike", "删除线", Command::Strike);
    let m_highlight = mk(&mut map, "highlight", "高亮", Command::Highlight);
    let m_code = mk(&mut map, "code", "行内代码", Command::Code);
    let m_link = mk(&mut map, "link", "插入链接", Command::Link);
    let m_image = mk(&mut map, "image", "插入图片", Command::Image);
    let m_clear = mk(&mut map, "clear_format", "清除格式", Command::ClearFormat);
    let format_menu = Submenu::with_items(
        "格式",
        true,
        &[
            &m_bold,
            &m_italic,
            &m_underline,
            &m_strike,
            &m_highlight,
            &m_code,
            &m_link,
            &m_image,
            &m_clear,
        ],
    )
    .unwrap();

    // ---------------- 视图 ----------------
    let m_source = mk(&mut map, "source", "源码模式", Command::ToggleSource);
    let m_focus = mk(&mut map, "focus", "专注模式", Command::ToggleFocus);
    let m_typewriter = mk(
        &mut map,
        "typewriter",
        "打字机模式",
        Command::ToggleTypewriter,
    );
    let m_line_numbers = mk(
        &mut map,
        "line_numbers",
        "显示/隐藏行号",
        Command::ToggleLineNumbers,
    );
    let m_sidebar = mk(
        &mut map,
        "sidebar",
        "显示/隐藏侧边栏",
        Command::ToggleSidebar,
    );
    let m_outline = mk(&mut map, "outline", "显示/隐藏大纲", Command::ToggleOutline);
    let m_header = mk(
        &mut map,
        "header",
        "显示/隐藏侧栏标签栏",
        Command::ToggleSidebarHeader,
    );
    let m_wsbar = mk(
        &mut map,
        "wsbar",
        "显示/隐藏工作区栏",
        Command::ToggleWorkspaceBar,
    );
    let m_search = mk(&mut map, "search", "工作区全文搜索", Command::Search);
    let m_graph = mk(&mut map, "graph", "双向链接关系图", Command::Graph);
    let m_stats = mk(&mut map, "stats", "写作统计", Command::Stats);
    let m_terminal = mk(
        &mut map,
        "terminal",
        "切换终端面板",
        Command::ToggleTerminal,
    );
    let view_menu = Submenu::with_items(
        "视图",
        true,
        &[
            &m_source,
            &m_focus,
            &m_typewriter,
            &m_line_numbers,
            &m_sidebar,
            &m_outline,
            &m_header,
            &m_wsbar,
            &m_terminal,
            &m_search,
            &m_graph,
            &m_stats,
        ],
    )
    .unwrap();

    // ---------------- 主题 ----------------
    let m_theme = mk(&mut map, "theme", "切换下一主题", Command::ThemeNext);
    let theme_menu = Submenu::with_items("主题", true, &[&m_theme]).unwrap();

    // ---------------- 窗口 ----------------
    let m_minimize = mk(&mut map, "minimize", "最小化窗口", Command::Minimize);
    let m_maximize = mk(
        &mut map,
        "maximize",
        "最大化 / 还原窗口",
        Command::ToggleMaximize,
    );
    let window_menu = Submenu::with_items("窗口", true, &[&m_minimize, &m_maximize]).unwrap();

    // ---------------- 帮助 ----------------
    let m_shortcuts = mk(&mut map, "shortcuts", "快捷键一览", Command::Shortcuts);
    let m_about2 = mk(&mut map, "about_help", "关于 Markd", Command::About);
    let help_menu = Submenu::with_items("帮助", true, &[&m_shortcuts, &m_about2]).unwrap();

    let menu = Menu::with_items(&[
        &app_menu,
        &file_menu,
        &edit_menu,
        &paragraph_menu,
        &format_menu,
        &view_menu,
        &theme_menu,
        &window_menu,
        &help_menu,
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
