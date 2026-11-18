//! 侧边栏文件树图标。
//!
//! 图标来自 `assets/file-icons`（Material Icon Theme 风格的 16×16 SVG，只保留
//! 下面 `icons!` 清单实际用到的子集）。编译期用
//! `include_bytes!` 嵌入，运行时用 resvg 栅格化成 egui 纹理并缓存，所以发布包
//! 不需要额外携带图标目录，也不依赖工作区里是否存在 assets。
//!
//! 文件夹按名字猜类型（src / node_modules / .git …），文件按后缀和特殊文件名
//! （README、Cargo.toml、Dockerfile …）映射到不同图标。
//!
//! 新增图标类型时，从本地完整素材库 `assets/icons/`（未入库）复制对应 SVG 到
//! `assets/file-icons/`，再在下方 `icons!` 清单里登记即可。

use egui::{ColorImage, TextureHandle, TextureOptions, Ui};
use std::collections::HashMap;

/// 首次显示时栅格化的尺寸。侧栏图标画 28pt，按 ~3.4x 渲染保证高分屏边缘不发虚。
const PIXELS: u32 = 96;
/// 侧栏图标的逻辑尺寸（pt）。
pub const SIZE: f32 = 28.0;
/// 内容归一化后允许的最大放大倍数。
///
/// 素材库里既有 16 viewBox 的满幅图标，也有 32/24 viewBox 的图标，画布内留白
/// 差异很大（实测可见图形只占画布长边的 75%～94%）。按内容包围盒归一化可以让
/// 所有图标视觉大小一致；但极个别素材的内容只有几个单位（如 16 viewBox 里画了
/// 一个 4 单位的小点），无限放大会变成一坨马赛克，用这个上限兜住。
const MAX_UPSCALE: f32 = 4.0;
/// 内容长边占目标框的比例，剩下的当呼吸空间，避免图标贴死文件名。
const FILL: f32 = 0.94;

#[derive(Default)]
pub struct IconCache {
    textures: HashMap<String, TextureHandle>,
}

impl IconCache {
    /// 画一个图标。未知 key 会退回普通文件图标，不会出现空白。
    pub fn show(&mut self, ui: &mut Ui, key: &str, size: f32) -> egui::Response {
        let ctx = ui.ctx().clone();
        if !self.textures.contains_key(key) {
            let bytes = icon_bytes(key)
                .or_else(|| icon_bytes("file"))
                .unwrap_or_default();
            let image = rasterize(bytes, PIXELS);
            let handle = ctx.load_texture(key, image, TextureOptions::LINEAR);
            self.textures.insert(key.to_string(), handle);
        }
        let texture = self.textures.get(key).expect("刚插入的图标纹理").clone();
        ui.add(
            egui::Image::new(&texture)
                .fit_to_exact_size(egui::vec2(size, size))
                .sense(egui::Sense::hover()),
        )
    }

    /// 画一个 16×16 画布的单色 UI 图标（Codicon 风格），用 `tint` 上色。
    ///
    /// 与文件图标不同：这里不做内容包围盒归一化，保留素材本身的留白和线宽，
    /// 这样 16px 下才是 VS Code 那种标准网格里的线性图标。
    pub fn paint_ui(
        &mut self,
        ui: &Ui,
        rect: egui::Rect,
        key: &str,
        size: f32,
        tint: egui::Color32,
    ) {
        let texture = match self.textures.get(key) {
            Some(t) => t.clone(),
            None => {
                let bytes = ui_icon_bytes(key).unwrap_or_default();
                let image = rasterize_canvas(bytes, PIXELS);
                let handle = ui.ctx().load_texture(key, image, TextureOptions::LINEAR);
                self.textures.insert(key.to_string(), handle.clone());
                handle
            }
        };
        let r = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
        egui::Image::new(&texture).tint(tint).paint_at(ui, r);
    }
}

