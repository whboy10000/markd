//! Author: aurelianus <aurelianus@126.com>

//! 主题系统：内置多套配色 + 全局字号/行高设置。

use egui::{Color32, Context, FontFamily, FontId, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    // 旧主题 Github/Sepia 是浅色系，反序列化时归到 Light，避免老配置失效。
    #[serde(alias = "Github", alias = "Sepia")]
    Light,
    // 旧主题 Nord/Solarized 是深色系，归到 Dark。
    #[serde(alias = "Nord", alias = "Solarized")]
    Dark,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 2] = [ThemeKind::Light, ThemeKind::Dark];

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Light => "浅色",
            ThemeKind::Dark => "深色",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, ThemeKind::Dark)
    }
}

#[derive(Clone)]
pub struct Theme {
    pub kind: ThemeKind,
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub code_bg: Color32,
    /// 代码块顶部功能栏背景：比 `code_bg` 深一档，区分「功能栏 / 显示区」。
    pub code_header: Color32,
    pub border: Color32,
    pub heading: Color32,
    pub link: Color32,
    pub quote: Color32,
    pub table_head: Color32,
    /// 表格线（外框 + 行列分隔线）。浅色主题固定 #F3F3F3，比通用 border 更浅一档。
    pub table_line: Color32,
    pub selection: Color32,
    pub body: f32,
    pub mono: f32,
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// 用户可自定义的主题颜色项。数据驱动的 UI 与覆盖逻辑共用此枚举，
/// 避免为 14 个颜色字段各写一遍重复代码。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeColor {
    Bg,
    Panel,
    Text,
    Dim,
    Accent,
    CodeBg,
    CodeHeader,
    Border,
    Heading,
    Link,
    Quote,
    TableHead,
    TableLine,
    Selection,
}

impl ThemeColor {
    pub const ALL: [ThemeColor; 14] = [
        ThemeColor::Bg,
        ThemeColor::Panel,
        ThemeColor::Text,
        ThemeColor::Dim,
        ThemeColor::Accent,
        ThemeColor::CodeBg,
        ThemeColor::CodeHeader,
        ThemeColor::Border,
        ThemeColor::Heading,
        ThemeColor::Link,
        ThemeColor::Quote,
        ThemeColor::TableHead,
        ThemeColor::TableLine,
        ThemeColor::Selection,
    ];

    /// 设置面板里显示的中文名。
    pub fn label(self) -> &'static str {
        match self {
            ThemeColor::Bg => "编辑区背景",
            ThemeColor::Panel => "侧栏背景",
            ThemeColor::Text => "正文文字",
            ThemeColor::Dim => "次要文字",
            ThemeColor::Accent => "强调色",
            ThemeColor::CodeBg => "代码块背景",
            ThemeColor::CodeHeader => "代码块标题栏",
            ThemeColor::Border => "边框",
            ThemeColor::Heading => "标题",
            ThemeColor::Link => "链接",
            ThemeColor::Quote => "引用",
            ThemeColor::TableHead => "表头背景",
            ThemeColor::TableLine => "表格线",
            ThemeColor::Selection => "选中高亮",
        }
    }

    /// 从主题里读出该项颜色的 RGB。
    pub fn rgb_of(self, t: &Theme) -> [u8; 3] {
        let c = match self {
            ThemeColor::Bg => t.bg,
            ThemeColor::Panel => t.panel,
            ThemeColor::Text => t.text,
            ThemeColor::Dim => t.dim,
            ThemeColor::Accent => t.accent,
            ThemeColor::CodeBg => t.code_bg,
            ThemeColor::CodeHeader => t.code_header,
            ThemeColor::Border => t.border,
            ThemeColor::Heading => t.heading,
            ThemeColor::Link => t.link,
            ThemeColor::Quote => t.quote,
            ThemeColor::TableHead => t.table_head,
            ThemeColor::TableLine => t.table_line,
            ThemeColor::Selection => t.selection,
        };
        [c.r(), c.g(), c.b()]
    }

    /// 某个主题下该项的默认 RGB（默认配色的唯一来源仍是 `Theme::get`）。
    pub fn default_rgb(self, kind: ThemeKind) -> [u8; 3] {
        self.rgb_of(&Theme::get(kind))
    }

    /// 该项在主题里的可变引用，供覆盖写入。
    fn field_mut<'a>(self, t: &'a mut Theme) -> &'a mut Color32 {
        match self {
            ThemeColor::Bg => &mut t.bg,
            ThemeColor::Panel => &mut t.panel,
            ThemeColor::Text => &mut t.text,
            ThemeColor::Dim => &mut t.dim,
            ThemeColor::Accent => &mut t.accent,
            ThemeColor::CodeBg => &mut t.code_bg,
            ThemeColor::CodeHeader => &mut t.code_header,
            ThemeColor::Border => &mut t.border,
            ThemeColor::Heading => &mut t.heading,
            ThemeColor::Link => &mut t.link,
            ThemeColor::Quote => &mut t.quote,
            ThemeColor::TableHead => &mut t.table_head,
            ThemeColor::TableLine => &mut t.table_line,
            ThemeColor::Selection => &mut t.selection,
        }
    }
}

