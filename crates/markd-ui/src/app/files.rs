//! 文件树与文件系统操作（新建、重命名、删除、粘贴、图片插入）。

use super::*;

impl MarkdApp {
    /// 把文件记进「最近打开」：最新在前、去重、最多 10 条。
    pub(super) fn push_recent(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        self.config.recent.retain(|x| x != &s);
        self.config.recent.insert(0, s);
        self.config.recent.truncate(10);
        self.config.save();
    }

    /// 插入磁盘上的图片：按设置复制到文档旁的资源目录（默认 imgs/），
    /// 再在光标所在块之后插入 Markdown 图片链接。
    ///
    /// 启用了图床时，先插入本地链接让图片立刻可见，同时后台上传；
    /// 上传完成后把链接替换成公网 URL（失败则保留本地图片并提示）。
    pub(super) fn insert_image_path(&mut self, src: &Path, ctx: &egui::Context) {
        let (copy, assets, relative) = (
            self.config.insert_image_to_assets,
            self.config.image_assets_dir.clone(),
            self.config.image_relative,
        );
        match crate::image_insert::from_file(src, self.doc.path.as_deref(), copy, &assets, relative)
        {
            Ok(img) => {
                editor::insert_image(&mut self.doc, &mut self.editor, &img.markdown);
                self.on_changed();
                self.status = format!("已插入图片：{}", src.display());
                self.maybe_upload_image(&img, ctx);
            }
            Err(e) => self.status = format!("插入图片失败：{e}"),
        }
    }

    /// 插入剪贴板图片：保存为 PNG 到文档旁的资源目录，再插入 Markdown 链接。
    pub(super) fn insert_clipboard_image(&mut self, img: &egui::ColorImage, ctx: &egui::Context) {
        let (assets, relative) = (
            self.config.image_assets_dir.clone(),
            self.config.image_relative,
        );
        match crate::image_insert::from_clipboard(
            img.size,
            &img.pixels,
            self.doc.path.as_deref(),
            &assets,
            relative,
        ) {
            Ok(img) => {
                editor::insert_image(&mut self.doc, &mut self.editor, &img.markdown);
                self.on_changed();
                self.status = "已插入剪贴板图片".to_string();
                self.maybe_upload_image(&img, ctx);
            }
            Err(e) => self.status = format!("插入剪贴板图片失败：{e}"),
        }
    }

    /// 图床启用时，把刚插入的图片交给后台线程上传。
    /// 上传在后台进行，UI 不卡顿；完成后由 [`Self::poll_image_uploads`] 替换链接。
    fn maybe_upload_image(
        &mut self,
        img: &crate::image_insert::InsertedImage,
        ctx: &egui::Context,
    ) {
        if !self.config.image_host.is_enabled() {
            return;
        }
        let cfg = crate::image_host::ImageHostConfig::from_config(&self.config);
        if let Err(e) = cfg.validate() {
            self.notify(ctx, format!("图床配置不完整：{e}（图片已保留本地）"));
            return;
        }
        let Some(path) = img.path.clone() else {
            return;
        };
        self.spawn_upload(cfg, img.url.clone(), img.alt.clone(), path, ctx);
    }

