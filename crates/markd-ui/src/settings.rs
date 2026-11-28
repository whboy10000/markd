//! Author: aurelianus <aurelianus@126.com>

//! 设置窗口：左侧「搜索 + 分组导航」，右侧该页的设置项，WorkBuddy 风格左右分栏。
//! 窗口本身是独立的系统窗口（副视口），能拖出主窗口、用系统标题栏和边角缩放。

use crate::theme::{Theme, ThemeColor, ThemeKind};
use crate::workspace::{Config, Language};
use egui::RichText;

/// 右侧内容区的一页，同时也是左侧导航里的一项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    General,
    Appearance,
    Shortcuts,
    Personalize,
    Editor,
    Files,
    Images,
    Markdown,
    Export,
}

/// 导航分组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    General,
    Features,
    Content,
}

const GROUPS: [Group; 3] = [Group::General, Group::Features, Group::Content];

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::General => "通用",
            Group::Features => "功能",
            Group::Content => "内容",
        }
    }
}

const PAGES: [Page; 9] = [
    Page::General,
    Page::Appearance,
    Page::Shortcuts,
    Page::Personalize,
    Page::Editor,
    Page::Files,
    Page::Images,
    Page::Markdown,
    Page::Export,
];

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::General => "常规",
            Page::Appearance => "外观",
            Page::Shortcuts => "快捷键",
            Page::Personalize => "个性化",
            Page::Editor => "编辑器",
            Page::Files => "文件",
            Page::Images => "图像",
            Page::Markdown => "Markdown",
            Page::Export => "导出",
        }
    }

    fn group(self) -> Group {
        match self {
            Page::General | Page::Appearance | Page::Shortcuts => Group::General,
            Page::Personalize | Page::Editor => Group::Features,
            Page::Files | Page::Images | Page::Markdown | Page::Export => Group::Content,
        }
    }
}

/// 搜索索引：每条是 (设置项标题, 额外关键词, 所在页)。
/// 标题必须能在对应页里原样渲染出来——`tests::search_index_matches_rendered_titles`
/// 逐条比对，改了界面上的文案却忘了改索引会直接红。
const SEARCH: &[(&str, &str, Page)] = &[
    ("语言", "中文 english 界面语言", Page::General),
    ("启动时恢复上次打开的文件", "恢复 上次 启动", Page::General),
    ("退出前确认", "关闭 确认 未保存", Page::General),
    (
        "主题",
        "浅色 深色 跟随系统 夜间 dark light",
        Page::Appearance,
    ),
    ("自定义颜色", "配色 颜色 背景 文字 强调色", Page::Appearance),
    ("界面缩放", "放大 缩小 比例 字号整体", Page::Personalize),
    ("侧栏宽度", "文件栏 宽度", Page::Personalize),
    ("显示侧栏标签栏", "侧栏 标签", Page::Personalize),
    ("显示工作区栏", "工作区 顶栏", Page::Personalize),
    ("显示格式工具条", "工具条 按钮", Page::Personalize),
    ("字体大小（正文）", "字号 font 文字大小", Page::Editor),
    ("源码模式行距", "行高 行间距 leading", Page::Editor),
    ("Tab 宽度", "缩进 制表位", Page::Editor),
    ("输入括号/引号时自动补全", "自动配对 括号", Page::Editor),
    ("显示行号栏", "行号 源码", Page::Editor),
    ("自动保存", "保存 定时", Page::Editor),
    ("自动保存间隔", "秒 时间", Page::Editor),
    ("源码模式", "markdown 纯文本 切换", Page::Editor),
    ("专注模式", "打字 高亮当前段", Page::Editor),
    ("打字机模式", "光标居中", Page::Editor),
    ("默认工作区", "目录 文件夹 路径", Page::Files),
    ("插入图片时复制到资源目录", "图片 资源 拷贝", Page::Images),
    ("资源目录名", "imgs 目录", Page::Images),
    ("使用相对路径引用图片", "相对路径 图片", Page::Images),
    (
        "平台",
        "图床 github gitee oss 七牛 火山  bos obs cos",
        Page::Images,
    ),
    ("目录", "保存目录 前缀 图床", Page::Images),
    ("保存路径", "模板 年月日 时间戳", Page::Images),
    ("测试上传", "图床 连通 上传", Page::Images),
    ("数学公式 $…$ / $$…$$", "latex 公式 math", Page::Markdown),
    ("Mermaid / 流程图", "图表 流程图", Page::Markdown),
    ("目录 [TOC]", "生成目录", Page::Markdown),
    ("脚注", "footnote", Page::Markdown),
    ("代码块语法高亮", "高亮 代码", Page::Markdown),
    ("标题自动编号（预览）", "编号 标题", Page::Markdown),
    ("导出 HTML 时内嵌图片", "html 导出 内嵌", Page::Export),
    ("导出完成后打开文件", "导出 打开", Page::Export),
    (
        "Pandoc 输出格式",
        "pandoc docx pdf epub pptx html 转换 导出",
        Page::Export,
    ),
    (
        "Pandoc 可执行文件路径",
        "pandoc 外部 转换 导出 路径",
        Page::Export,
    ),
    ("Pandoc 额外参数", "pandoc 参数 pdf-engine 命令行", Page::Export),
    (
        "PDF 打印浏览器路径",
        "pdf 浏览器 chrome edge chromium 打印",
        Page::Export,
    ),
];

/// 主题下拉的三档视图。「跟随系统」不是一个配色档，所以不塞进 `ThemeKind`，
/// 而是单独一个配置布尔位：选了浅/深色就顺手清掉它（见 `Config::set_theme`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThemeChoice {
    Light,
    Dark,
    System,
}

impl ThemeChoice {
    const ALL: [ThemeChoice; 3] = [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System];

    fn name(self) -> &'static str {
        match self {
            ThemeChoice::Light => "浅色",
            ThemeChoice::Dark => "深色",
            ThemeChoice::System => "跟随系统",
        }
    }
}

fn theme_choice(cfg: &Config) -> ThemeChoice {
    if cfg.theme_follow_system {
        ThemeChoice::System
    } else {
        match cfg.theme {
            ThemeKind::Light => ThemeChoice::Light,
            ThemeKind::Dark => ThemeChoice::Dark,
        }
    }
}

/// 快捷键表：从原「快捷键一览」小窗搬进来，设置导航的快捷键页是唯一出处。
const SHORTCUTS: &[(&str, &str)] = &[
    ("Cmd/Ctrl + S", "保存：已有路径直接写回，未命名等同另存为"),
    ("Cmd/Ctrl + Shift + S", "另存为：弹窗选择保存路径"),
    ("Cmd/Ctrl + O", "浏览并打开 MD 文件"),
    (
        "Cmd/Ctrl + Shift + O / P",
        "快速打开（按文件名找当前工作区）/ 命令面板",
    ),
    ("Cmd/Ctrl + N", "新建文档"),
    ("Cmd/Ctrl + 0 / 1 … 6", "段落、一至六级标题"),
    ("Cmd/Ctrl + B / I / U", "加粗 / 斜体 / 下划线"),
    ("Cmd/Ctrl + K", "插入链接"),
    ("Cmd/Ctrl + Shift + H", "高亮 ==文本=="),
    ("Cmd/Ctrl + Shift + `", "行内代码"),
    ("Ctrl + Shift + `", "删除线（macOS 额外识别该组合）"),
    ("Cmd/Ctrl + \\", "清除格式"),
    ("Cmd + Option + C / B", "代码块 / 公式块"),
    ("Cmd + Option + T / Q", "表格 / 引用"),
    ("Cmd + Option + O / U / X", "有序 / 无序 / 任务列表"),
    ("Cmd + Option + −", "分隔线"),
    ("Cmd/Ctrl + F", "文档内查找"),
    ("Cmd + Option + F / Ctrl + H", "查找并替换 / 文档内替换"),
    ("Cmd + Ctrl + I", "插入图片（macOS）"),
    ("Cmd/Ctrl + E 或 /", "切换源码模式"),
    ("Cmd/Ctrl + Shift + F", "切换专注模式"),
    ("Cmd/Ctrl + Shift + L", "显示 / 隐藏侧边栏"),
    ("Cmd + Ctrl + 1", "显示大纲（macOS）"),
    ("F8 / F9", "专注 / 打字机模式"),
    ("Cmd/Ctrl + J", "显示 / 隐藏终端面板"),
    ("Cmd/Ctrl + Z / Shift + Z", "撤销 / 重做"),
    ("Cmd/Ctrl + Shift + E", "导出 HTML"),
    ("Cmd/Ctrl + ,", "打开设置窗口"),
    ("F1", "快捷键（本页）"),
    ("Enter / Shift + Tab", "拆分块 / 减少列表缩进"),
    ("Cmd/Ctrl + 单击链接", "打开链接（编辑态内不会误触发）"),
    ("Esc", "关闭查找面板 / 菜单 / 弹窗"),
];

/// 一行设置项：左侧标题 + 说明，右侧控件。`focus` 命中的行滚动到位并描一圈强调色。
/// 控件闭包只画控件本身，返回是否改动。
fn row(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    desc: &str,
    focus: Option<&str>,
    add: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    const RIGHT_W: f32 = 250.0;
    let left_w = (ui.available_width() - RIGHT_W - ui.spacing().item_spacing.x).max(120.0);
    let mut changed = false;
    let resp = ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(left_w);
            ui.label(title);
            if !desc.is_empty() {
                ui.label(RichText::new(desc).small().color(theme.dim));
            }
        });
        ui.vertical(|ui| {
            ui.set_width(RIGHT_W);
            changed = add(ui);
        });
    });
    let resp = resp.response;
    if focus == Some(title) {
        resp.scroll_to_me(Some(egui::Align::Center));
        ui.painter().rect_stroke(
            resp.rect.expand(4.0),
            6.0,
            egui::Stroke::new(1.5, theme.accent),
            egui::StrokeKind::Outside,
        );
    } else {
        // 行与行之间留一点呼吸，说明行不会贴到上一行。
        ui.add_space(2.0);
    }
    changed
}

/// 布尔项的一行：右侧一个不带文字的复选框（0.37 没有 Switch 控件）。
fn bool_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    desc: &str,
    focus: Option<&str>,
    value: &mut bool,
) -> bool {
    row(ui, theme, title, desc, focus, |ui| {
        ui.checkbox(value, "").changed()
    })
}