/// 用户对主题颜色的覆盖。`None` 表示未覆盖（跟随当前主题默认色），
/// `Some(rgb)` 表示用户自定义。每项独立，故「恢复默认」只需写回 None。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ColorOverrides {
    #[serde(default)]
    pub bg: Option<[u8; 3]>,
    #[serde(default)]
    pub panel: Option<[u8; 3]>,
    #[serde(default)]
    pub text: Option<[u8; 3]>,
    #[serde(default)]
    pub dim: Option<[u8; 3]>,
    #[serde(default)]
    pub accent: Option<[u8; 3]>,
    #[serde(default)]
    pub code_bg: Option<[u8; 3]>,
    #[serde(default)]
    pub code_header: Option<[u8; 3]>,
    #[serde(default)]
    pub border: Option<[u8; 3]>,
    #[serde(default)]
    pub heading: Option<[u8; 3]>,
    #[serde(default)]
    pub link: Option<[u8; 3]>,
    #[serde(default)]
    pub quote: Option<[u8; 3]>,
    #[serde(default)]
    pub table_head: Option<[u8; 3]>,
    #[serde(default)]
    pub table_line: Option<[u8; 3]>,
    #[serde(default)]
    pub selection: Option<[u8; 3]>,
}

impl ColorOverrides {
    pub fn get(&self, c: ThemeColor) -> Option<[u8; 3]> {
        match c {
            ThemeColor::Bg => self.bg,
            ThemeColor::Panel => self.panel,
            ThemeColor::Text => self.text,
            ThemeColor::Dim => self.dim,
            ThemeColor::Accent => self.accent,
            ThemeColor::CodeBg => self.code_bg,
            ThemeColor::CodeHeader => self.code_header,
            ThemeColor::Border => self.border,
            ThemeColor::Heading => self.heading,
            ThemeColor::Link => self.link,
            ThemeColor::Quote => self.quote,
            ThemeColor::TableHead => self.table_head,
            ThemeColor::TableLine => self.table_line,
            ThemeColor::Selection => self.selection,
        }
    }

    pub fn set(&mut self, c: ThemeColor, value: Option<[u8; 3]>) {
        let slot = match c {
            ThemeColor::Bg => &mut self.bg,
            ThemeColor::Panel => &mut self.panel,
            ThemeColor::Text => &mut self.text,
            ThemeColor::Dim => &mut self.dim,
            ThemeColor::Accent => &mut self.accent,
            ThemeColor::CodeBg => &mut self.code_bg,
            ThemeColor::CodeHeader => &mut self.code_header,
            ThemeColor::Border => &mut self.border,
            ThemeColor::Heading => &mut self.heading,
            ThemeColor::Link => &mut self.link,
            ThemeColor::Quote => &mut self.quote,
            ThemeColor::TableHead => &mut self.table_head,
            ThemeColor::TableLine => &mut self.table_line,
            ThemeColor::Selection => &mut self.selection,
        };
        *slot = value;
    }

