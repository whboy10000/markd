#!/bin/bash
# 下载 Pandoc 官方二进制到 vendor/ 目录，供三平台打包脚本嵌入。
# 用法：./scripts/fetch-pandoc.sh macos|windows|linux
#
# 设计要点：
# - 版本固定在 PANDOC_VERSION，避免 CI 今天和明天打出不同的包；
# - 下载结果放进 vendor/.cache/ 复用，CI 的 actions/cache 可以缓存这个目录；
# - 每个平台产出的文件统一叫 pandoc（Windows 为 pandoc.exe），
#   让运行时 bundled_executable() 只需查一个固定名字；
# - macOS 两个架构各下载一份，universal 包由 package-macos.sh 用 lipo 合并。
set -euo pipefail
cd "$(dirname "$0")/.."

PLATFORM="${1:-}"
PANDOC_VERSION="${PANDOC_VERSION:-3.14}"
CACHE="vendor/.cache"
BASE="https://github.com/jgm/pandoc/releases/download/${PANDOC_VERSION}"

log() { printf '[fetch-pandoc] %s\n' "$*"; }

fetch() {
  local url="$1" out="$2"
  if [ -s "$out" ]; then
    log "已缓存：$out"
    return
  fi
  mkdir -p "$(dirname "$out")"
  log "下载 $url"
  curl -fL --retry 3 --retry-delay 2 --connect-timeout 20 -o "$out.part" "$url"
  mv "$out.part" "$out"
}

extract_binary() {
  local archive="$1" member_hint="$2" out="$3"
  # member 在整个函数里都要用（tar 分支也引用），必须在 case 之前声明。
  local member="$member_hint"
  mkdir -p "$(dirname "$out")"
  case "$archive" in
    *.zip)
      # 官方包成员都带版本目录（如 pandoc-3.14/bin/pandoc.exe），本地缓存包
      # 可能用 zip -j 压成裸文件名。按「精确 → 按 basename 后缀匹配」顺序找。
      if command -v unzip >/dev/null 2>&1; then
        local base
        base="$(basename "$member_hint")"
        if ! unzip -Z1 "$archive" | grep -qx "$member"; then
          member="$(unzip -Z1 "$archive" | grep -E "(^|/)${base}$" | head -n 1)"
        fi
        test -n "$member" || { echo "归档里找不到 $base：$archive" >&2; exit 1; }
        unzip -p "$archive" "$member" > "$out.part"
      elif command -v powershell.exe >/dev/null 2>&1; then
        # Git Bash 未必带 unzip；Windows runner 一定有 PowerShell。
        local tmpdir
        tmpdir="$(mktemp -d)"
        powershell.exe -NoProfile -Command \
          "Expand-Archive -LiteralPath '$(cygpath -w "$archive")' -DestinationPath '$(cygpath -w "$tmpdir")' -Force" >/dev/null
        cp "$tmpdir/$member_hint" "$out.part" 2>/dev/null \
          || cp "$tmpdir/$(basename "$member_hint")" "$out.part"
        rm -rf "$tmpdir"
      else
        echo "需要 unzip 或 PowerShell 来解压 $archive" >&2
        exit 1
      fi
      ;;
    *.tar.gz)
      tar -xzOf "$archive" "$member" > "$out.part"
      ;;
    *) echo "未知归档格式：$archive" >&2; exit 1 ;;
  esac
  mv "$out.part" "$out"
  chmod +x "$out"
  log "解出 $out ($(du -h "$out" | cut -f1))"
}

case "$PLATFORM" in
  macos)
    fetch "$BASE/pandoc-${PANDOC_VERSION}-x86_64-macOS.zip" \
      "$CACHE/pandoc-${PANDOC_VERSION}-x86_64-macOS.zip"
    fetch "$BASE/pandoc-${PANDOC_VERSION}-arm64-macOS.zip" \
      "$CACHE/pandoc-${PANDOC_VERSION}-arm64-macOS.zip"
    # 官方 macOS zip 成员名带版本目录；本地 zip -j 做的缓存包则不带路径，
    # extract_binary 会自动探测（见上面的成员名兼容逻辑）。
    extract_binary "$CACHE/pandoc-${PANDOC_VERSION}-x86_64-macOS.zip" \
      "pandoc-${PANDOC_VERSION}-x86_64/bin/pandoc" "vendor/macos-x86_64/pandoc"
    extract_binary "$CACHE/pandoc-${PANDOC_VERSION}-arm64-macOS.zip" \
      "pandoc-${PANDOC_VERSION}-arm64/bin/pandoc" "vendor/macos-aarch64/pandoc"
    ;;
  windows)
    fetch "$BASE/pandoc-${PANDOC_VERSION}-windows-x86_64.zip" \
      "$CACHE/pandoc-${PANDOC_VERSION}-windows-x86_64.zip"
    extract_binary "$CACHE/pandoc-${PANDOC_VERSION}-windows-x86_64.zip" \
      "pandoc.exe" "vendor/windows/pandoc.exe"
    ;;
  linux)
    fetch "$BASE/pandoc-${PANDOC_VERSION}-linux-amd64.tar.gz" \
      "$CACHE/pandoc-${PANDOC_VERSION}-linux-amd64.tar.gz"
    # tar 包内路径形如 pandoc-3.14/bin/pandoc。
    extract_binary "$CACHE/pandoc-${PANDOC_VERSION}-linux-amd64.tar.gz" \
      "pandoc-${PANDOC_VERSION}/bin/pandoc" "vendor/linux/pandoc"
    ;;
  *)
    echo "用法：$0 macos|windows|linux" >&2
    exit 2
    ;;
esac

# 合规：随包分发的 GPL 文本已由仓库自带（packaging/third-party/PANDOC-LICENSE.md）。
# 这里只做存在性检查，避免打包脚本静默漏掉授权文件。
test -s packaging/third-party/PANDOC-LICENSE.md || {
  echo "缺少 Pandoc 许可证文本 packaging/third-party/PANDOC-LICENSE.md" >&2
  exit 1
}
log "完成（Pandoc ${PANDOC_VERSION}）"
