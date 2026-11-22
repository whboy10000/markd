#!/usr/bin/env python3
"""生成 Markd 压测文档，口径与 test_docs/markd-*.md 一致。

    python3 gen-stress-doc.py 输出.md 10000000        # 第二个参数是「正文字符」目标

「正文字符」指 Markd 状态栏 / 统计面板里的「字符」，即 crates/markd-core 里
`Block::plain()`（剥掉 Markdown 标记）之后的非空白字符数，不是文件字节数，
也不是文件的 Unicode 字符数（后者约为前者的 1.40 倍）。`plain_chars()` 是本机
对同一口径的 Python 复刻，用于在生成时决定章数；写完后必须用应用自己的夹具复核：

    MARKD_STRESS=1 MARKD_STRESS_FILE=输出.md cargo test --release -p markd-ui \\
      stress_stats_of_file -- --nocapture --test-threads=1   # 看 ##S file stats_chars

语料结构照抄既有阶梯（每章 4 个 H3 小节、1 个表格、1 个代码块、1 段两行引用），
使不同规模文档的 plain 比例（约 0.714）与块密度保持一致，横向比较才有意义。
"""

from __future__ import annotations

import os
import re
import sys

ASSET = "/Users/whboy/Development/myprojects/markd/assets/screenshots/main-light.png"

TOPIC_A = [
    "瓷片", "季风", "批注", "旧地图", "古河道", "气象气球", "盐场", "驿路", "潮汐", "测绘",
    "活字", "勘误", "界桩", "水文", "梯田", "灯塔", "桥梁", "运河", "关隘", "驿站",
    "冰川", "沙漠", "森林", "渔港", "矿脉", "铁路", "公路", "港湾", "岛屿", "半岛",
    "雪线", "冻土", "湿地", "草原", "盆地", "高原", "丘陵", "峡谷", "瀑布", "温泉",
    "火山", "地震", "台风", "暴雨", "干旱", "洪涝", "霜冻", "雾霾", "沙尘", "雷暴",
    "物候", "土壤", "植被", "兽径", "鸟类", "鱼类", "昆虫", "菌类", "藻类", "药材",
]
TOPIC_B = [
    "窑口", "前沿", "扫描件", "底图", "断面", "探空", "晒盐", "里程", "预报", "控制网",
    "版心", "手稿", "坐标", "站网", "坡度", "灯质", "荷载", "船闸", "地形", "档案",
    "样本", "记录", "图例", "目录", "索引", "注记", "图幅", "比例尺", "投影", "基准",
    "高程", "重力", "磁力", "电价", "水位", "流量", "含沙", "水温", "盐度", "浊度",
    "风速", "气压", "湿度", "辐射", "云量", "降水", "蒸发", "下渗", "径流", "汇流",
    "区划", "界线", "界址", "权属", "地籍", "图斑", "地类", "耕地", "园地", "林地",
]

REMARKS = [
    "通视良好", "折光显著", "复测一次", "需剔除 3 点", "正常", "雨天作业",
    "夜间观测", "仪器换代", "人员轮换", "记录缺失", "基准点位移", "重做平差",
]

UNIT_PHRASES = ["水准仪与平板仪", "全站仪与电子水准", "RTK 与机载激光雷达", "多波束测深", "InSAR 形变场"]