    /// 把覆盖项盖到主题上（只动 Some 的项）。
    pub fn apply_to(&self, t: &mut Theme) {
        for c in ThemeColor::ALL {
            if let Some(rgb) = self.get(c) {
                *c.field_mut(t) = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        ThemeColor::ALL.iter().all(|c| self.get(*c).is_none())
    }
}

/// 语法高亮的语言分组：决定用哪套 token 配色，让不同语言有区分度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangFamily {
    Rust,
    Python,
    JavaScript,
    CLike,  // C / C++ / Java / Go
    Markup, // HTML / XML / Vue template
    Css,
    Data,  // YAML / YML / JSON / TOML
    Shell, // Bash / ZSH / sh
    Sql,
    Generic,
}

/// 围栏信息行的规范键：取第一个词、转小写、去掉 `.` 前缀（`cpp title="x"`、`{.h}` 都能认）。
pub fn lang_key(lang: &str) -> String {
    lang.split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

/// 把语言标识（含别名、大小写、`cpp title="x"` 这类带参数的围栏）映射到配色族。
/// 这里的键要覆盖「代码块语言下拉」`LANGUAGE_OPTIONS` 的每个 id 与其别名：
/// 漏一项就退化成通用配色，只剩字符串和数字上色（有测试逐条比对两张表）。
pub fn lang_family(lang: &str) -> LangFamily {
    use LangFamily::*;
    match lang_key(lang).as_str() {
        "rust" | "rs" | "sway" => Rust,
        "python" | "py" | "py2" | "py3" | "python2" | "python3" | "ipython" | "cython" | "pyx"
        | "mojo" | "micropython" | "jupyter" | "ipynb" | "notebook" | "vyper" | "starlark"
        | "bzl" | "bazel" | "meson" | "matlab" | "octave" | "scilab" | "julia" | "jl" | "r"
        | "rlang" | "rscript" | "maxima" | "gnuplot" | "elixir" | "ex" | "exs" | "phoenix" => {
            Python
        }
        "javascript" | "js" | "mjs" | "cjs" | "node" | "nodejs" | "ecmascript" | "es6"
        | "typescript" | "ts" | "mts" | "cts" | "deno" | "bun" | "jsx" | "tsx" | "react"
        | "coffeescript" | "coffee" | "livescript" | "lsc" | "actionscript" | "as3" | "svelte"
        | "astro" | "mdx" | "reason" | "rescript" | "res" | "mongo" | "mongosh" | "mongodb"
        | "jsonnet" | "libsonnet" | "re" | "sveltekit" => JavaScript,
        "c" | "h" | "cpp" | "c++" | "cxx" | "cc" | "hpp" | "hh" | "hxx" | "objc" | "obj-c"
        | "objective-c" | "objective-c++" | "objective-cpp" | "swift" | "java" | "jvm"
        | "android" | "kotlin" | "kt" | "kts" | "scala" | "sbt" | "groovy" | "gradle"
        | "jenkinsfile" | "jenkins" | "csharp" | "cs" | "c#" | "dotnet" | "unity" | "php"
        | "php4" | "php5" | "wordpress" | "go" | "golang" | "dart" | "flutter" | "nim" | "nims"
        | "zig" | "v" | "vlang" | "odin" | "hare" | "c3" | "crystal" | "cr" | "d" | "dlang"
        | "vala" | "haxe" | "hx" | "pascal" | "delphi" | "objectpascal" | "pas" | "ada"
        | "fortran" | "f" | "f77" | "f90" | "glsl" | "hlsl" | "wgsl" | "metal" | "msl"
        | "shader" | "cuda" | "nvcc" | "cu" | "opencl" | "ocl" | "qml" | "qt" | "gdscript"
        | "godot" | "processing" | "pde" | "arduino" | "ino" | "sketch" | "verilog"
        | "systemverilog" | "sv" | "protobuf" | "proto" | "thrift" | "idl" | "solidity" | "sol"
        | "cairo" | "move" | "motoko" | "bicep" | "minizinc" | "mzn" | "supercollider" | "apex"
        | "1c" | "bsl" | "stan" | "stata" | "sas" | "applescript" | "scpt" | "lean" | "lean4"
        | "dot" | "graphviz" | "gv" | "roc" | "azure" | "xcode" | "salesforce" | "directx"
        | "webgpu" | "evm" | "sui" | "aptos" | "dfinity" => CLike,
        "html" | "htm" | "xhtml" | "xml" | "rss" | "atom" | "xsd" | "xslt" | "xsl" | "svg"
        | "plist" | "docbook" | "vue" | "nuxt" | "blade" | "laravel" | "twig" | "symfony"
        | "liquid" | "shopify" | "jinja" | "jinja2" | "j2" | "django" | "ejs" | "erb" | "pug"
        | "jade" | "haml" | "slim" | "handlebars" | "hbs" | "mustache" | "razor" | "cshtml"
        | "velocity" | "mediawiki" | "wiki" | "textile" => Markup,
        "css" | "scss" | "sass" | "less" | "stylus" | "styl" | "postcss" | "stylesheet"
        | "indented" => Css,
        "json" | "jsonc" | "json5" | "jsonl" | "ndjson" | "geojson" | "yaml" | "yml" | "k8s"
        | "kubernetes" | "helm" | "ansible" | "playbook" | "github-actions" | "workflow"
        | "docker-compose" | "prometheus" | "toml" | "cargo" | "edn" | "hocon"
        | "typesafe-config" | "avro" | "avsc" | "ini" | "cfg" | "conf" | "properties"
        | "editorconfig" | "gitconfig" | "gitignore" | "gitattributes" | "systemd" | "desktop"
        | "env" | "dotenv" | "cue" | "dhall" | "config" | "props" => Data,
        "bash" | "sh" | "shell" | "zsh" | "zshell" | "fish" | "ksh" | "csh" | "nushell" | "nu"
        | "powershell" | "ps1" | "psm1" | "psd1" | "pwsh" | "bat" | "cmd" | "dosbatch"
        | "batch" | "autohotkey" | "ahk" | "awk" | "gawk" | "mawk" | "sed" | "gsed" | "jq"
        | "tcl" | "tk" | "expect" | "perl" | "pl" | "cperl" | "raku" | "ruby" | "rb" | "jruby"
        | "rails" | "rake" | "gemfile" | "dockerfile" | "docker" | "containerfile" | "makefile"
        | "make" | "gmake" | "bsdmake" | "mk" | "cmake" | "cmakelists" | "ninja" | "nix"
        | "nixos" | "puppet" | "nginx" | "apache" | "httpd" | "htaccess" | "caddy"
        | "caddyfile" | "crontab" | "cron" | "tmux" | "m4" | "autoconf" | "aclocal" | "http"
        | "rest" | "openapi" | "swagger" | "curl" | "po" | "gettext" | "pot" | "shell-session"
        | "console" | "terminal" | "bourne" | "dos" | "terraform" | "tf" | "tfvars" | "hcl"
        | "nomad" | "vault" => Shell,
        "sql" | "mysql" | "mariadb" | "postgresql" | "postgres" | "psql" | "plpgsql" | "plsql"
        | "oracle" | "tsql" | "mssql" | "sqlserver" | "sqlite" | "sqlite3" | "duckdb"
        | "clickhouse" | "bigquery" | "bq" | "hive" | "hiveql" | "hql" | "influxql" | "influx"
        | "promql" | "kql" | "cypher" | "neo4j" | "sparql" | "cql" | "cassandra" | "redis" => Sql,
        _ => Generic,
    }
}

/// 配色族的默认行注释符（与把注释并入配色族的旧行为一致）。
fn default_line_comments(family: LangFamily) -> &'static [&'static str] {
    use LangFamily::*;
    match family {
        Python | Shell | Data => &["#"],
        Sql => &["--"],
        Rust | JavaScript | CLike | Css | Markup | Generic => &["//"],
    }
}

/// 行注释前缀，按语言而不是按配色族：同一族里 `#` / `%` / `;` / `--` 的差异比关键字还多。
/// 表里只列与所属族默认值不同的语言，其余走 [`default_line_comments`]。
/// 前缀以字母结尾的（`rem`、`dnl`）按整词匹配，其余按字符匹配。
pub fn line_comments_for(lang: &str) -> &'static [&'static str] {
    match lang_key(lang).as_str() {
        // 没有行注释的语言：别让默认的 `//` 把正文里的 URL 染成注释色
        "" | "text" | "txt" | "plain" | "plaintext" | "log" | "logs" | "csv" | "tsv" | "diff"
        | "patch" | "udiff" | "markdown" | "md" | "mdown" | "gfm" | "commonmark" | "rst"
        | "restructuredtext" | "regex" | "regexp" | "json" | "json5" | "jsonl" | "ndjson"
        | "geojson" | "avro" | "avsc" | "plantuml" | "puml" | "uml" | "mathematica" | "nb"
        | "wolfram" | "none" | "cobol" | "cbl" | "smalltalk" | "abap" | "sap" | "xquery" | "xq"
        | "xpath" | "xml" | "rss" | "atom" | "xsd" | "xslt" | "xsl" | "svg" | "plist"
        | "docbook" | "textile" | "mediawiki" | "wiki" | "coq" | "ocaml" | "ml" | "sml" | "vim"
        | "viml" | "gvim" | "nvim" | "brainfuck" => &[],
        // % —— LaTeX 家族、MATLAB/Octave、Prolog/Erlang、MiniZinc、Gnuplot
        "latex" | "tex" | "ltx" | "cls" | "sty" | "dtx" | "beamer" | "bibtex" | "bib"
        | "matlab" | "octave" | "scilab" | "prolog" | "erlang" | "erl" | "minizinc" | "mzn" => {
            &["%"]
        }
        // # 与 ; 都算的配置文件
        "ini" | "cfg" | "conf" | "config" | "properties" | "props" | "editorconfig"
        | "gitconfig" | "gitattributes" | "systemd" | "desktop" => &["#", ";"],
        // ; —— Lisp 家族、汇编、WAT/LLVM IR、G 代码
        "lisp" | "common-lisp" | "scheme" | "scm" | "racket" | "rkt" | "elisp" | "emacs"
        | "emacs-lisp" | "clojure" | "clj" | "cljs" | "edn" | "asm" | "assembly" | "nasm"
        | "masm" | "gas" | "x86" | "arm" | "s" | "wasm" | "webassembly" | "wat" | "wast"
        | "llvm" | "llvmir" | "gcode" | "nc" | "autohotkey" | "ahk" | "spice" | "ngspice" => &[";"],
        // -- —— Lua/Haskell/VHDL/Ada/Agda-Idris，以及默认 `//` 或 `#` 的语言里真正用 -- 的
        "lua" | "luau" | "haskell" | "hs" | "agda" | "idris" | "idris2" | "vhdl" | "ada"
        | "applescript" | "scpt" | "roc" | "unison" | "dhall" | "elm" | "isabelle" => &["--"],
        "sparql" => &["--", "#"],
        "lean" | "lean4" => &["--", "//"],
        // # 与 // 并存：HCL/Nix/JSONC/Thrift/HOCON/Jsonnet
        "terraform" | "tf" | "tfvars" | "hcl" | "nomad" | "vault" | "nix" | "nixos" | "puppet"
        | "jsonc" | "hocon" | "typesafe-config" | "jsonnet" | "libsonnet" | "thrift" | "idl" => {
            &["#", "//"]
        }
        // 归属族给的是 # 或 --，但实际用 //
        "cue" | "prisma" | "cypher" | "neo4j" | "kql" => &["//"],
        "fortran" | "f" | "f77" | "f90" => &["!"],
        "bat" | "cmd" | "dosbatch" | "batch" | "dos" => &["::", "rem"],
        "gherkin" | "cucumber" | "bdd" | "org" | "org-mode" | "redis" | "promql" | "nim"
        | "nims" | "crystal" | "cr" | "gdscript" | "godot" | "graphql" | "gql" | "apollo" => &["#"],
        "mermaid" => &["%%"],
        "m4" | "autoconf" | "aclocal" => &["dnl", "#"],
        "tla" | "tla+" | "tlaplus" => &["\\*"],
        _ => default_line_comments(lang_family(lang)),
    }
}

