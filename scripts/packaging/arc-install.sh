#!/usr/bin/env sh
# arc-install.sh — Arc SDK installer (Unix, rustup-style)
#
# 下载 tar.xz → SHA256 校验 → 解压到 $ARC_HOME/versions/<pkg> → 指针布局
# （versions/current + bin/arc 启动器，与 `arc self-update` 一致）→ PATH 注入
# （~/.profile / ~/.bashrc / ~/.zshrc，可跳过）→ 打印版本 + arc doctor。
#
# 用法（URL 为占位；真实发布端点定版后启用）：
#   curl --proto '=https' --tlsv1.2 -sSf https://static.arc.dev/install.sh | sh
#   sh arc-install.sh --url <pkg-url> [--sha256 <hex>] [--ca <cert>] [--to <dir>]
#                     [--no-modify-path] [--force]
#
# 选项：
#   --url <url>          分发包下载地址（HTTPS，tar.xz）
#   --sha256 <hex>       期望 SHA256（缺省取 <url>.sha256 清单）
#   --ca <cert>          自定义 CA 证书（curl --cacert；自托管镜像/企业内网）
#   --to <dir>           安装根（缺省 $ARC_HOME 或 ~/.arc）
#   --no-modify-path     不修改 shell 启动文件（~/.profile 等）
#   --force              目标版本已安装时强制重装（缺省刷新指针后结束）
#
# 安全：强制 HTTPS + SHA256 校验（与 Windows install.ps1 同一契约）。
# POSIX sh only（无 bashism）；实机验收 harness 见
# scripts/packaging/verify-arc-install.sh（Linux/macOS/WSL 可重复执行）。

set -e

URL=""
SHA256=""
CA=""
TO="${ARC_HOME:-$HOME/.arc}"
MODIFY_PATH=1
FORCE=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --url) URL="$2"; shift 2 ;;
        --sha256) SHA256="$2"; shift 2 ;;
        --ca) CA="$2"; shift 2 ;;
        --to) TO="$2"; shift 2 ;;
        --no-modify-path) MODIFY_PATH=0; shift ;;
        --force) FORCE=1; shift ;;
        *) echo "arc-install.sh: unknown option $1" >&2; exit 1 ;;
    esac
done

# --- 前置检查（工具可用性，给出明确错误而非晦涩失败）---
need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "arc-install.sh: required tool not found: $1" >&2
        exit 1
    fi
}
need curl
need tar
command -v sha256sum >/dev/null 2>&1 && SHA_BIN="sha256sum" \
    || { need shasum; SHA_BIN="shasum"; }
case "$SHA_BIN" in
    sha256sum) SHA256_CMD="sha256sum" ;;
    shasum) SHA256_CMD="shasum -a 256" ;;
esac

if [ -n "$CA" ] && [ ! -f "$CA" ]; then
    echo "arc-install.sh: --ca certificate not found: $CA" >&2
    exit 1
fi

if [ -z "$URL" ]; then
    echo "arc-install.sh: --url is required (placeholder; see scripts/packaging/arc-pack.ps1 output)" >&2
    exit 1
fi
case "$URL" in
    https://*) ;;
    *) echo "arc-install.sh: URL must be HTTPS ($URL)" >&2; exit 1 ;;
esac

PKG=$(basename "$URL" .tar.xz)
[ -z "$PKG" ] && { echo "arc-install.sh: cannot derive package name from $URL" >&2; exit 1; }

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/arc-install.XXXXXX")
trap 'rm -rf "$TMP_DIR"' EXIT

# curl 附加参数（--ca 时走自定信任锚；缺省系统 CA 库）。
CURL_ARGS="--proto =https --tlsv1.2 -sSfL"
if [ -n "$CA" ]; then
    CURL_ARGS="$CURL_ARGS --cacert $CA"
fi

# --- 下载 + SHA256 校验 ---
echo "==> downloading $URL"
# shellcheck disable=SC2086 — CURL_ARGS 为受控白名单参数，有意分词。
curl $CURL_ARGS "$URL" -o "$TMP_DIR/$PKG.tar.xz"

