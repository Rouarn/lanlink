#!/usr/bin/env bash
# 拉取 Android 端构建所需的 EasyTier 源码与它引用的 GitHub 依赖。
#
# 产物（均被 .gitignore 忽略，不入库）：
#   .easytier/       EasyTier 源码（固定 commit）
#   .easytier-deps/  EasyTier 用 git 引用的 5 个依赖（本机 GitHub git 协议不可达，
#                    统一换成 codeload 压缩包 + Cargo [patch] 指向本地目录）
#
# 用法：bash scripts/fetch-easytier.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="$ROOT/.easytier"
DEPS_DIR="$ROOT/.easytier-deps"
EASYTIER_REV="f07d8d198ab4b8f046d392f5a51308eca87cdde5"

# codeload 直连可用（实测 ~5MB/s）；ghproxy 作为兜底。
download() {
  local url="$1" out="$2"
  curl -fsSL --connect-timeout 15 --max-time 600 -o "$out" "$url" \
    || curl -fsSL --connect-timeout 15 --max-time 600 -o "$out" "https://ghproxy.net/$url"
}

fetch_tar() {
  # fetch_tar <dest> <repo> <ref> <expected-marker>
  local dest="$1" repo="$2" ref="$3" marker="$4"
  if [ -e "$dest/$marker" ]; then
    echo "[skip] $dest 已存在"
    return 0
  fi
  local tmp
  tmp="$(mktemp -d)"
  echo "[fetch] $repo @ $ref"
  download "https://codeload.github.com/$repo/tar.gz/$ref" "$tmp/src.tar.gz"
  tar -xzf "$tmp/src.tar.gz" -C "$tmp"
  mkdir -p "$dest"
  local inner
  inner="$(find "$tmp" -maxdepth 1 -type d ! -path "$tmp" | head -1)"
  cp -R "$inner"/. "$dest"/
  rm -rf "$tmp"
}

fetch_tar "$SRC_DIR" "EasyTier/EasyTier" "$EASYTIER_REV" "easytier/Cargo.toml"
fetch_tar "$DEPS_DIR/thunk" "EasyTier/thunk" "refs/heads/master" "Cargo.toml"
fetch_tar "$DEPS_DIR/service-manager" "EasyTier/service-manager-rs" "refs/heads/main" "Cargo.toml"
fetch_tar "$DEPS_DIR/rust-tun" "EasyTier/rust-tun" "43621f70b02c47086b40d34fdde997719668a6c0" "Cargo.toml"
fetch_tar "$DEPS_DIR/kcp-sys" "EasyTier/kcp-sys" "268533568d734ae89dc89603078da3ca522effe1" "Cargo.toml"
fetch_tar "$DEPS_DIR/tokio-websockets" "EasyTier/tokio-websockets" "refs/heads/main" "Cargo.toml"

echo "完成：$SRC_DIR , $DEPS_DIR"