/// 块注释的（起始串, 结束串）；没有块注释返回 None。
pub fn block_comment_for(lang: &str) -> Option<(&'static str, &'static str)> {
    const C: Option<(&str, &str)> = Some(("/*", "*/"));
    const ML: Option<(&str, &str)> = Some(("<!--", "-->"));
    match lang_key(lang).as_str() {
        "haskell" | "hs" => Some(("{-", "-}")),
        "lua" | "luau" => Some(("--[[", "]]")),
        "pascal" | "delphi" | "objectpascal" | "pas" => Some(("{", "}")),
        "ocaml" | "ml" | "sml" | "coq" | "isabelle" | "fsharp" | "fs" | "f#" | "fsi"
        | "mathematica" | "nb" | "wolfram" => Some(("(*", "*)")),
        "julia" | "jl" => Some(("#=", "=#")),
        "xquery" | "xq" => Some(("(:", ":)")),
        "nix" | "nixos" | "jsonc" => C,
        "sql" | "mysql" | "mariadb" | "postgresql" | "postgres" | "psql" | "plpgsql" | "plsql"
        | "oracle" | "tsql" | "mssql" | "sqlserver" => C,
        "markdown" | "md" | "mdown" | "gfm" | "commonmark" | "asciidoc" | "adoc" | "asc"
        | "rst" | "restructuredtext" | "mediawiki" | "wiki" | "textile" | "pug" | "haml"
        | "slim" => ML,
        _ => match lang_family(lang) {
            LangFamily::Markup => ML,
            LangFamily::Rust | LangFamily::JavaScript | LangFamily::CLike | LangFamily::Css => C,
            _ => None,
        },
    }
}