/// SVG → egui 图像。resvg 输出预乘 RGBA，正好对应 `from_rgba_premultiplied`。
///
/// 缩放基准是**内容包围盒**而不是画布：素材库里画布尺寸不统一（viewBox 有
/// 16 / 24 / 32 / 64 / 256 …），按画布缩放会把透明边距也一起算进去，导致不同
/// 图标视觉大小差好几档、整体偏小。改成按内容长边铺满目标框后，所有图标看起来
/// 一样大。
fn rasterize(bytes: &[u8], px: u32) -> ColorImage {
    let tree = match resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()) {
        Ok(t) => t,
        Err(_) => {
            return ColorImage::filled([px as usize, px as usize], egui::Color32::TRANSPARENT);
        }
    };
    let mut pixmap = match resvg::tiny_skia::Pixmap::new(px, px) {
        Some(p) => p,
        None => return ColorImage::filled([px as usize, px as usize], egui::Color32::TRANSPARENT),
    };

    // 内容包围盒；退化素材（空路径）退回整块画布，避免除零。
    let bbox = tree.root().abs_bounding_box();
    let canvas = tree.size();
    let (bw, bh) = if bbox.width() > 0.5 && bbox.height() > 0.5 {
        (bbox.width(), bbox.height())
    } else {
        (canvas.width(), canvas.height())
    };

    // 内容长边铺满目标框（留一点呼吸空间，不让图标贴死文字）。
    let fit = px as f32 * FILL / bw.max(bh);
    // 极小内容（画布上只有几个单位）需要限制放大倍数，否则 96px 下会糊成一团。
    let base = px as f32 / canvas.width().max(canvas.height()).max(1.0);
    let scale = fit.min(base * MAX_UPSCALE);

    let cx = if bbox.width() > 0.5 {
        bbox.x() + bw / 2.0
    } else {
        canvas.width() / 2.0
    };
    let cy = if bbox.height() > 0.5 {
        bbox.y() + bh / 2.0
    } else {
        canvas.height() / 2.0
    };
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale)
        .post_translate(px as f32 / 2.0 - cx * scale, px as f32 / 2.0 - cy * scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data())
}

/// 直接按 SVG 画布等比渲染，不做内容包围盒归一化。
/// 用于需要保持 Codicon 原始 16×16 网格留白的 UI 图标。
fn rasterize_canvas(bytes: &[u8], px: u32) -> ColorImage {
    let tree = match resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()) {
        Ok(t) => t,
        Err(_) => {
            return ColorImage::filled([px as usize, px as usize], egui::Color32::TRANSPARENT);
        }
    };
    let mut pixmap = match resvg::tiny_skia::Pixmap::new(px, px) {
        Some(p) => p,
        None => return ColorImage::filled([px as usize, px as usize], egui::Color32::TRANSPARENT),
    };
    let canvas = tree.size();
    let scale = px as f32 / canvas.width().max(canvas.height()).max(1.0);
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data())
}

/// 文件夹名字 → 图标 key。展开时优先用 `-open` 变体。
pub fn folder_icon(name: &str, expanded: bool) -> String {
    let lower = name.to_ascii_lowercase();
    let base = match lower.as_str() {
        ".git" | ".gitattributes" => "folder-git",
        ".github" | ".gitlab" | ".github-workflows" => "folder-github",
        ".hg" | ".hgignore" => "folder-mercurial",
        "node_modules" => "folder-node",
        "src" | "source" => "folder-src",
        "lib" | "libs" | "library" => "folder-lib",
        "assets" | "asset" | "images" | "img" | "image" => "folder-images",
        "docs" | "doc" | "documentation" => "folder-docs",
        "test" | "tests" | "__tests__" | "spec" | "specs" => "folder-test",
        "config" | "configs" | ".config" | "cfg" => "folder-config",
        "public" | "static" => "folder-public",
        "build" | "dist" | "out" | "output" => "folder-dist",
        "target" => "folder-target",
        "script" | "scripts" | "bin" | "cmd" => "folder-scripts",
        "style" | "styles" | "css" => "folder-css",
        "sass" | "scss" => "folder-sass",
        "less" => "folder-less",
        "component" | "components" => "folder-components",
        "view" | "views" | "page" | "pages" => "folder-views",
        "route" | "routes" | "router" => "folder-routes",
        "api" | "apis" => "folder-api",
        "server" | "backend" => "folder-server",
        "client" | "frontend" => "folder-client",
        "database" | "db" | "data" => "folder-database",
        "migration" | "migrations" => "folder-migrations",
        "docker" | "containers" => "folder-docker",
        "kubernetes" | "k8s" | "helm" => "folder-kubernetes",
        "terraform" | "infra" | "infrastructure" => "folder-terraform",
        "python" | "venv" | ".venv" | "site-packages" => "folder-python",
        "rust" => "folder-rust",
        "go" | "golang" => "folder-go",
        "java" | "jvm" => "folder-java",
        "javascript" | "js" => "folder-javascript",
        "typescript" | "ts" => "folder-typescript",
        "vue" => "folder-vue",
        "svelte" => "folder-svelte",
        "angular" => "folder-angular",
        "next" | "nextjs" => "folder-next",
        "nuxt" | "nuxtjs" => "folder-nuxt",
        "android" => "folder-android",
        "ios" => "folder-ios",
        "macos" | "darwin" => "folder-macos",
        "linux" => "folder-linux",
        "windows" | "win" => "folder-windows",
        "font" | "fonts" => "folder-font",
        "audio" | "sound" | "sounds" => "folder-audio",
        "video" | "videos" => "folder-video",
        "log" | "logs" => "folder-log",
        "backup" | "backups" => "folder-backup",
        "temp" | "tmp" | ".tmp" => "folder-temp",
        "example" | "examples" | "sample" | "samples" => "folder-examples",
        "template" | "templates" => "folder-template",
        "util" | "utils" | "helper" | "helpers" => "folder-utils",
        "hook" | "hooks" => "folder-hook",
        "middleware" => "folder-middleware",
        "store" | "stores" | "state" => "folder-store",
        "coverage" => "folder-coverage",
        "package" | "packages" => "folder-packages",
        "tool" | "tools" => "folder-tools",
        "resource" | "resources" | "res" => "folder-resource",
        "command" | "commands" | "cli" => "folder-command",
        "markdown" | "md" => "folder-markdown",
        "json" => "folder-json",
        "storybook" | "stories" => "folder-storybook",
        "vscode" | ".vscode" => "folder-vscode",
        "theme" | "themes" => "folder-theme",
        "react" => "folder-react-components",
        _ => "folder",
    };
    if expanded {
        format!("{base}-open")
    } else {
        base.to_string()
    }
}

