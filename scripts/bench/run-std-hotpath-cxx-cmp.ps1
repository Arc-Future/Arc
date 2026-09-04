# run-std-hotpath-cxx-cmp.ps1 - Arc vs C++ (std containers) same-machine compare
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority: RFC 099 / 08-rfcs V1-SPRINT track G
#            docs/chapters/03-compiler/03-standard-library.md
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-std-hotpath-cxx-cmp.ps1
#
# Isomorphic to run-std-hotpath-rust-cmp.ps1; third anchor = C++ (std::vector /
# std::unordered_map / std::unordered_set / std::string).
# Requires clang++ (Arc LLVM 22 toolchain, default C:\Program Files\LLVM\bin\clang++.exe).
#
# Measurement protocol (RFC 024 / RFC 013):
#   - each side does 30 in-bench iterations and takes the min (noise only adds time)
#   - paired 11 runs per side, per-scenario spike-removal median (keep >= 7)
#   - no industry-leadership claim; same-machine reproducible anchor only
#
# Exit:
#   0 = compare produced
#   2 = missing toolchain (clang++ / arc e2e failed)
#
# NOTE: Keep this file ASCII-only (no CJK comments). PowerShell 5.1 reads
# BOM-less scripts in the ANSI code page, which corrupts UTF-8 CJK bytes and can
# break parsing. ASCII comments avoid any encoding dependency.

$ErrorActionPreference = 'Continue'
$prevEap = $ErrorActionPreference
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Collect-Ok([string[]]$lines) {
    $m = @{}
    # statistical output: `OK: name iters=..` then `claim: min_per_op=X.XXns`
    $cur = $null
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+iters=\d+\s+ops=[\d.]+') {
            $cur = $Matches[1]
        } elseif ($cur -and $line -match 'claim:\s+min_per_op=([\d.]+)ns') {
            $m[$cur] = [double]$Matches[1]
            $cur = $null
        } elseif ($line -match 'OK:\s+(\S+)\s+ops=([\d.]+)\s+ns_total=([\d.]+)\s+ns_per_op=([\d.]+)') {
            $m[$Matches[1]] = [double]$Matches[4]
        }
    }
    return $m
}

function Get-Median([double[]]$vals) {
    $s = @($vals | Sort-Object)
    if ($s.Count -eq 0) { return 0.0 }
    $mid = [int][Math]::Floor(($s.Count - 1) / 2.0)
    return [double]$s[$mid]
}

function Get-MedianNoSpike([double[]]$vals) {
    $s = @($vals | Sort-Object)
    if ($s.Count -eq 0) { return 0.0 }
    $med = Get-Median $s
    $kept = @($s | Where-Object { $_ -le 2.0 * $med })
    if ($kept.Count -lt 7) { $kept = $s }
    return Get-Median $kept
}

# Locate clang++ (Arc LLVM 22 toolchain).
$Clang = $env:CLANGPP
if (-not $Clang) {
    $cand = @(
        'C:\Program Files\LLVM\bin\clang++.exe',
        "$env:USERPROFILE\scoop\apps\llvm\current\bin\clang++.exe"
    )
    $Clang = $cand | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Clang) {
    Write-Host 'Missing toolchain: clang++ not found. Set $env:CLANGPP or install LLVM.'
    exit 2
}

Write-Host '===== 1. Build C++ --release (-O2 -DNDEBUG) ====='
$srcDir = Join-Path $Root 'scripts/bench/cxx-hotpath'
$exe = Join-Path $srcDir 'cxx-hotpath.exe'
$ErrorActionPreference = 'Continue'
$buildRaw = & $Clang -O2 -DNDEBUG -o $exe (Join-Path $srcDir 'main.cpp') 2>&1
$buildExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
if ($buildExit -ne 0) {
    Write-Host ($buildRaw | ForEach-Object { "$_" }) -join "`n"
    throw 'C++ harness build failed'
}

