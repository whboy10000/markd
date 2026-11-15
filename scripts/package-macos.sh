#!/bin/bash
# 一条命令产出带图标的 macOS 应用：./scripts/package-macos.sh
# macOS 的 Dock 图标只能来自 .app 包（Info.plist + Resources/AppIcon.icns），
# 直接运行 target/release/markd 是裸可执行文件，系统只会给通用图标。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(awk -F'"' '/^version/{print $2; exit}' Cargo.toml)
APP="dist/Markd.app"

cargo build --release -p markd

rm -rf "$APP" dist/dmg
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/markd "$APP/Contents/MacOS/markd"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
cp assets/icon.icns "$APP/Contents/Resources/AppIcon.icns"
chmod +x "$APP/Contents/MacOS/markd"
codesign --force --deep --sign - "$APP" 2>/dev/null || true

mkdir -p dist/dmg
cp -R "$APP" dist/dmg/
ln -sfn /Applications dist/dmg/Applications
rm -f "Markd-$VERSION-macos.dmg"
hdiutil create -volname "Markd" -srcfolder dist/dmg -ov -format UDZO \
  "Markd-$VERSION-macos.dmg" >/dev/null

echo "完成：$APP"
echo "安装包：Markd-$VERSION-macos.dmg"
echo "可直接运行：open \"$APP\""
