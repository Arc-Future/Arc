#!/bin/bash
# POSIX EH smoke on Ubuntu-24.04 (RFC 010 milestone ⑨).
# Logs → /tmp (workspace hygiene).
#   wsl -d Ubuntu-24.04 -- bash /mnt/d/GitCode/RF/dlang/scripts/verify/wsl-eh-smoke.sh
set -euo pipefail
REPO="${REPO:-/mnt/d/GitCode/RF/dlang}"
LOG="${TMPDIR:-/tmp}/arc-eh-smoke-$$.log"
# 产物落 Linux 本地盘：WSL DrvFs（/mnt/d）上硬链/copy 共享 runtime 易 EPERM，
# 导致 arc_runtime.so 半写 → 运行期 "file too short"。
PROBE="${PROBE:-${TMPDIR:-/tmp}/arc-e2e/eh_posix_linux}"
export PATH="${HOME}/.cargo/bin:${HOME}/bin:/usr/bin:/bin:${PATH}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/arc-target}"
export ARC_HOME="${ARC_HOME:-/tmp/arc-home}"
mkdir -p "$ARC_HOME" "$HOME/bin"
ln -sf /usr/bin/clang-18 "$HOME/bin/clang" 2>/dev/null || true
ln -sf /usr/bin/clang++-18 "$HOME/bin/clang++" 2>/dev/null || true

exec > >(tee "$LOG") 2>&1
echo "== POSIX EH smoke $(date -Is) =="
echo "repo=$REPO"
uname -a
command -v clang
clang --version | head -1

cd "$REPO"
ARC_BIN="${CARGO_TARGET_DIR}/debug/arc"
if [[ ! -x "$ARC_BIN" ]]; then
  echo "building arc (debug)…"
  if rustup run stable cargo build -p arc 2>/dev/null; then
    :
  else
    cargo build -p arc
  fi
fi
ARC="$ARC_BIN"

rm -rf "$PROBE"
mkdir -p "$PROBE/obj/Debug" "$PROBE/bin/Debug"
cat > "$PROBE/arc.toml" <<'EOF'
[package]
name = "eh_posix_linux"
edition = "1"
EOF
cat > "$PROBE/Program.as" <<'EOF'
using Arc;

void Main() {
    int caught = 0;
    try {
        throw new ArgumentNullException("buf");
    } catch (ArgumentNullException e) {
        caught = 1;
        if (e.Message == null) {
            caught = -1;
        }
    }
    if (caught != 1) {
        Environment.Exit(2);
    }
    int caught2 = 0;
    try {
        throw new ArgumentNullException("x");
    } catch (Exception e) {
        caught2 = 1;
        if (e == null) {
            caught2 = -1;
        }
    }
    if (caught2 != 1) {
        Environment.Exit(3);
    }
    int fin = 0;
    try {
        throw new InvalidOperationException("f");
    } catch (InvalidOperationException e) {
        if (e.Message == null) {
            Environment.Exit(4);
        }
    } finally {
        fin = 1;
    }
    if (fin != 1) {
        Environment.Exit(5);
    }
    Console.WriteLine("eh-ok-linux");
}
EOF

"$ARC" build "$PROBE/Program.as" --obj-dir "$PROBE/obj/Debug" -o "$PROBE/bin/Debug/eh_posix_linux"
"$PROBE/bin/Debug/eh_posix_linux"
echo "PASS exit=$? log=$LOG"