def topic(i: int) -> str:
    a = TOPIC_A[i % len(TOPIC_A)]
    b = TOPIC_B[(i // len(TOPIC_A) + i) % len(TOPIC_B)]
    return f"{a}与{b}"


# ---------------------------------------------------------------- 块级模板

def para_lead(i: int, name: str) -> str:
    unit = UNIT_PHRASES[i % len(UNIT_PHRASES)]
    return (
        f"在**第四纪**更新世以来的地貌演化过程中，{name}的记录往往散见于不同年代的调查报告中："
        f"早期的测量以{unit}为主，*精度受限*但覆盖面广；近现代复测引入了 `RTK` 与机载激光雷达，"
        f"把平面误差压缩到厘米级，却也让==两套数据基准==难以直接拼接。本节把两类结果放在同一坐标系下"
        f"比较，重点关注差异出现的位置与量级（详见 [[{name}·勘误]]），并给出一套可复现的换算流程。"
    )


def para_time(i: int) -> str:
    return (
        "整理材料时最容易忽略的是**时间戳的口径**。同一件事在日志、报告与信件里可能分别使用地方时、"
        "标准时与观测时，若不在读取阶段统一到 UTC，后续所有差分都会带上系统偏差。这里用 "
        "$t_{\\mathrm{corr}} = t_{\\mathrm{obs}} + \\lambda/15 - Z$ 说明换算关系，其中 λ 为经度、"
        "Z 为时区序号。校核脚本见下一段代码块。"
    )


def para_weight(i: int) -> str:
    return (
        f"该测段的权阵按距离平方倒数取 $w_i = 1/d_i^2$，自由度为 $n-u$，单位权方差用 $\\sigma_0^2$ "
        f"表示；当 $\\sigma_0 > {2 + i % 3}$ 时应回到野外重测而不是继续平差。"
    )


def math_block(i: int) -> str:
    return (
        "$$\n"
        "\\Delta h = \\frac{1}{2}\\sum_{i=1}^{n} w_i\\,v_i^2,\\qquad\n"
        f"w_i = \\frac{{1}}{{d_i^2}},\\qquad\n"
        f"\\sigma_0 = \\sqrt{{\\frac{{\\sum w_i v_i^2}}{{n-u}}}},\\qquad N = {i}\n"
        "$$\n"
    )


def bullets_block(i: int, name: str) -> str:
    return (
        f"### {i}-2 要点清单\n\n"
        f"- 坐标基准统一（含高程基准 {i} 例）\n"
        "- 仪器改正项\n"
        "  - 地磁偏角\n"
        "  - 折光差与温度改正\n"
        "- 数据分段\n"
        "  1. 原始观测\n"
        "  2. 二次平差\n"
        "- 质量标记\n"
    )


def quote_block(i: int) -> str:
    return (
        "> 记录一条经验：凡是**只用一次**的参数，一定要写进当天的备注里；"
        "三个月后没有人能凭记忆还原它。\n"
        ">\n"
        "> 引用可以嵌套：第二段保留了原始措辞，删节处以省略号标示。\n"
    )


def tasks_block(i: int, name: str) -> str:
    return (
        f"### {i}-3 待办\n\n"
        f"- [ ] 复核 {name}的第三组读数\n"
        "- [x] 建立统一坐标框架\n"
        "- [ ] 把批注扫描件按页登记\n"
        "- [x] 编写换算脚本并联调\n"
        "- [ ] 补写图例说明\n"
    )


def code_rust(i: int) -> str:
    return (
        f"```rust\n"
        f"/// 把观测时刻统一到 UTC（{i} 号测段）。\n"
        "fn to_utc(t_obs: f64, lon_deg: f64, zone: i32) -> f64 {\n"
        "    let lambda_correction = lon_deg / 15.0;\n"
        "    t_obs + lambda_correction - f64::from(zone)\n"
        "}\n\n"
        "fn main() {\n"
        f"    let samples = [{12.0 + i % 7 / 1000:.3}, {12.044 + i % 5 / 1000:.3}, 12.039];\n"
        "    let mean = samples.iter().sum::<f64>() / samples.len() as f64;\n"
        '    println!("mean = {:.4}", to_utc(mean, 116.4, 8));\n'
        "}\n"
        "```\n"
    )


def code_python(i: int) -> str:
    return (
        "```python\n"
        "import statistics as st\n\n"
        f"def unify(rows, lon={116.4 + (i % 10) / 100}, zone={8}):\n"
        "    \"\"\"把地方时观测值统一到 UTC（%d 号测段）。\"\"\" % 0\n"
        "    return [r[0] + lon / 15.0 - zone for r in rows]\n\n\n"
        f"if __name__ == \"__main__\":\n"
        f"    rows = [(12.03 + k / 1000, 0.0{10 + i % 50}) for k in range({20 + i % 30})]\n"
        "    print(st.mean(unify(rows)))\n"
        "```\n"
    )


def code_json(i: int) -> str:
    return (
        "```json\n"
        "{\n"
        f'  "campaign": {i},\n'
        '  "datum": "CGCS2000",\n'
        '  "stations": [\n'
        f'    {{"id": "S-{i}", "lon": 116.4, "lat": 39.9, "zone": 8}},\n'
        f'    {{"id": "S-{i + 1}", "lon": 121.5, "lat": 31.2, "zone": 8}}\n'
        "  ],\n"
        '  "tolerance_m": 0.02,\n'
        f'  "notes": "残差超限记录见 obs 表（{i}）"\n'
        "}\n"
        "```\n"
    )


def code_bash(i: int) -> str:
    return (
        "```bash\n"
        "#!/usr/bin/env bash\n"
        f"# {i} 号测段批量转换\n"
        "set -euo pipefail\n"
        'in_dir="${1:-./raw}"\n'
        'out_dir="${2:-./utc}"\n'
        'mkdir -p "$out_dir"\n'
        'for f in "$in_dir"/*.csv; do\n'
        '  name="$(basename "$f" .csv)"\n'
        f'  python3 unify.py --lon 116.4 --zone 8 "$f" > "$out_dir/$name.csv"\n'
        "done\n"
        'echo "done: $(find "$out_dir" -name \'*.csv\' | wc -l) files"\n'
        "```\n"
    )


def code_sql(i: int) -> str:
    return (
        "```sql\n"
        f"-- {i} 号测段：按测站汇总残差超限时数\n"
        "CREATE TEMP VIEW resid AS\n"
        "SELECT station_id, obs_time, value - model_value AS d\n"
        f"FROM obs\n"
        f"WHERE campaign = {i};\n\n"
        "SELECT station_id,\n"
        "       COUNT(*) FILTER (WHERE ABS(d) > 0.02) AS bad_n,\n"
        "       ROUND(AVG(ABS(d)), 4)                  AS mean_abs\n"
        "FROM resid\n"
        "GROUP BY station_id\n"
        "HAVING COUNT(*) > 10\n"
        "ORDER BY bad_n DESC\n"
        "LIMIT 20;\n"
        "```\n"
    )


CODE_FENCES = [code_rust, code_python, code_json, code_bash, code_sql]


def mermaid_block(i: int) -> str:
    kind = (i // 3) % 3
    if kind == 0:
        body = (
            "flowchart TD\n"
            "  A[原始观测] --> B{坐标统一}\n"
            "  B -->|通过| C[平差]\n"
            "  B -->|超限| D[重测]\n"
            "  D --> A\n"
            "  C --> E[成品图]\n"
            f"  E --> F[归档 {i}]\n"
        )
    elif kind == 1:
        body = (
            "sequenceDiagram\n"
            "  participant 观测员\n"
            "  participant 手簿\n"
            "  participant 平差程序\n"
            f"  观测员->>手簿: 记录 {i} 组读数\n"
            "  手簿->>平差程序: 提交原始序列\n"
            "  平差程序-->>观测员: 返回单位权方差\n"
        )
    else:
        body = (
            f"pie title 各测段残差占比（{i}）\n"
            '  "通视良好" : 42\n'
            '  "折光显著" : 28\n'
            '  "需剔除" : 17\n'
            '  "复测" : 13\n'
        )
    return f"```mermaid\n{body}\n```\n"


def image_block(i: int, name: str) -> str:
    return (
        f"![{name}总览]({ASSET})\n\n"
        f"*图 {i}：左半为成品图截图（宽度属性可拖动右下角手柄调整）。*\n"
    )


def table_block(i: int) -> str:
    rows = []
    for k, letter in enumerate("ABCDEF"):
        pts = 128 if k % 6 == 0 else [96, 142, 77, 151, 88][k % 5]
        plan = 0.014 if k % 6 == 0 else [0.021, 0.009, 0.037, 0.011, 0.026][k % 5]
        height = 0.027 if k % 6 == 0 else [0.033, 0.018, 0.051, 0.022, 0.040][k % 5]
        rows.append(
            f"| {i}-{letter} | {pts} | {plan:.3f} | {height:.3f} | {REMARKS[k % len(REMARKS)]} |"
        )
    return (
        f"### {i}-5 分段统计\n\n"
        "| 测段 | 点数 | 平面残差 (m) | 高程残差 (m) | 备注 |\n"
        "|:---|---:|---:|---:|:---|\n"
        + "\n".join(rows)
        + "\n"
    )


def summary(i: int, name: str) -> str:
    return (
        f"本章小结：{name}段的平面残差中位数 {0.010 + (i % 60) / 1000:.3f} m，"
        f"高程残差中位数 {0.030 + (i % 40) / 1000:.3f} m；剔除超限点后的平差结果见第 {i + 1} 章，"
        f"与前一测段的接边差为 {(i * 7) % 23} mm。"
    )


def footnote_pair(i: int, name: str) -> tuple[str, str]:
    ref = (
        f"这里的换算模型只在测区高程小于 {800 + i % 300} m 时成立[^fn{i}]，"
        "超出后需要加入正常重力梯度项。"
    )
    definition = (
        f"[^fn{i}]: 第 {i} 条脚注：原始推导手稿保存在资料室第 {i} 柜，编号 MS-{i}；"
        f"数字化版本见档案库扫描件，页码 12–{i}。"
    )
    return ref, definition


def chapter(i: int) -> str:
    """一章：与 test_docs/markd-1000万字.md 的块密度对齐。

    代码语言 5 循环（每章 1 块）、Mermaid 3 种各 10 章一次、块级公式 3 章一次、
    图片 50 章一次、脚注 25 章一次——这几条配比是按既有文档的实数反推的。
    标题层级也与阶梯一致：每章一个 H2、四个 H3，全篇只有一处 H1。
    """
    name = topic(i)
    parts = [f"## 第 {i} 章 · {name}\n", para_lead(i, name), para_time(i), para_weight(i)]
    if i % 3 == 0:
        parts.append(math_block(i))
    parts.append(bullets_block(i, name))
    parts.append(quote_block(i))
    parts.append(tasks_block(i, name))
    parts.append(f"### {i}-4 代码与换算\n")
    parts.append(CODE_FENCES[i % 5](i))
    if i % 10 in (0, 3, 6):
        parts.append(mermaid_block(i))
    if i % 25 == 0:
        ref, definition = footnote_pair(i, name)
        parts.append(ref)
        parts.append(definition)
    if i % 50 == 0:
        parts.append(image_block(i, name))
    parts.append(table_block(i))
    parts.append(summary(i, name))
    parts.append("---\n")
    return "\n\n".join(parts) + "\n"


# ---------------------------------------------------------------- 正文字符计数

INLINE_PATTERNS = [
    (re.compile(r"!\[([^\]]*)\]\([^)]*\)"), r"\1"),   # 图片只留 alt
    (re.compile(r"\[([^\]]*)\]\([^)]*\)"), r"\1"),    # 链接只留文字，URL 丢弃
    (re.compile(r"\[\[([^\]]*)\]\]"), r"\1"),         # wiki 链接
    (re.compile(r"\[\^([^\]]+)\]"), r"\1"),           # 脚注引用
    (re.compile(r"`{1,3}([^`]*)`{1,3}"), r"\1"),      # 行内代码
    (re.compile(r"\$([^$]*)\$"), r"\1"),              # 行内公式
    (re.compile(r"\*\*([^*]*)\*\*"), r"\1"),
    (re.compile(r"__([^_]*)__"), r"\1"),
    (re.compile(r"~~([^~]*)~~"), r"\1"),
    (re.compile(r"==([^=]*)=="), r"\1"),
    (re.compile(r"\*([^*]*)\*"), r"\1"),
    (re.compile(r"<(u|ins|sub|sup)>([^<]*)</\1>"), r"\2"),
]

RE_FRONT = re.compile(r"^---\s*$")


def plain_chars(text: str) -> int:
    """复刻 `Block::plain()` + 非空白计数：统计面板里的「字符」。

    代码围栏的语言名与 ``` 本身、表格竖线与分隔行、引用尖角、列表符号、标题 `#`、
    前置元数据、分隔线都不计入；代码正文与公式 tex 计入（含其中的符号）。
    """
    lines = text.split("\n")
    out: list[str] = []      # 走 Markdown 行内剥标的普通段
    raw: list[str] = []      # 代码与公式正文：`plain()` 原样返回，不能再剥 `*` `_`
    in_fence = False
    in_math = False
    in_front = False
    for idx, line_raw in enumerate(lines):
        line = line_raw.rstrip()
        if not in_front and idx == 0 and RE_FRONT.match(line):
            in_front = True
            continue
        if in_front:
            if RE_FRONT.match(line):
                in_front = False
            continue
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            raw.append(line)          # 代码正文原样计入，含 * 与 _
            continue
        if line == "$$":
            in_math = not in_math
            continue
        if in_math:
            raw.append(line)          # tex 同样原样计入
            continue
        if line.startswith("|"):
            if re.match(r"^\|[\s:|-]+\|$", line):
                continue              # 表格分隔行
            out.append(line.replace("|", " "))
            continue
        if RE_FRONT.match(line):
            continue                  # 章节分隔线
        m = re.match(r"^(\s*)([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?(.*)$", line)
        if m:
            out.append(m.group(4))
            continue
        m = re.match(r"^\s*>\s?(.*)$", line)
        if m:
            out.append(m.group(1))
            continue
        m = re.match(r"^#{1,6}\s+(.*)$", line)
        if m:
            out.append(m.group(1))
            continue
        m = re.match(r"^\[\^[^\]]+\]:\s*(.*)$", line)
        if m:
            out.append(m.group(1))
            continue
        out.append(line)
    joined = "\n".join(out)
    for pat, rep in INLINE_PATTERNS:
        joined = pat.sub(rep, joined)
    counted = joined + "\n" + "\n".join(raw)
    return sum(1 for c in counted if not c.isspace())


def count_stream(lines) -> int:
    """从行迭代器累加「正文字符」，全程不持有全文。

    与 `plain_chars()` 是两条独立实现（这条按 Markdown 块边界缓冲，
    那条整篇扫描），共用同一套行内剥标规则；两者在 5000 万字/1 亿字/2 亿字
    成品上都对到了 `stress_stats_of_file` 的 `stats_chars`，所以 GiB 级文件
    用这条计数，不必把整篇读进内存。
    """
    RE_LIST = re.compile(r"^(\s*)([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?(.*)$")
    RE_QUOTE = re.compile(r"^\s*>\s?(.*)$")
    RE_HEAD = re.compile(r"^#{1,6}\s+(.*)$")
    RE_FN = re.compile(r"^\[\^[^\]]+\]:\s*(.*)$")
    RE_SEP = re.compile(r"^\|[\s:|-]+\|$")
    total = 0
    buf: list[str] = []
    in_fence = in_math = in_front = False
    first = True

    def flush() -> int:
        nonlocal buf
        if not buf:
            return 0
        text = "\n".join(buf)
        for pat, rep in INLINE_PATTERNS:
            text = pat.sub(rep, text)
        buf = []
        return sum(1 for c in text if not c.isspace())

    for raw_line in lines:
        line = raw_line.rstrip("\n").rstrip()
        if first:
            first = False
            if RE_FRONT.match(line):
                in_front = True
                continue
        if in_front:
            if RE_FRONT.match(line):
                in_front = False
            continue
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if line == "$$":              # 先切换状态再累计，否则闭合的 $$ 会被当成正文
            in_math = not in_math
            continue
        if in_fence or in_math:
            total += sum(1 for c in line if not c.isspace())
            continue
        if not line:
            total += flush()
            continue
        if line.startswith("|"):
            if not RE_SEP.match(line):
                buf.append(line.replace("|", " "))
            continue
        if RE_FRONT.match(line):
            total += flush()
            continue
        m = RE_LIST.match(line)
        if m:
            buf.append(m.group(4))
            continue
        for pat in (RE_QUOTE, RE_HEAD, RE_FN):
            m = pat.match(line)
            if m:
                buf.append(m.group(1))
                break
        else:
            buf.append(line)
    total += flush()
    return total


def count_file(path: str) -> int:
    with open(path, encoding="utf-8") as f:
        return count_stream(f)


# ---------------------------------------------------------------- 文档装配

def wan_label(target: int) -> str:
    """标题里的规模名：不足 1 亿用「万字」，够 1 亿用「亿字」。"""
    return f"{target / 100000000:g} 亿字" if target >= 100000000 else f"{target // 10000:,} 万字"


def header(target: int, plain: int, unicode_chars: int, chapters: int, blocks: int,
           blocks_measured: bool = False) -> str:
    ratio = plain / unicode_chars if unicode_chars else 0.0
    return (
        "---\n"
        f"title: Markd 压测文档（{wan_label(target)}）\n"
        "author: 自动生成\n"
        "date: 2026-11-21\n"
        "tags: [压测, 长文档]\n"
        "---\n\n"
        f"# Markd 压测文档：正文 {plain:,} 字符\n\n"
        f"> 本文档的**正文字符数**（Markd 状态栏 / 统计面板里的「字符」）为 {plain:,}"
        f"（生成目标 {target:,}）；文件 Unicode 字符数 {unicode_chars:,}"
        f"（含 Markdown 标记与空白），共 {chapters:,} 章、{blocks_text}，比例 {ratio:.3f}。\n"
        "> 上面的数字是镜像口径逐章累加后写进本标题的，复核请用应用自己的统计：\n"
        "> `MARKD_STRESS=1 MARKD_STRESS_FILE=本文档 cargo test --release -p markd-ui "
        "stress_stats_of_file -- --nocapture --test-threads=1`，看 `##S file stats_chars`。\n"
        ">\n"
        "> 内容包含标题、段落、无序/有序/任务列表、引用、表格、五种语言的代码块、"
        "Mermaid 三种图、行内与块级公式、脚注、图片与分隔线，用来验证超长文档下的"
        "打开、滚动、搜索、编辑、撤销与导出表现。由 `gen-stress-doc.py` 生成，"
        "块密度对齐既有的 markd-1000万字.md（约 52 块/章）。\n\n"
        "## 建议的测试动作\n\n"
        "| 动作 | 怎么看 |\n"
        "|:---|:---|\n"
        "| 打开文件 | 从冷启动到可交互的等待时间（读盘 / 解析 / 大纲 / 统计各段） |\n"
        "| 快速滚动 | 拖动滚动条与滚轮连滚，观察是否出现空白帧、回弹与掉帧 |\n"
        "| 全文搜索 | 搜一个只出现在末尾的词，看扫描与跳转耗时 |\n"
        "| 编辑与撤销 | 在中间章节打一个字再撤销，看输入延迟与内存增长 |\n"
        "| 源码模式 | 切到源码视图再切回，看两态是否都保持流畅 |\n"
        "| 导出 | 导出 HTML / PDF，看是否走完且不崩 |\n\n"
        "## 阅读位置标记\n\n"
        "开头标记：正文从这里开始，用于确认冷启动首屏。\n\n"
    )


def tail(chapters: int) -> str:
    return (
        "\n## 结尾标记\n\n"
        f"结尾标记：全文共 {chapters:,} 章，能滚动到这里说明末屏渲染与大纲跳转都正常。\n"
    )


# 每章块数：本语料自己生成的 markd-5000万字.md 实测 2,181,609 块 / 41,825 章。
# 头部只写「约」，因为块数由应用的解析器决定，Python 侧只能按密度推。
BLOCKS_PER_CHAPTER = 2181609 / 41825

# 头部与尾部的正文字符量级（个位数字级别波动），只用于估章数，最终值按不动点算。
HEAD_TAIL_PLAIN = 700
COPY_BUF = 1 << 22


def settle(target: int, body_plain: int, body_uni: int, n: int) -> tuple[int, int]:
    """头部数字 ↔ 全文字数互为输入，迭代到不动点（数字位数一变就影响字数）。"""
    claim = (body_plain, body_uni)
    for _ in range(8):
        head = header(target, claim[0], claim[1], n, int(n * BLOCKS_PER_CHAPTER))
        foot = tail(n)
        nxt = (plain_chars(head) + body_plain + plain_chars(foot), len(head) + body_uni + len(foot))
        if nxt == claim:
            return claim
        claim = nxt
    return claim


def write_doc(out_path: str, target: int) -> tuple[int, int, int, int]:
    """流式生成「正文字符 ≥ target」的文档，返回（正文字符, Unicode 字符, 章数, 块数）。

    全文不落内存：正文按章写进二进制临时文件（10 亿字级是 2.3 GiB，留在 Python 字符串
    里要 5 GB+），内存只保留每章的正文字符数、Unicode 字符数与起始字节偏移。
    章数先按 150 章周期窗口的均值估，逐章精确累加到够数，再从尾部逐章回收，
    把超出量压到一章以内；回收只改计数与临时文件长度，不重排正文。
    """
    per_chapter = sum(plain_chars(chapter(i)) for i in range(1, 151)) / 150
    tmp_path = out_path + ".body.tmp"
    sizes: list[int] = []       # 每章正文字符
    unis: list[int] = []        # 每章 Unicode 字符
    starts: list[int] = []      # 每章起始字节偏移（临时文件按字节定位，回收时直接截断）

    with open(tmp_path, "wb") as f:
        total_plain = 0
        total_uni = 0
        i = 1
        while total_plain + HEAD_TAIL_PLAIN < target:
            c = chapter(i)
            data = c.encode("utf-8")
            starts.append(f.tell())
            f.write(data)
            p, u = plain_chars(c), len(c)
            sizes.append(p)
            unis.append(u)
            total_plain += p
            total_uni += u
            i += 1
            if i % 200000 == 0:
                print(f"  …已写 {i:,} 章 / {total_plain:,} 正文字符", file=sys.stderr)
        n = i - 1
        while n > 1:
            trial = settle(target, total_plain - sizes[n - 1], total_uni - unis[n - 1], n - 1)
            if trial[0] < target:
                break
            total_plain -= sizes[n - 1]
            total_uni -= unis[n - 1]
            sizes.pop()
            unis.pop()
            n -= 1
        claim = settle(target, total_plain, total_uni, n)
        if n < len(starts):
            f.truncate(starts[n])

    head = header(target, claim[0], claim[1], n, int(n * BLOCKS_PER_CHAPTER)).encode("utf-8")
    foot = tail(n).encode("utf-8")
    # 先写 .new 再原子改名：中途被打断不会把既有文档截断成半份。
    new_path = out_path + ".new"
    with open(new_path, "wb") as out:
        out.write(head)
        with open(tmp_path, "rb") as body:
            while True:
                block = body.read(COPY_BUF)
                if not block:
                    break
                out.write(block)
        out.write(foot)
    os.remove(tmp_path)
    os.replace(new_path, out_path)
    return claim[0], claim[1], n, int(n * BLOCKS_PER_CHAPTER)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    out_path, target = sys.argv[1], int(sys.argv[2].replace("_", ""))
    plain, uni, n, blocks = write_doc(out_path, target)
    size = os.path.getsize(out_path)
    print(f"写入 {out_path}")
    print(f"  正文字符（镜像口径）= {plain:,}   目标 = {target:,}   超出 = {plain - target:+,}")
    print(f"  Unicode 字符 = {uni:,}")
    print(f"  字节 = {size:,}")
    print(f"  章数 = {n:,}   块数 ≈ {blocks:,}")
    print(f"复核： MARKD_STRESS=1 MARKD_STRESS_FILE={out_path} cargo test --release "
          "-p markd-ui stress_stats_of_file -- --nocapture --test-threads=1")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
