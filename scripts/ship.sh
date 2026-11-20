#!/bin/bash
# 构建 LanLink → dmg（默认，macOS）或 NSIS 安装包（--win32）→ 上传并发布到 VaKao 软件中心
# 用法: scripts/ship.sh [--win32] [--no-build] [--dry-run] [--no-publish]
#   --win32        改出 Windows x64 安装包；不加则保持原有 macOS dmg 行为
#   --no-build     跳过构建，直接用已有产物上传
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${HUB_CLI:-/Users/woyuxiaji/Rouarn/AliyunCloud/vakao-hub/tools/hub-cli.mjs}"

# 读 tauri.conf.json 的 version。macOS 上有 python3，Windows（Git Bash）上没有，
# 回落到 node，两者输出一致，避免为 Windows 另写一套脚本。
read_version() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c "import json;print(json.load(open('$1'))['version'])"
  else
    node -p "require('$1').version"
  fi
}

VERSION=$(read_version "$ROOT/packages/desktop/src-tauri/tauri.conf.json")
CODE=$(echo "$VERSION" | awk -F. '{printf "%d", $1*100+$2*10+$3}')

PLATFORM="darwin"
BUILD=1
PASSTHRU=()
for arg in "$@"; do
  case "$arg" in
    --win32) PLATFORM="win32" ;;
    --macos) PLATFORM="darwin" ;;
    --no-build) BUILD=0 ;;
    *) PASSTHRU+=("$arg") ;;
  esac
done

if [ "$PLATFORM" = "win32" ]; then
  # Tauri v2 的 Windows 产物：NSIS 安装器。必须在 Windows 上构建（需要 MSVC + WebView2）。
  ARTIFACT="$ROOT/packages/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/LanLink_${VERSION}_x64-setup.exe"
  if [ "$BUILD" = 1 ]; then
    ( cd "$ROOT/packages/desktop" && pnpm exec tauri build --target x86_64-pc-windows-msvc --bundles nsis )
  fi
  [ -f "$ARTIFACT" ] || { echo "找不到产物 $ARTIFACT" >&2; exit 1; }
  exec node "$CLI" release --app lanlink --version "$VERSION" --code "$CODE" \
    --file "$ARTIFACT" --platform win32 --arch x64 --format exe --min-os 10.0 \
    --notes "LanLink $VERSION" ${PASSTHRU[@]+"${PASSTHRU[@]}"}
fi

DMG="$ROOT/packages/desktop/src-tauri/target/release/bundle/dmg/LanLink_${VERSION}_aarch64.dmg"
if [ "$BUILD" = 1 ]; then
  ( cd "$ROOT/packages/desktop" && pnpm exec tauri build )
fi
[ -f "$DMG" ] || { echo "找不到产物 $DMG" >&2; exit 1; }
exec node "$CLI" release --app lanlink --version "$VERSION" --code "$CODE" \
  --file "$DMG" --platform darwin --arch arm64 --format dmg --min-os 11.0 \
  --notes "LanLink $VERSION" ${PASSTHRU[@]+"${PASSTHRU[@]}"}
