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
        .width(150.0)
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
                    .hint_text("搜索语言（如 C++、flutter）")
                    .desired_width(142.0),
            );
            let f = filter.trim().to_lowercase();
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(filter_id, f.clone()));
            // 当前语言可能是手写的写法（```C++ / ```CPP）：解析端统一按小写键，
            // 这里也按小写键比对，手写写法照样能对上对应行。
            let cur = lang_key(current);
            // A→Z 正序；纯文本（""）自然排最前
            let rows = lang_rows();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for r in rows.iter() {
                        if !lang_hit(r, &f) {
                            continue;
                        }
                        let mut resp = ui.selectable_label(cur == r.text, lang_label(r));
                        // 别名行说清它和哪个规范名等价（点谁就把谁写进围栏）
                        if r.text != r.canonical {
                            resp = resp.on_hover_text(format!("与 `{}` 等价", r.canonical));
                        }
                        if resp.clicked() {
                            picked = Some(r.text.to_string());
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

/// 每种语言的写法登记表：`id` 是规范名，`aliases` 是其它常见写法
/// （符号名 `c++`、框架名 `flutter`、文件名 `docker-compose`、扩展名 `py`……）。
/// 下拉里规范名与每个别名**各占一行**，点谁就把谁写进围栏信息行；
/// 大小写不敏感——手写 ```CPP、```C++ 与 `cpp` 一行等价。
/// 每一项都要在 `markd-core::theme::lang_family` / `line_comments_for` /
/// `keywords_for` 里有对应条目，`render::tests` 有用例逐条校验两张表不脱节。
#[derive(Clone, Copy)]
pub struct LangOption {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
}

const fn lang(id: &'static str, aliases: &'static [&'static str]) -> LangOption {
    LangOption { id, aliases }
}

/// 下拉里可点的一行。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LangRow {
    /// 点它时写进围栏信息行的写法
    pub text: &'static str,
    /// 所属规范名，用于「与 cpp 等价」提示
    pub canonical: &'static str,
}

/// 展平后的全部可选项：规范名 + 每个别名各一行，A→Z 排序，纯文本（""）排最前。
pub fn lang_rows() -> Vec<LangRow> {
    let mut rows: Vec<LangRow> = Vec::new();
    for o in LANGUAGE_OPTIONS {
        rows.push(LangRow {
            text: o.id,
            canonical: o.id,
        });
        for a in o.aliases {
            rows.push(LangRow {
                text: a,
                canonical: o.id,
            });
        }
    }
    rows.sort_unstable_by_key(|r| r.text);
    rows
}

/// 行标签：空写法显示「纯文本」。
pub fn lang_label(r: &LangRow) -> &'static str {
    if r.text.is_empty() {
        "纯文本"
    } else {
        r.text
    }
}

/// 搜索关键词是否命中这一行：子串匹配（输入 `sh` 也能看到 bash/zsh）。
pub fn lang_hit(r: &LangRow, query: &str) -> bool {
    query.is_empty() || lang_label(r).contains(query)
}

pub const LANGUAGE_OPTIONS: &[LangOption] = &[
    // ---- 纯文本 / 日志 / 差异 ----
    lang("", &["none", "plain"]),
    lang("text", &["txt"]),
    lang("plaintext", &[]),
    lang("log", &["logs"]),
    lang("diff", &["patch", "udiff"]),
    lang("regex", &["regexp"]),
    // ---- 数据 / 配置 ----
    lang("json", &[]),
    lang("jsonc", &[]),
    lang("json5", &[]),
    lang("jsonl", &["ndjson"]),
    lang(
        "yaml",
        &[
            "yml",
            "k8s",
            "kubernetes",
            "helm",
            "ansible",
            "github-actions",
            "docker-compose",
            "prometheus",
        ],
    ),
    lang("toml", &["cargo"]),
    lang("ini", &["cfg", "conf", "config", "editorconfig", "systemd"]),
    lang("properties", &["props"]),
    lang("env", &["dotenv"]),
    lang("xml", &["rss", "atom", "xsd"]),
    lang("xslt", &["xsl"]),
    lang("xpath", &[]),
    lang("xquery", &["xq"]),
    lang("svg", &[]),
    lang("plist", &[]),
    lang("hocon", &["typesafe-config"]),
    lang("edn", &[]),
    lang("jsonnet", &["libsonnet"]),
    lang("cue", &[]),
    lang("dhall", &[]),
    lang("avro", &["avsc"]),
    lang("protobuf", &["proto"]),
    lang("thrift", &["idl"]),
    lang("graphql", &["gql", "apollo"]),
    lang("prisma", &[]),
    lang("http", &["rest", "curl", "openapi", "swagger"]),
    lang("csv", &[]),
    lang("tsv", &[]),
    // ---- 构建 / 运维 / IaC ----
    lang("dockerfile", &["docker", "containerfile"]),
    lang("makefile", &["make", "gmake", "mk"]),
    lang("cmake", &["cmakelists"]),
    lang("meson", &[]),
    lang("bazel", &["starlark", "bzl"]),
    lang("ninja", &[]),
    lang("m4", &["autoconf"]),
    lang("nix", &["nixos"]),
    lang("terraform", &["tf", "tfvars"]),
    lang("hcl", &["nomad", "vault"]),
    lang("puppet", &[]),
    lang("bicep", &["azure"]),
    lang("nginx", &[]),
    lang("apache", &["httpd", "htaccess"]),
    lang("caddy", &["caddyfile"]),
    lang("crontab", &["cron"]),
    lang("gitignore", &[]),
    lang("gitattributes", &[]),
    lang("gitconfig", &[]),
    lang("po", &["gettext", "pot"]),
    // ---- 命令行 / 脚本 ----
    lang("bash", &["shell", "shell-session", "console", "terminal"]),
    lang("sh", &["bourne"]),
    lang("zsh", &[]),
    lang("fish", &[]),
    lang("ksh", &[]),
    lang("nushell", &["nu"]),
    lang("powershell", &["ps1", "pwsh"]),
    lang("bat", &["cmd", "batch", "dos"]),
    lang("autohotkey", &["ahk"]),
    lang("awk", &["gawk"]),
    lang("sed", &[]),
    lang("jq", &[]),
    lang("tcl", &["expect"]),
    lang("vim", &["viml"]),
    lang("tmux", &[]),
    // ---- 主流编程语言 ----
    lang("rust", &["rs"]),
    lang("c", &["h"]),
    lang("cpp", &["c++", "cxx", "cc"]),
    lang("objective-c", &["objc", "objective-c++"]),
    lang("csharp", &["cs", "c#", "dotnet", "unity"]),
    lang("java", &["jvm", "android"]),
    lang("kotlin", &["kt", "kts"]),
    lang("scala", &["sbt"]),
    lang("groovy", &["gradle", "jenkinsfile"]),
    lang("swift", &["xcode"]),
    lang("dart", &["flutter"]),
    lang("go", &["golang"]),
    lang("python", &["py", "python3", "ipython"]),
    lang("javascript", &["js", "node", "nodejs", "ecmascript", "es6"]),
    lang("typescript", &["ts", "deno", "bun"]),
    lang("jsx", &["react"]),
    lang("tsx", &[]),
    lang("php", &["wordpress"]),
    lang("ruby", &["rb", "rails", "rake"]),
    lang("perl", &["pl", "raku"]),
    lang("lua", &["luau"]),
    lang("r", &["rlang"]),
    lang("julia", &["jl"]),
    lang("matlab", &["octave"]),
    lang("cobol", &[]),
    lang("fortran", &["f90"]),
    lang("pascal", &["delphi"]),
    lang("ada", &[]),
    lang("nim", &[]),
    lang("zig", &[]),
    lang("v", &["vlang"]),
    lang("odin", &[]),
    lang("hare", &[]),
    lang("c3", &[]),
    lang("crystal", &["cr"]),
    lang("d", &["dlang"]),
    lang("vala", &[]),
    lang("haxe", &["hx"]),
    lang("actionscript", &["as3"]),
    lang("coffeescript", &["coffee"]),
    lang("livescript", &["lsc"]),
    lang("1c", &["bsl"]),
    lang("abap", &["sap"]),
    lang("apex", &["salesforce"]),
    lang("applescript", &["scpt"]),
    lang("gherkin", &["cucumber", "bdd"]),
    // ---- 函数式 / 学术 ----
    lang("haskell", &["hs"]),
    lang("ocaml", &["ml"]),
    lang("fsharp", &["fs", "f#"]),
    lang("elixir", &["ex", "phoenix"]),
    lang("erlang", &["erl"]),
    lang("clojure", &["clj", "cljs"]),
    lang("lisp", &["common-lisp"]),
    lang("scheme", &["scm"]),
    lang("racket", &["rkt"]),
    lang("elisp", &["emacs", "emacs-lisp"]),
    lang("prolog", &[]),
    lang("smalltalk", &[]),
    lang("reason", &["re"]),
    lang("rescript", &[]),
    lang("gleam", &[]),
    lang("elm", &[]),
    lang("roc", &[]),
    lang("unison", &[]),
    lang("forth", &["fth"]),
    lang("apl", &[]),
    lang("idris", &[]),
    lang("agda", &[]),
    lang("coq", &[]),
    lang("lean", &["lean4"]),
    lang("isabelle", &[]),
    lang("tla", &["tlaplus", "tla+"]),
    lang("alloy", &[]),
    lang("minizinc", &["mzn"]),
    // ---- 科学计算 / 统计 ----
    lang("mathematica", &["nb", "wolfram"]),
    lang("maxima", &[]),
    lang("scilab", &[]),
    lang("stan", &[]),
    lang("stata", &[]),
    lang("sas", &[]),
    lang("gnuplot", &[]),
    lang("jupyter", &["ipynb"]),
    lang("spice", &["ngspice"]),
    // ---- 数据库 / 查询 ----
    lang("sql", &[]),
    lang("mysql", &["mariadb"]),
    lang("postgresql", &["psql", "plpgsql"]),
    lang("plsql", &["oracle"]),
    lang("tsql", &["mssql", "sqlserver"]),
    lang("sqlite", &["sqlite3"]),
    lang("duckdb", &[]),
    lang("clickhouse", &[]),
    lang("bigquery", &["bq"]),
    lang("hive", &["hiveql", "hql"]),
    lang("influxql", &["influx"]),
    lang("promql", &[]),
    lang("kql", &[]),
    lang("cypher", &["neo4j"]),
    lang("gremlin", &[]),
    lang("sparql", &[]),
    lang("mongo", &["mongosh", "mongodb"]),
    lang("redis", &[]),
    lang("cql", &["cassandra"]),
    // ---- Web / 样式 / 模板 ----
    lang("html", &["htm", "xhtml"]),
    lang("css", &["stylesheet"]),
    lang("scss", &[]),
    lang("sass", &["indented"]),
    lang("less", &[]),
    lang("stylus", &["styl"]),
    lang("postcss", &[]),
    lang("vue", &["nuxt"]),
    lang("svelte", &["sveltekit"]),
    lang("astro", &[]),
    lang("mdx", &[]),
    lang("handlebars", &["hbs"]),
    lang("mustache", &[]),
    lang("ejs", &[]),
    lang("pug", &["jade"]),
    lang("haml", &[]),
    lang("slim", &[]),
    lang("twig", &["symfony"]),
    lang("liquid", &["shopify"]),
    lang("jinja2", &["jinja", "j2"]),
    lang("django", &[]),
    lang("blade", &["laravel"]),
    lang("erb", &[]),
    lang("razor", &["cshtml"]),
    lang("velocity", &[]),
    // ---- 标记 / 排版 / 图表 ----
    lang("markdown", &["md", "gfm", "commonmark"]),
    lang("rst", &["restructuredtext"]),
    lang("asciidoc", &["adoc"]),
    lang("org", &["org-mode"]),
    lang("mediawiki", &["wiki"]),
    lang("textile", &[]),
    lang("latex", &["tex", "beamer", "cls", "sty"]),
    lang("bibtex", &["bib"]),
    lang("mermaid", &[]),
    lang("plantuml", &["puml", "uml"]),
    lang("dot", &["graphviz", "gv"]),
    // ---- 底层 / 硬件 / 着色器 ----
    lang("asm", &["assembly", "nasm", "masm", "gas"]),
    lang("wasm", &["webassembly"]),
    lang("wat", &["wast"]),
    lang("llvm", &["llvmir"]),
    lang("verilog", &["systemverilog", "sv"]),
    lang("vhdl", &[]),
    lang("glsl", &["shader"]),
    lang("hlsl", &["directx"]),
    lang("wgsl", &["webgpu"]),
    lang("metal", &["msl"]),
    lang("cuda", &["nvcc"]),
    lang("opencl", &["ocl"]),
    lang("gcode", &["nc"]),
    lang("arduino", &["ino", "sketch"]),
    lang("processing", &["pde"]),
    lang("gdscript", &["godot"]),
    lang("qml", &["qt"]),
    lang("supercollider", &[]),
    // ---- 智能合约 / Web3 ----
    lang("solidity", &["sol", "evm"]),
    lang("vyper", &[]),
    lang("cairo", &[]),
    lang("move", &["sui", "aptos"]),
    lang("motoko", &["dfinity"]),
    lang("sway", &[]),
];