impl Theme {
    pub fn get(kind: ThemeKind) -> Self {
        let (
            bg,
            panel,
            text,
            dim,
            accent,
            code_bg,
            code_header,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
        ) = match kind {
            // 对齐 VS Code 默认浅色（Light+）：editor #FFFFFF、侧栏 #F3F3F3、链接 #006AB1、选区 #ADD6FF
            // 代码块显示区 #FAFAFB，功能栏 #F0F0F2（比显示区深一档）
            ThemeKind::Light => (
                rgb(255, 255, 255),
                rgb(243, 243, 243),
                rgb(0, 0, 0),
                rgb(139, 139, 139),
                rgb(0, 106, 177),
                rgb(250, 250, 251),
                rgb(240, 240, 242),
                rgb(229, 229, 229),
                rgb(0, 0, 0),
                rgb(0, 106, 177),
                rgb(106, 115, 125),
                // 表格标题行 #F7F7F7：比数据行（#FFFFFF）略深，铺满整个标题行
                rgb(247, 247, 247),
                rgb(173, 214, 255),
            ),
            // 对齐 VS Code 默认深色（Dark+）：editor #1E1E1E、侧栏 #252526、链接 #3794FF、选区 #264F78
            ThemeKind::Dark => (
                rgb(30, 30, 30),
                rgb(37, 37, 38),
                rgb(212, 212, 212),
                rgb(133, 133, 133),
                rgb(55, 148, 255),
                rgb(45, 45, 45),
                rgb(38, 38, 38),
                rgb(69, 69, 69),
                rgb(255, 255, 255),
                rgb(55, 148, 255),
                rgb(156, 163, 175),
                rgb(51, 51, 51),
                rgb(38, 79, 120),
            ),
        };
        Self {
            kind,
            bg,
            panel,
            text,
            dim,
            accent,
            code_bg,
            code_header,
            border,
            heading,
            link,
            quote,
            table_head,
            selection,
            body: 16.0,
            mono: 13.5,
            table_line: if kind == ThemeKind::Light {
                // 表格线统一用 #F3F3F3（与侧栏同色），比通用 border 更浅
                rgb(243, 243, 243)
            } else {
                border
            },
        }
    }

