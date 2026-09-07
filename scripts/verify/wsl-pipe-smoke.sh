#!/bin/bash
# POSIX NamedPipe sync smoke on Ubuntu-24.04 (RFC 048 M0/M1 Linux gate).
# Logs → /tmp (workspace hygiene). Probe under Linux local disk (DrvFs staging issues).
#   wsl -d Ubuntu-24.04 -- bash /mnt/d/GitCode/RF/dlang/scripts/verify/wsl-pipe-smoke.sh
set -euo pipefail
REPO="${REPO:-/mnt/d/GitCode/RF/dlang}"
LOG="${TMPDIR:-/tmp}/arc-pipe-smoke-$$.log"
PROBE="${PROBE:-${TMPDIR:-/tmp}/arc-e2e/pipe_posix_linux}"
export PATH="${HOME}/.cargo/bin:${HOME}/bin:/usr/bin:/bin:${PATH}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/arc-target}"
export ARC_HOME="${ARC_HOME:-/tmp/arc-home}"
mkdir -p "$ARC_HOME" "$HOME/bin"
ln -sf /usr/bin/clang-18 "$HOME/bin/clang" 2>/dev/null || true
ln -sf /usr/bin/clang++-18 "$HOME/bin/clang++" 2>/dev/null || true

exec > >(tee "$LOG") 2>&1
echo "== POSIX pipe smoke $(date -Is) =="
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
# 用 printf 写 arc.toml，避免 heredoc + CRLF/DrvFs 同步坑。
printf '%s\n' \
  '[package]' \
  'name = "pipe_posix_linux"' \
  'edition = "1"' \
  '' \
  '[dependencies]' \
  "\"Arc.Net.Pipes\" = { path = \"${REPO}/std/Net/Pipes\" }" \
  > "$PROBE/arc.toml"
echo "--- arc.toml ---"
cat "$PROBE/arc.toml"
echo "---"
cat > "$PROBE/Program.as" <<'EOF'
using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

class PipeHost {
    private NamedPipeServerStream _server;
    public bool Served;

    public PipeHost(string name) {
        _server = new NamedPipeServerStream(name);
        this.Served = false;
    }

    public void ServeInBackground() {
        Thread t = new Thread(() => { _server.WaitForConnection(); this.Served = true; });
        t.Start();
    }

    public NamedPipeServerStream Server { get { return _server; } }
}

void Main() {
    PipeHost host = new PipeHost("arc.wsl.pipe.roundtrip");
    host.ServeInBackground();
    NamedPipeClientStream client = new NamedPipeClientStream("arc.wsl.pipe.roundtrip");
    bool connected = client.Connect(5000);
    if (!connected) {
        Console.WriteLine("pipe-fail-connect");
        Environment.Exit(2);
    }
    // 同步自旋握手：勿用 await Task.Delay——Linux 上 Delay 续体曾现零唤醒源挂死
    //（RFC 048 同步握手前置；与 Windows 契约测同构）。
    int spins = 0;
    while (!host.Server.IsConnected && spins < 500) {
        Thread.Sleep(10);
        spins = spins + 1;
    }
    if (!host.Served || !host.Server.IsConnected || !client.IsConnected) {
        Console.WriteLine("pipe-fail-handshake");
        Environment.Exit(3);
    }
    byte[] payload = [72, 101, 0, 108, 111, 0, 33];
    client.Write(payload, 0, 7);
    byte[] inbox = [0, 0, 0, 0, 0, 0, 0, 0];
    int n = host.Server.Read(inbox, 0, 8);
    if (n != 7) {
        Console.WriteLine("pipe-fail-read=" + n);
        Environment.Exit(4);
    }
    host.Server.Write(inbox, 0, n);
    byte[] back = [0, 0, 0, 0, 0, 0, 0, 0];
    int m = client.Read(back, 0, 8);
    if (m != 7) {
        Console.WriteLine("pipe-fail-echo=" + m);
        Environment.Exit(5);
    }
    for (int i = 0; i < 7; i++) {
        if (back[i] != payload[i]) {
            Console.WriteLine("pipe-fail-payload");
            Environment.Exit(6);
        }
    }
    client.Terminate();
    host.Server.Terminate();
    Console.WriteLine("pipe-ok-linux");
}
EOF

"$ARC" build "$PROBE/Program.as" --obj-dir "$PROBE/obj/Debug" -o "$PROBE/bin/Debug/pipe_posix_linux"
"$PROBE/bin/Debug/pipe_posix_linux"
echo "sync PASS exit=$?"

# ── M2 async roundtrip（io_uring 真 Reactor；对齐 Windows pipe_async_roundtrip）──
ASYNC_PROBE="${TMPDIR:-/tmp}/arc-e2e/pipe_posix_async_linux"
rm -rf "$ASYNC_PROBE"
mkdir -p "$ASYNC_PROBE/obj/Debug" "$ASYNC_PROBE/bin/Debug"
printf '%s\n' \
  '[package]' \
  'name = "pipe_posix_async_linux"' \
  'edition = "1"' \
  '' \
  '[dependencies]' \
  "\"Arc.Net.Pipes\" = { path = \"${REPO}/std/Net/Pipes\" }" \
  > "$ASYNC_PROBE/arc.toml"
cat > "$ASYNC_PROBE/Program.as" <<'EOF'
using Arc;
using Arc.IO;
using Arc.Net.Pipes;
using Arc.Threading;

async Task<void> Main() {
    NamedPipeServerStream server = new NamedPipeServerStream("arc.wsl.pipe.async");
    NamedPipeClientStream client = new NamedPipeClientStream("arc.wsl.pipe.async");
    Task wait = server.WaitForConnectionAsync();
    if (!await client.ConnectAsync(5000)) {
        Console.WriteLine("pipe-async-fail-connect");
        Environment.Exit(12);
    }
    await wait;
    if (!server.IsConnected || !client.IsConnected) {
        Console.WriteLine("pipe-async-fail-handshake");
        Environment.Exit(13);
    }
    byte[] payload = [72, 0, 105, 33];
    await client.WriteAsync(payload, 0, 4);
    byte[] inbox = [0, 0, 0, 0, 0, 0, 0, 0];
    int n = await server.ReadAsync(inbox, 0, 8);
    if (n != 4) {
        Console.WriteLine("pipe-async-fail-read=" + n);
        Environment.Exit(14);
    }
    await server.WriteAsync(inbox, 0, n);
    byte[] back = [0, 0, 0, 0, 0, 0, 0, 0];
    int m = await client.ReadAsync(back, 0, 8);
    if (m != 4 || back[0] != 72 || back[1] != 0 || back[2] != 105 || back[3] != 33) {
        Console.WriteLine("pipe-async-fail-echo");
        Environment.Exit(15);
    }
    client.Terminate();
    server.Terminate();
    Console.WriteLine("pipe-async-ok-linux");
}
EOF

"$ARC" build "$ASYNC_PROBE/Program.as" --obj-dir "$ASYNC_PROBE/obj/Debug" -o "$ASYNC_PROBE/bin/Debug/pipe_posix_async_linux"
"$ASYNC_PROBE/bin/Debug/pipe_posix_async_linux"
echo "async PASS exit=$? log=$LOG"