/// 一行标题（页内分区用），与小节说明。
fn section(ui: &mut egui::Ui, theme: &Theme, title: &str, desc: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong());
    if !desc.is_empty() {
        ui.label(RichText::new(desc).small().color(theme.dim));
    }
    ui.add_space(2.0);
}

/// 设置窗口的副视口 Id。同时只开一个，所以固定。
fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("settings")
}

/// 独立系统窗口的参数。`inner_size` 只是新建时的初始尺寸，之后由系统记住用户拉成的尺寸；
/// 最小宽度按「220 导航 + 图床那行 300pt 输入框 + 边距」算，再窄就要裁掉控件。
fn viewport_builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title("设置")
        .with_inner_size([860.0, 500.0])
        .with_min_inner_size([700.0, 380.0])
        .with_resizable(true)
}

/// egui 面板文字默认 13pt，比正文（16pt）小一档，在设置里明显偏小。
const TEXT_BUMP: f32 = 2.0;

/// 把这一层 Ui 的文字样式整体加 `TEXT_BUMP`，只影响设置窗口。
/// 只动字号不动控件尺寸：按钮、滑块的高度由文字撑开，会自然跟着长高。
fn bump_text(ui: &mut egui::Ui) {
    for (_, font) in ui.style_mut().text_styles.iter_mut() {
        font.size += TEXT_BUMP;
    }
}

/// 图床表单的一行：右对齐标签 + 定宽输入框，返回内容是否改动。
/// `secret` 打码（令牌 / AccessKey Secret），`hint` 是空值时的灰字提示。
fn host_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    width: f32,
    hint: &str,
    secret: bool,
) -> bool {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(label);
    });
    let mut text = egui::TextEdit::singleline(value).desired_width(width);
    if secret {
        text = text.password(true);
    }
    if !hint.is_empty() {
        text = text.hint_text(hint);
    }
    ui.add(text).changed()
}

#[derive(Default)]
pub struct Settings {
    pub open: bool,
    page: Option<Page>,
    /// 导航顶部的搜索框内容。
    query: String,
    /// 搜索命中后要定位的设置项标题；只在跳转那一帧生效（每帧开头 take 掉）。
    focus: Option<String>,
    /// 后台「测试上传」的结果通道：网络请求不能阻塞 UI 线程。
    host_test: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    host_test_status: String,
}

impl Settings {
    pub fn open(&mut self, ctx: &egui::Context) {
        self.goto(ctx, None);
    }

    /// 「快捷键一览」入口：打开设置并直接停在快捷键页。
    pub fn open_shortcuts(&mut self, ctx: &egui::Context) {
        self.goto(ctx, Some(Page::Shortcuts));
    }