    /// 按主题默认配色 + 用户覆盖构造主题。默认配色仍唯一来源于 `get()`，
    /// 这里只在其上盖一层用户自定义颜色。
    pub fn get_with_overrides(kind: ThemeKind, overrides: &ColorOverrides) -> Self {
        let mut theme = Self::get(kind);
        overrides.apply_to(&mut theme);
        theme
    }

    /// 加粗文字的强调色：中文加粗只靠字重不够醒目，
    /// 每个主题单独配一个暖色，和链接蓝、标题黑拉开距离。
    pub fn bold_color(&self) -> Color32 {
        match self.kind {
            ThemeKind::Light => rgb(180, 83, 9),
            ThemeKind::Dark => rgb(255, 198, 109),
        }
    }

    /// 行内代码背景：浅色用极浅灰蓝（方案 B），深色用低透明度天蓝叠加
    /// （方案 C：rgba(125,211,252,0.12)，叠在正文底色上更柔和）。
    pub fn inline_code_bg(&self) -> Color32 {
        match self.kind {
            ThemeKind::Light => rgb(239, 241, 245),
            ThemeKind::Dark => Color32::from_rgba_unmultiplied(125, 211, 252, 31),
        }
    }

    /// 行内代码文字色：浅色用品红点缀、深色用亮天蓝，和正文拉开层次，
    /// 不至于淹没在大段文字里。
    pub fn inline_code_fg(&self) -> Color32 {
        match self.kind {
            ThemeKind::Light => rgb(214, 51, 108),
            ThemeKind::Dark => rgb(125, 211, 252),
        }
    }

    /// 行内代码描边：浅色方案 B 带一圈很淡的冷灰边，深色方案 C 不描边。
    pub fn inline_code_border(&self) -> Option<Color32> {
        match self.kind {
            ThemeKind::Light => Some(rgb(226, 230, 238)),
            ThemeKind::Dark => None,
        }
    }

