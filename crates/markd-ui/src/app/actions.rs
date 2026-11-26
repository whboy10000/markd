//! 命令执行、快捷键与拖放文件处理。

use super::*;

impl MarkdApp {
    pub(super) fn execute(&mut self, cmd: Command, ctx: &egui::Context) {
        match cmd {
            // 直接新建，不弹窗；命名留到保存时
            Command::New => self.new_document(),
            Command::QuickOpen => self.open_quick_palette(),
            Command::CommandPalette => self.palette.open_with(PaletteMode::Commands),
            Command::OpenWorkspace => {
                self.config.show_sidebar = true;
                self.side_tab = SideTab::Files;
                self.workspace_dialog = true;
                self.status = "输入工作区文件夹路径".to_string();
            }
            Command::Save => self.save(),
            Command::SaveAs => self.save_as(),
            Command::ExportHtml => self.export_html(),
            Command::Undo => {
                if self.doc.undo() {
                    self.on_changed();
                }
            }
            Command::Redo => {
                if self.doc.redo() {
                    self.on_changed();
                }
            }
            // 源码 / 专注 / 打字机三种模式互斥：打开一个会关掉另外两个
            Command::ToggleSource => {
                let on = !self.config.source_mode;
                self.config.source_mode = on;
                if on {
                    self.config.focus_mode = false;
                    self.config.typewriter = false;
                }
                self.config.save();
            }
            Command::ToggleFocus => {
                let on = !self.config.focus_mode;
                self.config.focus_mode = on;
                if on {
                    self.config.source_mode = false;
                    self.config.typewriter = false;
                }
                self.config.save();
            }
            Command::ToggleTypewriter => {
                let on = !self.config.typewriter;
                self.config.typewriter = on;
                if on {
                    self.config.source_mode = false;
                    self.config.focus_mode = false;
                }
                self.config.save();
            }
            Command::ToggleLineNumbers => {
                self.config.show_line_numbers = !self.config.show_line_numbers;
                self.config.save();
            }
            Command::ToggleSidebar => {
                self.config.show_sidebar = !self.config.show_sidebar;
                self.config.save();
            }
            Command::ToggleOutline => self.toggle_outline(),
            Command::ToggleAutoSave => {
                self.config.auto_save = !self.config.auto_save;
                self.config.save();
            }
            Command::ToggleSidebarHeader => {
                self.config.show_sidebar_header = !self.config.show_sidebar_header;
                self.config.save();
                self.status = if self.config.show_sidebar_header {
                    "侧栏标签栏已显示".to_string()
                } else {
                    "侧栏标签栏已隐藏".to_string()
                };
            }
            Command::ToggleWorkspaceBar => {
                self.config.show_workspace_bar = !self.config.show_workspace_bar;
                self.config.save();
            }
            Command::ToggleTerminal => self.terminal.toggle(),
            Command::OpenFileDialog => {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("打开 Markdown 文件")
                    .add_filter("Markdown", &["md", "markdown"]);
                let start = if self.palette.dir.trim().is_empty() {
                    self.root.clone()
                } else {
                    Some(PathBuf::from(self.palette.dir.trim()))
                };
                if let Some(d) = start.filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(d);
                }
                if let Some(p) = dialog.pick_file() {
                    self.open_file(p);
                }
            }
            Command::ThemeNext => self.next_theme(),
            Command::Paragraph => editor::make_paragraph(&mut self.doc, &mut self.editor),
            Command::H1 => editor::apply_prefix(&mut self.doc, &mut self.editor, "# "),
            Command::H2 => editor::apply_prefix(&mut self.doc, &mut self.editor, "## "),
            Command::H3 => editor::apply_prefix(&mut self.doc, &mut self.editor, "### "),
            Command::H4 => editor::apply_prefix(&mut self.doc, &mut self.editor, "#### "),
            Command::H5 => editor::apply_prefix(&mut self.doc, &mut self.editor, "##### "),
            Command::H6 => editor::apply_prefix(&mut self.doc, &mut self.editor, "###### "),
            // 加粗用标准 Markdown 的 **：即时渲染态由 inline_job 隐藏标记、显示粗体，
            // 源码态则原样显示 **加粗的部分**（WYSIWYG 与源码两态一致）。
            Command::Bold => editor::wrap(&mut self.doc, &mut self.editor, "**", "**", true),
            Command::Italic => editor::wrap(&mut self.doc, &mut self.editor, "<em>", "</em>", true),
            Command::Strike => editor::wrap(&mut self.doc, &mut self.editor, "~~", "~~", true),
            Command::Underline => {
                editor::wrap(&mut self.doc, &mut self.editor, "<u>", "</u>", true)
            }
            Command::Highlight => editor::wrap(&mut self.doc, &mut self.editor, "==", "==", true),
            Command::Code => editor::wrap(&mut self.doc, &mut self.editor, "`", "`", true),
            Command::Link => self.open_link_dialog(),
            Command::Image => {
                // 从磁盘选图片：按设置复制到文档旁的资源目录（默认 imgs/），
                // 文档里写相对路径；未保存的文档回退到当前工作目录。
                let base = crate::image_insert::doc_dir(self.doc.path.as_deref());
                let mut dialog = rfd::FileDialog::new()
                    .set_title("插入图片")
                    .add_filter("图片", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"]);
                if base.is_dir() {
                    dialog = dialog.set_directory(&base);
                }
                if let Some(p) = dialog.pick_file() {
                    self.insert_image_path(&p, ctx);
                }
            }
            // 视频链接要先认得出是 B 站才插，所以走弹窗而不是直接插模板。
            Command::Video => self.open_video_dialog(),
            Command::UploadImages => self.upload_local_images(ctx),
            Command::ClearFormat => {
                // 与工具条橡皮擦同一动作：当前块去掉块级/行内标记变回普通段落。
                if editor::clear_format(&mut self.doc, &mut self.editor) {
                    self.on_changed();
                    self.status = "已清除格式".to_string();
                } else {
                    self.status = "这一块不是文本段落，没有可清除的格式".to_string();
                }
            }
            Command::Quote => editor::apply_prefix(&mut self.doc, &mut self.editor, "> "),
            Command::CodeBlock => editor::insert_code_block(&mut self.doc, &mut self.editor),
            Command::CodeGroup => editor::insert_code_group(&mut self.doc, &mut self.editor),
            Command::Bullet => editor::apply_prefix(&mut self.doc, &mut self.editor, "- "),
            Command::Ordered => editor::apply_prefix(&mut self.doc, &mut self.editor, "1. "),
            // 任务列表：把当前块/选中文字转成任务项（保留正文、不新增块），
            // 与标题/列表按钮一致走 apply_prefix；这样回车继承逻辑才能衔接。
            Command::Task => editor::apply_prefix(&mut self.doc, &mut self.editor, "- [ ] "),
            Command::Rule => editor::insert_template(&mut self.doc, &mut self.editor, "---"),
            // 和 Typora 一样：先问几行几列，再插入
            Command::Table => self.table_dialog = true,
            Command::Math => {
                editor::insert_template(&mut self.doc, &mut self.editor, "$$\nE = mc^2\n$$")
            }
            Command::MathCode => editor::insert_template(
                &mut self.doc,
                &mut self.editor,
                "```math\n\\frac{a}{b} = c\n```",
            ),
            Command::InlineMath => {
                editor::insert_template(&mut self.doc, &mut self.editor, "$E = mc^2$")
            }
            // 原生菜单的「插入流程图 / Mermaid 图」复用下拉里的标准模板，
            // 两处入口插入的示例保持一致。
            Command::Flowchart => {
                editor::insert_template(&mut self.doc, &mut self.editor, mermaid_template("流程图"))
            }
            Command::Mermaid => {
                editor::insert_template(&mut self.doc, &mut self.editor, mermaid_template("饼图"))
            }
            Command::Toc => editor::insert_template(&mut self.doc, &mut self.editor, "[TOC]"),
            Command::Footnote => {
                editor::insert_template(&mut self.doc, &mut self.editor, "[^1]: 脚注内容")
            }
            Command::FrontMatter => {
                editor::insert_template(&mut self.doc, &mut self.editor, "---\ntitle: 未命名\n---")
            }
            Command::Snapshot => {
                self.snapshots.capture(&self.doc);
                self.snapshots.open = true;
            }
            Command::Search => self.search.open = true,
            Command::Find => self.find.open_for(false),
            Command::Graph => {
                self.spawn_link_graph(ctx);
                self.graph.open = true;
            }
            Command::Stats => self.stats.open = true,
            Command::Shortcuts => self.settings.open_shortcuts(ctx),
            Command::Settings => self.settings.open(ctx),
            Command::About => self.about = true,
            Command::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            Command::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            Command::Quit => {
                // 不再静默保存：由 close_requested 的统一拦截决定要不要弹确认框。
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Command::OpenFile(p) => self.open_file(p),
        }
        self.on_changed();
    }

    /// 快捷键尽量与 Typora 保持一致，方便无损切换。
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mac = cfg!(target_os = "macos");
        let (cmd, shift, alt, ctrl) = ctx.input(|i| {
            (
                i.modifiers.command,
                i.modifiers.shift,
                i.modifiers.alt,
                i.modifiers.ctrl,
            )
        });
        let key = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        #[cfg(test)]
        eprintln!(
            "[shortcuts] cmd={cmd} ctrl={ctrl} alt={alt} shift={shift} C_pressed={} terminal_focus={}",
            key(egui::Key::C),
            self.terminal.has_focus()
        );

        // ---- 剪贴板图片 ----
        // Cmd/Ctrl+V 时若剪贴板里只有图片（截图、从画图/Finder 复制的图片），
        // egui-winit 发的是 `Event::PasteImage` 而不是 `Paste(文本)`，
        // 这里把它落盘到文档旁的资源目录并插入 Markdown 图片链接。
        if !self.terminal.has_focus() {
            let pasted = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::PasteImage(img) => Some(img.clone()),
                    _ => None,
                })
            });
            if let Some(img) = pasted {
                self.insert_clipboard_image(&img, ctx);
                return;
            }
        }

        // ---- 跨块选区的复制 / 剪切 ----
        // egui-winit 会把 Cmd/Ctrl+C、Cmd/Ctrl+X 直接转成虚拟事件
        // `Event::Copy` / `Event::Cut`，不再产生 Key 事件——只按 Key 判断
        // 永远收不到（这正是之前拖选后复制/剪切没反应的原因）。
        // 终端有焦点时让给终端（它有自己的一套复制粘贴）。
        if !self.terminal.has_focus() {
            let (copy, cut) = ctx.input(|i| {
                (
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                )
            });
            if copy && self.copy_cross_sel(ctx) {
                return;
            }
            if cut && self.cut_cross_sel(ctx) {
                return;
            }
            // 没有跨块选区时，活动块内的文本选区也走带格式复制/剪切。
            // 成功后会从输入里移除对应 Event，阻止 TextEdit 用纯文本覆盖剪贴板。
            if copy && self.copy_single_sel_rich(ctx) {
                return;
            }
            if cut && self.cut_single_sel_rich(ctx) {
                return;
            }
        }

        // ---- 无修饰键：Typora 的 F8 专注 / F9 打字机 / F1 快捷键速查 ----
        if key(egui::Key::F8) {
            self.execute(Command::ToggleFocus, ctx);
            return;
        }
        if key(egui::Key::F9) {
            self.execute(Command::ToggleTypewriter, ctx);
            return;
        }
        if key(egui::Key::F1) {
            self.execute(Command::Shortcuts, ctx);
            return;
        }
        // Typora: Ctrl+Shift+` 删除线（macOS 上 Ctrl 与 Cmd 是分开的）
        if mac && ctrl && shift && key(egui::Key::Backtick) {
            self.execute(Command::Strike, ctx);
            return;
        }
        // macOS 上 Ctrl 与 Cmd 是分开的两个键，用户习惯按 Ctrl+S，这里额外认一下。
        if mac && ctrl && !shift && key(egui::Key::S) {
            self.execute(Command::Save, ctx);
            return;
        }
        // Ctrl+C / Ctrl+X 也认（与 Ctrl+S 同理）。终端有焦点时不抢：
        // Ctrl+C 在终端里是「中断正在运行的程序」，抢走会打断命令。
        if mac && ctrl && !shift && !alt && !self.terminal.has_focus() {
            if key(egui::Key::C) && self.copy_cross_sel(ctx) {
                return;
            }
            if key(egui::Key::X) && self.cut_cross_sel(ctx) {
                return;
            }
        }
        // 文档内查找 / 替换：Ctrl+F、Ctrl+H（macOS 上 Cmd+H 是系统「隐藏」，只能用 Ctrl+H）。
        if mac && ctrl && !shift && !alt && key(egui::Key::F) {
            self.find.open_for(false);
            return;
        }
        if mac && ctrl && !shift && !alt && key(egui::Key::H) {
            self.find.open_for(true);
            return;
        }

        if !cmd {
            return;
        }
        // Cmd/Ctrl+J 始终用于显隐终端；终端获得焦点后其余快捷键交给 PTY，
        // 避免 Ctrl+P / Ctrl+B 之类的按键被应用抢走而打断交互式命令输入。
        if key(egui::Key::J) {
            self.execute(Command::ToggleTerminal, ctx);
            return;
        }
        if self.terminal.has_focus() {
            return;
        }

        // ---- Typora 的 Command+Option 组合 ----
        if alt {
            if key(egui::Key::Q) {
                self.execute(Command::Quote, ctx);
            } else if key(egui::Key::C) {
                self.execute(Command::CodeBlock, ctx);
            } else if key(egui::Key::B) {
                self.execute(Command::Math, ctx);
            } else if key(egui::Key::T) {
                self.execute(Command::Table, ctx);
            } else if key(egui::Key::O) {
                self.execute(Command::Ordered, ctx);
            } else if key(egui::Key::U) {
                self.execute(Command::Bullet, ctx);
            } else if key(egui::Key::F) {
                // macOS 上 Cmd+H 被系统占用来隐藏窗口，替换用 Cmd+Option+F。
                self.find.open_for(true);
            } else if key(egui::Key::X) {
                self.execute(Command::Task, ctx);
            } else if key(egui::Key::Minus) {
                self.execute(Command::Rule, ctx);
            }
            return;
        }

        // ---- Typora 的 Command+Control 组合（仅 macOS，其它平台 Cmd==Ctrl） ----
        if mac && ctrl {
            if key(egui::Key::I) {
                self.execute(Command::Image, ctx);
            } else if key(egui::Key::Num1) {
                self.config.show_sidebar = true;
                self.config.show_outline = true;
                self.side_tab = SideTab::Outline;
                self.config.save();
            }
            return;
        }

        // ---- 纯 Command 组合 ----
        if key(egui::Key::C) && cmd {
            // 跨块选区（渲染态拖选）的复制：无编辑态焦点，不会与 TextEdit 冲突。
            // 没有跨块选区时不拦截，单块选区交给 TextEdit 原生复制。
            if self.copy_cross_sel(ctx) {
                return;
            }
        } else if key(egui::Key::X) && cmd {
            // 剪切跨块选区：复制到剪贴板后删除选中内容。
            if self.cut_cross_sel(ctx) {
                return;
            }
        } else if key(egui::Key::S) && shift {
            self.execute(Command::SaveAs, ctx);
        } else if key(egui::Key::S) {
            self.execute(Command::Save, ctx);
        } else if key(egui::Key::O) && shift {
            self.open_quick_palette();
        } else if key(egui::Key::O) {
            self.execute(Command::OpenFileDialog, ctx);
        } else if key(egui::Key::P) && shift {
            self.open_quick_palette();
        } else if key(egui::Key::P) {
            self.palette.open_with(PaletteMode::Commands);
        } else if key(egui::Key::N) {
            self.execute(Command::New, ctx);
        } else if key(egui::Key::B) {
            self.execute(Command::Bold, ctx);
        } else if key(egui::Key::I) {
            self.execute(Command::Italic, ctx);
        } else if key(egui::Key::U) {
            self.execute(Command::Underline, ctx);
        } else if key(egui::Key::K) {
            self.execute(Command::Link, ctx);
        } else if key(egui::Key::Backslash) {
            self.execute(Command::ClearFormat, ctx);
        } else if key(egui::Key::C) {
            // 带格式复制：跨块选区优先，其次是活动块内的单块选区。
            if self.copy_cross_sel(ctx) || self.copy_single_sel_rich(ctx) {
                return;
            }
        } else if key(egui::Key::Backtick) && shift {
            self.execute(Command::Code, ctx);
        } else if key(egui::Key::Slash) {
            // Typora: Cmd+/ 切换源码模式
            self.execute(Command::ToggleSource, ctx);
        } else if key(egui::Key::E) && shift {
            self.execute(Command::ExportHtml, ctx);
        } else if key(egui::Key::E) {
            self.execute(Command::ToggleSource, ctx);
        } else if key(egui::Key::F) && shift {
            self.execute(Command::ToggleFocus, ctx);
        } else if key(egui::Key::F) {
            // 文档内查找（VS Code / Typora 习惯）；工作区全文搜索仍在「搜索」按钮里。
            self.find.open_for(false);
        } else if key(egui::Key::H) && !shift && !mac {
            self.find.open_for(true);
        } else if key(egui::Key::L) && shift {
            self.execute(Command::ToggleSidebar, ctx);
        } else if key(egui::Key::H) && shift {
            self.execute(Command::Highlight, ctx);
        } else if key(egui::Key::Z) && shift {
            self.execute(Command::Redo, ctx);
        } else if key(egui::Key::Z) {
            self.execute(Command::Undo, ctx);
        } else if key(egui::Key::Comma) {
            self.execute(Command::Settings, ctx);
        } else if key(egui::Key::Num0) {
            self.execute(Command::Paragraph, ctx);
        } else if key(egui::Key::Num1) {
            self.execute(Command::H1, ctx);
        } else if key(egui::Key::Num2) {
            self.execute(Command::H2, ctx);
        } else if key(egui::Key::Num3) {
            self.execute(Command::H3, ctx);
        } else if key(egui::Key::Num4) {
            self.execute(Command::H4, ctx);
        } else if key(egui::Key::Num5) {
            self.execute(Command::H5, ctx);
        } else if key(egui::Key::Num6) {
            self.execute(Command::H6, ctx);
        }
    }

    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let drops: Vec<_> = ctx.input(|i| i.raw.dropped_files.clone());
        for d in drops {
            let path = d.path().to_path_buf();
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                self.open_file(path);
            } else if workspace::is_image(&path.to_string_lossy()) {
                let base = self.doc.path.as_ref().and_then(|p| p.parent());
                let rel = base
                    .and_then(|b| path.strip_prefix(b).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                editor::insert_template(&mut self.doc, &mut self.editor, &format!("![]({})", rel));
                self.on_changed();
            }
        }
    }
}

/// 按标签取 Mermaid 标准模板：原生菜单入口与工具条下拉共用同一份模板。
fn mermaid_template(label: &str) -> &'static str {
    super::menu::MERMAID_TEMPLATES
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, t)| *t)
        .unwrap_or("```mermaid\nflowchart LR\n    A[开始] --> B[结束]\n```")
}
