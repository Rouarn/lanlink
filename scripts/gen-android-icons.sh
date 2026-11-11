#!/usr/bin/env bash
# 把品牌视觉资源里的 Android 图标原样铺进 gen/android（gen/ 被 gitignore 忽略）。
#
# 只做复制与资源引用，不缩放、不重绘、不加边距 —— 品牌包已按规范处理好星形比例。
# 依据：VaKao-品牌视觉资源/docs/integration/android.md、docs/ASSET-SPEC.md（§3 Android）。
#
# 为什么不用自适应图标（mipmap-anydpi-v26）：
#   品牌包的前景层 ic_launcher_foreground.png 星形外接框为 260/432 = 60.2%，
#   在 108dp 画布上是 65dp，正好压在 66dp 圆形安全区边界；小米的圆角遮罩下星形会贴边，
#   与同机其他应用（如"点墨"）并排明显偏大。品牌包没有留好安全区的前景素材，
#   而传统图标 mipmap-*/ic_launcher.png 与"点墨"的实际图标同源，系统走传统图标路径
#   会自动缩小进底板，视觉一致。因此这里只铺传统图标，不生成 anydpi-v26。
#   若将来品牌包补了带安全区的自适应前景，再把 adaptive icon 打开即可。
#
# 用法：scripts/gen-android-icons.sh [品牌资源目录]
# 默认从 ~/Pictures/VaKao-品牌视觉资源 读取。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${1:-$HOME/Pictures/VaKao-品牌视觉资源}"
ICONS="$SRC/icons/android"
RES="$ROOT/packages/desktop/src-tauri/gen/android/app/src/main/res"
MANIFEST="$ROOT/packages/desktop/src-tauri/gen/android/app/src/main/AndroidManifest.xml"

if [[ ! -d "$RES" ]]; then
  echo "找不到 $RES —— 先运行 pnpm exec tauri android init 生成工程" >&2
  exit 1
fi
if [[ ! -f "$ICONS/mipmap-xxxhdpi/ic_launcher.png" ]]; then
  echo "找不到品牌图标资源：$ICONS" >&2
  exit 1
fi

# 1. 各密度方图 / 圆图（manifest 的 icon / roundIcon 直接指向它们）
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  mkdir -p "$RES/mipmap-$d"
  cp "$ICONS/mipmap-$d/ic_launcher.png" "$RES/mipmap-$d/ic_launcher.png"
  cp "$ICONS/mipmap-$d/ic_launcher_round.png" "$RES/mipmap-$d/ic_launcher_round.png"
  # Tauri 模板留下的满幅前景，与本方案无关，清掉
  rm -f "$RES/mipmap-$d/ic_launcher_foreground.png"
done

# 2. 清掉自适应图标相关的残留（anydpi-v26 的 XML 与 432×432 三层），避免系统优先走自适应路径
rm -f "$RES/mipmap-anydpi-v26/ic_launcher.xml" "$RES/mipmap-anydpi-v26/ic_launcher_round.xml"
rmdir "$RES/mipmap-anydpi-v26" 2>/dev/null || true
rm -f "$RES/mipmap-xxxhdpi/ic_launcher_foreground.png" \
      "$RES/mipmap-xxxhdpi/ic_launcher_background.png" \
      "$RES/mipmap-xxxhdpi/ic_launcher_monochrome.png"
rm -f "$RES/drawable-nodpi/ic_launcher_foreground.png" \
      "$RES/drawable-nodpi/ic_launcher_background.png" \
      "$RES/drawable-nodpi/ic_launcher_monochrome.png"

# 模板自带的默认图标（manifest 指向 @mipmap/ic_launcher，这两个无人引用）
rm -f "$RES/drawable/ic_launcher_background.xml" "$RES/drawable-v24/ic_launcher_foreground.xml"

# 3. 清单补 roundIcon（品牌规范要求；已存在则跳过）
if [[ -f "$MANIFEST" ]] && ! grep -q 'android:roundIcon=' "$MANIFEST"; then
  python3 - "$MANIFEST" <<'PY'
import sys
path = sys.argv[1]
text = open(path, encoding="utf-8").read()
needle = 'android:icon="@mipmap/ic_launcher"'
if needle in text:
    text = text.replace(
        needle,
        needle + '\n        android:roundIcon="@mipmap/ic_launcher_round"',
        1,
    )
    open(path, "w", encoding="utf-8").write(text)
PY
fi

echo "Android 图标已写入 $RES（传统图标路径，不含自适应图标）"
