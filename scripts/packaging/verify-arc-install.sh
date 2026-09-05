#!/usr/bin/env sh
# verify-arc-install.sh — arc-install.sh 实机验收 harness（Linux/macOS/WSL）。
#
# 以 stub 分发包走完整 HTTPS 链路（自签证书 + openssl s_server），端到端验证
# 安装脚本协议，替代「骨架未实机验证」状态；Linux/macOS CI 可重复执行：
#
#   sh scripts/packaging/verify-arc-install.sh
#   # WSL（Windows 侧）：
#   #   wsl -e sh -c "cd /mnt/<repo> && sh scripts/packaging/verify-arc-install.sh"
#
# 覆盖用例：
#   T1  安装（.sha256 sidecar 校验）→ 指针/版本/启动器就绪 + doctor 通过
#   T2  SHA256 显式不符 → 拒绝安装
#   T3  --force 重装 → 幂等
#   T4  PATH 注入既有 ~/.profile（HOME 沙箱）
#   T5  破损包（缺 bin/arc）→ 拒绝且不污染 versions/
#   T6  --from-dir 就地安装（目录改名 + version.txt 推导包名）
#   T7  SDK 根内无参运行（安装器内嵌场景）→ 自动就地安装
#
# 依赖：curl tar xz openssl sha256sum|shasum。端口经 VERIFY_PORT 覆盖（默认 18443）。

set -u

WORK=$(mktemp -d "${TMPDIR:-/tmp}/verify-arc.XXXXXX")
PORT="${VERIFY_PORT:-18443}"
PKG="arc-1.0.0-x86_64-unknown-linux-gnu"
REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
INSTALL_SH="${ARC_INSTALL_SH:-$REPO_ROOT/scripts/packaging/arc-install.sh}"
PASS=0
FAIL=0
SERVER_PID=""

fail() { echo "FAIL: $1" >&2; FAIL=$((FAIL + 1)); }
pass() { echo "ok:   $1"; PASS=$((PASS + 1)); }

cleanup() {
    [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
    rm -rf "$WORK"
}
trap cleanup EXIT

command -v curl >/dev/null 2>&1 || { echo "need curl" >&2; exit 1; }
command -v openssl >/dev/null 2>&1 || { echo "need openssl" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
    SHA_CMD="sha256sum"
else
    SHA_CMD="shasum -a 256"
fi
[ -f "$INSTALL_SH" ] || { echo "arc-install.sh not found: $INSTALL_SH" >&2; exit 1; }

# --- stub 分发包：bin/arc 以 shell stub 模拟真实 CLI 面（--version / doctor）---
mkdir -p "$WORK/pkg/$PKG/bin" "$WORK/srv"
cat > "$WORK/pkg/$PKG/bin/arc" <<'STUB'
#!/bin/sh
case "$1" in
    --version) echo "arc 1.0.0" ;;
    doctor) echo "doctor: ok (stub)"; exit 0 ;;
    *) echo "stub arc: $*"; exit 0 ;;
esac
STUB
chmod +x "$WORK/pkg/$PKG/bin/arc"
tar -cJf "$WORK/srv/$PKG.tar.xz" -C "$WORK/pkg" "$PKG"
SHA_OK=$($SHA_CMD "$WORK/srv/$PKG.tar.xz" | awk '{print $1}')
printf '%s  %s.tar.xz\n' "$SHA_OK" "$PKG" > "$WORK/srv/$PKG.tar.xz.sha256"

# T2/T5 用的第二份包：内容不同（SHA 不同）+ 破损布局。
mkdir -p "$WORK/pkg2/arc-9.9.9-x86_64-unknown-linux-gnu"
tar -cJf "$WORK/srv/arc-9.9.9-x86_64-unknown-linux-gnu.tar.xz" -C "$WORK/pkg2" "arc-9.9.9-x86_64-unknown-linux-gnu"

# --- 自签证书 + 本地 HTTPS（openssl s_server -WWW：按 GET 路径回源文件）---
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/srv/key.pem" \
    -out "$WORK/srv/cert.pem" -days 2 \
    -subj "/CN=127.0.0.1" \
    -addext "subjectAltName=IP:127.0.0.1" >/dev/null 2>&1 || {
    echo "openssl req failed" >&2
    exit 1
}
(cd "$WORK/srv" && exec openssl s_server -accept "$PORT" -cert cert.pem -key key.pem -WWW -quiet) \
    >"$WORK/srv/server.log" 2>&1 &
SERVER_PID=$!

READY=0
i=0
while [ "$i" -lt 50 ]; do
    if curl --proto '=https' --tlsv1.2 -sS --cacert "$WORK/srv/cert.pem" \
        -o /dev/null "https://127.0.0.1:$PORT/$PKG.tar.xz.sha256" 2>/dev/null; then
        READY=1
        break
    fi
    sleep 0.2
    i=$((i + 1))
done
[ "$READY" -eq 1 ] || { echo "local HTTPS server did not become ready" >&2; exit 1; }
URL="https://127.0.0.1:$PORT"

BASE_URL="$URL/$PKG.tar.xz"
CA="$WORK/srv/cert.pem"

