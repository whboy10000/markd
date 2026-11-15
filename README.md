# Markd

纯 Rust + egui 从零实现的 Markdown 编辑器，界面与交互对齐主流所见即所得编辑器。

## 这是什么

Markd 是一个桌面 Markdown 编辑器。Markdown 解析、块级实时渲染、代码高亮、流程图绘制全部自研，不依赖现成的编辑器内核。打开即写：**当前块显示源码，其余块实时渲染**（Hybrid View）。

## 功能

- **Hybrid View**：点击任意段落进入编辑，当前块显示 Markdown 源码，其余块渲染成富文本，源码无损往返。
- **块级 WYSIWYG**：标题、段落、引用、列表（含任务列表）、表格、代码块（自研词法高亮 + 一键复制）、公式、Mermaid 流程图、图片、脚注、YAML Front Matter、`[TOC]`。
- **工作区**：打开文件夹、扫描 Markdown 文件树、大纲跳转、拖拽打开文件。
- **写作模式**：源码模式、专注模式、打字机模式、自动保存。
- **特色功能**：命令面板、快速打开、工作区全文搜索、双向链接关系图、写作统计、版本快照（时间机器）。
- **集成终端**：基于 PTY 的交互式 shell（VS Code 风格），支持彩色输出、备用屏（vim / less / htop），面板缩放自动同步行列。
- **6 套主题**：默认浅色 / 默认深色 / GitHub / 羊皮纸 / Nord / Solarized。
- **导出**：自包含 HTML（可用浏览器打印为 PDF）。
- **macOS 原生菜单栏**，自定义标题栏：系统红绿灯右侧是命令面板 / 快速打开 / 搜索 / 保存 / 终端快捷按钮。

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
| `Cmd/Ctrl + S` | 保存 |
| `Cmd/Ctrl + N` | 新建文档 |
| `Cmd/Ctrl + P` | 命令面板 |
| `Cmd/Ctrl + Shift + P` | 快速打开文件 |
| `Cmd/Ctrl + F` | 工作区全文搜索 |
| `Cmd/Ctrl + J` | 显示 / 隐藏终端 |
| `Cmd/Ctrl + E` | 切换源码模式 |
| `Cmd/Ctrl + Shift + E` | 导出 HTML |
| `Cmd/Ctrl + B / I / K` | 加粗 / 斜体 / 链接 |
| `Cmd/Ctrl + Z / Shift + Z` | 撤销 / 重做 |
| `Cmd/Ctrl + 1 / 2 / 3` | 一级 / 二级 / 三级标题 |
| `Cmd/Ctrl + Shift + F` | 专注模式 |
| `Cmd/Ctrl + Shift + T` | 打字机模式 |
| `Cmd/Ctrl + /` | 快捷键一览 |

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

## 配置

配置与版本快照保存在用户目录 `~/.markd/`，不写入项目目录。

## 构建

GitHub Actions 会在 macOS / Windows / Linux 三个平台自动构建（`.github/workflows/build.yml`）：

- push 到 `main` / `master` 或提 PR 时执行编译与测试；
- 编译产物上传为 Actions Artifacts，可直接下载运行；
- 推送 `v*` 标签时自动创建 Release 并附带三平台可执行文件。
