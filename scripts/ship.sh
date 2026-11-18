#!/bin/bash
# 构建 LanLink → dmg → 上传并发布到 VaKao 软件中心
# 用法: scripts/ship.sh [--no-build] [--dry-run] [--no-publish]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${HUB_CLI:-/Users/woyuxiaji/Rouarn/AliyunCloud/vakao-hub/tools/hub-cli.mjs}"
VERSION=$(python3 -c "import json;print(json.load(open('$ROOT/packages/desktop/src-tauri/tauri.conf.json'))['version'])")
CODE=$(echo "$VERSION" | awk -F. '{printf "%d", $1*100+$2*10+$3}')
DMG="$ROOT/packages/desktop/src-tauri/target/release/bundle/dmg/LanLink_${VERSION}_aarch64.dmg"
if [ "${1:-}" != "--no-build" ]; then
  ( cd "$ROOT/packages/desktop" && pnpm exec tauri build )
fi
[ -f "$DMG" ] || { echo "找不到产物 $DMG" >&2; exit 1; }
exec node "$CLI" release --app lanlink --version "$VERSION" --code "$CODE" \
  --file "$DMG" --platform darwin --arch arm64 --format dmg --min-os 11.0 \
  --notes "LanLink $VERSION" "$@"
