# run-concurrency-protocol.ps1 — RFC 015 并发性能协议（对照跑）
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority: docs/chapters/08-rfcs/015-concurrency.md（M5 性能协议）
#            docs/chapters/08-rfcs/013-async-model.md（M5 async 性能预算）
#            docs/chapters/08-rfcs/025-reliability-charter.md §1.3
#
# 四路：
#   1. Arc  concurrent_bench_e2e  —— 并发集合吞吐（Dict/Queue/Bag/Stack/Blocking，32 线程）
#   2. Arc  roofline_bench        —— work-stealing 延迟 / Task 分配 / Parallel.For 扩展性
#   3. .NET std-concurrent-dotnet-cmp —— ConcurrentDictionary/ConcurrentQueue/Parallel.For/Task
#   4. Rust rust-concurrent       —— std::thread 分块并行扩展性（std 无 task/并发集合 → 如实 N/A）
#
# 输出：各侧原始 OK 行 + 对比摘要。不硬失败于吞吐阈值（硬件相关）；硬门禁=正确性（各侧自带断言）。
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-concurrency-protocol.ps1

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Invoke-Raw([string]$label, [scriptblock]$sb) {
    Write-Host ''
    Write-Host "===== $label ====="
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & $sb 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    $out = ($raw | ForEach-Object { "$_" })
    if ($code -ne 0) { Write-Host "WARN: $label exited $code" }
    return $out
}

# ---- 1. Arc concurrent_bench_e2e ----
$arcCc = Invoke-Raw '1. Arc concurrent_bench_e2e (concurrent collections, 32-thread)' {
    cargo test -p arc-integration --test concurrent_bench_e2e -- --nocapture
}
$arcCc | Where-Object { $_ -match 'OK: bench_' } | ForEach-Object { Write-Host $_.Trim() }

# ---- 2. Arc roofline_bench ----
$arcRl = Invoke-Raw '2. Arc roofline_bench (work-stealing / alloc / Parallel.For scaling)' {
    cargo test -p arc-integration --test roofline_bench -- --nocapture
}
$arcRl | ForEach-Object { Write-Host $_ }

# ---- 3. .NET ----
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if ($dotnetCmd) {
    $netProj = Join-Path $Root 'scripts/bench/std-concurrent-dotnet-cmp\StdConcurrentDotnetCmp.csproj'
    $netOut = Invoke-Raw '3. .NET TPL / System.Collections.Concurrent' {
        dotnet run -c Release --project $netProj --no-launch-profile
    }
    $netOut | Where-Object { $_ -match 'OK: ' } | ForEach-Object { Write-Host $_.Trim() }
} else {
    Write-Host 'WARN: dotnet missing - .NET comparison left empty'
}

# ---- 4. Rust ----
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargoCmd) {
    $rustProj = Join-Path $Root 'scripts/bench/rust-concurrent'
    $rustTarget = Join-Path $rustProj 'target'
    $buildOut = Invoke-Raw '4a. Rust rust-concurrent build' {
        cargo build --release --manifest-path (Join-Path $rustProj 'Cargo.toml') --target-dir $rustTarget
    }
    if ($LASTEXITCODE -eq 0) {
        $rustExe = Join-Path $rustTarget 'release\arc_rust-concurrent.exe'
        $rustOut = Invoke-Raw '4b. Rust std::thread parallel scaling' { & $rustExe }
        $rustOut | Where-Object { $_ -match 'OK: ' } | ForEach-Object { Write-Host $_.Trim() }
    }
} else {
    Write-Host 'WARN: cargo missing - Rust comparison left empty'
}

Write-Host ''
Write-Host '===== summary ====='
Write-Host 'Policy: same-machine anchors; throughput is hardware-dependent (this host: 22 logical cores). No absolute claims.'
Write-Host 'RFC 013 M5 budget (Arc side, see roofline output): task_create_1m <3ms/1e6; work-stealing dispatch <30ns.'
Write-Host 'RFC 015 comparison scope: Rust std has no work-stealing/concurrent collections -> only std::thread parallel scaling is comparable; rayon/crossbeam need external deps (not pulled in here).'
Write-Host 'Done.'
exit 0
