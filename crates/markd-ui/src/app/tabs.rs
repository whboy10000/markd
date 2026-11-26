//! 标签页管理、打开/保存文档与链接跳转。

use super::*;

impl MarkdApp {
    /// 大纲显隐开关：正在显示时切回文件树（隐藏），否则显示大纲面板。
    pub(super) fn toggle_outline(&mut self) {
        let showing = self.config.show_sidebar && self.side_tab == SideTab::Outline;
        let (show_outline, tab) = outline_toggle(showing);
        self.config.show_outline = show_outline;
        self.side_tab = tab;
        if !showing {
            self.config.show_sidebar = true;
        }
        self.config.save();
    }

    /// 打开快速打开面板，路径默认填当前工作区。
    pub(super) fn open_quick_palette(&mut self) {
        self.palette.open_with(PaletteMode::Files);
        if self.palette.dir.is_empty() {
            self.palette.dir = self
                .root
                .as_ref()
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_default();
        }
    }

    /// 生成新的标签编号。
    pub(super) fn next_tab_uid(&mut self) -> u64 {
        self.next_uid += 1;
        self.next_uid
    }

    /// 把当前活动标签的状态存回 tabs[active]，切标签/关标签前调用。
    /// 存完后 self.doc 等字段是空占位值，必须紧跟 load_tab。
    pub(super) fn stash_active(&mut self) {
        if self.active >= self.tabs.len() {
            return;
        }
        let tab = &mut self.tabs[self.active];
        tab.path = self.doc.path.clone();
        tab.state = Some(TabState {
            doc: std::mem::replace(&mut self.doc, Document::new("", None)),
            editor: std::mem::take(&mut self.editor),
            outline: std::mem::take(&mut self.outline),
            stats: std::mem::take(&mut self.stats_cache),
        });
    }

    /// 把 tabs[i] 暂存的状态换进活动字段。
    pub(super) fn load_tab(&mut self, i: usize) {
        let Some(st) = self.tabs.get_mut(i).and_then(|t| t.state.take()) else {
            return;
        };
        self.active = i;
        self.doc = st.doc;
        self.editor = st.editor;
        self.outline = st.outline;
        self.stats_cache = st.stats;
        self.stats_at = std::time::Instant::now();
        self.stats.tick(self.stats_cache.words);
        self.config.last_file = self
            .doc
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
    }