echo "== T1: install from sidecar sha256 =="
TO1="$WORK/home1/.arc"
if sh "$INSTALL_SH" --url "$BASE_URL" --ca "$CA" --to "$TO1" --no-modify-path >/dev/null 2>&1; then
    pass "install exit 0"
else
    fail "install exit 0"
fi
[ "$(cat "$TO1/versions/current" 2>/dev/null)" = "1.0.0" ] && pass "versions/current=1.0.0" || fail "versions/current marker"
[ -x "$TO1/bin/arc" ] && pass "bin/arc launcher executable" || fail "bin/arc launcher"
[ "$("$TO1/bin/arc" --version 2>/dev/null)" = "arc 1.0.0" ] && pass "launcher --version" || fail "launcher --version"
[ -x "$TO1/versions/$PKG/bin/arc" ] && pass "versioned dir layout" || fail "versioned dir layout"

echo "== T2: sha256 mismatch rejected =="
if sh "$INSTALL_SH" --url "$URL/arc-9.9.9-x86_64-unknown-linux-gnu.tar.xz" \
    --sha256 "$SHA_OK" --ca "$CA" --to "$WORK/home2/.arc" --no-modify-path >/dev/null 2>&1; then
    fail "mismatched sha256 rejected"
else
    pass "mismatched sha256 rejected"
fi

echo "== T3: --force reinstall =="
if sh "$INSTALL_SH" --url "$BASE_URL" --ca "$CA" --to "$TO1" --no-modify-path --force >/dev/null 2>&1; then
    pass "force reinstall exit 0"
else
    fail "force reinstall exit 0"
fi

echo "== T4: PATH injection into existing ~/.profile =="
HOME4="$WORK/home4"
mkdir -p "$HOME4"
printf '# profile\n' > "$HOME4/.profile"
TO4="$HOME4/.arc"
# 沙箱 HOME：rc 注入必须落在 $HOME4/.profile（不触碰执行者真实 rc）。
if HOME="$HOME4" sh "$INSTALL_SH" --url "$BASE_URL" --ca "$CA" --to "$TO4" >/dev/null 2>&1; then
    if grep -qF "$TO4/bin" "$HOME4/.profile"; then
        pass "PATH line appended to ~/.profile"
    else
        fail "PATH line appended to ~/.profile"
    fi
else
    fail "install (PATH mode) exit 0"
fi

echo "== T5: broken package rejected, versions/ untouched =="
TO5="$WORK/home5/.arc"
if sh "$INSTALL_SH" --url "$URL/arc-9.9.9-x86_64-unknown-linux-gnu.tar.xz" \
    --ca "$CA" --to "$TO5" --no-modify-path >/dev/null 2>&1; then
    fail "broken package rejected"
else
    pass "broken package rejected"
fi
[ -d "$TO5/versions/arc-9.9.9-x86_64-unknown-linux-gnu" ] && fail "versions/ not polluted" || pass "versions/ not polluted"

echo "== T6: --from-dir install (renamed dir, version.txt derives pkg) =="
TO6="$WORK/home6/.arc"
mkdir -p "$WORK/sdk6-renamed/bin"
cp "$WORK/pkg/$PKG/bin/arc" "$WORK/sdk6-renamed/bin/arc"
printf 'arc=1.0.0\ntriple=x86_64-unknown-linux-gnu\n' > "$WORK/sdk6-renamed/version.txt"
if sh "$INSTALL_SH" --from-dir "$WORK/sdk6-renamed" --to "$TO6" --no-modify-path >/dev/null 2>&1; then
    pass "--from-dir install exit 0"
else
    fail "--from-dir install exit 0"
fi
[ "$(cat "$TO6/versions/current" 2>/dev/null)" = "1.0.0" ] && pass "from-dir versions/current=1.0.0" || fail "from-dir current marker"
[ -x "$TO6/versions/$PKG/bin/arc" ] && pass "pkg derived from version.txt" || fail "pkg derived from version.txt"
[ ! -d "$WORK/sdk6-renamed" ] && pass "source dir moved into layout" || fail "source dir moved into layout"

echo "== T7: no-arg run from SDK root (embedded installer) =="
mkdir -p "$WORK/sdk7/$PKG/bin"
cp "$WORK/pkg/$PKG/bin/arc" "$WORK/sdk7/$PKG/bin/arc"
printf 'arc=1.0.0\ntriple=x86_64-unknown-linux-gnu\n' > "$WORK/sdk7/$PKG/version.txt"
cp "$INSTALL_SH" "$WORK/sdk7/$PKG/install.sh"
TO7="$WORK/home7/.arc"
(cd "$WORK/sdk7/$PKG" && HOME="$WORK/home7" sh ./install.sh --no-modify-path >/dev/null 2>&1) \
    && pass "embedded no-arg install exit 0" || fail "embedded no-arg install exit 0"
[ "$(cat "$TO7/versions/current" 2>/dev/null)" = "1.0.0" ] && pass "embedded versions/current=1.0.0" || fail "embedded current marker"
[ -x "$TO7/versions/$PKG/bin/arc" ] && pass "embedded launcher ready" || fail "embedded launcher ready"

echo "== summary: $PASS passed, $FAIL failed =="
[ "$FAIL" -eq 0 ]