/// 文件名 → 图标 key。先看完整文件名（README、Cargo.toml…），再看后缀。
pub fn file_icon(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "readme" | "readme.md" | "readme.markdown" | "readme.txt" => return "readme",
        "license" | "licence" | "license.md" | "licence.md" | "copying" => return "license",
        "changelog" | "changelog.md" | "changes" => return "changelog",
        "authors" | "contributors" => return "authors",
        "contributing" | "contributing.md" => return "contributing",
        "makefile" | "gnumakefile" => return "makefile",
        "cmakelists.txt" => return "cmake",
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => return "docker",
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitconfig" => return "git",
        ".editorconfig" => return "editorconfig",
        ".eslintrc" | ".eslintrc.js" | ".eslintrc.cjs" | ".eslintrc.json" => return "eslint",
        "cargo.toml" | "cargo.lock" | "rust-toolchain.toml" => return "rust",
        "package.json" | "package-lock.json" => return "npm",
        "pnpm-lock.yaml" | "pnpm-workspace.yaml" => return "pnpm",
        "yarn.lock" => return "yarn",
        "go.mod" | "go.sum" => return "go",
        "pyproject.toml" | "requirements.txt" | "pipfile" | "pipfile.lock" => return "python",
        "gemfile" | "gemfile.lock" | "rakefile" => return "ruby",
        "composer.json" | "composer.lock" => return "php",
        "pom.xml" => return "maven",
        "build.gradle" | "build.gradle.kts" | "settings.gradle" => return "gradle",
        "pubspec.yaml" | "pubspec.lock" => return "dart",
        "mix.exs" | "mix.lock" => return "elixir",
        "vite.config.js" | "vite.config.ts" | "vite.config.mjs" => return "vite",
        "next.config.js" | "next.config.mjs" | "next.config.ts" => return "next",
        "nuxt.config.js" | "nuxt.config.ts" => return "nuxt",
        "tailwind.config.js" | "tailwind.config.ts" | "tailwind.config.cjs" => return "tailwindcss",
        "tsconfig.json" => return "tsconfig",
        ".env" | ".env.local" | ".env.development" | ".env.production" => return "settings",
        _ => {}
    }

    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext {
        "md" | "markdown" | "mdown" | "mkd" | "mdx" => "markdown",
        "txt" | "rtf" | "nfo" => "document",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" => "image",
        "svg" => "svg",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" => "audio",
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" => "video",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" => "zip",
        "woff" | "woff2" | "ttf" | "otf" | "eot" => "font",
        "html" | "htm" | "xhtml" => "html",
        "css" => "css",
        "scss" => "sass",
        "sass" => "sass",
        "less" => "less",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "react",
        "ts" => "typescript",
        "tsx" => "react_ts",
        "vue" => "vue",
        "svelte" => "svelte",
        "json" | "jsonc" | "json5" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" | "properties" => "settings",
        "xml" | "xsl" | "xslt" | "plist" => "xml",
        "csv" | "tsv" | "xls" | "xlsx" => "table",
        "sql" | "sqlite" | "db" => "database",
        "py" | "pyi" | "pyw" => "python",
        "ipynb" => "jupyter",
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "class" | "jar" => "jar",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "rb" | "rake" => "ruby",
        "sh" | "bash" | "zsh" | "fish" | "ksh" => "console",
        "ps1" | "psm1" | "psd1" => "powershell",
        "lua" => "lua",
        "r" => "r",
        "dart" => "dart",
        "scala" | "sc" => "scala",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "hs" | "lhs" => "haskell",
        "clj" | "cljs" | "cljc" => "clojure",
        "ml" | "mli" => "ocaml",
        "pl" | "pm" => "perl",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "tf" | "tfvars" | "hcl" => "terraform",
        "tex" | "sty" | "cls" => "document",
        "epub" | "mobi" | "azw3" => "epub",
        "doc" | "docx" => "word",
        "ppt" | "pptx" => "powerpoint",
        "mermaid" | "mmd" => "mermaid",
        "lock" => "lock",
        "node" | "nodejs" => "nodejs",
        "wasm" => "webassembly",
        _ => "file",
    }
}

