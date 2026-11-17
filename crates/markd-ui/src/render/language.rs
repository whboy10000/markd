//! 代码块语言选择下拉（带搜索过滤）。

use super::*;

/// 代码组顶部的语言标签。选中的标签用正文色 + 强调色下划线，
/// 其余标签用弱化色；返回是否被点击。
pub fn code_group_tab(ui: &mut egui::Ui, theme: &Theme, active: bool, label: &str) -> bool {
    let text = if label.trim().is_empty() {
        "纯文本"
    } else {
        label
    };
    let color = if active { theme.text } else { theme.dim };
    let resp = ui
        .add(
            egui::Button::new(
                egui::RichText::new(text)
                    .size(theme.body * 0.9)
                    .color(color)
                    .strong(),
            )
            .frame(false)
            .min_size(egui::vec2(0.0, theme.body * 1.6)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if active {
        let r = resp.rect;
        ui.painter().line_segment(
            [
                Pos2::new(r.left() + 4.0, r.bottom() - 1.0),
                Pos2::new(r.right() - 4.0, r.bottom() - 1.0),
            ],
            Stroke::new(2.0, theme.accent),
        );
    }
    resp.clicked()
}

/// 代码组标题栏的「+ / −」按钮：新增一个语言标签 / 删掉当前标签。
/// 与「复制」「删除」按钮同款样式与字号。只剩一个标签时「−」不可点。
/// 返回 (是否新增, 是否删除)。
pub fn code_group_tab_buttons(ui: &mut egui::Ui, theme: &Theme, can_remove: bool) -> (bool, bool) {
    let size = theme.body * 0.85;
    // 「+ / −」只有一个字符，默认按钮会比「复制 / 删除」窄很多；
    // 按两个字宽 + 内边距给一个最小宽度，视觉大小与它们一致。
    let min_w = theme.body * 1.8 + 8.0;
    let add = ui
        .add(
            egui::Button::new(egui::RichText::new("+").size(size)).min_size(egui::vec2(min_w, 0.0)),
        )
        .on_hover_cursor(egui::CursorIcon::Default)
        .on_hover_text("新增一个语言标签")
        .clicked();
    let remove = ui
        .add_enabled(
            can_remove,
            egui::Button::new(egui::RichText::new("−").size(size)).min_size(egui::vec2(min_w, 0.0)),
        )
        .on_hover_cursor(egui::CursorIcon::Default)
        .on_hover_text("删除当前语言标签")
        .clicked();
    (add, remove)
}

/// 代码块可选语言，覆盖常见前端/后端/脚本/配置/标记语言。
/// 带搜索的语言选择下拉：顶部输入框实时过滤，语言按 A→Z 正序排列
/// （纯文本排最前）。返回用户选中的语言（与 current 不同时为 Some）。
#[allow(deprecated)]
pub fn language_combo(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
) -> Option<String> {
    let id = egui::Id::new(id_salt);
    let mut picked: Option<String> = None;
    let inner = egui::ComboBox::from_id_salt(id)
        .selected_text(if current.is_empty() {
            "纯文本".to_string()
        } else {
            current.to_string()
        })
        .width(118.0)
        // 默认 CloseOnClick 会在弹层内任意点击（包括点搜索框）时关闭整个下拉，
        // 表现就是「一点搜索框就消失、没法搜索」；改成只在外部点击时关闭，
        // 选中语言后再在 show_ui 返回之后手动收起。
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let filter_id = id.with("filter");
            let mut filter = ui
                .ctx()
                .memory(|m| m.data.get_temp::<String>(filter_id).unwrap_or_default());
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("搜索语言…")
                    .desired_width(110.0),
            );
            let f = filter.trim().to_lowercase();
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(filter_id, f.clone()));
            // A→Z 正序；"" (纯文本) 自然排最前
            let mut list = LANGUAGES.to_vec();
            list.sort_unstable();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for l in list {
                        if !f.is_empty() && !l.to_lowercase().contains(&f) {
                            continue;
                        }
                        let label = if l.is_empty() { "纯文本" } else { l };
                        if ui.selectable_label(current == l, label).clicked() {
                            picked = Some(l.to_string());
                        }
                    }
                });
        });
    // 选好了就收起下拉（CloseOnClickOutside 模式下不会自动关）。
    // 弹层 id 必须从 egui 自己返回的按钮响应派生（Popup::default_response_id）：
    // 之前手算 button_id.with("popup")，但 ComboBox 内部先把 id_salt 转成
    // IdSalt 再 make_persistent_id，与直接传 Id 的哈希路径不同，算出来的
    // id 和真实弹层对不上，close_id 是空操作——表现为选完语言下拉还挂在屏幕上。
    let popup_id = egui::Popup::default_response_id(&inner.response);
    if picked.is_some() {
        egui::Popup::close_id(ui.ctx(), popup_id);
    }
    #[cfg(test)]
    if let Ok(mut trace) = COMBO_TRACE.lock() {
        trace.push((
            inner.response.rect,
            popup_id,
            inner.response.clicked(),
            egui::Popup::is_id_open(ui.ctx(), popup_id),
            std::thread::current().id(),
        ));
    }
    picked
}

/// 测试支持：language_combo 每帧记录（按钮矩形, popup id, 本帧是否被点, 弹层是否开）。
/// 供真实帧测试定位点击坐标与断言弹层状态。仅测试构建存在。
#[cfg(test)]
pub(crate) static COMBO_TRACE: std::sync::Mutex<
    Vec<(egui::Rect, egui::Id, bool, bool, std::thread::ThreadId)>,
> = std::sync::Mutex::new(Vec::new());

pub const LANGUAGES: &[&str] = &[
    "",
    "json",
    "xml",
    "yaml",
    "toml",
    "ini",
    "csv",
    "rust",
    "c",
    "cpp",
    "csharp",
    "java",
    "kotlin",
    "scala",
    "swift",
    "go",
    "dart",
    "python",
    "javascript",
    "typescript",
    "jsx",
    "tsx",
    "vue",
    "svelte",
    "php",
    "ruby",
    "perl",
    "lua",
    "r",
    "julia",
    "matlab",
    "objective-c",
    "fsharp",
    "haskell",
    "ocaml",
    "clojure",
    "elixir",
    "erlang",
    "groovy",
    "lisp",
    "prolog",
    "fortran",
    "cobol",
    "sql",
    "plsql",
    "graphql",
    "prisma",
    "html",
    "css",
    "scss",
    "sass",
    "less",
    "stylus",
    "properties",
    "bash",
    "sh",
    "zsh",
    "fish",
    "powershell",
    "bat",
    "awk",
    "sed",
    "vim",
    "makefile",
    "dockerfile",
    "cmake",
    "nginx",
    "apache",
    "terraform",
    "hcl",
    "protobuf",
    "thrift",
    "markdown",
    "latex",
    "tex",
    "mermaid",
    "plantuml",
    "diff",
    "asm",
    "wasm",
    "solidity",
    "verilog",
    "vhdl",
    "text",
    "plaintext",
];