    /// 代码高亮配色：[关键字, 字符串, 数字, 注释, 函数名, 类型/标签/属性名]。
    /// 按语言族返回不同配色，浅色对齐 VS Code Light+、深色对齐 Dark+，
    /// 保证每种语言在代码块背景上清晰协调、彼此有区分度。
    pub fn syntax(&self, lang: &str) -> [Color32; 6] {
        let family = lang_family(lang);
        let (kw, st, num, com, fun, typ) = match (self.kind, family) {
            // ---- 浅色 ----
            (ThemeKind::Light, LangFamily::Rust) => (
                rgb(111, 66, 193),
                rgb(3, 47, 98),
                rgb(227, 98, 9),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(0, 128, 128),
            ),
            (ThemeKind::Light, LangFamily::Python) => (
                rgb(0, 0, 255),
                rgb(34, 134, 58),
                rgb(176, 0, 32),
                rgb(112, 128, 144),
                rgb(0, 102, 204),
                rgb(128, 0, 128),
            ),
            (ThemeKind::Light, LangFamily::JavaScript) => (
                rgb(130, 80, 223),
                rgb(10, 48, 105),
                rgb(5, 80, 174),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(0, 128, 128),
            ),
            (ThemeKind::Light, LangFamily::CLike) => (
                rgb(0, 0, 255),
                rgb(163, 21, 21),
                rgb(9, 134, 88),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(43, 145, 175),
            ),
            (ThemeKind::Light, LangFamily::Markup) => (
                rgb(163, 21, 21),
                rgb(163, 21, 21),
                rgb(227, 98, 9),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(163, 21, 21),
            ),
            (ThemeKind::Light, LangFamily::Css) => (
                rgb(111, 66, 193),
                rgb(163, 21, 21),
                rgb(227, 98, 9),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(0, 128, 128),
            ),
            (ThemeKind::Light, LangFamily::Data) => (
                rgb(0, 0, 255),
                rgb(34, 134, 58),
                rgb(227, 98, 9),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(128, 0, 128),
            ),
            (ThemeKind::Light, LangFamily::Shell) => (
                rgb(111, 66, 193),
                rgb(34, 134, 58),
                rgb(227, 98, 9),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(43, 145, 175),
            ),
            (ThemeKind::Light, LangFamily::Sql) => (
                rgb(5, 80, 174),
                rgb(163, 21, 21),
                rgb(0, 128, 128),
                rgb(106, 115, 125),
                rgb(111, 66, 193),
                rgb(0, 128, 128),
            ),
            (ThemeKind::Light, LangFamily::Generic) => (
                rgb(0, 0, 255),
                rgb(163, 21, 21),
                rgb(9, 134, 88),
                rgb(106, 115, 125),
                rgb(0, 102, 204),
                rgb(43, 145, 175),
            ),
            // ---- 深色 ----
            (ThemeKind::Dark, LangFamily::Rust) => (
                rgb(197, 134, 192),
                rgb(206, 145, 120),
                rgb(215, 186, 125),
                rgb(106, 153, 85),
                rgb(86, 156, 214),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::Python) => (
                rgb(197, 134, 192),
                rgb(152, 195, 121),
                rgb(86, 182, 194),
                rgb(92, 99, 112),
                rgb(86, 156, 214),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::JavaScript) => (
                rgb(86, 156, 214),
                rgb(206, 145, 120),
                rgb(181, 206, 168),
                rgb(106, 153, 85),
                rgb(220, 220, 170),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::CLike) => (
                rgb(86, 156, 214),
                rgb(206, 145, 120),
                rgb(181, 206, 168),
                rgb(106, 153, 85),
                rgb(220, 220, 170),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::Markup) => (
                rgb(206, 145, 120),
                rgb(206, 145, 120),
                rgb(215, 186, 125),
                rgb(106, 153, 85),
                rgb(156, 220, 254),
                rgb(206, 145, 120),
            ),
            (ThemeKind::Dark, LangFamily::Css) => (
                rgb(220, 220, 170),
                rgb(206, 145, 120),
                rgb(181, 206, 168),
                rgb(106, 153, 85),
                rgb(86, 156, 214),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::Data) => (
                rgb(86, 156, 214),
                rgb(152, 195, 121),
                rgb(86, 182, 194),
                rgb(106, 153, 85),
                rgb(220, 220, 170),
                rgb(197, 134, 192),
            ),
            (ThemeKind::Dark, LangFamily::Shell) => (
                rgb(197, 134, 192),
                rgb(152, 195, 121),
                rgb(215, 186, 125),
                rgb(106, 153, 85),
                rgb(86, 156, 214),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::Sql) => (
                rgb(86, 156, 214),
                rgb(206, 145, 120),
                rgb(78, 201, 176),
                rgb(106, 153, 85),
                rgb(197, 134, 192),
                rgb(78, 201, 176),
            ),
            (ThemeKind::Dark, LangFamily::Generic) => (
                rgb(86, 156, 214),
                rgb(206, 145, 120),
                rgb(181, 206, 168),
                rgb(106, 153, 85),
                rgb(220, 220, 170),
                rgb(78, 201, 176),
            ),
        };
        [kw, st, num, com, fun, typ]
    }

    pub fn heading_size(&self, level: u8) -> f32 {
        let factor = match level {
            1 => 1.9,
            2 => 1.55,
            3 => 1.3,
            4 => 1.15,
            5 => 1.05,
            _ => 1.0,
        };
        self.body * factor
    }

