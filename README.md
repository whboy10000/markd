# Markd

纯 Rust + egui 从零实现的 Markdown 编辑器，界面与交互对齐主流所见即所得编辑器。

## 这是什么

Markd 是一个桌面 Markdown 编辑器。Markdown 解析、块级实时渲染、代码高亮、流程图绘制全部自研，不依赖现成的编辑器内核。打开即写：**当前块显示源码，其余块实时渲染**（Hybrid View）。

## 功能

- **Hybrid View**：点击任意段落进入编辑，当前块显示 Markdown 源码，其余块渲染成富文本，源码无损往返。
- **块级 WYSIWYG**：标题、段落、引用、列表（含任务列表）、表格、代码块（自研词法高亮 + 一键复制 + 语言切换）、公式、Mermaid 流程图、图片、脚注、YAML Front Matter、`[TOC]`。
- **工作区**：打开文件夹、扫描 Markdown 文件树、大纲跳转、拖拽打开文件。
- **写作模式**：源码模式、专注模式、打字机模式、自动保存。
- **特色功能**：命令面板、快速打开、工作区全文搜索、双向链接关系图、写作统计、版本快照（时间机器）。
- **集成终端**：基于 PTY 的交互式 shell（VS Code 风格），支持彩色输出、备用屏（vim / less / htop），面板缩放自动同步行列。
- **6 套主题**：默认浅色 / 默认深色 / GitHub / 羊皮纸 / Nord / Solarized。
- **导出**：自包含 HTML（可用浏览器打印为 PDF）。
- **macOS 原生菜单栏**，自定义标题栏：系统红绿灯右侧是命令面板 / 快速打开 / 搜索 / 保存 / 终端快捷按钮。

## 安装