ACTUAL=$( $SHA256_CMD "$TMP_DIR/$PKG.tar.xz" | awk '{print $1}' )
if [ -z "$SHA256" ]; then
    # shellcheck disable=SC2086
    SHA256=$(curl $CURL_ARGS "$URL.sha256" | awk '{print $1}')
fi
if [ "$ACTUAL" != "$SHA256" ]; then
    echo "arc-install.sh: SHA256 mismatch — expected $SHA256, got $ACTUAL" >&2
    exit 1
fi
echo "==> sha256 ok: $ACTUAL"

# --- 解压到 staging → 布局自检 → 落位 <to>/versions/<pkg> ---
# 先解压到临时 staging 并验证 bin/arc 存在，再原子落位——
# 包内顶层目录与包名不符（破损包）不会污染 versions/。
UNPACK="$TMP_DIR/unpack"
mkdir -p "$UNPACK"
tar -xJf "$TMP_DIR/$PKG.tar.xz" -C "$UNPACK"
[ -x "$UNPACK/$PKG/bin/arc" ] || {
    echo "arc-install.sh: package $PKG has no $PKG/bin/arc (broken package or layout mismatch)" >&2
    exit 1
}

TARGET="$TO/versions/$PKG"
mkdir -p "$TO/versions"
if [ -d "$TARGET" ]; then
    if [ "$FORCE" -eq 0 ]; then
        echo "==> already installed: $TARGET (--force to reinstall)"
    else
        rm -rf "$TARGET"
        mv "$UNPACK/$PKG" "$TARGET"
    fi
else
    mv "$UNPACK/$PKG" "$TARGET"
fi

[ -x "$TARGET/bin/arc" ] || { echo "arc-install.sh: $PKG has no bin/arc (broken package)" >&2; exit 1; }

# --- 指针布局：versions/current 标记 + 根 bin/arc 启动器（唯一 PATH 注入点）---
# 与 `arc self-update` 一致：多版本共存，切换只改指针与标记，PATH 永不变。
VER=$(echo "$PKG" | cut -d- -f2)
[ -z "$VER" ] && { echo "arc-install.sh: cannot derive version from $PKG" >&2; exit 1; }
mkdir -p "$TO/bin"
printf '%s\n' "$VER" > "$TO/versions/current"
cp "$TARGET/bin/arc" "$TO/bin/arc"
echo "==> pointer layout: versions/current=$VER, $TO/bin/arc"

# --- PATH 注入（~/.profile / ~/.bashrc / ~/.zshrc；已含则跳过）---
if [ "$MODIFY_PATH" -eq 1 ]; then
    BIN_DIR="$TO/bin"
    line="export PATH=\"$BIN_DIR:\$PATH\""
    appended=0
    skipped=0
    # 仅注入已存在的启动文件（不替用户创建文件）。
    for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
        [ -f "$rc" ] || continue
        if grep -qF "$BIN_DIR" "$rc" 2>/dev/null; then
            skipped=1
        else
            printf '\n%s\n' "$line" >> "$rc"
            echo "==> PATH updated in $rc"
            appended=1
        fi
    done
    if [ "$appended" -eq 1 ]; then
        echo "    (new shells only — current shell needs 'source' or restart)"
    elif [ "$skipped" -eq 1 ]; then
        echo "==> PATH already contains $BIN_DIR (skipped)"
    fi
else
    echo "==> --no-modify-path: PATH not modified (add $TO/bin to PATH manually)"
fi

# --- 版本 + doctor ---
echo "==> installed: $TARGET (active via $TO/bin)"
"$TO/bin/arc" --version
"$TO/bin/arc" doctor || { echo "arc-install.sh: arc doctor reported failures" >&2; exit 1; }
echo "==> install complete. Uninstall: rm -rf $TO/bin $TO/versions and remove $TO/bin from PATH."