    /// 把当前文档里所有本地图片批量上传到图床（「插入 → 上传本地图片」）。
    pub(super) fn upload_local_images(&mut self, ctx: &egui::Context) {
        if !self.config.image_host.is_enabled() {
            self.notify(ctx, "请先在「设置 → 图像」里启用图床".to_string());
            return;
        }
        let cfg = crate::image_host::ImageHostConfig::from_config(&self.config);
        if let Err(e) = cfg.validate() {
            self.notify(ctx, format!("图床配置不完整：{e}"));
            return;
        }
        let base = self.doc.path.as_deref().and_then(|p| p.parent());
        let mut urls: Vec<String> = Vec::new();
        for b in &self.doc.blocks {
            for inl in crate::document::block_inlines(b) {
                collect_image_urls(inl, &mut urls);
            }
        }
        urls.sort();
        urls.dedup();
        let mut jobs: Vec<(String, PathBuf)> = Vec::new();
        for url in urls {
            if crate::image_host::is_remote(&url) {
                continue;
            }
            if let Some(p) = workspace::resolve(base, &url)
                && p.is_file()
            {
                jobs.push((url, p));
            }
        }
        if jobs.is_empty() {
            self.notify(ctx, "当前文档没有需要上传的本地图片".to_string());
            return;
        }
        let n = jobs.len();
        // 批量上传在同一个后台线程里顺序执行，并留一点间隔：
        // 图床 API 有频率限制，一次并发几十个请求容易被拒绝。
        let jobs: Vec<(String, String, PathBuf)> = jobs
            .into_iter()
            .map(|(url, path)| {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "image".to_string());
                (url, name, path)
            })
            .collect();
        self.spawn_bulk_upload(cfg, jobs, ctx);
        self.status = format!("正在上传 {n} 张图片到图床…");
    }

    fn spawn_bulk_upload(
        &mut self,
        cfg: crate::image_host::ImageHostConfig,
        jobs: Vec<(String, String, PathBuf)>,
        ctx: &egui::Context,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.image_uploads.push(rx);
        let tab_uid = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            for (old_url, name, path) in jobs {
                let result = std::fs::read(&path)
                    .map_err(|e| format!("读取图片失败：{e}"))
                    .and_then(|bytes| {
                        crate::image_host::upload(&cfg, &path.to_string_lossy(), &bytes)
                    });
                let _ = tx.send(ImageUpload {
                    tab_uid,
                    old_url,
                    name,
                    result,
                });
                ctx2.request_repaint();
                // 留一点间隔，避免触发图床的频率限制。
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
    }

    fn spawn_upload(
        &mut self,
        cfg: crate::image_host::ImageHostConfig,
        old_url: String,
        name: String,
        path: PathBuf,
        ctx: &egui::Context,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.image_uploads.push(rx);
        let tab_uid = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let result = std::fs::read(&path)
                .map_err(|e| format!("读取图片失败：{e}"))
                .and_then(|bytes| crate::image_host::upload(&cfg, &path.to_string_lossy(), &bytes));
            let _ = tx.send(ImageUpload {
                tab_uid,
                old_url,
                name,
                result,
            });
            ctx2.request_repaint();
        });
    }

    /// 每帧收一下后台上传结果：成功就把文档里的本地链接换成图床 URL。
    pub(super) fn poll_image_uploads(&mut self, ctx: &egui::Context) {
        if self.image_uploads.is_empty() {
            return;
        }
        let mut done = Vec::new();
        self.image_uploads.retain(|rx| match rx.try_recv() {
            Ok(u) => {
                done.push(u);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
                true
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for u in done {
            let active_uid = self.tabs.get(self.active).map(|t| t.uid).unwrap_or(0);
            match u.result {
                Ok(url) => {
                    if active_uid != u.tab_uid {
                        // 上传期间切到了别的标签：不擅自改当前文档，把地址提示出来。
                        self.notify(ctx, format!("图片已上传：{url}"));
                    } else if editor::replace_image_url(&mut self.doc, &u.old_url, &url) {
                        self.on_changed();
                        self.notify(ctx, format!("已上传到图床：{}", u.name));
                    } else {
                        // 上传期间链接被手动改掉：把地址告诉用户，不强行覆盖。
                        self.notify(ctx, format!("图片已上传：{url}"));
                    }
                }
                Err(e) => self.notify(ctx, format!("图片上传失败：{e}（已保留本地图片）")),
            }
        }
    }

    pub(super) fn refresh_tree(&mut self) {
        if let Some(r) = &self.root {
            self.tree = workspace::scan_tree(r, &self.expanded);
        }
        // 链接图按需生成（打开关系图时），这里不再同步扫盘。
        self.link_docs.clear();
    }

    // ---------------- 文件树右键菜单的文件操作 ----------------

    /// 复制文件路径（绝对或相对工作区根）。
    pub(super) fn copy_file_path(&mut self, p: &Path, relative: bool, ctx: &egui::Context) {
        let text = if relative {
            self.root
                .as_ref()
                .and_then(|r| p.strip_prefix(r).ok())
                .map(|s| s.display().to_string())
                .unwrap_or_else(|| p.display().to_string())
        } else {
            p.display().to_string()
        };
        ctx.copy_text(text.clone());
        self.notify(ctx, format!("已复制：{text}"));
    }

    /// 在系统文件管理器中定位文件（Finder / 资源管理器）。
    pub(super) fn reveal_in_system(&self, p: &Path) {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", p.display()))
                .spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let dir = p.parent().unwrap_or(p);
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }

    /// 关闭所有指向该路径的标签（从后往前，避免下标漂移）。
    pub(super) fn close_tabs_with_path(&mut self, p: &Path) {
        for i in (0..self.tabs.len()).rev() {
            if self.tabs[i].path.as_deref() == Some(p) {
                self.close_tab(i);
            }
        }
    }

    pub(super) fn fs_delete(&mut self, p: &Path) {
        let result = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        match result {
            Ok(_) => {
                self.close_tabs_with_path(p);
                self.file_clipboard = None;
                self.refresh_tree();
                self.status = format!("已删除 {}", p.display());
            }
            Err(e) => self.status = format!("删除失败：{e}"),
        }
    }

    pub(super) fn fs_rename(&mut self, p: &Path, new_name: &str) {
        let name = new_name.trim();
        if name.is_empty() {
            return;
        }
        let Some(parent) = p.parent() else {
            return;
        };
        let target = parent.join(name);
        if target.exists() {
            self.status = format!("已存在同名文件：{}", target.display());
            return;
        }
        if std::fs::rename(p, &target).is_err() {
            self.status = "重命名失败".to_string();
            return;
        }
        // 打开的标签与最近打开列表同步改路径
        for t in self.tabs.iter_mut() {
            if t.path.as_deref() == Some(p) {
                t.path = Some(target.clone());
            }
        }
        if self.doc.path.as_deref() == Some(p) {
            self.doc.path = Some(target.clone());
        }
        let old = p.display().to_string();
        let new = target.display().to_string();
        if let Some(r) = self.config.recent.iter_mut().find(|x| x.as_str() == old) {
            *r = new;
        }
        self.config.save();
        self.refresh_tree();
        self.status = format!("已重命名为 {}", name);
    }

    pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            let to = dst.join(e.file_name());
            if e.file_type()?.is_dir() {
                Self::copy_dir_recursive(&e.path(), &to)?;
            } else {
                std::fs::copy(e.path(), to)?;
            }
        }
        Ok(())
    }

    /// 粘贴剪贴板里的文件/文件夹到目标目录；同名自动追加 (2) (3)…
    /// 剪切的文件若正被打开，标签路径同步改到新位置（内容不丢）。
    pub(super) fn fs_paste(&mut self, dest_dir: &Path) {
        let Some((src, cut)) = self.file_clipboard.take() else {
            return;
        };
        let Some(name) = src.file_name().map(|n| n.to_os_string()) else {
            return;
        };
        let mut dest = dest_dir.join(name);
        if dest.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let ext = src
                .extension()
                .map(|s| format!(".{}", s.to_string_lossy()))
                .unwrap_or_default();
            let mut n = 2;
            loop {
                dest = dest_dir.join(format!("{stem} ({n}){ext}"));
                if !dest.exists() {
                    break;
                }
                n += 1;
            }
        }
        let result = if src.is_dir() {
            Self::copy_dir_recursive(&src, &dest)
        } else {
            std::fs::copy(&src, &dest).map(|_| ())
        };
        match result {
            Ok(_) => {
                if cut {
                    let _ = std::fs::remove_dir_all(&src).or_else(|_| std::fs::remove_file(&src));
                    for t in self.tabs.iter_mut() {
                        if t.path.as_deref() == Some(src.as_path()) {
                            t.path = Some(dest.clone());
                        }
                    }
                    if self.doc.path.as_deref() == Some(src.as_path()) {
                        self.doc.path = Some(dest.clone());
                    }
                }
                self.refresh_tree();
                self.status = format!("已粘贴到 {}", dest.display());
            }
            Err(e) => self.status = format!("粘贴失败：{e}"),
        }
    }

    /// 新建文件（并打开）/ 新建文件夹。
    pub(super) fn fs_new(&mut self, dir: &Path, is_dir: bool, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let target = dir.join(name);
        let result = if is_dir {
            std::fs::create_dir_all(&target)
        } else {
            std::fs::File::create(&target).map(|_| ())
        };
        match result {
            Ok(_) => {
                self.refresh_tree();
                self.status = format!("已创建 {}", target.display());
                if !is_dir {
                    self.open_file(target);
                }
            }
            Err(e) => self.status = format!("创建失败：{e}"),
        }
    }

    /// 后台线程构建双向链接关系图，算完回填并唤醒界面。
    pub(super) fn spawn_link_graph(&mut self, ctx: &egui::Context) {
        let Some(r) = self.root.clone() else {
            self.link_docs.clear();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.link_rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let m = build_link_graph(Some(&r));
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    pub(super) fn files(&self) -> Vec<PathBuf> {
        self.root
            .as_ref()
            .map(|r| workspace::collect_markdown(r))
            .unwrap_or_default()
    }

    /// 快速打开：按面板里指定的目录扫描 MD 文件，用缓存避免每帧遍历磁盘。
    pub(super) fn quick_open_files(&mut self) -> Vec<PathBuf> {
        let dir = self.palette.dir.trim().to_string();
        if dir.is_empty() {
            return self.files();
        }
        let root = PathBuf::from(&dir);
        if !root.is_dir() {
            return Vec::new();
        }
        if dir != self.quick_cache_dir {
            self.quick_cache_dir = dir;
            self.quick_cache = workspace::collect_markdown(&root);
        }
        self.quick_cache.clone()
    }
}

/// 递归收集行内图片地址（图片可能嵌在加粗、链接等行内样式里）。
fn collect_image_urls(inl: &crate::ast::Inline, out: &mut Vec<String>) {
    use crate::ast::Inline;
    match inl {
        Inline::Image { url, .. } => out.push(url.clone()),
        Inline::Strong(v)
        | Inline::Emph(v)
        | Inline::Strike(v)
        | Inline::Underline(v)
        | Inline::Highlight(v) => {
            for i in v {
                collect_image_urls(i, out);
            }
        }
        Inline::Link { label, .. } => {
            for i in label {
                collect_image_urls(i, out);
            }
        }
        _ => {}
    }
}
