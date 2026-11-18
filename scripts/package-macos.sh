#!/bin/bash
# 一条命令产出带图标的 macOS 应用与三种 DMG：./scripts/package-macos.sh
# - Markd-<version>-macos-intel.dmg          x86_64
# - Markd-<version>-macos-apple-silicon.dmg  aarch64
# - Markd-<version>-macos-universal.dmg      lipo 合成，两种芯片都能跑
# macOS 的 Dock 图标只能来自 .app 包（Info.plist + Resources/AppIcon.icns），
# 直接运行 target/release/markd 是裸可执行文件，系统只会给通用图标。
# dist/Markd.app 保留的是通用二进制版本，本机 open 直接可用。
set -euo pipefail
cd "$(dirname "$0")/.."

# CI 在 tag 构建时会注入 MARKD_VERSION（与 Release tag 一致），优先使用；
# 本地/滚动构建回落到 Cargo.toml 的 version。
VERSION="${MARKD_VERSION:-$(awk -F'\"' '/^version/{print $2; exit}' Cargo.toml)}"

# PATH 里 /opt/local/bin 的 MacPorts rustc 排在 rustup shim 之前，会让 cargo
# 实际用 MacPorts 工具链编译（它没有交叉 target 的 std，报 can't find crate
# for `core`）。把 rustup 的 shim 提到最前，保证 rustup target add 与实际
# 编译用的是同一套工具链。
export PATH="$HOME/.cargo/bin:$PATH"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release -p markd --target aarch64-apple-darwin
cargo build --release -p markd --target x86_64-apple-darwin

mkdir -p target/release
lipo -create \
  target/aarch64-apple-darwin/release/markd \
  target/x86_64-apple-darwin/release/markd \
  -output target/release/markd-universal
lipo -info target/release/markd-universal

# 把单个架构的二进制打包成 .app + DMG。
pack() {
  local bin="$1" name="$2"
  local APP="dist/Markd-$name.app"
  rm -rf "$APP" dist/dmg
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp "$bin" "$APP/Contents/MacOS/markd"
  sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
  cp assets/morphe_aurora.icns "$APP/Contents/Resources/AppIcon.icns"
  chmod +x "$APP/Contents/MacOS/markd"
  codesign --force --deep --sign - "$APP" 2>/dev/null || true
  mkdir -p dist/dmg
  cp -R "$APP" dist/dmg/
  ln -sfn /Applications dist/dmg/Applications
  rm -f "Markd-$VERSION-macos-$name.dmg"
  hdiutil create -volname "Markd" -srcfolder dist/dmg -ov -format UDZO \
    "Markd-$VERSION-macos-$name.dmg" >/dev/null
  echo "安装包：Markd-$VERSION-macos-$name.dmg"
}

pack target/x86_64-apple-darwin/release/markd intel
pack target/aarch64-apple-darwin/release/markd apple-silicon
pack target/release/markd-universal universal

# 留一份通用 .app 作为本机直接运行的入口（对齐旧版 dist/Markd.app 习惯）
rm -rf dist/Markd.app
cp -R dist/Markd-universal.app dist/Markd.app
rm -rf dist/Markd-intel.app dist/Markd-apple-silicon.app dist/Markd-universal.app

echo "完成：dist/Markd.app（通用二进制）"
echo "可直接运行：open \"dist/Markd.app\""