    /// 光标所在行的整行底色：主题强调色的低透明度版本（约 10%），
    /// 浅色主题下接近 VS Code 的 editor.lineHighlightBackground，不遮挡文字。
    pub fn line_highlight(&self) -> Color32 {
        Color32::from_rgba_unmultiplied(self.accent.r(), self.accent.g(), self.accent.b(), 26)
    }

    /// 行内高亮（`==文字==`）的底色：浅色用暖黄，深色用低饱和琥珀，
    /// 保证黑/白正文都清晰。
    pub fn highlight_bg(&self) -> Color32 {
        if self.kind.is_dark() {
            Color32::from_rgb(92, 74, 18)
        } else {
            Color32::from_rgb(255, 235, 130)
        }
    }

    pub fn fmt(&self, size: f32) -> egui::text::TextFormat {
        egui::text::TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color: self.text,
            ..Default::default()
        }
    }

    pub fn mono_fmt(&self) -> egui::text::TextFormat {
        egui::text::TextFormat {
            font_id: FontId::new(self.mono, FontFamily::Monospace),
            color: self.text,
            ..Default::default()
        }
    }

    pub fn bold_fmt(&self, size: f32, color: Color32) -> egui::text::TextFormat {
        egui::text::TextFormat {
            // 用专门注册的 markd-bold 字族（真粗体字重），与编辑态 inline_job 一致。
            // 不能用 Proportional + wght variation：中文字体多为静态字重，不生效，
            // 表现就是「加粗时是粗的、鼠标离开后变细」。
            font_id: FontId::new(size, FontFamily::Name("markd-bold".into())),
            color,
            ..Default::default()
        }
    }

    pub fn apply(&self, ctx: &Context) {
        let mut visuals = if self.kind.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.override_text_color = Some(self.text);
        visuals.panel_fill = self.panel;
        visuals.window_fill = self.bg;
        visuals.extreme_bg_color = self.code_bg;
        visuals.hyperlink_color = self.link;
        visuals.selection.bg_fill = self.selection;
        visuals.widgets.noninteractive.bg_fill = self.bg;
        visuals.widgets.inactive.bg_fill = self.code_bg;
        visuals.widgets.hovered.bg_fill = self.border;
        visuals.widgets.active.bg_fill = self.selection;
        ctx.set_visuals(visuals);
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    fn colors_eq(a: &Theme, b: &Theme) -> bool {
        ThemeColor::ALL.iter().all(|c| c.rgb_of(a) == c.rgb_of(b))
    }

    #[test]
    fn overrides_only_touch_selected_fields() {
        for kind in ThemeKind::ALL {
            let base = Theme::get(kind);
            let mut o = ColorOverrides::default();
            o.set(ThemeColor::Heading, Some([1, 2, 3]));
            o.set(ThemeColor::Bg, Some([10, 20, 30]));
            let themed = Theme::get_with_overrides(kind, &o);
            assert_eq!(themed.heading, Color32::from_rgb(1, 2, 3));
            assert_eq!(themed.bg, Color32::from_rgb(10, 20, 30));
            // 其余颜色必须与默认主题一致。
            let mut base_rest = base.clone();
            base_rest.heading = themed.heading;
            base_rest.bg = themed.bg;
            assert!(
                colors_eq(&themed, &base_rest),
                "未覆盖的颜色不应被改动 ({:?})",
                kind
            );
            // 字号不受影响。
            assert_eq!(themed.body, base.body);
            assert_eq!(themed.mono, base.mono);
        }
    }

    #[test]
    fn clearing_override_restores_default() {
        let mut o = ColorOverrides::default();
        assert!(o.is_empty());
        o.set(ThemeColor::Link, Some([9, 9, 9]));
        assert!(!o.is_empty());
        let changed = Theme::get_with_overrides(ThemeKind::Light, &o);
        assert_eq!(changed.link, Color32::from_rgb(9, 9, 9));
        o.set(ThemeColor::Link, None);
        assert!(o.is_empty());
        let restored = Theme::get_with_overrides(ThemeKind::Light, &o);
        assert_eq!(restored.link, Theme::get(ThemeKind::Light).link);
    }

    #[test]
    fn defaults_follow_kind() {
        assert_ne!(
            ThemeColor::Heading.default_rgb(ThemeKind::Light),
            ThemeColor::Heading.default_rgb(ThemeKind::Dark)
        );
        assert_eq!(
            ThemeColor::Bg.default_rgb(ThemeKind::Light),
            [255, 255, 255]
        );
    }
}