# Warmup: first e2e triggers compile + cache warm, then paired measurement.
$ErrorActionPreference = 'Continue'
$warm = & cargo test -p arc-integration --test std_hotpath_bench_e2e -- --nocapture 2>&1
$ErrorActionPreference = $prevEap

# Run a native command with bounded retries (shared-machine transient failures:
# compile contention / occasional timeout). Only a sustained failure is real.
function Invoke-NativeRetry([scriptblock]$body, [int]$retries = 2) {
    for ($a = 0; $a -le $retries; $a++) {
        $ErrorActionPreference = 'Continue'
        $out = & $body
        $code = $LASTEXITCODE
        $ErrorActionPreference = $prevEap
        if ($code -eq 0) { return @{ Code = 0; Out = $out } }
        if ($a -lt $retries) { Write-Host ("    (retry {0}/{1} after transient exit {2})" -f ($a + 1), $retries, $code) }
    }
    return @{ Code = $code; Out = $out }
}

# Paired protocol (RFC 024): run nRun times per side, per-scenario spike-removal median.
$nRun = 11
Write-Host ''
Write-Host ("===== 2. Paired Arc vs C++ (n={0}, spike-removal median) =====" -f $nRun)
$arcSets = @{}
$cxxSets = @{}

foreach ($i in 1..$nRun) {
    $ar = Invoke-NativeRetry { cargo test -p arc-integration --test std_hotpath_bench_e2e -- --nocapture 2>&1 }
    $aout = $ar.Out
    $aexit = $ar.Code
    $am = Collect-Ok ((($aout | ForEach-Object { "$_" }) -join "`n") -split "`n")
    if ($aexit -ne 0) { throw 'Arc hotpath e2e failed' }
    foreach ($k in $am.Keys) {
        if (-not $arcSets.ContainsKey($k)) { $arcSets[$k] = @() }
        $arcSets[$k] += [double]$am[$k]
    }
    $cr = Invoke-NativeRetry { & $exe 2>&1 }
    $cout = $cr.Out
    $cexit = $cr.Code
    $cm = Collect-Ok ((($cout | ForEach-Object { "$_" }) -join "`n") -split "`n")
    if ($cexit -ne 0) { throw 'C++ harness failed' }
    foreach ($k in $cm.Keys) {
        if (-not $cxxSets.ContainsKey($k)) { $cxxSets[$k] = @() }
        $cxxSets[$k] += [double]$cm[$k]
    }
    Write-Host ("  run {0}/{1} done" -f $i, $nRun)
}

$names = @('list_add_get', 'dict_set_get', 'hashset_add_contains', 'stringbuilder_append', 'file_io_throughput')
Write-Host '| scenario | Arc ns/op | C++ ns/op | Arc/C++ | note |'
Write-Host '|----------|-----------|-----------|---------|------|'
foreach ($scenario in $names) {
    $a = $arcSets[$scenario]
    $b = $cxxSets[$scenario]
    if ($null -eq $a -or $null -eq $b -or $a.Count -eq 0 -or $b.Count -eq 0) {
        Write-Host ("| {0} | (missing) | (missing) | - | parse failed |" -f $scenario)
        continue
    }
    $amed = Get-MedianNoSpike $a
    $bmed = Get-MedianNoSpike $b
    $ratio = if ($bmed -gt 0) { $amed / $bmed } else { 0 }
    $note = if ($ratio -gt 1.05) { 'Arc slower' } elseif ($ratio -lt 0.95) { 'Arc faster' } else { '~parity' }
    Write-Host ("| {0} | {1:0.00} | {2:0.00} | {3:0.00}x | {4} |" -f $scenario, $amed, $bmed, $ratio, $note)
}
Write-Host ''
Write-Host ("Paired protocol: n={0} per side, spike-removal median (RFC 024)." -f $nRun)
Write-Host 'In-benchmark 30-iter min lower bound (RFC 013).'
Write-Host 'Policy: same-machine microbench anchors only. Do NOT claim industry leadership.'
exit 0