    fn goto(&mut self, ctx: &egui::Context, page: Option<Page>) {
        if let Some(p) = page {
            self.page = Some(p);
        }
        if self.open {
            // 已经开着（比如又按了一次 Cmd+,）：把它带到前面就够了。
            ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Focus);
            return;
        }
        self.open = true;
        if self.page.is_none() {
            self.page = Some(Page::General);
        }
    }

    /// 画设置窗口。返回 (是否有改动, 是否点了「恢复默认」)。
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, cfg: &mut Config) -> (bool, bool) {
        if !self.open {
            return (false, false);
        }
        let mut changed = false;
        let mut reset = false;
        let mut close = false;
        let mut page = self.page.unwrap_or(Page::General);
        // 定位只生效一帧：取出来用完就空，下一帧不再画强调框。
        let focus = self.focus.take();

        // 用 immediate 副视口：闭包在当前帧内同步跑完，所以能直接借用 cfg 和 self，
        // 不必像 deferred 那样把状态包进 Arc<Mutex<_>>。代价是父子窗口联动重绘——
        // 设置窗口只在有交互时才动，这点重复开销可以接受。
        // 后端不支持多视口时（单测就是这个条件）egui 自动退化成画在主窗口里的
        // egui::Window，下面的布局和交互逻辑两种情况下是同一套。
        ctx.show_viewport_immediate(viewport_id(), viewport_builder(), |ui, _class| {
            // 系统红点关闭：下一帧不再显示这个视口，窗口由后端销毁。
            if ui.input(|i| i.viewport().close_requested()) {
                close = true;
            }
            bump_text(ui);
            let block = |x: i8, y: i8| {
                egui::Frame::new()
                    .fill(theme.bg)
                    .inner_margin(egui::Margin::symmetric(x, y))
            };
            // 三个区域各自铺满主题底色：副视口每帧清屏成透明黑，内容不铺满就会透出黑底。
            egui::Panel::bottom("settings_footer")
                .frame(block(12, 10))
                .show(ui, |ui| {
                    if ui.button("恢复默认").clicked() {
                        reset = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("完成").clicked() {
                            close = true;
                        }
                    });
                });
            // 左侧导航：0.37 已把 SidePanel 合并成 Panel::left。
            egui::Panel::left("settings_nav")
                .exact_size(220.0)
                .frame(block(10, 10))
                .show(ui, |ui| {
                    self.show_sidebar(ui, theme, &mut page);
                });
            egui::CentralPanel::default()
                .frame(block(16, 10))
                .show(ui, |ui| {
                    // 内容区必须可滚动：页面比窗口高时（外观页那 14 行取色器）
                    // 靠这里撑出无限高度，`row` 的搜索定位也靠它 scroll_to_me。
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            changed |= self.show_content(ui, theme, cfg, page, focus.as_deref());
                        });
                });
        });

        self.open = !close;
        self.page = Some(page);
        (changed, reset)
    }

    /// 搜索命中：先是页名命中的整页，再是索引里命中的具体设置项。
    /// 空查询返回空表（那时导航列的是分组清单，不是命中）。
    fn matches(&self) -> Vec<(&'static str, Page)> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let hit = |hay: String| hay.to_lowercase().contains(&q);
        let mut out: Vec<(&'static str, Page)> = PAGES
            .iter()
            .filter(|p| hit(format!("{} {}", p.title(), p.group().title())))
            .map(|p| (p.title(), *p))
            .collect();
        out.extend(
            SEARCH
                .iter()
                .filter(|(title, keys, p)| hit(format!("{title} {keys} {}", p.title())))
                .map(|(title, _, p)| (*title, *p)),
        );
        out
    }

    /// 左侧导航：顶部搜索框 + 分组条目；有查询词时改成列命中项。
    fn show_sidebar(&mut self, ui: &mut egui::Ui, theme: &Theme, page: &mut Page) {
        let w = ui.available_width();
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("搜索设置项…")
                .desired_width(w),
        );
        let hits = self.matches();
        if search.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some((title, target)) = hits.first() {
                *page = *target;
                self.focus = Some(title.to_string());
            }
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.query.trim().is_empty() {
                    for group in GROUPS {
                        ui.label(RichText::new(group.title()).strong().color(theme.dim));
                        for p in PAGES.iter().filter(|p| p.group() == group) {
                            if ui.selectable_label(*page == *p, p.title()).clicked() {
                                *page = *p;
                            }
                        }
                        ui.add_space(8.0);
                    }
                } else if hits.is_empty() {
                    ui.label(RichText::new("没有匹配的设置项").small().color(theme.dim));
                } else {
                    for (title, target) in &hits {
                        // 页名命中时右侧标分组，具体项命中时标所在页；
                        // 只有后者需要滚动定位（前者整页从头看）。
                        let badge = if *title == target.title() {
                            target.group().title()
                        } else {
                            target.title()
                        };
                        let item_hit = *title != target.title();
                        ui.horizontal(|ui| {
                            if ui.selectable_label(*page == *target, *title).clicked() {
                                *page = *target;
                                if item_hit {
                                    self.focus = Some(title.to_string());
                                }
                            }
                            ui.label(RichText::new(badge).small().color(theme.dim));
                        });
                    }
                }
            });
    }

    /// 右侧内容区：按当前页渲染设置表单。每行是「左标题+说明 / 右控件」。
    fn show_content(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        cfg: &mut Config,
        page: Page,
        focus: Option<&str>,
    ) -> bool {
        let mut changed = false;
        // 页名只在内容区顶部写一次，各页里不再重复。
        ui.label(RichText::new(page.title()).strong());
        ui.separator();
        match page {
            Page::General => {
                let lang = cfg.language;
                let mut picked = lang;
                changed |= row(
                    ui,
                    theme,
                    "语言",
                    "界面语言。目前只写入配置文件，界面文字还没接多语言，切换后暂时不变。",
                    focus,
                    |ui| {
                        egui::ComboBox::from_id_salt("settings_language")
                            .selected_text(lang.name())
                            .show_ui(ui, |ui| {
                                for l in Language::ALL {
                                    ui.selectable_value(&mut picked, l, l.name());
                                }
                            });
                        picked != lang
                    },
                );
                if picked != lang {
                    cfg.language = picked;
                }
                changed |= bool_row(
                    ui,
                    theme,
                    "启动时恢复上次打开的文件",
                    "下次启动自动打开上次的文档。",
                    focus,
                    &mut cfg.restore_last_file,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "退出前确认",
                    "有未保存修改时，关窗前先问一句。",
                    focus,
                    &mut cfg.confirm_on_exit,
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!(
                        "Markd {} · 纯 Rust + egui 实现的 Markdown 编辑器",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .small()
                    .color(theme.dim),
                );
            }
            Page::Appearance => {
                let choice = theme_choice(cfg);
                let mut picked = choice;
                let mut apply = false;
                changed |= row(
                    ui,
                    theme,
                    "主题",
                    "浅色 / 深色 / 跟随系统。选「跟随系统」时系统切深浅档会立刻跟着换。",
                    focus,
                    |ui| {
                        egui::ComboBox::from_id_salt("settings_theme")
                            .selected_text(choice.name())
                            .show_ui(ui, |ui| {
                                for c in ThemeChoice::ALL {
                                    ui.selectable_value(&mut picked, c, c.name());
                                }
                            });
                        if picked == choice {
                            false
                        } else {
                            apply = true;
                            true
                        }
                    },
                );
                if apply {
                    match picked {
                        ThemeChoice::System => cfg.theme_follow_system = true,
                        ThemeChoice::Light => cfg.set_theme(ThemeKind::Light),
                        ThemeChoice::Dark => cfg.set_theme(ThemeKind::Dark),
                    }
                }
                section(
                    ui,
                    theme,
                    "自定义颜色",
                    "修改后立即生效；「恢复默认」只清除该项并跟随当前主题。",
                );
                // 默认值取自「当前实际生效的那一档」（Theme::get 是默认配色唯一来源）：
                // 用 theme.kind 而不是 cfg.theme，跟随系统时才是系统那一档的默认色。
                let defaults = Theme::get(theme.kind);
                egui::Grid::new("settings_theme_colors")
                    .num_columns(3)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        for c in ThemeColor::ALL {
                            ui.label(c.label());
                            let customized = cfg.color_overrides.get(c).is_some();
                            let mut rgb = cfg
                                .color_overrides
                                .get(c)
                                .unwrap_or_else(|| c.rgb_of(&defaults));
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                cfg.color_overrides.set(c, Some(rgb));
                                changed = true;
                            }
                            if ui
                                .add_enabled(customized, egui::Button::new("恢复默认"))
                                .on_hover_text("恢复为当前主题的默认颜色")
                                .clicked()
                            {
                                cfg.color_overrides.set(c, None);
                                changed = true;
                            }
                            ui.end_row();
                        }
                    });
            }
            Page::Shortcuts => {
                section(
                    ui,
                    theme,
                    "快捷键",
                    "下面是当前生效的组合；写 Cmd/Ctrl 的项两个键都能按。",
                );
                egui::Grid::new("settings_shortcuts")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for (key, what) in SHORTCUTS {
                            ui.label(RichText::new(*key).monospace().color(theme.accent));
                            ui.label(*what);
                            ui.end_row();
                        }
                    });
            }
            Page::Personalize => {
                changed |= row(
                    ui,
                    theme,
                    "界面缩放",
                    "整个界面的缩放倍数，包含这个窗口。",
                    focus,
                    |ui| {
                        ui.add(egui::Slider::new(&mut cfg.ui_scale, 0.8..=1.6).text("倍"))
                            .changed()
                    },
                );
                changed |= row(
                    ui,
                    theme,
                    "侧栏宽度",
                    "文件栏与大纲栏的宽度。",
                    focus,
                    |ui| {
                        ui.add(egui::Slider::new(&mut cfg.sidebar_width, 160.0..=520.0).text("pt"))
                            .changed()
                    },
                );
                section(ui, theme, "界面元素", "关掉后编辑区直接顶到窗口边缘。");
                changed |= bool_row(
                    ui,
                    theme,
                    "显示侧栏标签栏",
                    "侧栏顶部的「文件 / 大纲」切换条。",
                    focus,
                    &mut cfg.show_sidebar_header,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "显示工作区栏",
                    "窗口顶部显示工作区名与搜索入口。",
                    focus,
                    &mut cfg.show_workspace_bar,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "显示格式工具条",
                    "编辑区上方那排加粗 / 标题 / 列表按钮。",
                    focus,
                    &mut cfg.show_toolbar,
                );
            }
            Page::Editor => {
                changed |= row(
                    ui,
                    theme,
                    "字体大小（正文）",
                    "编辑区正文字号；代码与公式按比例缩放。",
                    focus,
                    |ui| {
                        ui.add(egui::Slider::new(&mut cfg.font_size, 12.0..=26.0).text("pt"))
                            .changed()
                    },
                );
                changed |= row(
                    ui,
                    theme,
                    "源码模式行距",
                    "源码模式每行高度 = 字号 × 这个倍数。",
                    focus,
                    |ui| {
                        ui.add(egui::Slider::new(&mut cfg.line_height, 1.0..=2.4).text("倍"))
                            .changed()
                    },
                );
                changed |= row(
                    ui,
                    theme,
                    "Tab 宽度",
                    "一个 Tab 占的空格数。",
                    focus,
                    |ui| {
                        ui.add(egui::DragValue::new(&mut cfg.tab_size).range(1..=8))
                            .changed()
                    },
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "输入括号/引号时自动补全",
                    "打「(」自动补上「)」。",
                    focus,
                    &mut cfg.auto_pair,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "显示行号栏",
                    "源码模式与混合模式都会显示行号；关闭后正文占满整个编辑区。",
                    focus,
                    &mut cfg.show_line_numbers,
                );
                section(ui, theme, "保存", "");
                changed |= bool_row(
                    ui,
                    theme,
                    "自动保存",
                    "边打字边写盘，不用按保存。",
                    focus,
                    &mut cfg.auto_save,
                );
                changed |= row(
                    ui,
                    theme,
                    "自动保存间隔",
                    "两次自动写盘之间的最短间隔。",
                    focus,
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut cfg.auto_save_interval)
                                .range(5..=600)
                                .suffix(" 秒"),
                        )
                        .changed()
                    },
                );
                section(ui, theme, "写作模式（互斥）", "");
                changed |= bool_row(
                    ui,
                    theme,
                    "源码模式",
                    "整篇按 Markdown 源码显示。",
                    focus,
                    &mut cfg.source_mode,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "专注模式",
                    "只点亮当前段落，其余压暗。",
                    focus,
                    &mut cfg.focus_mode,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "打字机模式",
                    "当前行保持在视口中间。",
                    focus,
                    &mut cfg.typewriter,
                );
            }
            Page::Files => {
                section(
                    ui,
                    theme,
                    "默认工作区",
                    "启动时打开的目录；留空表示不固定工作区。",
                );
                let mut ws = cfg.workspace.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    // 给「选择…」留固定宽度，输入框吃剩下的，避免按钮被挤到下一行。
                    let w = (ui.available_width() - 84.0).max(120.0);
                    let edited = ui
                        .add(egui::TextEdit::singleline(&mut ws).desired_width(w))
                        .changed();
                    let picked = if ui.button("选择…").clicked() {
                        match rfd::FileDialog::new().pick_folder() {
                            Some(p) => {
                                ws = p.to_string_lossy().to_string();
                                true
                            }
                            None => false,
                        }
                    } else {
                        false
                    };
                    if edited || picked {
                        cfg.workspace = if ws.trim().is_empty() {
                            None
                        } else {
                            Some(ws.clone())
                        };
                        changed = true;
                    }
                });
                let last = cfg
                    .last_file
                    .clone()
                    .unwrap_or_else(|| "（无）".to_string());
                ui.label(
                    RichText::new(format!("上次打开：{last}"))
                        .small()
                        .color(theme.dim),
                );
            }
            Page::Images => {
                changed |= bool_row(
                    ui,
                    theme,
                    "插入图片时复制到资源目录",
                    "拖进来的图片先拷一份到文档旁边的资源目录。",
                    focus,
                    &mut cfg.insert_image_to_assets,
                );
                changed |= row(
                    ui,
                    theme,
                    "资源目录名",
                    "相对文档目录的文件夹名。",
                    focus,
                    |ui| ui.text_edit_singleline(&mut cfg.image_assets_dir).changed(),
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "使用相对路径引用图片",
                    "关掉就写绝对路径。",
                    focus,
                    &mut cfg.image_relative,
                );
                section(
                    ui,
                    theme,
                    "图床",
                    "开启后插入的图片上传到远端，文档里直接写公网可读的 URL。",
                );
                changed |= row(
                    ui,
                    theme,
                    "平台",
                    "选一个已配好的图床；不上传就留「不使用」。",
                    focus,
                    |ui| {
                        let mut kind = cfg.image_host;
                        egui::ComboBox::from_id_salt("settings_image_host")
                            .selected_text(kind.label())
                            .show_ui(ui, |ui| {
                                for k in crate::image_host::ImageHostKind::ALL {
                                    ui.selectable_value(&mut kind, k, k.label());
                                }
                            });
                        if kind == cfg.image_host {
                            false
                        } else {
                            cfg.image_host = kind;
                            true
                        }
                    },
                );
                if cfg.image_host.is_enabled() {
                    // 每个字段单独一行：标签右对齐 + 输入框统一宽度，
                    // 避免两列长短不一挤在一起。仓库平台问用户名/仓库/分支/令牌，
                    // OSS 问地域/存储空间/AccessKey，七牛问区域/空间名称/密钥/访问域名，
                    // 火山引擎 TOS 问地域/存储桶/两把密钥，目录与保存路径是公用的。
                    let w = 300.0;
                    let oss = cfg.image_host == crate::image_host::ImageHostKind::Oss;
                    let qiniu = cfg.image_host == crate::image_host::ImageHostKind::Qiniu;
                    let volc = cfg.image_host == crate::image_host::ImageHostKind::Volcengine;
                    let bos = cfg.image_host == crate::image_host::ImageHostKind::Baidu;
                    let obs = cfg.image_host == crate::image_host::ImageHostKind::Huawei;
                    let cos = cfg.image_host == crate::image_host::ImageHostKind::Tencent;
                    egui::Grid::new("settings_image_host_grid")
                        .num_columns(2)
                        .min_col_width(52.0)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            if cos {
                                changed |= host_row(
                                    ui,
                                    "地域",
                                    &mut cfg.image_host_cos_region,
                                    w,
                                    "ap-nanjing，或「南京」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "存储桶",
                                    &mut cfg.image_host_cos_bucket,
                                    w,
                                    "形如 名称-APPID",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "SecretId",
                                    &mut cfg.image_host_cos_secret_id,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "SecretKey",
                                    &mut cfg.image_host_cos_secret_key,
                                    w,
                                    "只在创建时可见，丢了重新创建",
                                    true,
                                );
                                ui.end_row();
                            } else if obs {
                                changed |= host_row(
                                    ui,
                                    "区域",
                                    &mut cfg.image_host_obs_region,
                                    w,
                                    "cn-east-3，或「华东-上海一」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "桶名",
                                    &mut cfg.image_host_obs_bucket,
                                    w,
                                    "Bucket 名",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "访问密钥 ID",
                                    &mut cfg.image_host_obs_access_key,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "秘密访问密钥",
                                    &mut cfg.image_host_obs_secret_key,
                                    w,
                                    "只在创建时可见，丢了重新创建",
                                    true,
                                );
                                ui.end_row();
                            } else if bos {
                                changed |= host_row(
                                    ui,
                                    "地域",
                                    &mut cfg.image_host_bos_region,
                                    w,
                                    "bj，或「华北-北京」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "存储桶",
                                    &mut cfg.image_host_bos_bucket,
                                    w,
                                    "Bucket 名",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "AccessKey",
                                    &mut cfg.image_host_bos_access_key,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "Secret Access Key",
                                    &mut cfg.image_host_bos_secret_key,
                                    w,
                                    "只在创建时可见，丢了重新生成",
                                    true,
                                );
                                ui.end_row();
                            } else if volc {
                                changed |= host_row(
                                    ui,
                                    "地域",
                                    &mut cfg.image_host_volc_region,
                                    w,
                                    "cn-beijing，或「华北2（北京）」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "存储桶",
                                    &mut cfg.image_host_volc_bucket,
                                    w,
                                    "Bucket 名",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "AccessKey",
                                    &mut cfg.image_host_volc_access_key,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "Secret Access Key",
                                    &mut cfg.image_host_volc_secret_key,
                                    w,
                                    "只在创建时可见，丢了重新生成",
                                    true,
                                );
                                ui.end_row();
                            } else if qiniu {
                                changed |= host_row(
                                    ui,
                                    "区域",
                                    &mut cfg.image_host_qiniu_region,
                                    w,
                                    "z2，或「华南-广东」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "空间名称",
                                    &mut cfg.image_host_qiniu_bucket,
                                    w,
                                    "Bucket 名",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "AccessKey",
                                    &mut cfg.image_host_qiniu_access_key,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "SecretKey",
                                    &mut cfg.image_host_qiniu_secret_key,
                                    w,
                                    "只在创建时可见，丢了重新生成",
                                    true,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "访问域名",
                                    &mut cfg.image_host_qiniu_domain,
                                    w,
                                    "如 xxx.hn-bkt.clouddn.com",
                                    false,
                                );
                                ui.end_row();
                            } else if oss {
                                changed |= host_row(
                                    ui,
                                    "地域",
                                    &mut cfg.image_host_oss_region,
                                    w,
                                    "oss-cn-shanghai，或「华东2（上海）」",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "存储空间",
                                    &mut cfg.image_host_oss_bucket,
                                    w,
                                    "Bucket 名",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "AccessKey ID",
                                    &mut cfg.image_host_oss_key_id,
                                    w,
                                    "",
                                    false,
                                );
                                ui.end_row();
                                changed |= host_row(
                                    ui,
                                    "AccessKey Secret",
                                    &mut cfg.image_host_oss_key_secret,
                                    w,
                                    "阿里云 AccessKey 管理里的那串密钥",
                                    true,
                                );
                                ui.end_row();
                            } else {
                                changed |=
                                    host_row(ui, "用户名", &mut cfg.image_host_owner, w, "", false);
                                ui.end_row();
                                changed |=
                                    host_row(ui, "仓库", &mut cfg.image_host_repo, w, "", false);
                                ui.end_row();
                                changed |=
                                    host_row(ui, "分支", &mut cfg.image_host_branch, w, "", false);
                                ui.end_row();
                            }

                            changed |= host_row(ui, "目录", &mut cfg.image_host_dir, w, "", false);
                            ui.end_row();

                            changed |= host_row(
                                ui,
                                "保存路径",
                                &mut cfg.image_host_path,
                                w,
                                "{year}/{month}/{day}",
                                false,
                            );
                            ui.end_row();

                            if !oss && !qiniu && !volc && !bos && !obs && !cos {
                                let token_hint = match cfg.image_host {
                                    crate::image_host::ImageHostKind::Github => {
                                        "GitHub 令牌（需 Contents 写权限）"
                                    }
                                    _ => "Gitee 私人令牌",
                                };
                                changed |= host_row(
                                    ui,
                                    "令牌",
                                    &mut cfg.image_host_token,
                                    w,
                                    token_hint,
                                    true,
                                );
                                ui.end_row();
                            }
                        });
                    let host = crate::image_host::ImageHostConfig::from_config(cfg);
                    ui.label(
                        RichText::new(
                            "保存路径支持 {year} {month} {day} {hour} {minute} {random} \
                             {filename} {.suffix}，默认按年月日分文件夹；留空表示直接放在目录下。\
                             路径里写了 {filename} 时文件名完全由模板决定，否则自动加毫秒时间戳防撞名。",
                        )
                        .small(),
                    );
                    ui.label(
                        RichText::new(format!("示例：{}", crate::image_host::path_preview(&host)))
                            .small()
                            .color(theme.dim),
                    );
                    let token_help = match cfg.image_host {
                        crate::image_host::ImageHostKind::Tencent => {
                            "SecretId / SecretKey 在腾讯云的「API 密钥管理」里创建，只保存在本地 ~/.markd/config.json，请勿提交到仓库；这把密钥要有目标桶的写入权限（PutObject）。上传走 PUT Object + v5 签名（Authorization 形如 q-sign-algorithm=sha1&q-ak=<SecretId>&q-sign-time=<起>;<止>&q-header-list=…&q-signature=<hex>），签名时间窗只有 10000 秒，本机时钟不准会一直 403。桶名必须写成「名称-APPID」，地域与桶要配对：地址拼的是 https://<桶>.cos.<地域>.myqcloud.com/<对象名>，地域写 ap-nanjing 或「南京」都行，也可以直接粘 endpoint。桶是私有读时文档里的链接公网打不开，需要设成公有读或自己配 CDN（image_host_public_base）。"
                        }
                        crate::image_host::ImageHostKind::Huawei => {
                            "访问密钥（AK/SK）在华为云「我的凭证 → 访问密钥」里创建，只保存在本地 ~/.markd/config.json，请勿提交到仓库；这把密钥所属用户要有目标桶的 OBS 写入权限。上传走 PUT Object + V2 签名（`Authorization: OBS <ID>:<base64 的 HMAC-SHA1>`），待签字符串里带 GMT 时间，本机时钟偏差超过 15 分钟就 403。 桶名与区域要配对：地址拼的是 https://<桶>.obs.<区域>.myhuaweicloud.com/<对象名>， 区域写 cn-east-3 或「华东-上海一」都行，也可以直接粘 endpoint。 桶是私有读时文档里的链接公网打不开，需要设成公共读或自己配 CDN （image_host_public_base）。"
                        }
                        crate::image_host::ImageHostKind::Baidu => {
                            "AccessKey / Secret Access Key 在百度智能云「安全认证 → 访问控制 → AccessKey 管理」里创建，只保存在本地 ~/.markd/config.json，请勿提交到仓库； 这把密钥要有目标桶的写入权限。上传走 PUT Object + bce-auth-v1 签名， 信任状里带 UTC 时间与 1800 秒有效期，本机时钟不准会一直 403。 桶名与地域要配对：地址拼的是 https://<桶>.<地域>.bcebos.com/<对象名>， 地域写 bj 或「华北-北京」都行。桶是私有读时文档里的链接公网打不开， 需要把桶设成公共读或自己配 CDN（image_host_public_base）。"
                        }
                        crate::image_host::ImageHostKind::Volcengine => {
                            "AccessKey / Secret Access Key 在火山引擎「访问控制 → API 访问密钥」里创建，\
                             只保存在本地 ~/.markd/config.json，请勿提交到仓库；这把密钥要有目标桶的 \
                             PutObject 权限（建议只授权这一个桶）。上传走 PUT Object + V4 签名\
                             （TOS4-HMAC-SHA256），签名里带 UTC 时间戳，本机时钟不准会一直 403。\
                             桶名与地域要配对：地址拼的是 https://<桶>.tos-<地域>.volces.com/<对象名>，\
                             地域写 cn-beijing 或「华北2（北京）」都行。桶是私有读时文档里的链接公网打不开，\
                             需要把桶设成公共读或自己配 CDN（image_host_public_base）。"
                        }
                        crate::image_host::ImageHostKind::Qiniu => {
                            "AccessKey / SecretKey 在七牛「个人面板 → 证书管理」里查看，\
                             SecretKey 只在创建时显示一次；两者只保存在本地 \
                             ~/.markd/config.json，请勿提交到仓库。上传走「表单上传」，\
                             上传凭证每次现签、1 小时有效，所以本机时钟不准会一直报 401。\
                             访问域名填空间绑定的域名或七牛给的测试域名：测试域名\
                             （*.clouddn.com）没有 HTTPS 证书，Markd 会自动写成 http:// 链接，\
                             正式使用建议绑定自己的域名。"
                        }
                        crate::image_host::ImageHostKind::Oss => {
                            "密钥只保存在本地 ~/.markd/config.json，请勿提交到仓库。\
                             AccessKey 在阿里云「访问控制 RAM → AccessKey 管理」里创建，\
                             建议只给这一个桶的读写权限（AliyunOSSFullAccess 范围太大）。\
                             桶是私有权限时，文档里的图片链接公网打不开：要么把桶的读写权限\
                             设成公共读，要么手改 config.json 的 image_host_public_base 走只读域名。"
                        }
                        crate::image_host::ImageHostKind::Github => {
                            "令牌只保存在本地 ~/.markd/config.json，请勿提交到仓库。\
                             GitHub 令牌在「Settings → Developer settings → Fine-grained \
                             personal access tokens」生成，Repository permissions 的 Contents \
                             要 Read and write；每张图都是一次仓库提交。"
                        }
                        _ => {
                            "令牌只保存在本地 ~/.markd/config.json，请勿提交到仓库。\
                             Gitee 令牌在「设置 → 私人令牌」生成，需要 projects 权限。"
                        }
                    };
                    ui.label(RichText::new(token_help).small());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("测试上传").clicked() {
                            let host = crate::image_host::ImageHostConfig::from_config(cfg);
                            let (tx, rx) = std::sync::mpsc::channel();
                            self.host_test = Some(rx);
                            self.host_test_status = "正在上传测试图片…".to_string();
                            std::thread::spawn(move || {
                                let _ = tx.send(crate::image_host::test_upload(&host));
                            });
                        }
                        let polled = self.host_test.as_ref().map(|rx| rx.try_recv());
                        match polled {
                            Some(Ok(Ok(url))) => {
                                self.host_test = None;
                                self.host_test_status = format!("上传成功：{url}");
                            }
                            Some(Ok(Err(e))) => {
                                self.host_test = None;
                                self.host_test_status = format!("上传失败：{e}");
                            }
                            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {
                                ui.ctx()
                                    .request_repaint_after(std::time::Duration::from_millis(150));
                            }
                            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                                self.host_test = None;
                            }
                            None => {}
                        }
                        if !self.host_test_status.is_empty() {
                            ui.label(RichText::new(&self.host_test_status).small());
                        }
                    });
                    ui.label(
                        RichText::new(
                            "插入图片会先显示本地副本，上传完成后自动替换为公网 URL；\
                             已有文档可用「插入 → 上传本地图片」批量上传。",
                        )
                        .small(),
                    );
                }
            }
            Page::Markdown => {
                section(ui, theme, "扩展语法", "关掉后相应写法按普通文本渲染。");
                changed |= bool_row(
                    ui,
                    theme,
                    "数学公式 $…$ / $$…$$",
                    "行内与独立公式块。",
                    focus,
                    &mut cfg.math_enabled,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "Mermaid / 流程图",
                    "```mermaid 围栏画图。",
                    focus,
                    &mut cfg.mermaid_enabled,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "目录 [TOC]",
                    "自动生成文章目录。",
                    focus,
                    &mut cfg.toc_enabled,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "脚注",
                    "行尾 [^1] 加文末定义。",
                    focus,
                    &mut cfg.footnote_enabled,
                );
                section(ui, theme, "代码与标题", "");
                changed |= bool_row(
                    ui,
                    theme,
                    "代码块语法高亮",
                    "按语言着色。",
                    focus,
                    &mut cfg.highlight_code,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "标题自动编号（预览）",
                    "预览里给标题补 1. / 1.1 这样的序号。",
                    focus,
                    &mut cfg.auto_number_heading,
                );
            }
            Page::Export => {
                changed |= bool_row(
                    ui,
                    theme,
                    "导出 HTML 时内嵌图片",
                    "图片转 base64 塞进 HTML，单文件可发人；关掉则保持链接引用本地文件。",
                    focus,
                    &mut cfg.export_embed_images,
                );
                changed |= bool_row(
                    ui,
                    theme,
                    "导出完成后打开文件",
                    "导完直接用系统默认程序打开。",
                    focus,
                    &mut cfg.export_open_after,
                );
                section(ui, theme, "Pandoc 导出", "后台转换，不阻塞编辑；点工具条按钮后在弹窗里选本次格式。");
                changed |= row(
                    ui,
                    theme,
                    "Pandoc 输出格式",
                    "弹窗里的默认格式；每次导出仍可单独改，不写回这里。",
                    focus,
                    |ui| {
                        let current = crate::pandoc::format_by_id(&cfg.pandoc_format)
                            .copied()
                            .unwrap_or_else(|| *crate::pandoc::format_by_id("html5").unwrap());
                        let mut picked = false;
                        egui::ComboBox::from_id_salt("settings_pandoc_format")
                            .selected_text(current.label)
                            .width(230.0)
                            .show_ui(ui, |ui| {
                                let mut last_group = None;
                                for f in crate::pandoc::FORMATS {
                                    if last_group != Some(f.group) {
                                        if last_group.is_some() {
                                            ui.separator();
                                        }
                                        ui.label(RichText::new(f.group.label()).small().strong());
                                        last_group = Some(f.group);
                                    }
                                    if ui
                                        .selectable_label(f.id == current.id, f.label)
                                        .clicked()
                                    {
                                        cfg.pandoc_format = f.id.to_string();
                                        picked = true;
                                    }
                                }
                            });
                        picked
                    },
                );
                changed |= host_row(
                    ui,
                    "Pandoc 可执行文件路径",
                    &mut cfg.pandoc_path,
                    260.0,
                    "留空：优先用内置 Pandoc",
                    false,
                );
                changed |= host_row(
                    ui,
                    "Pandoc 额外参数",
                    &mut cfg.pandoc_extra_args,
                    260.0,
                    "如 --pdf-engine=xelatex",
                    false,
                );
                changed |= host_row(
                    ui,
                    "PDF 打印浏览器路径",
                    &mut cfg.pdf_browser_path,
                    260.0,
                    "留空自动检测 Chrome / Edge / Chromium",
                    false,
                );
                if let Some(f) = crate::pandoc::format_by_id(&cfg.pandoc_format) {
                    if !f.hint.is_empty() {
                        ui.add_space(4.0);
                        ui.label(RichText::new(f.hint).small().color(theme.dim));
                    }
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "PDF 由工具页「PDF 导出」直接生成：自包含 HTML 经浏览器无头打印，无需手动操作。",
                    )
                    .small(),
                );
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Id, Pos2, Rect, UiBuilder};

    /// 跑三帧设置窗口（让 Window/ScrollArea 布局稳定），最后一帧注入 events。
    /// root Ui 用固定 Id：按下与松开必须算同一个控件，点击类测试依赖这一点。
    /// 设置窗口的帧回放器：所有帧跑在同一个 Context 里。
    /// 弹窗、焦点这类状态存在 egui 的内存里，每帧新建 Context 的写法点不开下拉，
    /// 所以交互类测试必须用这个。
    struct Frames {
        ctx: egui::Context,
        theme: Theme,
    }

    impl Frames {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                theme: Theme::get(ThemeKind::Light),
            }
        }

        /// 跑一帧，返回这一帧的形状。
        fn frame(
            &mut self,
            s: &mut Settings,
            cfg: &mut Config,
            events: Vec<egui::Event>,
        ) -> Vec<egui::epaint::ClippedShape> {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                events,
                ..Default::default()
            };
            let Self { ctx, theme } = self;
            let mut out = ctx.run_ui(input, |root| {
                let ui = egui::Ui::new(
                    root.clone(),
                    Id::unique("settings_test_root"),
                    UiBuilder::new(),
                );
                let ctx = ui.ctx().clone();
                let _ = s.show(&ctx, theme, cfg);
            });
            out.textures_delta.clear();
            std::mem::take(&mut out.shapes)
        }

        /// 连跑 `count` 帧空帧（让 Window / ScrollArea 布局稳定），返回最后一帧。
        fn idle(
            &mut self,
            s: &mut Settings,
            cfg: &mut Config,
            count: usize,
        ) -> Vec<egui::epaint::ClippedShape> {
            let mut shapes = Vec::new();
            for _ in 0..count {
                shapes = self.frame(s, cfg, Vec::new());
            }
            shapes
        }
    }

    /// 三帧：前两帧静默，最后一帧注入 events。
    fn frames(
        s: &mut Settings,
        cfg: &mut Config,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        let mut h = Frames::new();
        let mut shapes = Vec::new();
        for i in 0..3 {
            let ev = if i == 2 { events.clone() } else { Vec::new() };
            shapes = h.frame(s, cfg, ev);
        }
        shapes
    }

    /// 以「该页 + 传入配置」渲染一页。
    fn render(page: Page, cfg: &mut Config) -> Vec<egui::epaint::ClippedShape> {
        let mut s = Settings::default();
        s.open = true;
        s.page = Some(page);
        frames(&mut s, cfg, Vec::new())
    }

    /// 在 `at` 所在的那块滚动区滚轮向下 `d` 像素。
    fn scroll_down(at: Pos2, d: f32) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                // 内容被推上去才是「往下看」，所以是负 y。
                delta: egui::vec2(0.0, -d),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Default::default(),
            },
        ]
    }

    /// 相邻两帧之间内容滚动了多少：同一帧内所有文字都是刚性平移，所以把两帧同名文字的
    /// y 差全列出来，出现次数最多的那个就是位移。导航栏与底部按钮不跟着滚，它们的差是 0，
    /// 被「只认 4pt 以上」筛掉。
    fn shift_between(
        prev: &[egui::epaint::ClippedShape],
        cur: &[egui::epaint::ClippedShape],
    ) -> f32 {
        let ys = |sh: &[egui::epaint::ClippedShape], name: &str| -> Vec<f32> {
            sh.iter()
                .filter_map(|cs| match &cs.shape {
                    egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos.y),
                    _ => None,
                })
                .collect()
        };
        let mut votes: std::collections::HashMap<i32, usize> = Default::default();
        let mut names: Vec<String> = Vec::new();
        for cs in cur {
            if let egui::Shape::Text(t) = &cs.shape {
                let n = t.galley.text().to_string();
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        for n in &names {
            for py in ys(prev, n) {
                for cy in ys(cur, n) {
                    let d = py - cy;
                    if d > 4.0 {
                        *votes.entry((d * 10.0).round() as i32).or_default() += 1;
                    }
                }
            }
        }
        votes
            .into_iter()
            .max_by_key(|(d, n)| (*n, -*d))
            .map(|(d, _)| d as f32 / 10.0)
            .unwrap_or(0.0)
    }

    /// 默认窗口只有 860×500，比图床表单、外观页 14 行取色器矮，一帧只画得出上半页，
    /// 折到页面下方的文字根本进不了绘制列表（导航栏 12 行在更矮时也会截）。这些测试要验的是
    /// 「整页渲染了哪些项、各项各占一行」，与一眼能看到多少无关，所以把内容区和导航栏
    /// 都逐段滚一遍，按每帧位移把文字折算回同一套文档 y 坐标，拼成一份「整页绘制结果」。
    fn render_whole_page(page: Page, cfg: &mut Config) -> Vec<egui::epaint::ClippedShape> {
        let mut s = Settings::default();
        s.open = true;
        s.page = Some(page);
        let mut h = Frames::new();
        let mut out = stitch(scroll_shots(&mut h, &mut s, cfg, egui::pos2(600.0, 200.0)));
        out.extend(stitch(scroll_shots(
            &mut h,
            &mut s,
            cfg,
            egui::pos2(110.0, 200.0),
        )));
        out
    }

    /// 从当前位置把 `at` 那块滚动区滚到底，返回每一帧的绘制结果（含起始帧）。
    fn scroll_shots(
        h: &mut Frames,
        s: &mut Settings,
        cfg: &mut Config,
        at: Pos2,
    ) -> Vec<Vec<egui::epaint::ClippedShape>> {
        // 每帧滚 200pt：比行高（≈27pt）大得多，也比一屏小，相邻两帧必有一段重叠可供比对。
        const STEP: f32 = 200.0;
        let mut shots = vec![h.idle(s, cfg, 1)];
        let mut still = 0;
        for _ in 0..40 {
            let last = shots.last().expect("至少有一帧");
            let f = h.frame(s, cfg, scroll_down(at, STEP));
            // 滚动比鼠标慢一帧生效，所以「这一帧和上一帧一字不差」出现两次才算到底。
            still = if texts(&f) == texts(last) { still + 1 } else { 0 };
            shots.push(f);
            if still >= 2 {
                break;
            }
        }
        shots
    }

    /// 把逐帧滚动的结果折算回同一套文档 y 坐标。
    fn stitch(shots: Vec<Vec<egui::epaint::ClippedShape>>) -> Vec<egui::epaint::ClippedShape> {
        let (first, rest) = shots.split_first().expect("至少有一帧");
        let mut out = first.clone();
        let mut acc = 0.0;
        let mut prev = first;
        for cur in rest {
            acc += shift_between(prev, cur);
            for cs in cur {
                let egui::Shape::Text(t) = &cs.shape else { continue };
                let name = t.galley.text().to_string();
                // 上一帧里同位置已有这条文字：那是导航栏/底部按钮，不跟着滚，已在 out 里。
                let anchored = prev.iter().any(|p| match &p.shape {
                    egui::Shape::Text(pt) => {
                        pt.galley.text() == name && (pt.pos.y - t.pos.y).abs() < 0.5
                    }
                    _ => false,
                });
                if anchored {
                    continue;
                }
                let mut moved = cs.clone();
                if let egui::Shape::Text(mt) = &mut moved.shape {
                    mt.pos.y += acc;
                }
                out.push(moved);
            }
            prev = cur;
        }
        out
    }

    /// 某个文本第一次出现的位置（整个窗口范围内找）。
    fn find_pos(shapes: &[egui::epaint::ClippedShape], name: &str) -> Option<Pos2> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
    }

    /// 这一帧里所有文本，用于断言「某段文字根本没出现」。
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// 在 `name` 那块文字上点一下：移动 + 按下 + 松开。
    fn click_at(shapes: &[egui::epaint::ClippedShape], name: &str) -> Vec<egui::Event> {
        let p = find_pos(shapes, name).expect(name) + egui::vec2(6.0, 5.0);
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    fn find_y(shapes: &[egui::epaint::ClippedShape], name: &str) -> Option<f32> {
        shapes.iter().find_map(|cs| match &cs.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos.y),
            _ => None,
        })
    }

    /// 图床各字段（用户名/仓库/分支/目录/令牌）应各占一行。
    #[test]
    fn image_host_fields_are_on_separate_rows() {
        let mut cfg = Config::default();
        cfg.image_host = crate::image_host::ImageHostKind::Gitee;
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let ys: Vec<f32> = ["用户名", "仓库", "分支", "目录", "保存路径", "令牌"]
            .iter()
            .filter_map(|n| find_y(&shapes, n))
            .collect();
        assert_eq!(ys.len(), 6, "未找到全部图床字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
    }

    /// 令牌输入框的空值提示要按平台分开写：GitHub 必须点出 Contents 写权限。
    #[test]
    fn token_hint_names_the_permission_github_needs() {
        let mut cfg = Config {
            image_host: crate::image_host::ImageHostKind::Github,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let gh = find_y(&shapes, "GitHub 令牌（需 Contents 写权限）");
        assert!(
            gh.is_some(),
            "GitHub 应提示令牌权限，实际文本里没有那条提示"
        );
        // 同一份渲染里不能同时挂着 Gitee 的提示（否则平台分支没生效）。
        assert!(
            find_y(&shapes, "Gitee 私人令牌").is_none(),
            "选 GitHub 时不该显示 Gitee 的提示"
        );
        let mut cfg = Config {
            image_host: crate::image_host::ImageHostKind::Gitee,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        assert!(
            find_y(&shapes, "Gitee 私人令牌").is_some(),
            "Gitee 分支要显示自己的提示"
        );
        assert!(
            find_y(&shapes, "GitHub 令牌（需 Contents 写权限）").is_none(),
            "选 Gitee 时不该显示 GitHub 的提示"
        );
    }

    /// OSS 表单换成地域/存储空间/两把 AccessKey，仓库平台那几行不该出现。
    #[test]
    fn oss_form_shows_oss_fields_instead_of_repo_fields() {
        let mut cfg = Config {
            image_host: crate::image_host::ImageHostKind::Oss,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "地域",
            "存储空间",
            "AccessKey ID",
            "AccessKey Secret",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部 OSS 字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in ["用户名", "仓库", "分支", "令牌"] {
            assert!(find_y(&shapes, gone).is_none(), "OSS 表单不该有 {gone}");
        }
        // 防空转：同一批标签在 Gitee 下必须反过来。
        let mut cfg2 = Config {
            image_host: crate::image_host::ImageHostKind::Gitee,
            ..Default::default()
        };
        let s2 = render_whole_page(Page::Images, &mut cfg2);
        assert!(find_y(&s2, "用户名").is_some(), "Gitee 应显示用户名");
        assert!(
            find_y(&s2, "AccessKey Secret").is_none(),
            "Gitee 不该显示 AccessKey 行"
        );
    }

    /// 七牛表单换成区域/空间名称/两把密钥/访问域名，仓库与 OSS 的行都不该出现；
    /// 说明文字要点出「时钟不准会 401」与「测试域名只能 http」这两个实测会踩的坑。
    #[test]
    fn qiniu_form_shows_qiniu_fields_and_warns_about_the_test_domain() {
        use crate::image_host::ImageHostKind;
        let mut cfg = Config {
            image_host: ImageHostKind::Qiniu,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "区域",
            "空间名称",
            "AccessKey",
            "SecretKey",
            "访问域名",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部七牛字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in ["用户名", "仓库", "分支", "令牌", "地域", "存储空间"] {
            assert!(find_y(&shapes, gone).is_none(), "七牛表单不该有 {gone}");
        }
        let says = |needle: &str| {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(says("时钟"), "七牛说明要点出上传凭证依赖本机时钟：{ys:?}");
        assert!(says("http://"), "七牛说明要交代测试域名只能走 http");
        // 防空转：换回 OSS / Gitee，同一批标签必须反过来。
        let mut oss = Config {
            image_host: ImageHostKind::Oss,
            ..Default::default()
        };
        let s = render_whole_page(Page::Images, &mut oss);
        assert!(find_y(&s, "存储空间").is_some(), "OSS 仍应显示存储空间");
        assert!(
            find_y(&s, "访问域名").is_none(),
            "OSS 不该出现七牛的访问域名"
        );
        assert!(
            find_y(&s, "区域").is_none(),
            "OSS 用的是「地域」不是「区域」"
        );
        let mut gitee = Config {
            image_host: ImageHostKind::Gitee,
            ..Default::default()
        };
        let s = render_whole_page(Page::Images, &mut gitee);
        assert!(
            find_y(&s, "SecretKey").is_none(),
            "Gitee 不该显示七牛的 SecretKey"
        );
        assert!(find_y(&s, "区域").is_none(), "Gitee 不该显示七牛的区域");
    }

    /// 火山引擎 TOS 表单换成 地域/存储桶/AccessKey/Secret Access Key，
    /// 仓库平台、OSS 与七牛的行都不该出现；说明里要交代签名依赖本机时间。
    #[test]
    fn volc_form_shows_tos_fields_instead_of_the_other_platforms() {
        use crate::image_host::ImageHostKind;
        let mut cfg = Config {
            image_host: ImageHostKind::Volcengine,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "地域",
            "存储桶",
            "AccessKey",
            "Secret Access Key",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部火山字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in [
            "用户名",
            "仓库",
            "分支",
            "令牌",
            "存储空间",
            "AccessKey ID",
            "AccessKey Secret",
            "区域",
            "空间名称",
            "访问域名",
        ] {
            assert!(find_y(&shapes, gone).is_none(), "火山表单不该有 {gone}");
        }
        let says = |needle: &str| {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(says("TOS4-HMAC-SHA256"), "说明要点出签名方案");
        assert!(says("时钟"), "说明要交代签名依赖本机时间");
        assert!(says("tos-"), "说明要给出地址拼法，便于核对桶与地域配对");
        // 防空转：换回 OSS 与七牛，同一批标签必须反过来。
        let mut oss = Config {
            image_host: ImageHostKind::Oss,
            ..Default::default()
        };
        let s = render_whole_page(Page::Images, &mut oss);
        assert!(find_y(&s, "存储空间").is_some(), "OSS 仍应显示存储空间");
        assert!(find_y(&s, "存储桶").is_none(), "OSS 不该出现火山的存储桶");
        assert!(
            find_y(&s, "Secret Access Key").is_none(),
            "OSS 用的是 AccessKey Secret"
        );
        let mut qiniu = Config {
            image_host: ImageHostKind::Qiniu,
            ..Default::default()
        };
        let s = render_whole_page(Page::Images, &mut qiniu);
        assert!(find_y(&s, "访问域名").is_some(), "七牛仍应显示访问域名");
        assert!(
            find_y(&s, "Secret Access Key").is_none(),
            "七牛用的是 SecretKey"
        );
    }

    /// 百度 BOS 表单同样是 地域/存储桶/AccessKey/Secret Access Key 四行，
    /// 但与火山 TOS 的标签一字不差，所以这里靠说明文案与占位符把两个平台分开，
    /// 而不是只看标签在不在（那样两个平台会同时通过，等于没测）。
    #[test]
    fn bos_form_shows_baidu_help_and_not_the_other_platforms() {
        use crate::image_host::ImageHostKind;
        let mut cfg = Config {
            image_host: ImageHostKind::Baidu,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "地域",
            "存储桶",
            "AccessKey",
            "Secret Access Key",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部百度字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in [
            "用户名",
            "仓库",
            "分支",
            "令牌",
            "存储空间",
            "AccessKey ID",
            "AccessKey Secret",
            "区域",
            "空间名称",
            "访问域名",
        ] {
            assert!(find_y(&shapes, gone).is_none(), "百度表单不该有 {gone}");
        }
        let says = |needle: &str| {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(says("bce-auth-v1"), "说明要点出百度的签名方案");
        assert!(says("bcebos.com"), "说明要给出百度的地址拼法");
        assert!(says("华北-北京"), "地域占位符该给百度的控制台写法");
        assert!(!says("TOS4-HMAC-SHA256"), "百度的表单不该出现火山的签名名");
        assert!(!says("华北2"), "百度的表单不该出现火山的地域写法");
        // 防空转：切到火山（标签完全相同的那个平台）与七牛，上述特征必须反过来。
        let mut volc = Config {
            image_host: ImageHostKind::Volcengine,
            ..Default::default()
        };
        let v = render_whole_page(Page::Images, &mut volc);
        let vsays = |needle: &str| {
            v.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(vsays("TOS4-HMAC-SHA256"), "火山仍应说自己的签名");
        assert!(!vsays("bce-auth-v1"), "火山表单不该出现百度的签名名");
        assert!(find_y(&v, "存储桶").is_some(), "火山的存储桶行还在");
        let mut qiniu = Config {
            image_host: ImageHostKind::Qiniu,
            ..Default::default()
        };
        let q = render_whole_page(Page::Images, &mut qiniu);
        assert!(
            find_y(&q, "Secret Access Key").is_none(),
            "七牛用的是 SecretKey，不是 Secret Access Key"
        );
        assert!(find_y(&q, "访问域名").is_some(), "七牛仍应显示访问域名");
    }

    /// 华为云 OBS 表单是 区域/桶名/访问密钥 ID/秘密访问密钥 四行。
    /// 「区域」这个标签七牛也用，所以互斥断言挑华为独有的三项，再加说明文案里的域名形态。
    #[test]
    fn obs_form_shows_huawei_fields_instead_of_the_other_platforms() {
        use crate::image_host::ImageHostKind;
        let mut cfg = Config {
            image_host: ImageHostKind::Huawei,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "区域",
            "桶名",
            "访问密钥 ID",
            "秘密访问密钥",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部华为字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in [
            "用户名",
            "仓库",
            "分支",
            "令牌",
            "存储空间",
            "AccessKey Secret",
            "空间名称",
            "访问域名",
            "存储桶",
            "Secret Access Key",
        ] {
            assert!(find_y(&shapes, gone).is_none(), "华为表单不该有 {gone}");
        }
        let says = |needle: &str| {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(says("OBS"), "说明要点出华为的服务名");
        assert!(says("myhuaweicloud"), "说明要给出地址拼法");
        assert!(says("cn-east-3"), "区域占位符该给华为的区域编码写法");
        assert!(says("15 分钟"), "说明要交代 Date 头允许的时钟偏差");
        // 防空转：切到同样有「区域」行的七牛，与用「存储桶」的火山，特征必须反过来。
        let mut qiniu = Config {
            image_host: ImageHostKind::Qiniu,
            ..Default::default()
        };
        let q = render_whole_page(Page::Images, &mut qiniu);
        assert!(find_y(&q, "区域").is_some(), "七牛也有区域行");
        assert!(find_y(&q, "秘密访问密钥").is_none(), "七牛用的是 SecretKey");
        assert!(!q.iter().any(|cs| match &cs.shape {
            egui::Shape::Text(t) => t.galley.text().contains("myhuaweicloud"),
            _ => false,
        }));
        let mut volc = Config {
            image_host: ImageHostKind::Volcengine,
            ..Default::default()
        };
        let v = render_whole_page(Page::Images, &mut volc);
        assert!(find_y(&v, "存储桶").is_some(), "火山仍应显示存储桶");
        assert!(find_y(&v, "桶名").is_none(), "火山的标签不是桶名");
    }

    /// 外观页应为每个可自定义颜色提供标签与「恢复默认」按钮。
    #[test]
    fn appearance_tab_has_custom_color_pickers() {
        let mut cfg = Config::default();
        let shapes = render_whole_page(Page::Appearance, &mut cfg);
        // 记录每个标签的 y，确认 14 项颜色标签都渲染出来、且纵向依次排列。
        let missing: Vec<&str> = ThemeColor::ALL
            .iter()
            .map(|c| c.label())
            .filter(|n| find_y(&shapes, n).is_none())
            .collect();
        assert!(missing.is_empty(), "这些颜色标签没渲染出来：{missing:?}");
        let ys: Vec<f32> = ThemeColor::ALL
            .iter()
            .filter_map(|c| find_y(&shapes, c.label()))
            .collect();
        assert_eq!(ys.len(), 14, "应渲染全部 14 项颜色标签，实际 {ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] >= w[0], "颜色项应自上而下排列：{ys:?}");
        }
        // 14 个单项「恢复默认」+ 底部 1 个全局「恢复默认」= 15。
        let restore_count = shapes
            .iter()
            .filter(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text() == "恢复默认",
                _ => false,
            })
            .count();
        assert!(
            restore_count >= 15,
            "每项颜色都应有独立的恢复默认按钮，实际按钮数 {restore_count}"
        );
    }

    /// 腾讯云 COS 表单是 地域/存储桶/SecretId/SecretKey 四行。
    /// 「地域」与百度重合、「SecretKey」与七牛重合，所以特征取 SecretId 与说明文案里的
    /// 域名形态、签名串字段名；再切到华为/百度做反向对照，防两个平台同时通过的空转。
    #[test]
    fn cos_form_shows_tencent_fields_instead_of_the_other_platforms() {
        use crate::image_host::ImageHostKind;
        let mut cfg = Config {
            image_host: ImageHostKind::Tencent,
            ..Default::default()
        };
        let shapes = render_whole_page(Page::Images, &mut cfg);
        let names = [
            "地域",
            "存储桶",
            "SecretId",
            "SecretKey",
            "目录",
            "保存路径",
        ];
        let ys: Vec<f32> = names.iter().filter_map(|n| find_y(&shapes, n)).collect();
        assert_eq!(ys.len(), names.len(), "未找到全部腾讯字段标签：{ys:?}");
        for w in ys.windows(2) {
            assert!(w[1] - w[0] > 10.0, "字段应各占一行：{ys:?}");
        }
        for gone in [
            "用户名",
            "仓库",
            "分支",
            "令牌",
            "存储空间",
            "空间名称",
            "访问域名",
            "桶名",
            "访问密钥 ID",
            "秘密访问密钥",
            "Secret Access Key",
            "AccessKey",
        ] {
            assert!(find_y(&shapes, gone).is_none(), "腾讯表单不该有 {gone}");
        }
        let says = |needle: &str| {
            shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(says("myqcloud"), "说明要给出地址拼法");
        assert!(says("q-sign-algorithm"), "说明应点出 v5 签名串的字段");
        assert!(
            says("名称-APPID"),
            "腾讯的桶名必须带 APPID 后缀，说明要交代"
        );
        assert!(says("ap-nanjing"), "地域占位符该给腾讯的区域编码写法");
        assert!(says("10000 秒"), "说明要交代签名时间窗");
        assert!(!says("myhuaweicloud"), "腾讯页不该混进华为的域名");
        assert!(!says("TOS4"), "腾讯页不该混进火山的签名名");
        assert!(!says("bce-auth-v1"), "腾讯页不该混进百度的签名名");
        // 标签一字相同不够：把两把值写进各自的字段，输入框里必须显示腾讯那一份，
        // 否则说明「地域」这行连错了配置项。
        let mut bound = Config {
            image_host: ImageHostKind::Tencent,
            ..Default::default()
        };
        bound.image_host_cos_region = "腾讯地域值-MARKER".into();
        bound.image_host_obs_region = "华为地域值-MARKER".into();
        let bs = render_whole_page(Page::Images, &mut bound);
        let says2 = |needle: &str| {
            bs.iter().any(|cs| match &cs.shape {
                egui::Shape::Text(t) => t.galley.text().contains(needle),
                _ => false,
            })
        };
        assert!(
            says2("腾讯地域值-MARKER"),
            "「地域」行应绑定 image_host_cos_region"
        );
        assert!(!says2("华为地域值-MARKER"), "腾讯页不该画华为的区域字段");
        // 反向对照：华为仍显示自己的「桶名 / 秘密访问密钥」，且没有 SecretId。
        let mut huawei = Config {
            image_host: ImageHostKind::Huawei,
            ..Default::default()
        };
        let h = render_whole_page(Page::Images, &mut huawei);
        assert!(find_y(&h, "SecretId").is_none(), "华为表单没有 SecretId");
        assert!(
            find_y(&h, "秘密访问密钥").is_some(),
            "华为仍用自己的密钥标签"
        );
        // 反向对照：百度有「Secret Access Key」但没有 SecretId。
        let mut baidu = Config {
            image_host: ImageHostKind::Baidu,
            ..Default::default()
        };
        let b = render_whole_page(Page::Images, &mut baidu);
        assert!(find_y(&b, "SecretId").is_none(), "百度表单没有 SecretId");
        assert!(
            find_y(&b, "Secret Access Key").is_some(),
            "百度仍用自己的密钥标签"
        );
    }

    /// 每个平台的说明文案都不该有连续两个空格：早先华为与百度这两条是从多行字符串
    /// 拼来的，续行符丢了就成了字面空格串，设置页里会画出一段段大洞。
    #[test]
    fn every_platform_help_text_has_no_double_space_runs() {
        use crate::image_host::ImageHostKind;
        let mut hit: Vec<ImageHostKind> = Vec::new();
        for kind in ImageHostKind::ALL {
            let mut cfg = Config {
                image_host: kind,
                ..Default::default()
            };
            let shapes = render_whole_page(Page::Images, &mut cfg);
            for cs in &shapes {
                if let egui::Shape::Text(t) = &cs.shape {
                    let text = t.galley.text();
                    // 只挑各平台的密钥说明段（都提到本地配置文件路径）。
                    if text.contains("config.json") {
                        hit.push(kind);
                        assert!(
                            !text.contains("  "),
                            "{:?} 的说明文案里有连续空格：{text:?}",
                            kind
                        );
                    }
                }
            }
        }
        // 六家对象存储都必须在这段里提到本地配置文件（密钥去向要说清楚）。
        for kind in [
            ImageHostKind::Oss,
            ImageHostKind::Qiniu,
            ImageHostKind::Volcengine,
            ImageHostKind::Baidu,
            ImageHostKind::Huawei,
            ImageHostKind::Tencent,
        ] {
            assert!(
                hit.contains(&kind),
                "{kind:?} 的密钥说明段没找到，空格扫描没覆盖到它"
            );
        }
    }

    /// 编辑器设置里应提供「显示行号栏」开关（用户可自行关闭行号）。
    #[test]
    fn editor_tab_has_line_number_toggle() {
        let mut cfg = Config::default();
        let shapes = render(Page::Editor, &mut cfg);
        assert!(
            find_y(&shapes, "显示行号栏").is_some(),
            "编辑器设置应包含行号栏开关"
        );
    }

    /// 一段文字请求的字号：直接读 galley 的 job sections，不在测试里重算一遍放大逻辑。
    fn glyph_sizes(shapes: &[egui::epaint::ClippedShape]) -> Vec<f32> {
        let mut sizes = Vec::new();
        for cs in shapes {
            if let egui::Shape::Text(t) = &cs.shape {
                for section in &t.galley.job.sections {
                    sizes.push(section.format.font_id.size);
                }
            }
        }
        sizes.sort_by(|a, b| a.total_cmp(b));
        sizes.dedup();
        sizes
    }

    /// 设置窗口文字比 egui 面板默认大 2pt：正文默认 16pt，13pt 实测偏小。
    /// 目标字号写死成「默认 + 2」，不引用 TEXT_BUMP——否则把增量改成 3pt 也测不出来。
    #[test]
    fn settings_text_is_two_points_above_default() {
        let mut cfg = Config::default();
        let sizes = glyph_sizes(&render(Page::Appearance, &mut cfg));
        let base = egui::Style::default().text_styles[&egui::TextStyle::Body].size;
        assert!(!sizes.is_empty(), "设置窗口一个字形都没渲染出来");
        assert!(
            !sizes.contains(&base),
            "仍有文字停在默认 {base}pt，说明放大没覆盖到它：实测 {sizes:?}"
        );
        assert!(
            sizes.contains(&(base + 2.0)),
            "应有 {}pt 的文字，实测 {sizes:?}",
            base + 2.0
        );
    }

    /// 设置窗口是独立系统窗口：默认 860×500、带原生标题、可手动缩放，
    /// 最小宽度要放得下图床表单那一行（300pt 输入框 + 标签 + 左侧分类栏）。
    #[test]
    fn settings_uses_a_resizable_native_window() {
        let b = viewport_builder();
        assert_eq!(b.title.as_deref(), Some("设置"));
        assert_eq!(b.resizable, Some(true), "窗口要能手动拉大缩小");
        assert_eq!(
            b.inner_size,
            Some(egui::vec2(860.0, 500.0)),
            "默认尺寸应是 860×500，实际 {:?}",
            b.inner_size
        );
        let min = b.min_inner_size.expect("应设最小尺寸");
        assert!(
            min.x > 500.0,
            "图床表单的 300pt 输入框会被裁掉：最小尺寸 {min:?}"
        );
    }
    /// 导航分组按 WorkBuddy 的分法：通用（常规/外观/快捷键）、功能（个性化/编辑器）、内容（其余）。
    #[test]
    fn nav_groups_match_the_planned_split() {
        use Page::*;
        assert_eq!(
            PAGES.iter().filter(|p| p.group() == Group::General).count(),
            3
        );
        for p in [General, Appearance, Shortcuts] {
            assert_eq!(p.group(), Group::General, "{}应归通用", p.title());
        }
        for p in [Personalize, Editor] {
            assert_eq!(p.group(), Group::Features, "{}应归功能", p.title());
        }
        for p in [Files, Images, Markdown, Export] {
            assert_eq!(p.group(), Group::Content, "{}应归内容", p.title());
        }
        let mut cfg = Config::default();
        let ts = texts(&render_whole_page(Page::General, &mut cfg));
        for g in ["通用", "功能", "内容"] {
            assert!(ts.iter().any(|t| t == g), "导航应显示分组标题 {g}：{ts:?}");
        }
        for p in PAGES {
            assert!(
                ts.iter().any(|t| t == p.title()),
                "导航应列出页名 {}",
                p.title()
            );
        }
    }

    /// 搜索索引不能和界面上的文案脱节：每条标题都要在它所登记的页里出现。
    /// 图床表单只在平台启用时渲染，所以图像页用 GitHub 平台当夹具。
    #[test]
    fn search_index_matches_rendered_titles() {
        for page in PAGES {
            let mut cfg = Config::default();
            if page == Page::Images {
                cfg.image_host = crate::image_host::ImageHostKind::Github;
            }
            let ts = texts(&render_whole_page(page, &mut cfg));
            for (title, _, target) in SEARCH {
                if *target != page {
                    continue;
                }
                assert!(
                    ts.iter().any(|t| t == title),
                    "索引里的「{title}」在 {} 页上没渲染出来（文案改了没同步索引，或页登记错了）",
                    page.title()
                );
            }
        }
    }

    /// 搜索时导航只列命中项：命中的排上、没命中的别赖着；整页无命中给提示。
    #[test]
    fn search_lists_only_hits_in_the_nav() {
        let mut cfg = Config::default();
        let mut s = Settings::default();
        s.open = true;
        // 从外观页搜「行距」：命中项在编辑器页，当前页面上没有那行文字，
        // 于是「出现该行文字」等价于「导航把它列出来了」。
        s.page = Some(Page::Appearance);
        s.query = "行距".to_string();
        let ts = texts(&frames(&mut s, &mut cfg, Vec::new()));
        assert!(
            ts.iter().any(|t| t == "源码模式行距"),
            "导航应列出命中项：{ts:?}"
        );
        // 「自定义颜色」不能列在这里：当前页就是外观页，它本来该出现在内容区。
        for absent in ["退出前确认", "打字机模式"] {
            assert!(
                !ts.iter().any(|t| t == absent),
                "未命中/未打开的页不该显示「{absent}」：{ts:?}"
            );
        }
        assert!(
            !ts.iter().any(|t| t == "内容"),
            "搜索时不再画分组标题：{ts:?}"
        );

        s.query = "zzzz-不存在的设置".to_string();
        let ts = texts(&frames(&mut s, &mut cfg, Vec::new()));
        assert!(
            ts.iter().any(|t| t == "没有匹配的设置项"),
            "无命中要有提示：{ts:?}"
        );
    }

    /// 点导航里的命中项：右侧切到该项所在页，下一帧给它画定位高亮框。
    #[test]
    fn clicking_a_nav_hit_opens_that_page_and_marks_the_row() {
        let mut cfg = Config::default();
        let mut s = Settings::default();
        s.open = true;
        s.page = Some(Page::Appearance);
        s.query = "行距".to_string();
        let mut h = Frames::new();
        let before = h.idle(&mut s, &mut cfg, 3);
        let click = click_at(&before, "源码模式行距");
        // 点击帧 + 之后一帧：高亮框只活在点击后的那一帧。
        h.frame(&mut s, &mut cfg, click);
        let once = h.frame(&mut s, &mut cfg, Vec::new());
        assert_eq!(s.page, Some(Page::Editor), "点命中项应切到编辑器页");
        let ts = texts(&once);
        assert!(
            ts.iter().any(|t| t == "源码模式行距"),
            "切页后该项应在内容区：{ts:?}"
        );
        // 高亮框：窗口淡入动画会把整层颜色乘上一个系数（单测里时间不推进，
        // 系数一直是 0.5），所以不比具体色值，只认这条描边——1.5pt 宽在本窗口里唯一。
        let marked = once.iter().any(|cs| {
            matches!(&cs.shape, egui::Shape::Rect(r)
                if r.stroke.width == 1.5 && r.stroke.color != egui::Color32::TRANSPARENT)
        });
        assert!(marked, "命中项那行应被描一圈定位框（1.5pt 强调色描边）");
    }

    /// 主题下拉读的是「跟随系统 + 手动档」两个字段合成的三档视图。
    #[test]
    fn theme_row_shows_the_three_way_choice() {
        let mut cfg = Config::default();
        let ts = texts(&render(Page::Appearance, &mut cfg));
        assert!(
            ts.iter().any(|t| t == "浅色"),
            "默认应显示手动档「浅色」：{ts:?}"
        );

        cfg.theme_follow_system = true;
        let ts = texts(&render(Page::Appearance, &mut cfg));
        assert!(
            ts.iter().any(|t| t == "跟随系统"),
            "开了跟随系统，下拉要显示「跟随系统」：{ts:?}"
        );
        assert_eq!(
            theme_choice(&cfg),
            ThemeChoice::System,
            "跟随系统优先于手动档"
        );

        // 手动改档（菜单/命令走的 set_theme）要顺手关掉跟随，否则下拉会显示旧状态。
        cfg.set_theme(ThemeKind::Dark);
        assert!(!cfg.theme_follow_system, "选深色后不该还挂着跟随系统");
        assert_eq!(theme_choice(&cfg), ThemeChoice::Dark);
    }
    /// 点开语言下拉、选中 English：配置要真的写进去（这一项目前只存配置，
    /// 所以能验的就是「写没写对」，界面文字不变是已知的）。
    #[test]
    fn language_dropdown_writes_the_config() {
        let mut cfg = Config::default();
        let mut s = Settings::default();
        s.open = true;
        s.page = Some(Page::General);
        let mut h = Frames::new();
        let before = h.idle(&mut s, &mut cfg, 3);
        let open = click_at(&before, "简体中文");
        // 点击帧之后的那一帧才画得出弹窗。
        h.frame(&mut s, &mut cfg, open);
        let popup = h.frame(&mut s, &mut cfg, Vec::new());
        let ts = texts(&popup);
        assert!(
            ts.iter().any(|t| t == "English") && ts.iter().any(|t| t == "简体中文"),
            "下拉应展开两个语言项：{ts:?}"
        );
        let pick = click_at(&popup, "English");
        h.frame(&mut s, &mut cfg, pick);
        h.frame(&mut s, &mut cfg, Vec::new());
        assert_eq!(cfg.language, Language::En, "选 English 后配置应写入");
    }

    /// 主题下拉选「跟随系统」：写的是跟随位，手动档保留原值。
    #[test]
    fn picking_follow_system_sets_the_flag_only() {
        let mut cfg = Config::default();
        let mut s = Settings::default();
        s.open = true;
        s.page = Some(Page::Appearance);
        let mut h = Frames::new();
        let before = h.idle(&mut s, &mut cfg, 3);
        let open = click_at(&before, "浅色");
        h.frame(&mut s, &mut cfg, open);
        let popup = h.frame(&mut s, &mut cfg, Vec::new());
        assert!(
            texts(&popup).iter().any(|t| t == "跟随系统"),
            "主题下拉要有「跟随系统」这一档"
        );
        let pick = click_at(&popup, "跟随系统");
        h.frame(&mut s, &mut cfg, pick);
        h.frame(&mut s, &mut cfg, Vec::new());
        assert!(cfg.theme_follow_system, "选「跟随系统」应写进跟随位");
        assert_eq!(cfg.theme, ThemeKind::Light, "手动档不该被顺手改掉");
    }
}