从 [Releases](https://github.com/whboy10000/markd/releases/latest) 下载对应安装包：

- **macOS**（三种架构任选其一）：
  - `Markd-*-macos-intel.dmg`：Intel 芯片
  - `Markd-*-macos-apple-silicon.dmg`：Apple Silicon（M 系列）芯片
  - `Markd-*-macos-universal.dmg`：通用二进制，两种芯片都能跑
  - 打开后把 Markd 拖进 Applications 即可。
- **Windows**：`Markd-*-windows-setup.exe`，安装向导里可选目录，会自动创建开始菜单与桌面快捷方式。
- **Linux**：`Markd-*-linux-amd64.deb`（`sudo dpkg -i Markd-*.deb`）、`Markd-*-linux-amd64.rpm`（`sudo rpm -i Markd-*.rpm`），都会注册应用菜单项；或直接用 `.tar.gz` 里的二进制。

## 运行

需要 **Rust 1.85+**（workspace 使用 `edition = "2024"`）。

```bash
# 直接启动
cargo run -p markd

# 打开指定文档
cargo run -p markd -- path/to/note.md
```

发布构建：

```bash
cargo build --release -p markd
# 产物：target/release/markd（Windows 为 target\release\markd.exe）
```

### Linux 运行依赖

```bash
sudo apt-get install -y libxkbcommon-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxrandr-dev libxi-dev libgl1-mesa-dev libegl1-mesa-dev \
  libwayland-dev libgtk-3-dev
```

## 快捷键

| 快捷键 | 功能 |
| --- | --- |
| `Cmd/Ctrl + S` / `Shift + S` | 保存 / 另存为 |
| `Cmd/Ctrl + O` | 打开文件 |
| `Cmd/Ctrl + Shift + O` | 快速打开 |
| `Cmd/Ctrl + N` | 新建文档 |
| `Cmd/Ctrl + P` | 命令面板 |
| `Cmd/Ctrl + 1 / 2 / 3` | 一级 / 二级 / 三级标题 |
| `Cmd/Ctrl + B / I / K` | 加粗 / 斜体 / 链接（弹窗粘贴地址） |
| `Cmd/Ctrl + Shift + `` `` | 行内代码 |
| `Ctrl + Shift + `` `` | 删除线 |
| `Cmd + Option + C` / `B` | 代码块 / 公式块 |
| `Cmd + Option + T` / `Q` | 表格 / 引用 |
| `Cmd + Option + O` / `U` | 有序 / 无序列表 |
| `Cmd/Ctrl + /` | 切换源码模式 |
| `Cmd/Ctrl + F` | 全文搜索 |
| `Cmd/Ctrl + Shift + L` | 显示 / 隐藏侧边栏 |
| `Cmd + Ctrl + 1` | 显示大纲 |
| `F8` / `F9` | 专注 / 打字机模式 |
| `Cmd/Ctrl + J` | 显示 / 隐藏终端 |
| `Cmd/Ctrl + Z` / `Shift + Z` | 撤销 / 重做 |
| `Cmd/Ctrl + Shift + E` | 导出 HTML |
| `F1` | 快捷键一览 |

> 终端获得焦点时，除 `Cmd/Ctrl + J` 外的应用快捷键都会让位给 shell，避免打断交互式命令。

## 项目结构

Cargo workspace，依赖方向严格单向：

```text
markd-core → markd-platform → markd-ui → markd (bin)
```

| crate | 说明 |
| --- | --- |
| `markd-core` | Markdown 解析（自研 AST）、文档模型与撤销栈、`Command` 定义、主题、工作区与配置持久化、HTML 导出 |
| `markd-platform` | macOS 原生菜单栏（muda）与系统中文字体加载 |
| `markd-ui` | egui 界面：块级渲染、Hybrid View 编辑器、侧栏、PTY 终端、各类浮层 |
| `markd` | 二进制入口 |

## 性能：50 万字长文打开不卡顿

Markd 的渲染管线为超长文档做过专门设计——**几十万字的单文件打开即写，输入与滚动全程流畅**。关键优化：

- **解析只做一次，编辑走增量**。打开文档时全文解析一次；之后每次按键只重新解析**当前编辑的那一块**（微秒级，~3µs），绝不全文重解析。只有粘贴空行等把单块拆成多块的情况才回退一次全文解析。这就是"编辑不随文档变长而变慢"的核心。
- **每帧成本与文档总长无关**。每帧只克隆活动块的源码用于编辑，非活动块只做只读排版；不存在"每帧对全文做 O(n) 字符串拼接"的路径。
- **不用虚拟滚动**。尝试过"估算高度 + 占位"的虚拟滚动，但估算高度与实测高度的差异会让滚动条反复回弹（表现为滚轮滚不动）。Markd 渲染全部块，把单块渲染做到足够快，实测 10 万字 / 989 块单帧 ~9.5ms（最差 ~13ms），远低于 60fps 预算。
- **显式宽度排版**。所有文本块都用固定可用宽度预排版成 galley 再交给 `Label`，避免横向布局里 `Label` 退化成无限宽导致长段落不换行、每帧重排。
- **撤销栈节流**。快照按 450ms 节流记录（上限 200 条），连续输入不会把撤销栈打爆，也不会每键复制一次全文。
- **懒加载的工作区**。文件树扫描有 4000 文件 / 6 万条目上限兜底；快速打开与双链关系图都只在面板打开时才后台扫描，绝不在启动阶段同步扫盘。
- **图标零运行时开销**。侧栏 220+ 个 SVG 图标编译期嵌入二进制，运行时只栅格化一次进纹理缓存，之后每帧只是纹理绘制。
- **开发构建也够快**。`[profile.dev.package."*"] opt-level = 3` 只对依赖做全量优化，`cargo run` 直接可日用，自己代码仍可断点调试。

实测基线（10 万字 / 989 块）：全文解析 ~11ms（仅打开时一次）；单块编辑增量写回 ~3µs；编辑器单帧 ~9.5ms。50 万字文档约为该基线 5 倍体量，打开与编辑同样全程流畅。

## 配置

配置与版本快照保存在用户目录 `~/.markd/`，不写入项目目录。

## 字体

正文默认使用 **阿里巴巴普惠体 3.0**：系统已安装会自动识别；未安装时可以把字体文件放到 `~/.markd/fonts/`（优先加载 Regular 字重），或安装到系统字体目录。都找不到时回退到系统中文字体。代码块始终使用等宽字体。

## 构建

GitHub Actions 会在 macOS / Windows / Linux 三个平台自动构建（`.github/workflows/build.yml`）：

- push 到 `main` / `master` 或提 PR 时执行编译与测试；
- 编译产物上传为 Actions Artifacts，可直接下载运行；
- 推送 `v*` / `20*` 标签时自动创建 Release：macOS 产出 Intel / Apple Silicon / 通用三种 DMG（`scripts/package-macos.sh`，本地与 CI 同一份逻辑），Linux 产出 `.deb` / `.rpm` / `.tar.gz`，Windows 产出 NSIS 安装程序。
