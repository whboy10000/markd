//! 应用生命周期：构造、配置同步、自动保存与退出前保存。

use super::*;

impl MarkdApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        // 首次启动（还没有配置文件）：不显示文件/大纲侧栏，直接进整个编辑区。
        let first_run = !Config::dir().join("config.json").is_file();
        let mut config = Config::load();
        if first_run {
            config.show_sidebar = false;
            config.show_outline = false;
            config.show_toolbar = false;
        }
        // 「跟随系统」时先按系统档解析：egui 的系统主题要等首个 RawInput 才送到，
        // 这里拿不到深色就当浅色，第一帧的检测会立刻纠正。
        let start_dark = matches!(cc.egui_ctx.system_theme(), Some(egui::Theme::Dark));
        let mut theme = Theme::get_with_overrides(
            Self::resolved_theme(&config, start_dark),
            &config.color_overrides,
        );
        theme.body = config.font_size;
        theme.mono = (config.font_size * 0.84).max(11.0);
        theme.line_height = config.line_height.clamp(1.0, 2.4);
        theme.apply(&cc.egui_ctx);
        // 放宽「点击」判定：鼠标/触控板轻微抖动（默认 6px）、按得稍久（默认 0.8s）
        // 都会被当成拖动，表现就是工具条按钮「点一次没反应、要点两次」。
        cc.egui_ctx.memory_mut(|m| {
            let o = &mut m.options.input_options;
            o.max_click_dist = 10.0;
            o.max_click_duration = 1.5;
        });
        // macOS 原生系统菜单栏（屏幕最左上角），事件通过队列转发。
        let menu_queue = crate::native_menu::install(&cc.egui_ctx);

        let initial = initial.filter(|p| p.is_file());
        // 没有指定要打开的文件时先亮首页（欢迎页 + 最近打开），像 VS Code 那样。
        let start_home = initial.is_none();
        // 没有命令行参数时，按设置恢复上次打开的文件
        let initial = initial.or_else(|| {
            if config.restore_last_file {
                config
                    .last_file
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_file())
            } else {
                None
            }
        });
        let start_file = initial.clone();
        // 首次启动直接给一个空白文档，不再停在首页欢迎页。
        let open_blank = first_run && start_file.is_none();
        let (doc, root) = if let Some(p) = initial {
            let t = std::time::Instant::now();
            let text = workspace::read(&p).unwrap_or_else(|_| WELCOME.to_string());
            crate::trace_open!("read_ms", t.elapsed());
            let r = p.parent().map(|x| x.to_path_buf());
            let t = std::time::Instant::now();
            let d = Document::open(text, Some(p));
            crate::trace_open!("parse_ms", t.elapsed());
            (d, r)
        } else {
            let r = config
                .workspace
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .or_else(|| std::env::current_dir().ok())
                // Finder / Dock 启动时 cwd 是根目录，不能拿来当工作区：
                // 建树与链接图会遍历整块磁盘，启动直接卡死。根目录没有父目录。
                .filter(|p| p.parent().is_some());
            (Document::new(WELCOME, None), r)
        };

        let mut expanded = HashSet::new();
        if let Some(r) = &root {
            expanded.insert(r.clone());
        }
        let t = std::time::Instant::now();
        let tree = root
            .as_ref()
            .map(|r| workspace::scan_tree(r, &expanded))
            .unwrap_or_default();
        crate::trace_open!("scan_ms", t.elapsed());
        let t = std::time::Instant::now();
        let outline = doc.outline();
        crate::trace_open!("outline_ms", t.elapsed());
        // 链接图要遍历整个工作区目录，桌面上目录一多就要好几秒。
        // 它只有「双向链接关系图」用得到，所以启动时不算，点开面板再后台算。
        let link_docs = HashMap::new();
        // 终端默认在工作区目录下执行命令。
        let terminal = Terminal::new(
            root.as_ref()
                .cloned()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
        );
        let workspace_input = root
            .as_ref()
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_default();

        let t = std::time::Instant::now();
        let stats_cache = doc.stats();
        crate::trace_open!("stats_ms", t.elapsed());
        let writing = WritingStats::new(stats_cache.words);
        let mut app = Self {
            stats: writing,
            stats_cache,
            stats_at: std::time::Instant::now(),
            doc,
            config,
            theme,
            editor: EditorState::default(),
            images: ImageCache::new(),
            icons: crate::icons::IconCache::default(),
            root,
            tree,
            expanded,
            outline,
            side_tab: SideTab::Files,
            outline_folded: HashSet::new(),
            menu: None,
            workspace_input,
            home_search: String::new(),
            home_sort: 0,
            palette: Palette::default(),
            search: SearchPanel::default(),
            find: FindPanel::default(),
            graph: GraphPanel::default(),
            snapshots: SnapshotStore::load(),
            status: String::new(),
            toast: None,
            about: false,
            table_dialog: false,
            table_rows: 3,
            table_cols: 3,
            link_dialog: LinkDialog::default(),
            video_dialog: VideoDialog::default(),
            attachment_dialog: AttachmentDialog::default(),
            html_dialog: HtmlDialog::default(),
            file_downloads: Vec::new(),
            pandoc: pandoc_export::PandocExport::new(),
            link_docs,
            link_rx: None,
            last_autosave: now_secs(),
            exit_dialog: false,
            code_delete_confirm: None,
            table_delete_confirm: None,
            image_uploads: Vec::new(),
            force_quit: false,
            theme_dirty: false,
            system_dark: start_dark,
            menu_guard: false,
            workspace_dialog: false,
            quick_cache_dir: String::new(),
            quick_cache: Vec::new(),
            menu_queue,
            terminal,
            settings: Default::default(),
            home: start_home,
            toolbar_tab: ToolbarTab::Start,
            toolbar_collapsed: false,
            // 启动时带了文件就把该文件做成第一个标签，否则只显示首页、没有标签。
            tabs: start_file
                .map(|p| Tab {
                    uid: 1,
                    path: Some(p),
                    seq: 0,
                    state: None,
                })
                .into_iter()
                .collect(),
            active: 0,
            untitled_seq: 0,
            next_uid: 1,
            frame_count: 0,
            ctx_menu: None,
            file_clipboard: None,
            rename_dialog: None,
            delete_confirm: None,
            new_dialog: None,
        };
        if open_blank {
            app.new_document();
            app.status.clear();
        }
        app
    }

    /// 按当前配置（主题类型 + 用户颜色覆盖 + 字号）重建主题并标脏。
    /// 所有改主题的入口统一走这里，避免漏掉颜色覆盖。
    pub(super) fn rebuild_theme(&mut self) {
        self.theme = Theme::get_with_overrides(
            Self::resolved_theme(&self.config, self.system_dark),
            &self.config.color_overrides,
        );
        self.theme.body = self.config.font_size;
        self.theme.mono = (self.config.font_size * 0.84).max(11.0);
        self.theme.line_height = self.config.line_height.clamp(1.0, 2.4);
        self.theme_dirty = true;
    }

    /// 「跟随系统」开启时，配色取系统那一档（系统给不到深色就按浅色），
    /// 否则用配置里的手动档位。返回的档位只会是浅色/深色，主题配色表按它查。
    fn resolved_theme(config: &Config, system_dark: bool) -> ThemeKind {
        if config.theme_follow_system && system_dark {
            ThemeKind::Dark
        } else {
            config.theme
        }
    }

    /// 把配置里的外观项应用到主题/缩放/编辑器。
    pub(super) fn apply_config(&mut self, ctx: &egui::Context) {
        self.rebuild_theme();
        ctx.set_zoom_factor(self.config.ui_scale.clamp(0.8, 1.6));
        self.editor.tab_size = self.config.tab_size.max(1);
        self.editor.auto_pair = self.config.auto_pair;
    }

    pub(super) fn on_changed(&mut self) {
        self.outline = self.doc.outline();
        // stats() 要扫全文，连续输入时按 200ms 节流，避免每个按键都全量统计一遍。
        let now = std::time::Instant::now();
        if now.duration_since(self.stats_at).as_millis() >= 200 {
            self.refresh_stats();
        }
    }

    /// 立即刷新统计（打开文档时用，不受节流影响）。
    pub(super) fn refresh_stats(&mut self) {
        self.stats_cache = self.doc.stats();
        self.stats_at = std::time::Instant::now();
        self.stats.tick(self.stats_cache.words);
    }

    /// 定时自动保存：已落盘的文档按间隔直接写回，不需要再按 Ctrl/Cmd+S。
    /// 未命名文档（没有路径）保持脏状态，等用户按保存时再选路径。
    pub(super) fn auto_save(&mut self) {
        let n = now_secs();
        if !autosave_due(n, self.last_autosave, self.config.auto_save_interval) {
            return;
        }
        self.last_autosave = n;
        let enabled = self.config.auto_save;
        let mut saved = false;
        if should_autosave(enabled, self.doc.dirty, self.doc.path.is_some())
            && self.doc.save().is_ok()
        {
            saved = true;
        }
        // 非活动标签的脏状态存在 state 里，同样按间隔写回。
        for t in self.tabs.iter_mut() {
            if let Some(st) = t.state.as_mut()
                && should_autosave(enabled, st.doc.dirty, st.doc.path.is_some())
                && st.doc.save().is_ok()
            {
                saved = true;
            }
        }
        if saved {
            self.status = "已自动保存".to_string();
        }
    }

    /// 有未保存修改的标签下标。
    pub(super) fn unsaved_tabs(&self) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&i| self.tab_dirty(i))
            .collect()
    }

    /// 未保存文档的显示名：已命名的用文件名，未命名的用「未命名-N.md」。
    pub(super) fn unsaved_label(&self, i: usize) -> String {
        match self.tabs.get(i) {
            Some(t) => match &t.path {
                Some(p) => p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string()),
                None => format!("未命名-{}.md", t.seq.max(1)),
            },
            None => String::new(),
        }
    }

    /// 退出前把有修改的标签全部写盘；未命名文档先问保存路径。
    /// 返回 false 表示用户取消了路径选择或写入失败，应当留在应用里。
    pub(super) fn save_all_for_exit(&mut self) -> bool {
        if self.tabs.is_empty() {
            return true;
        }
        // 活动标签的状态先换回 tabs，随后统一遍历。
        self.stash_active();
        let active = self.active;
        // 第一遍只问路径：rfd 需要 &self，不能和 &mut tabs 同时借。
        let mut paths: Vec<Option<PathBuf>> = self.tabs.iter().map(|t| t.path.clone()).collect();
        let mut cancelled = false;
        for (i, slot) in paths.iter_mut().enumerate() {
            let dirty = self.tabs[i].state.as_ref().is_some_and(|s| s.doc.dirty);
            if !dirty || slot.is_some() {
                continue;
            }
            match self.pick_save_path(i) {
                Some(p) => *slot = Some(p),
                None => {
                    cancelled = true;
                    break;
                }
            }
        }
        // 第二遍统一写盘。写出哪几个文件由 write_dirty_tabs 返回，用于更新最近打开。
        let written = match cancelled {
            true => {
                self.status = "已取消保存".to_string();
                None
            }
            false => match write_dirty_tabs(&mut self.tabs, &paths) {
                Ok(list) => Some(list),
                Err(e) => {
                    self.status = format!("保存失败: {}", e);
                    None
                }
            },
        };
        if let Some(list) = &written {
            for p in list {
                self.config.last_file = Some(p.to_string_lossy().to_string());
                self.push_recent(p);
            }
        }
        if active < self.tabs.len() {
            self.load_tab(active);
        }
        written.is_some()
    }

    /// 给未命名标签挑保存路径（退出确认用）。
    pub(super) fn pick_save_path(&self, i: usize) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_title("保存未保存的文档")
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(format!("{}.md", self.tab_label(i)));
        if let Some(d) = self.root.clone().filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(d);
        }
        dialog.save_file().map(with_md_ext)
    }

    /// 关闭窗口前的「有未保存文件」确认弹窗。
    pub(super) fn exit_confirm(&mut self, ctx: &egui::Context) {
        let names: Vec<String> = self
            .unsaved_tabs()
            .iter()
            .map(|&i| self.unsaved_label(i))
            .collect();
        let mut open = true;
        let mut save = false;
        let mut discard = false;
        let mut cancel = false;
        egui::Window::new("有未保存的修改")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("以下文件还没有保存到磁盘：");
                for n in &names {
                    ui.label(format!("· {}", n));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("保存并退出").clicked() {
                        save = true;
                    }
                    if ui.button("不保存").clicked() {
                        discard = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
        if save {
            if self.save_all_for_exit() {
                self.exit_dialog = false;
                self.force_quit = true;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                // 取消了路径选择或写入失败：收起弹窗，留在应用里。
                self.exit_dialog = false;
            }
        } else if discard {
            self.exit_dialog = false;
            self.force_quit = true;
            self.config.save();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if cancel || !open {
            self.exit_dialog = false;
        }
    }
}
