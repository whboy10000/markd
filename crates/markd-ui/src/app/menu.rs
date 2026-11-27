//! 菜单结构、侧栏页签切换与插入模板。

use super::*;

/// 大纲显隐开关的状态转移：返回 (新的 show_outline, 新的侧栏页签)。
pub(super) fn outline_toggle(showing: bool) -> (bool, SideTab) {
    if showing {
        (false, SideTab::Files)
    } else {
        (true, SideTab::Outline)
    }
}

/// 「文件 / 大纲」入口被点击后的侧栏状态：(show_sidebar, side_tab, show_outline)。
/// 当前正显示该页签时再点一次 → 收起；否则展开并切到该页签。
pub(super) fn sidebar_click(visible: bool, tab: SideTab, target: SideTab) -> (bool, SideTab, bool) {
    if visible && tab == target {
        (false, tab, tab == SideTab::Outline)
    } else {
        (true, target, target == SideTab::Outline)
    }
}

/// 生成 rows×cols 的空表格（含表头与分隔行）。
pub(super) fn table_template(rows: usize, cols: usize) -> String {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let mut s = String::new();
    s.push('|');
    for c in 1..=cols {
        s.push_str(&format!(" 列{c} |"));
    }
    s.push('\n');
    s.push('|');
    for _ in 0..cols {
        s.push_str(" --- |");
    }
    s.push('\n');
    for _ in 0..rows {
        s.push('|');
        for _ in 0..cols {
            s.push_str("  |");
        }
        s.push('\n');
    }
    s.trim_end().to_string()
}

