# run-syntax-perf-cmp.ps1 - Arc vs .NET / Rust basic-syntax perf same-machine compare
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Scenarios (mirror Arc syntax_perf_bench_e2e.rs; same N / ops / workload):
#   loop_sum             pure arithmetic loop (N=5e7)  - codegen loop throughput
#   string_replace_long  1MB text sparse-token replace (20x) - long-text processing
#   file_concurrency     8 threads x 50 write+read 64KB - concurrent file ops
#
# Arc side measured via clang -O2 C ABI (Arc LLVM backend codegen proxy).
# Outputs OK: lines parsed into a same-machine comparison table.
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-syntax-perf-cmp.ps1
#
# Exit: 0 = compare produced; 2 = Arc side failed or no reference succeeded.
# Policy: same-machine anchors only. No industry-leadership claim.

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Parse-Ok([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+ops=[\d.]+\s+ns_total=[\d.]+\s+ns_per_op=([\d.]+)\s+ops_per_s=([\d.]+)') {
            $m[$Matches[1]] = [pscustomobject]@{ NsPerOp = [double]$Matches[2]; ReqS = [double]$Matches[3] }
        }
    }
    return $m
}

Write-Host '===== env ====='
Write-Host ("date (local): {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("os: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
try {
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    Write-Host ("cpu: {0}" -f $cpu)
} catch { Write-Host 'cpu: (unavailable)' }
Write-Host ("logical cores: {0}" -f [Environment]::ProcessorCount)
Write-Host ''
Write-Host 'scenarios: loop_sum / string_replace_long / file_concurrency'

$results = @{}   # scenario -> { lang -> NsPerOp }
$scenarios = @('loop_sum', 'string_replace_long', 'file_concurrency')

# ---- 1. Arc ----
Write-Host ''
Write-Host '===== 1. Arc (clang -O2 C ABI) ====='
$prev = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$arcRaw = & cargo test -p arc-integration --test syntax_perf_bench_e2e -- --nocapture 2>&1
$arcExit = $LASTEXITCODE
$ErrorActionPreference = $prev
$arcOut = ($arcRaw | ForEach-Object { "$_" }) -join "`n"
Write-Host $arcOut
if ($arcExit -ne 0) { throw "Arc syntax_perf e2e failed (exit $arcExit)" }
$arcMap = Parse-Ok ($arcOut -split "`n")
foreach ($sc in $scenarios) { $results[$sc] = @{ 'Arc' = $arcMap[$sc].NsPerOp } }

# ---- 2. .NET ----
Write-Host ''
Write-Host '===== 2. .NET Release ====='
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) {
    Write-Host 'Missing SDK: dotnet not found.'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $proj = Join-Path $Root 'scripts/bench/syntax-perf-dotnet-cmp\SyntaxPerfDotnetCmp.csproj'
    $netOut = (& dotnet run -c Release --project $proj --no-launch-profile 2>&1) | ForEach-Object { "$_" }
    $ErrorActionPreference = $prev
    $netOut | ForEach-Object { Write-Host $_ }
    $netMap = Parse-Ok $netOut
    foreach ($sc in $scenarios) { if ($netMap[$sc]) { $results[$sc]['.NET'] = $netMap[$sc].NsPerOp } }
}

# ---- 3. Rust ----
Write-Host ''
Write-Host '===== 3. Rust --release ====='
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    Write-Host 'Missing toolchain: cargo not found.'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $rustProj = Join-Path $Root 'scripts/bench/syntax-perf-rust-cmp'
    $buildRaw = (& cargo build --release --manifest-path (Join-Path $rustProj 'Cargo.toml') 2>&1) | ForEach-Object { "$_" }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($buildExit -ne 0) {
        $buildRaw | ForEach-Object { Write-Host $_ }
        throw "Rust harness build failed (exit $buildExit)"
    }
    $rustExe = Join-Path $rustProj 'target\release\arc_syntax_perf_rust.exe'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $rustOut = (& $rustExe 2>&1) | ForEach-Object { "$_" }
    $ErrorActionPreference = $prev
    $rustOut | ForEach-Object { Write-Host $_ }
    $rustMap = Parse-Ok $rustOut
    foreach ($sc in $scenarios) { if ($rustMap[$sc]) { $results[$sc]['Rust'] = $rustMap[$sc].NsPerOp } }
}

# ---- 4. summary ----
Write-Host ''
Write-Host '===== 4. Arc vs .NET / Rust (same machine, ns/op, lower=better) ====='
Write-Host '| scenario | Arc | .NET | Rust | Arc/NET | Arc/Rust |'
Write-Host '|----------|-----|------|------|---------|----------|'
foreach ($sc in $scenarios) {
    $a = $results[$sc]['Arc']
    $n = $results[$sc]['.NET']
    $r = $results[$sc]['Rust']
    $na = if ($null -eq $a) { 'missing' } else { ('{0:0.00}' -f $a) }
    $nn = if ($null -eq $n) { 'missing' } else { ('{0:0.00}' -f $n) }
    $nr = if ($null -eq $r) { 'missing' } else { ('{0:0.00}' -f $r) }
    $rn = if ($null -eq $a -or $null -eq $n -or $n -eq 0) { '-' } else { ('{0:0.00}x' -f ($a / $n)) }
    $rr = if ($null -eq $a -or $null -eq $r -or $r -eq 0) { '-' } else { ('{0:0.00}x' -f ($a / $r)) }
    Write-Host ("| {0} | {1} | {2} | {3} | {4} | {5} |" -f $sc, $na, $nn, $nr, $rn, $rr)
}
Write-Host ''
Write-Host 'Policy: same-machine Windows anchors only. No industry-leadership claim.'
exit 0