    /// 标签标题：已保存的用文件名，未保存的用「未命名-N」。
    pub(super) fn tab_label(&self, i: usize) -> String {
        let Some(t) = self.tabs.get(i) else {
            return String::new();
        };
        let path = t
            .path
            .clone()
            .or_else(|| t.state.as_ref().and_then(|s| s.doc.path.clone()));
        match path {
            Some(p) => p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "未命名".to_string()),
            None => format!("未命名-{}", t.seq.max(1)),
        }
    }

    /// 标题栏标签的显示名：未命名文档统一叫「新标签页」，已保存的沿用文件名。
    pub(super) fn tab_display_label(&self, i: usize) -> String {
        let Some(t) = self.tabs.get(i) else {
            return String::new();
        };
        let named = t.path.is_some() || t.state.as_ref().is_some_and(|s| s.doc.path.is_some());
        if named {
            self.tab_label(i)
        } else {
            "新标签页".to_string()
        }
    }

    /// 标签是否有未保存的修改（活动标签的状态在 self.doc 里）。
    pub(super) fn tab_dirty(&self, i: usize) -> bool {
        if i == self.active && i < self.tabs.len() {
            return self.doc.dirty;
        }
        self.tabs
            .get(i)
            .and_then(|t| t.state.as_ref())
            .map(|s| s.doc.dirty)
            .unwrap_or(false)
    }

    /// 切换到第 i 个标签；首页随之退出。
    pub(super) fn switch_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.home = false;
        if i == self.active {
            return;
        }
        self.stash_active();
        self.load_tab(i);
    }

    /// 关闭第 i 个标签；关掉活动标签后由右侧（没有则左侧）接手，全关则回首页。
    pub(super) fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let was_active = i == self.active;
        let label = self.tab_label(i);
        let dirty = self.tab_dirty(i);
        if was_active && self.tabs.len() == 1 {
            self.tabs.clear();
            self.doc = Document::new("", None);
            self.editor = EditorState::default();
            self.outline.clear();
            self.stats_cache = Default::default();
            self.active = 0;
            self.home = true;
            self.status = "已关闭全部标签".to_string();
            return;
        }
        if was_active {
            self.stash_active();
        }
        self.tabs.remove(i);
        if was_active {
            let next = i.min(self.tabs.len() - 1);
            self.load_tab(next);
        } else if i < self.active {
            self.active -= 1;
        }
        self.status = if dirty {
            format!("已关闭 {}（未保存的修改已丢弃）", label)
        } else {
            format!("已关闭 {}", label)
        };
    }

    /// 打开文件：已经开着的标签直接切过去，否则新建一个标签。
    pub(super) fn open_file(&mut self, path: PathBuf) {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path.as_deref() == Some(path.as_path()))
        {
            self.switch_tab(i);
            self.status = format!(
                "已切换到 {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            return;
        }
        match workspace::read(&path) {
            Ok(text) => {
                self.home = false;
                self.push_recent(&path);
                self.stash_active();
                let doc = Document::open(text, Some(path.clone()));
                let mut editor = EditorState {
                    focus_req: Some(0),
                    ..Default::default()
                };
                // 还原上次保存的折叠状态。
                editor.folded = editor::load_folds(&path, &doc);
                let outline = doc.outline();
                let uid = self.next_tab_uid();
                self.tabs.push(Tab {
                    uid,
                    path: Some(path.clone()),
                    seq: 0,
                    state: Some(TabState {
                        doc,
                        editor,
                        outline,
                        stats: Default::default(),
                    }),
                });
                let i = self.tabs.len() - 1;
                self.load_tab(i);
                self.refresh_stats();
                self.refresh_tree();
                self.config.last_file = Some(path.to_string_lossy().to_string());
                self.config.save();
            }
            Err(e) => self.status = format!("打开失败: {}", e),
        }
    }

    /// 新建标签页：先在内存里建空白文档，Ctrl/Cmd+S 时才问保存路径。
    pub(super) fn new_document(&mut self) {
        self.untitled_seq += 1;
        self.stash_active();
        let editor = EditorState {
            focus_req: Some(0),
            ..Default::default()
        };
        let uid = self.next_tab_uid();
        self.tabs.push(Tab {
            uid,
            path: None,
            seq: self.untitled_seq,
            state: Some(TabState {
                doc: Document::new("", None),
                editor,
                outline: Vec::new(),
                stats: Default::default(),
            }),
        });
        let i = self.tabs.len() - 1;
        self.load_tab(i);
        self.home = false;
        self.status = "已新建 Markdown 文档，Ctrl/Cmd+S 选择保存路径".to_string();
    }

    /// 保存：已有路径的文档直接原地写回（像 VS Code 一样，不再弹框）；
    /// 新建的未命名文档才需要先选保存路径。
    pub(super) fn save(&mut self) {
        if self.tabs.is_empty() {
            self.status = "没有可保存的文档".to_string();
            return;
        }
        if let Some(p) = self.doc.path.clone() {
            self.save_to(p);
            return;
        }
        self.save_as();
    }

    /// 另存为：总是弹保存位置选择框（原生对话框），选好路径再写盘。
    pub(super) fn save_as(&mut self) {
        if self.tabs.is_empty() {
            self.status = "没有可保存的文档".to_string();
            return;
        }
        let current = self
            .doc
            .path
            .clone()
            .or_else(|| self.tabs.get(self.active).and_then(|t| t.path.clone()));
        let default_name = current
            .as_ref()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| format!("{}.md", self.tab_label(self.active)));
        let start_dir = current
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| self.root.clone());
        let mut dialog = rfd::FileDialog::new()
            .set_title("选择保存路径")
            .add_filter("Markdown", &["md", "markdown"])
            .set_file_name(default_name);
        if let Some(d) = start_dir.filter(|p| p.is_dir()) {
            dialog = dialog.set_directory(d);
        }
        match dialog.save_file() {
            Some(p) => self.save_to(p),
            None => self.status = "已取消保存".to_string(),
        }
    }

    /// 把当前文档写到指定路径，并同步标签、最近打开、文件树。
    pub(super) fn save_to(&mut self, path: PathBuf) {
        let path = with_md_ext(path);
        self.doc.path = Some(path.clone());
        match self.doc.save() {
            Ok(()) => {
                if let Some(t) = self.tabs.get_mut(self.active) {
                    t.path = Some(path.clone());
                }
                self.snapshots.capture(&self.doc);
                self.config.last_file = Some(path.to_string_lossy().to_string());
                self.push_recent(&path);
                self.refresh_tree();
                // 折叠状态随文档路径一起落盘（覆盖「首次另存为」后才确定路径的情况）。
                editor::save_folds(&path, &self.doc, &self.editor.folded);
                self.status = "已保存".to_string();
            }
            Err(e) => self.status = format!("保存失败: {}", e),
        }
    }

    pub(super) fn set_workspace(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.status = format!("不是有效目录: {}", path.display());
            return;
        }
        self.root = Some(path.clone());
        self.expanded.insert(path.clone());
        self.config.workspace = Some(path.to_string_lossy().to_string());
        self.config.save();
        self.workspace_input = path.to_string_lossy().to_string();
        self.terminal.set_cwd(path.clone());
        self.refresh_tree();
        self.status = format!("工作区: {}", path.display());
    }

    pub(super) fn export_html(&mut self) {
        match crate::export::export_html(
            self.doc.path.as_deref(),
            &self.doc.source(),
            &self.doc.title(),
            self.theme.kind.is_dark(),
        ) {
            Ok(_) => self.status = "已导出 HTML".to_string(),
            Err(e) => self.status = format!("导出失败: {}", e),
        }
    }

    pub(super) fn next_theme(&mut self) {
        let all = ThemeKind::ALL;
        let i = all
            .iter()
            .position(|t| *t == self.config.theme)
            .unwrap_or(0);
        self.config.set_theme(all[(i + 1) % all.len()]);
        self.rebuild_theme();
        self.config.save();
        self.status = format!("主题: {}", self.theme.kind.name());
    }

    /// 打开「插入链接」弹窗：先把当前选区取出来存好，确认时再写回 Markdown。
    pub(super) fn open_link_dialog(&mut self) {
        let sel = editor::selection_text(&self.doc, &self.editor);
        let (text, target) = match sel {
            Some((idx, a, z, text)) => (text, Some((idx, a, z))),
            None => (String::new(), None),
        };
        self.link_dialog = LinkDialog {
            open: true,
            url: String::new(),
            text,
            target,
            focus_url: true,
        };
    }

    /// 链接弹窗点「插入」：按记住的选区把 [文字](URL) 写回文档。
    pub(super) fn confirm_link_dialog(&mut self) {
        let url = self.link_dialog.url.trim().to_string();
        if url.is_empty() {
            self.status = "请先粘贴或输入链接地址".to_string();
            self.link_dialog.focus_url = true;
            return;
        }
        let label = self.link_dialog.text.clone();
        let target = self.link_dialog.target;
        if editor::insert_link(&mut self.doc, &mut self.editor, target, &label, &url) {
            self.on_changed();
            self.status = format!("已插入链接 {}", url);
            self.link_dialog.open = false;
        }
    }

    /// 打开「插入视频」弹窗：链接要能解析成 B 站视频才插，所以先让用户粘贴。
    pub(super) fn open_video_dialog(&mut self) {
        self.video_dialog = VideoDialog {
            open: true,
            url: String::new(),
            focus_url: true,
        };
    }

    /// 视频弹窗点「插入」：解析链接 → 插 ```video 围栏 → 卡片立刻以渲染态显示。
    pub(super) fn confirm_video_dialog(&mut self) {
        let raw = self.video_dialog.url.trim().to_string();
        if raw.is_empty() {
            self.status = "请先粘贴 B 站视频链接".to_string();
            self.video_dialog.focus_url = true;
            return;
        }
        let Some(src) = crate::video::parse_video(&raw) else {
            // 认不出就不动文档：插一个坏围栏只会得到一张永远取不到封面的卡片。
            self.status = "认不出 B 站视频链接（支持 BV/av/ep/ss 号或完整分享链接）".to_string();
            return;
        };
        editor::insert_rendered_block(
            &mut self.doc,
            &mut self.editor,
            &crate::video::video_block(&src),
        );
        self.on_changed();
        self.status = format!("已插入视频 {}（导出 HTML 后可在页面内播放）", src.id());
        self.video_dialog.open = false;
    }

    pub(super) fn open_link(&mut self, url: &str) {
        if url.starts_with("http://") || url.starts_with("https://") {
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", url])
                .spawn();
            self.status = format!("已打开链接 {}", url);
        } else if let Some(p) =
            workspace::resolve(self.doc.path.as_ref().and_then(|x| x.parent()), url)
        {
            if p.is_file() {
                if p.extension()
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
                {
                    self.open_file(p);
                } else {
                    self.status = format!("附件: {}", p.display());
                }
            } else {
                self.status = format!("目标不存在: {}", p.display());
            }
        }
    }
}
