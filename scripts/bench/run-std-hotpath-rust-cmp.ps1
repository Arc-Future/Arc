# run-std-hotpath-rust-cmp.ps1 — Arc vs Rust (std::collections) same-machine compare
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority: RFC 099 §2.3 / 08-rfcs V1-SPRINT 轨道 G
#            docs/chapters/03-compiler/03-standard-library.md §热路径
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-std-hotpath-rust-cmp.ps1
#
# Exit:
#   0 = compare produced
#   2 = missing toolchain (cargo/rustc) or arc e2e failed
#
# Does NOT claim industry leadership. Anchors only.

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Collect-Ok([string[]]$lines) {
    $m = @{}
    # 统计化输出（list/dict/hs/sb）：`OK: name iters=...` 后跟 `claim: min_per_op=X.XXns`
    $cur = $null
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+iters=\d+\s+ops=[\d.]+') {
            $cur = $Matches[1]
        } elseif ($cur -and $line -match 'claim:\s+min_per_op=([\d.]+)ns') {
            $m[$cur] = @{ NsPerOp = [double]$Matches[1]; Ops = 0.0 }
            $cur = $null
        } elseif ($line -match 'OK:\s+(\S+)\s+ops=([\d.]+)\s+ns_total=([\d.]+)\s+ns_per_op=([\d.]+)') {
            $m[$Matches[1]] = @{ NsPerOp = [double]$Matches[4]; Ops = [double]$Matches[2] }
        }
    }
    return $m
}

Write-Host '===== 1. Arc std_hotpath_bench_e2e (C ABI / -O2) ====='
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$arcRaw = & cargo test -p arc-integration --test std_hotpath_bench_e2e -- --nocapture 2>&1
$arcExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
$arcOut = ($arcRaw | ForEach-Object { "$_" }) -join "`n"
Write-Host $arcOut
if ($arcExit -ne 0) { throw "Arc hotpath e2e failed (exit $arcExit)" }
$arcMap = Collect-Ok ($arcOut -split "`n")

$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    Write-Host ''
    Write-Host '===== 2. Rust ====='
    Write-Host 'Missing toolchain: `cargo` not found on PATH. Install Rust and re-run.'
    Write-Host 'Harness: scripts/bench/rust-hotpath/  (cargo build --release)'
    exit 2
}

Write-Host ''
Write-Host '===== 2. Rust --release (same N / ops counting) ====='
$rustProj = Join-Path $Root 'scripts/bench/rust-hotpath'
$ErrorActionPreference = 'Continue'
$buildRaw = & cargo build --release --manifest-path (Join-Path $rustProj 'Cargo.toml') 2>&1
$buildExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
if ($buildExit -ne 0) {
    Write-Host ($buildRaw | ForEach-Object { "$_" }) -join "`n"
    throw "Rust harness build failed (exit $buildExit)"
}
$rustExe = if ($IsWindows -or $env:OS -match 'Windows') {
    Join-Path $rustProj 'target\release\arc_rust-hotpath.exe'
} else {
    Join-Path $rustProj 'target/release/arc_rust-hotpath'
}
$ErrorActionPreference = 'Continue'
$rustRaw = & $rustExe 2>&1
$rustExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
$rustOut = ($rustRaw | ForEach-Object { "$_" }) -join "`n"
Write-Host $rustOut
if ($rustExit -ne 0) { throw "Rust harness failed (exit $rustExit)" }
$rustMap = Collect-Ok ($rustOut -split "`n")

$names = @('list_add_get', 'dict_set_get', 'hashset_add_contains', 'stringbuilder_append', 'file_io_throughput')
Write-Host ''
Write-Host '===== 3. Arc vs Rust (same machine) ====='
Write-Host '| scenario | Arc ns/op | Rust ns/op | Arc/Rust | note |'
Write-Host '|----------|-----------|------------|----------|------|'
foreach ($n in $names) {
    $a = $arcMap[$n]
    $b = $rustMap[$n]
    if ($null -eq $a -or $null -eq $b) {
        Write-Host ("| {0} | (missing) | (missing) | - | parse failed |" -f $n)
        continue
    }
    $ratio = if ($b.NsPerOp -gt 0) { $a.NsPerOp / $b.NsPerOp } else { 0 }
    $note = if ($ratio -gt 1.05) { 'Arc slower' } elseif ($ratio -lt 0.95) { 'Arc faster' } else { '~parity' }
    Write-Host ("| {0} | {1:0.00} | {2:0.00} | {3:0.00}x | {4} |" -f $n, $a.NsPerOp, $b.NsPerOp, $ratio, $note)
}
Write-Host ''
Write-Host 'Policy: same-machine microbench anchors only. Do NOT claim industry leadership.'
exit 0