/// Mermaid 下拉的图类型与标准模板（覆盖 Mermaid 全部常用图类型）。
/// 这些类型都有自绘渲染器：流程图/时序图/类图/状态图/ER 图/甘特图/饼图/
/// 旅程图/Git 图/思维导图/时间线/象限图/XY 图表/雷达图/桑基图/矩形树图/
/// C4/块图/需求图/数据包图/看板/泳道图/Cynefin/铁路图。
pub(crate) static MERMAID_TEMPLATES: &[(&str, &str)] = &[
    (
        "流程图",
        r#"```mermaid
flowchart LR
    A[开始] --> B{判断}
    B -->|是| C[执行]
    B -->|否| D[结束]
    C --> E((终止))
```"#,
    ),
    (
        "时序图",
        r#"```mermaid
sequenceDiagram
    participant 用户
    participant 前端
    participant 后端
    participant 数据库

    用户->>前端: 点击登录
    前端->>后端: POST /login
    后端->>数据库: 查询用户
    数据库-->>后端: 返回用户信息
    后端-->>前端: 返回 Token
    前端-->>用户: 登录成功
```"#,
    ),
    (
        "类图",
        r#"```mermaid
classDiagram
    class Animal {
        +String name
        +int age
        +makeSound()
    }
    class Dog {
        +fetch()
    }
    class Cat {
        +purr()
    }
    Animal <|-- Dog
    Animal <|-- Cat
    Dog o-- "1..*" Toy : has
```"#,
    ),
    (
        "状态图",
        r#"```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Running : 启动
    Running --> Paused : 暂停
    Paused --> Running : 继续
    Paused --> Idle : 停止
    Running --> Error : 异常
    Error --> Idle : 重置
    Idle --> [*]
```"#,
    ),
    (
        "ER 图",
        r#"```mermaid
erDiagram
    CUSTOMER ||--o{ ORDER : "下单"
    ORDER ||--|{ LINE_ITEM : "包含"
    PRODUCT ||--o{ LINE_ITEM : "被购买"
    CUSTOMER {
        string name
        string email
    }
    ORDER {
        int id
        date created_at
        string status
    }
    PRODUCT {
        int id
        string title
        decimal price
    }
```"#,
    ),
    (
        "甘特图",
        r#"```mermaid
gantt
    title 项目排期
    dateFormat  YYYY-MM-DD
    section 准备
    需求分析       :a1, 2026-11-01, 5d
    原型设计       :a2, after a1, 4d
    section 开发
    后端接口       :b1, after a2, 7d
    前端页面       :b2, after a2, 6d
    联调测试       :b3, after b1, 3d
    section 上线
    预发布验证     :milestone, 2026-12-01, 0d
```"#,
    ),
    (
        "饼图",
        r#"```mermaid
pie showData
    title 浏览器市场份额
    "Chrome" : 65
    "Safari" : 15
    "Firefox" : 10
    "Edge" : 7
    "其他" : 3
```"#,
    ),
    (
        "旅程图",
        r#"```mermaid
journey
    title 用户购物旅程
    section 发现
      浏览首页: 3: 用户
      搜索商品: 4: 用户
    section 决策
      查看详情: 5: 用户
      加入购物车: 4: 用户
    section 购买
      提交订单: 3: 用户
      支付成功: 5: 用户
```"#,
    ),
    (
        "Git 图",
        r#"```mermaid
gitGraph
    commit id: "a3f82c1"
    branch develop
    checkout develop
    commit id: "b7e41d9"
    commit id: "c9d52e4"
    checkout main
    merge develop id: "d4e8f3a"
    commit id: "e1b6c90"
    branch feature
    checkout feature
    commit id: "f2a8d17"
    commit id: "a8c3f54"
    checkout main
    merge feature id: "b9d7e21"
```"#,
    ),
    (
        "思维导图",
        r#"```mermaid
mindmap
    root((知识管理))
        采集
            网页剪藏
            手动录入
        整理
            标签体系
            双向链接
        输出
            文章
            演示文稿
        工具
            Markdown
            Mermaid
            编辑器
```"#,
    ),
    (
        "时间线",
        r#"```mermaid
timeline
    title 产品迭代
    section 2024
        v0.1 : 原型上线
        v0.2 : 支持 Markdown
    section 2025
        v1.0 : 双向链接
        v1.2 : 插件系统
    section 2026
        v2.0 : AI 辅助写作
```"#,
    ),
    (
        "象限图",
        r#"```mermaid
quadrantChart
    title 功能优先级
    x-axis "低价值" --> "高价值"
    y-axis "低紧急" --> "高紧急"
    quadrant-1 "优先做"
    quadrant-2 "规划做"
    quadrant-3 "暂缓"
    quadrant-4 "快速做"
    "核心编辑": [0.8, 0.9]
    "双向链接": [0.7, 0.6]
    "插件市场": [0.4, 0.3]
    "主题切换": [0.6, 0.4]
    "AI 补全": [0.9, 0.7]
```"#,
    ),
    (
        "XY 图表",
        r#"```mermaid
xychart-beta
    title "月度活跃用户"
    x-axis ["1月", "2月", "3月", "4月", "5月", "6月"]
    y-axis "用户数" 0 --> 1200
    bar [300, 420, 580, 710, 900, 1100]
    line [300, 420, 580, 710, 900, 1100]
```"#,
    ),
    (
        "雷达图",
        r#"```mermaid
radar-beta
    title 能力评估
    axis "编程", "设计", "沟通", "管理", "学习"
    curve Alice [90, 60, 70, 50, 85]
    curve Bob [65, 80, 85, 75, 70]
    max 100
    min 0
```"#,
    ),
    (
        "桑基图",
        r#"```mermaid
sankey-beta
    流量来源,官网,400
    流量来源,搜索,300
    流量来源,社交媒体,200
    官网,注册,250
    搜索,注册,180
    社交媒体,注册,90
    注册,付费,120
```"#,
    ),
    (
        "矩形树图",
        r#"```mermaid
treemap-beta
    title 磁盘占用
    "项目 A" : 420
    "项目 B" : 310
    "项目 C" : 180
    "文档" : 90
    "日志" : 60
```"#,
    ),
    (
        "C4 上下文图",
        r#"```mermaid
C4Context
    title 系统上下文图
    Person(user, "用户")
    System(editor, "Markdown 编辑器")
    System_Ext(cloud, "云存储")
    System_Ext(ai, "AI 服务")

    Rel(user, editor, "使用")
    Rel(editor, cloud, "同步")
    Rel(editor, ai, "调用")
```"#,
    ),
    (
        "块图",
        r#"```mermaid
block-beta
    columns 3
    Client["客户端"] space:2
    block:Backend:2
        columns 2
        API["API 网关"] LB["负载均衡"]
        Svc1["服务 A"] Svc2["服务 B"]
    end
    DB
    Client --> API
    API --> Svc1
    API --> Svc2
    Svc1 --> DB
    Svc2 --> DB
```"#,
    ),
    (
        "需求图",
        r#"```mermaid
requirementDiagram
    requirement loginReq {
        id: 1
        text: "用户可以使用账号密码登录"
        risk: low
        verifymethod: test
    }
    functionalRequirement tokenReq {
        id: 2
        text: "登录成功后返回 JWT Token"
        risk: medium
        verifymethod: inspection
    }
    element authService {
        type: service
        docRef: "auth.md"
    }
    authService - satisfies -> loginReq
    tokenReq - derives -> loginReq
```"#,
    ),
    (
        "数据包图",
        r#"```mermaid
packet-beta
    title TCP 头部
    0-15: "Source Port"
    16-31: "Destination Port"
    32-63: "Sequence Number"
    64-95: "Acknowledgment Number"
    96-99: "Data Offset"
    100-105: "Reserved"
    106-111: "Flags"
    112-127: "Window"
    128-143: "Checksum"
    144-159: "Urgent Pointer"
```"#,
    ),
    (
        "看板",
        r#"```mermaid
kanban
    Todo
        [编写 Mermaid 模板]
        [整理图表文档]
    Doing
        [集成到编辑器]
    Done
        [流程图]
        [时序图]
        [类图]
```"#,
    ),
    (
        "泳道图",
        r#"```mermaid
swimlane-beta
    title 订单处理泳道图
    swimlane "用户"
        step "提交订单"
    swimlane "前端"
        step "校验表单"
        step "提交请求"
    swimlane "后端"
        step "创建订单"
        step "调用支付"
    swimlane "支付网关"
        step "扣款"
```"#,
    ),
    (
        "Cynefin 图",
        r#"```mermaid
cynefin-beta
    title 决策域
    domain "Clear" "有序"
    domain "Complicated" "专家域"
    domain "Complex" "探索域"
    domain "Chaotic" "危机域"
    domain "Confused" "混乱域"
```"#,
    ),
    (
        "铁路图",
        r#"```mermaid
railroad-beta
    title 简单表达式
    sequence
        terminal "let"
        non-terminal "identifier"
        terminal "="
        non-terminal "expression"
        terminal ";"
```"#,
    ),
];

/// 菜单栏结构对齐 Typora：文件 / 编辑 / 段落 / 格式 / 视图 / 主题 / 窗口 / 帮助。
/// 段落 = 块级结构（标题、列表、表格、代码块、公式块…），格式 = 行内样式（加粗、斜体、链接…）。
pub(super) fn menu_commands(k: MenuKind) -> Vec<Command> {
    match k {
        MenuKind::File => vec![
            Command::New,
            Command::QuickOpen,
            Command::CommandPalette,
            Command::OpenFileDialog,
            Command::OpenWorkspace,
            Command::Save,
            Command::SaveAs,
            Command::ExportHtml,
            Command::Snapshot,
            Command::Quit,
        ],
        MenuKind::Edit => vec![
            Command::Undo,
            Command::Redo,
            Command::Find,
            Command::ToggleAutoSave,
        ],
        MenuKind::Paragraph => vec![
            Command::Paragraph,
            Command::H1,
            Command::H2,
            Command::H3,
            Command::H4,
            Command::H5,
            Command::H6,
            Command::Quote,
            Command::Bullet,
            Command::Ordered,
            Command::Task,
            Command::Table,
            Command::Rule,
            Command::CodeBlock,
            Command::CodeGroup,
            Command::Math,
            Command::MathCode,
            Command::InlineMath,
            Command::Mermaid,
            Command::Flowchart,
            Command::Toc,
            Command::Footnote,
            Command::Video,
            Command::Attachment,
            Command::FrontMatter,
        ],
        MenuKind::Format => vec![
            Command::Bold,
            Command::Italic,
            Command::Underline,
            Command::Strike,
            Command::Highlight,
            Command::Code,
            Command::Link,
            Command::Image,
            Command::ClearFormat,
        ],
        MenuKind::View => vec![
            Command::ToggleSource,
            Command::ToggleFocus,
            Command::ToggleTypewriter,
            Command::ToggleLineNumbers,
            Command::ToggleSidebar,
            Command::ToggleOutline,
            Command::ToggleSidebarHeader,
            Command::ToggleWorkspaceBar,
            Command::ToggleTerminal,
            Command::Search,
            Command::Graph,
            Command::Stats,
        ],
        MenuKind::Theme => vec![Command::ThemeNext],
        MenuKind::Window => vec![Command::Minimize, Command::ToggleMaximize],
        MenuKind::Help => vec![Command::Shortcuts, Command::Settings, Command::About],
    }
}