/// 编译期嵌入的 UI 图标（单色 SVG，绘制时 tint 上色）。
fn ui_icon_bytes(key: &str) -> Option<&'static [u8]> {
    macro_rules! icons {
        ($($name:literal => $file:literal),* $(,)?) => {
            match key {
                $($name => Some(include_bytes!(concat!("../../../assets/ui-icons/", $file))),)*
                _ => None,
            }
        };
    }
    icons! {
        "ui:panel" => "panel.svg",
        "ui:panel-close" => "panel-close.svg",
    }
}

/// 编译期嵌入的图标。只收录常用类型，避免把 1200 多个 SVG 全塞进二进制。
fn icon_bytes(key: &str) -> Option<&'static [u8]> {
    macro_rules! icons {
        ($($name:literal => $file:literal),* $(,)?) => {
            match key {
                $($name => Some(include_bytes!(concat!("../../../assets/file-icons/", $file))),)*
                _ => None,
            }
        };
    }
    icons! {
        "folder" => "folder.svg",
        "folder-open" => "folder-open.svg",
        "folder-src" => "folder-src.svg",
        "folder-src-open" => "folder-src-open.svg",
        "folder-lib" => "folder-lib.svg",
        "folder-lib-open" => "folder-lib-open.svg",
        "folder-node" => "folder-node.svg",
        "folder-node-open" => "folder-node-open.svg",
        "folder-git" => "folder-git.svg",
        "folder-git-open" => "folder-git-open.svg",
        "folder-github" => "folder-github.svg",
        "folder-github-open" => "folder-github-open.svg",
        "folder-gitlab" => "folder-gitlab.svg",
        "folder-gitlab-open" => "folder-gitlab-open.svg",
        "folder-mercurial" => "folder-mercurial.svg",
        "folder-mercurial-open" => "folder-mercurial-open.svg",
        "folder-docs" => "folder-docs.svg",
        "folder-docs-open" => "folder-docs-open.svg",
        "folder-test" => "folder-test.svg",
        "folder-test-open" => "folder-test-open.svg",
        "folder-config" => "folder-config.svg",
        "folder-config-open" => "folder-config-open.svg",
        "folder-public" => "folder-public.svg",
        "folder-public-open" => "folder-public-open.svg",
        "folder-dist" => "folder-dist.svg",
        "folder-dist-open" => "folder-dist-open.svg",
        "folder-target" => "folder-target.svg",
        "folder-target-open" => "folder-target-open.svg",
        "folder-scripts" => "folder-scripts.svg",
        "folder-scripts-open" => "folder-scripts-open.svg",
        "folder-css" => "folder-css.svg",
        "folder-css-open" => "folder-css-open.svg",
        "folder-sass" => "folder-sass.svg",
        "folder-sass-open" => "folder-sass-open.svg",
        "folder-less" => "folder-less.svg",
        "folder-less-open" => "folder-less-open.svg",
        "folder-components" => "folder-components.svg",
        "folder-components-open" => "folder-components-open.svg",
        "folder-views" => "folder-views.svg",
        "folder-views-open" => "folder-views-open.svg",
        "folder-routes" => "folder-routes.svg",
        "folder-routes-open" => "folder-routes-open.svg",
        "folder-api" => "folder-api.svg",
        "folder-api-open" => "folder-api-open.svg",
        "folder-server" => "folder-server.svg",
        "folder-server-open" => "folder-server-open.svg",
        "folder-client" => "folder-client.svg",
        "folder-client-open" => "folder-client-open.svg",
        "folder-database" => "folder-database.svg",
        "folder-database-open" => "folder-database-open.svg",
        "folder-migrations" => "folder-migrations.svg",
        "folder-migrations-open" => "folder-migrations-open.svg",
        "folder-docker" => "folder-docker.svg",
        "folder-docker-open" => "folder-docker-open.svg",
        "folder-kubernetes" => "folder-kubernetes.svg",
        "folder-kubernetes-open" => "folder-kubernetes-open.svg",
        "folder-terraform" => "folder-terraform.svg",
        "folder-terraform-open" => "folder-terraform-open.svg",
        "folder-python" => "folder-python.svg",
        "folder-python-open" => "folder-python-open.svg",
        "folder-rust" => "folder-rust.svg",
        "folder-rust-open" => "folder-rust-open.svg",
        "folder-go" => "folder-go.svg",
        "folder-go-open" => "folder-go-open.svg",
        "folder-java" => "folder-java.svg",
        "folder-java-open" => "folder-java-open.svg",
        "folder-javascript" => "folder-javascript.svg",
        "folder-javascript-open" => "folder-javascript-open.svg",
        "folder-typescript" => "folder-typescript.svg",
        "folder-typescript-open" => "folder-typescript-open.svg",
        "folder-vue" => "folder-vue.svg",
        "folder-vue-open" => "folder-vue-open.svg",
        "folder-svelte" => "folder-svelte.svg",
        "folder-svelte-open" => "folder-svelte-open.svg",
        "folder-angular" => "folder-angular.svg",
        "folder-angular-open" => "folder-angular-open.svg",
        "folder-next" => "folder-next.svg",
        "folder-next-open" => "folder-next-open.svg",
        "folder-nuxt" => "folder-nuxt.svg",
        "folder-nuxt-open" => "folder-nuxt-open.svg",
        "folder-android" => "folder-android.svg",
        "folder-android-open" => "folder-android-open.svg",
        "folder-ios" => "folder-ios.svg",
        "folder-ios-open" => "folder-ios-open.svg",
        "folder-macos" => "folder-macos.svg",
        "folder-macos-open" => "folder-macos-open.svg",
        "folder-linux" => "folder-linux.svg",
        "folder-linux-open" => "folder-linux-open.svg",
        "folder-windows" => "folder-windows.svg",
        "folder-windows-open" => "folder-windows-open.svg",
        "folder-font" => "folder-font.svg",
        "folder-font-open" => "folder-font-open.svg",
        "folder-audio" => "folder-audio.svg",
        "folder-audio-open" => "folder-audio-open.svg",
        "folder-video" => "folder-video.svg",
        "folder-video-open" => "folder-video-open.svg",
        "folder-log" => "folder-log.svg",
        "folder-log-open" => "folder-log-open.svg",
        "folder-backup" => "folder-backup.svg",
        "folder-backup-open" => "folder-backup-open.svg",
        "folder-temp" => "folder-temp.svg",
        "folder-temp-open" => "folder-temp-open.svg",
        "folder-examples" => "folder-examples.svg",
        "folder-examples-open" => "folder-examples-open.svg",
        "folder-template" => "folder-template.svg",
        "folder-template-open" => "folder-template-open.svg",
        "folder-utils" => "folder-utils.svg",
        "folder-utils-open" => "folder-utils-open.svg",
        "folder-hook" => "folder-hook.svg",
        "folder-hook-open" => "folder-hook-open.svg",
        "folder-middleware" => "folder-middleware.svg",
        "folder-middleware-open" => "folder-middleware-open.svg",
        "folder-store" => "folder-store.svg",
        "folder-store-open" => "folder-store-open.svg",
        "folder-coverage" => "folder-coverage.svg",
        "folder-coverage-open" => "folder-coverage-open.svg",
        "folder-packages" => "folder-packages.svg",
        "folder-packages-open" => "folder-packages-open.svg",
        "folder-tools" => "folder-tools.svg",
        "folder-tools-open" => "folder-tools-open.svg",
        "folder-resource" => "folder-resource.svg",
        "folder-resource-open" => "folder-resource-open.svg",
        "folder-command" => "folder-command.svg",
        "folder-command-open" => "folder-command-open.svg",
        "folder-markdown" => "folder-markdown.svg",
        "folder-markdown-open" => "folder-markdown-open.svg",
        "folder-images" => "folder-images.svg",
        "folder-images-open" => "folder-images-open.svg",
        "folder-json" => "folder-json.svg",
        "folder-json-open" => "folder-json-open.svg",
        "folder-storybook" => "folder-storybook.svg",
        "folder-storybook-open" => "folder-storybook-open.svg",
        "folder-vscode" => "folder-vscode.svg",
        "folder-vscode-open" => "folder-vscode-open.svg",
        "folder-theme" => "folder-theme.svg",
        "folder-theme-open" => "folder-theme-open.svg",
        "folder-react-components" => "folder-react-components.svg",
        "folder-react-components-open" => "folder-react-components-open.svg",

        "file" => "file.svg",
        "document" => "document.svg",
        "markdown" => "markdown.svg",
        "readme" => "readme.svg",
        "license" => "license.svg",
        "changelog" => "changelog.svg",
        "authors" => "authors.svg",
        "contributing" => "contributing.svg",
        "makefile" => "makefile.svg",
        "cmake" => "cmake.svg",
        "docker" => "docker.svg",
        "git" => "git.svg",
        "editorconfig" => "editorconfig.svg",
        "eslint" => "eslint.svg",
        "rust" => "rust.svg",
        "npm" => "npm.svg",
        "pnpm" => "pnpm.svg",
        "yarn" => "yarn.svg",
        "go" => "go.svg",
        "python" => "python.svg",
        "ruby" => "ruby.svg",
        "php" => "php.svg",
        "maven" => "maven.svg",
        "gradle" => "gradle.svg",
        "dart" => "dart.svg",
        "elixir" => "elixir.svg",
        "vite" => "vite.svg",
        "next" => "next.svg",
        "nuxt" => "nuxt.svg",
        "tailwindcss" => "tailwindcss.svg",
        "tsconfig" => "tsconfig.svg",
        "settings" => "settings.svg",
        "image" => "image.svg",
        "svg" => "svg.svg",
        "pdf" => "pdf.svg",
        "zip" => "zip.svg",
        "audio" => "audio.svg",
        "video" => "video.svg",
        "font" => "font.svg",
        "html" => "html.svg",
        "css" => "css.svg",
        "sass" => "sass.svg",
        "less" => "less.svg",
        "javascript" => "javascript.svg",
        "react" => "react.svg",
        "react_ts" => "react_ts.svg",
        "typescript" => "typescript.svg",
        "vue" => "vue.svg",
        "svelte" => "svelte.svg",
        "json" => "json.svg",
        "yaml" => "yaml.svg",
        "toml" => "toml.svg",
        "xml" => "xml.svg",
        "table" => "table.svg",
        "database" => "database.svg",
        "jupyter" => "jupyter.svg",
        "java" => "java.svg",
        "jar" => "jar.svg",
        "kotlin" => "kotlin.svg",
        "swift" => "swift.svg",
        "c" => "c.svg",
        "cpp" => "cpp.svg",
        "csharp" => "csharp.svg",
        "console" => "console.svg",
        "powershell" => "powershell.svg",
        "lua" => "lua.svg",
        "r" => "r.svg",
        "scala" => "scala.svg",
        "erlang" => "erlang.svg",
        "haskell" => "haskell.svg",
        "clojure" => "clojure.svg",
        "ocaml" => "ocaml.svg",
        "perl" => "perl.svg",
        "proto" => "proto.svg",
        "graphql" => "graphql.svg",
        "terraform" => "terraform.svg",
        "epub" => "epub.svg",
        "word" => "word.svg",
        "powerpoint" => "powerpoint.svg",
        "mermaid" => "mermaid.svg",
        "lock" => "lock.svg",
        "nodejs" => "nodejs.svg",
        "webassembly" => "webassembly.svg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文件夹按名字、文件按后缀/特殊文件名映射到不同图标。
    #[test]
    fn maps_folders_and_files() {
        assert_eq!(folder_icon("src", false), "folder-src");
        assert_eq!(folder_icon("SRC", true), "folder-src-open");
        assert_eq!(folder_icon("node_modules", true), "folder-node-open");
        assert_eq!(folder_icon(".git", false), "folder-git");
        assert_eq!(folder_icon("不认识的目录", false), "folder");

        assert_eq!(file_icon("README.md"), "readme");
        assert_eq!(file_icon("Cargo.toml"), "rust");
        assert_eq!(file_icon("Dockerfile"), "docker");
        assert_eq!(file_icon("main.rs"), "rust");
        assert_eq!(file_icon("App.vue"), "vue");
        assert_eq!(file_icon("photo.PNG"), "image");
        assert_eq!(file_icon("unknown.xyz"), "file");
    }

    /// 映射表里出现的每个 key 都必须真的嵌入了对应 SVG，防止改名字后运行时变空白。
    #[test]
    fn every_mapped_icon_is_embedded() {
        for name in [
            "src",
            "lib",
            "node_modules",
            ".git",
            ".github",
            "assets",
            "docs",
            "tests",
            "config",
            "public",
            "dist",
            "target",
            "scripts",
            "styles",
            "sass",
            "components",
            "pages",
            "routes",
            "api",
            "server",
            "client",
            "database",
            "migrations",
            "docker",
            "k8s",
            "terraform",
            "python",
            "rust",
            "go",
            "java",
            "js",
            "ts",
            "vue",
            "svelte",
            "angular",
            "next",
            "nuxt",
            "android",
            "ios",
            "macos",
            "linux",
            "windows",
            "fonts",
            "audio",
            "video",
            "logs",
            "backup",
            "tmp",
            "examples",
            "templates",
            "utils",
            "hooks",
            "middleware",
            "store",
            "coverage",
            "packages",
            "tools",
            "resources",
            "cli",
            "markdown",
            "json",
            "storybook",
            ".vscode",
            "themes",
            "react",
            ".hg",
            "unmapped",
        ] {
            let closed = folder_icon(name, false);
            let opened = folder_icon(name, true);
            assert!(
                icon_bytes(&closed).is_some(),
                "缺少图标: {name} -> {closed}"
            );
            assert!(
                icon_bytes(&opened).is_some(),
                "缺少展开图标: {name} -> {opened}"
            );
        }

        for name in [
            "README.md",
            "LICENSE",
            "CHANGELOG.md",
            "Makefile",
            "CMakeLists.txt",
            "Dockerfile",
            ".gitignore",
            ".editorconfig",
            ".eslintrc.json",
            "Cargo.toml",
            "package.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "go.mod",
            "pyproject.toml",
            "Gemfile",
            "composer.json",
            "pom.xml",
            "build.gradle",
            "pubspec.yaml",
            "mix.exs",
            "vite.config.ts",
            "next.config.js",
            "nuxt.config.ts",
            "tailwind.config.js",
            "tsconfig.json",
            ".env",
            "a.md",
            "a.txt",
            "a.pdf",
            "a.png",
            "a.svg",
            "a.mp3",
            "a.mp4",
            "a.zip",
            "a.woff2",
            "a.html",
            "a.css",
            "a.scss",
            "a.less",
            "a.js",
            "a.jsx",
            "a.ts",
            "a.tsx",
            "a.vue",
            "a.json",
            "a.yaml",
            "a.toml",
            "a.ini",
            "a.xml",
            "a.csv",
            "a.sql",
            "a.py",
            "a.ipynb",
            "a.rs",
            "a.go",
            "a.java",
            "a.jar",
            "a.kt",
            "a.swift",
            "a.c",
            "a.cpp",
            "a.cs",
            "a.php",
            "a.rb",
            "a.sh",
            "a.ps1",
            "a.lua",
            "a.r",
            "a.dart",
            "a.scala",
            "a.ex",
            "a.erl",
            "a.hs",
            "a.clj",
            "a.ml",
            "a.pl",
            "a.proto",
            "a.graphql",
            "a.tf",
            "a.mmd",
            "a.wasm",
            "a.unknown",
        ] {
            let key = file_icon(name);
            assert!(icon_bytes(key).is_some(), "缺少图标: {name} -> {key}");
        }
    }

    /// resvg 必须真的把 SVG 渲染出像素，而不是返回一张全透明图。
    #[test]
    fn rasterizes_svg_to_pixels() {
        let bytes = icon_bytes("folder-mercurial-open").expect("参考图标应存在");
        let image = rasterize(bytes, 32);
        assert_eq!(image.size, [32, 32]);
        assert!(
            image.pixels.iter().any(|p| p.a() > 0),
            "SVG 渲染结果不应是全透明"
        );
    }

    /// 可见像素的包围盒（列/行范围），用于校验归一化效果。
    fn alpha_bbox(image: &ColorImage) -> Option<(usize, usize, usize, usize)> {
        let [w, h] = image.size;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        for y in 0..h {
            for x in 0..w {
                if image.pixels[y * w + x].a() > 8 {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        if x0 > x1 {
            None
        } else {
            Some((x0, y0, x1, y1))
        }
    }

    /// 归一化：不管素材画布是 16 / 24 / 32 / 256 / 960 还是留白多少，
    /// 渲染后可见图形的长边都应铺满目标框的 ~94%，视觉大小才一致。
    #[test]
    fn normalizes_content_across_canvas_sizes() {
        // 画布与留白差异极大的代表：满幅文件夹、竖高文件、扁宽 markdown、
        // 32 画布的 zip、256 画布的 clojure、960 画布负 y 偏移的 json。
        let keys = [
            "folder",
            "file",
            "markdown",
            "zip",
            "clojure",
            "json",
            "folder-src",
            "css",
        ];
        let px = PIXELS;
        for key in keys {
            let bytes = icon_bytes(key).unwrap_or_else(|| panic!("应嵌入图标: {key}"));
            let image = rasterize(bytes, px);
            let (x0, y0, x1, y1) = alpha_bbox(&image).unwrap_or_else(|| panic!("{key} 全透明"));
            let long = (x1 - x0 + 1).max(y1 - y0 + 1) as f32 / px as f32;
            assert!(
                (0.80..=0.999).contains(&long),
                "{key} 长边占框 {long:.2}，未归一化到位（期望 ~{FILL}）"
            );
            // 贴死最外圈像素说明内容被 pixmap 边界裁掉了。
            assert!(
                x0 > 0 && y0 > 0 && x1 + 1 < px as usize && y1 + 1 < px as usize,
                "{key} 内容触到画布边缘，疑似被裁切: bbox={x0},{y0}..{x1},{y1}"
            );
        }
    }

    /// 内容包围盒退化（无绘制内容）时必须走兜底，不能 panic 或除零。
    #[test]
    fn empty_svg_does_not_panic() {
        let bytes = b"<svg viewBox=\"0 0 16 16\" xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        let image = rasterize(bytes, 32);
        assert_eq!(image.size, [32, 32]);
        assert!(image.pixels.iter().all(|p| p.a() == 0));
    }

    /// 全量回归：素材目录里每个 SVG 都必须已嵌入，且归一化后可见图形长边
    /// 铺满图标框的绝大部分；任何素材被裁到画布边缘都算失败。
    #[test]
    fn every_asset_is_embedded_and_normalized() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/file-icons");
        let mut count = 0usize;
        let mut worst = (1.0f32, String::from("-"));
        for entry in std::fs::read_dir(&dir).expect("图标素材目录应存在") {
            let path = entry.expect("目录项可读").path();
            if path.extension().and_then(|s| s.to_str()) != Some("svg") {
                continue;
            }
            let key = path.file_stem().unwrap().to_str().unwrap().to_string();
            let bytes = icon_bytes(&key).unwrap_or_else(|| panic!("素材未嵌入二进制: {key}"));
            let image = rasterize(bytes, PIXELS);
            let (x0, y0, x1, y1) =
                alpha_bbox(&image).unwrap_or_else(|| panic!("渲染全透明: {key}"));
            assert!(
                x0 > 0 && y0 > 0 && x1 + 1 < PIXELS as usize && y1 + 1 < PIXELS as usize,
                "{key} 内容贴边，疑似被裁切: bbox={x0},{y0}..{x1},{y1}"
            );
            let long = (x1 - x0 + 1).max(y1 - y0 + 1) as f32 / PIXELS as f32;
            if long < worst.0 {
                worst = (long, key.clone());
            }
            count += 1;
        }
        println!(
            "共 {count} 个素材全部嵌入；归一化后最小长边占比 {:.3}（素材 {}），阈值 {}",
            worst.0, worst.1, FILL
        );
        assert!(count > 200, "素材数量异常: {count}");
        assert!(worst.0 > 0.5, "存在归一化后仍然过小的素材: {worst:?}");
    }
}
