# run-std-hotpath-dotnet-cmp.ps1 — G8 same-machine Arc (C ABI e2e) vs .NET hotpath compare
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-std-hotpath-dotnet-cmp.ps1
#
# Requires: clang (for Arc e2e), cargo; optionally `dotnet` (SDK).
# If `dotnet` is missing: prints missing-SDK note, still runs Arc side, exits 2 (G8 compare stays open until .NET runs).
#
# Prints a Markdown-friendly comparison table. Does NOT claim industry leadership.

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Parse-OkLine([string]$line) {
    # 统计化输出（list/dict/hs/sb）：`OK: name iters=...` 后跟 `claim: min_per_op=X.XXns`
    if ($line -match 'OK:\s+(\S+)\s+iters=\d+\s+ops=[\d.]+') {
        return [pscustomobject]@{ StatName = $Matches[1]; StatOnly = $true; NsPerOp = 0.0 }
    }
    if ($line -match 'claim:\s+min_per_op=([\d.]+)ns') {
        return [pscustomobject]@{ StatName = $null; StatOnly = $true; NsPerOp = [double]$Matches[1] }
    }
    # OK: name ops=... ns_total=... ns_per_op=... ops_per_s=...
    if ($line -notmatch 'OK:\s+(\S+)\s+ops=([\d.]+)\s+ns_total=([\d.]+)\s+ns_per_op=([\d.]+)\s+ops_per_s=([\d.]+)') {
        return $null
    }
    return [pscustomobject]@{
        Name    = $Matches[1]
        Ops     = [double]$Matches[2]
        NsTotal = [double]$Matches[3]
        NsPerOp = [double]$Matches[4]
        OpsPerS = [double]$Matches[5]
    }
}

function Collect-Ok([string[]]$lines) {
    $map = @{}
    $statPending = $null
    foreach ($line in $lines) {
        $p = Parse-OkLine $line
        if ($null -eq $p) { continue }
        if ($p.StatOnly) {
            if ($null -ne $p.StatName) { $statPending = $p.StatName }
            elseif ($null -ne $statPending) {
                $map[$statPending] = [pscustomobject]@{ Name = $statPending; Ops = 0.0; NsTotal = 0.0; NsPerOp = $p.NsPerOp; OpsPerS = 0.0 }
                $statPending = $null
            }
        } else {
            $map[$p.Name] = $p
        }
    }
    return $map
}

Write-Host '===== env ====='
Write-Host ("date (local): {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("os: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
Write-Host ("arch: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)
try {
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    Write-Host ("cpu: {0}" -f $cpu)
} catch {
    Write-Host 'cpu: (unavailable)'
}

Write-Host ''
Write-Host '===== 1. Arc std_hotpath_bench_e2e (C ABI / -O2) ====='
# cargo writes progress to stderr; do not treat as terminating under $ErrorActionPreference=Stop
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$arcRaw = & cargo test -p arc-integration --test std_hotpath_bench_e2e -- --nocapture 2>&1
$arcExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
$arcOut = ($arcRaw | ForEach-Object { "$_" }) -join "`n"
Write-Host $arcOut
if ($arcExit -ne 0) { throw "Arc hotpath e2e failed (exit $arcExit)" }
$arcMap = Collect-Ok ($arcOut -split "`n")

$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) {
    Write-Host ''
    Write-Host '===== 2. .NET ====='
    Write-Host 'Missing SDK: `dotnet` not found on PATH. Install .NET 8+ SDK and re-run.'
    Write-Host 'Harness: scripts/bench/std-hotpath-dotnet-cmp/  (dotnet run -c Release --project ...)'
    Write-Host 'G8 compare remains open (Arc-only anchor).'
    exit 2
}

Write-Host ''
Write-Host '===== 2. .NET Release (same N / ops counting) ====='
$ErrorActionPreference = 'Continue'
& dotnet --info 2>&1 | Select-Object -First 20 | ForEach-Object { Write-Host $_ }
$proj = Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj'
$dotnetRaw = & dotnet run -c Release --project $proj --no-launch-profile 2>&1
$dotnetExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap
$dotnetOut = ($dotnetRaw | ForEach-Object { "$_" }) -join "`n"
Write-Host $dotnetOut
if ($dotnetExit -ne 0) { throw ".NET hotpath compare failed (exit $dotnetExit)" }
$netMap = Collect-Ok ($dotnetOut -split "`n")

$names = @('list_add_get', 'dict_set_get', 'stringbuilder_append', 'hashset_add_contains')
Write-Host ''
Write-Host '===== 3. Arc vs .NET (same machine) ====='
Write-Host '| scenario | Arc ns/op | .NET ns/op | Arc/NET | note |'
Write-Host '|----------|-----------|------------|---------|------|'
foreach ($n in $names) {
    $a = $arcMap[$n]
    $b = $netMap[$n]
    if ($null -eq $a -or $null -eq $b) {
        Write-Host ("| {0} | (missing) | (missing) | - | parse failed |" -f $n)
        continue
    }
    $ratio = if ($b.NsPerOp -gt 0) { $a.NsPerOp / $b.NsPerOp } else { 0 }
    $note = if ($ratio -gt 1.05) { 'Arc slower' } elseif ($ratio -lt 0.95) { 'Arc faster' } else { '~parity' }
    Write-Host ("| {0} | {1:0.00} | {2:0.00} | {3:0.00}x | {4} |" -f $n, $a.NsPerOp, $b.NsPerOp, $ratio, $note)
}

Write-Host ''
Write-Host 'Policy: microbench anchors only. Do NOT claim industry leadership.'
Write-Host 'Required G8 trio: list_add_get / dict_set_get / stringbuilder_append.'
exit 0
