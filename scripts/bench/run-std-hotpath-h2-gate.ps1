# run-std-hotpath-h2-gate.ps1 — H2 crush-band gate (paired · fixed n · median · spike filter)
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority: docs/chapters/08-rfcs/024-maturity-perf.md §1.2（热路径对照协议）
#            docs/chapters/08-rfcs/025-reliability-charter.md §1.3（场景扩展）
#            docs/chapters/03-compiler/03-standard-library.md §热路径基准
#
# 场景（默认全量）：
#   list_add_get / dict_set_get / hashset_add_contains（三硬 H2 原三口）
#   stringbuilder_append / task_spawn_wait / file_io_throughput / concurrent_dict_1t
#   （025 §1.3 场景扩展：StringBuilder · async 任务 · IO 吞吐 · 并发集合）
# 基线：最新 .NET（net10.0）对全部场景；Rust（std::collections）对
#   list/dict/hs/stringbuilder/file_io（std 无 async 任务/并发集合，如实留空）。
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-std-hotpath-h2-gate.ps1
#   powershell ... -File .\scripts\bench\run-std-hotpath-h2-gate.ps1 -Passes 11 -SpikeFactor 2.0 -MinKept 7
#   powershell ... -File .\scripts\bench\run-std-hotpath-h2-gate.ps1 -Scenarios list_add_get,dict_set_get,hashset_add_contains
#   powershell ... -File .\scripts\bench\run-std-hotpath-h2-gate.ps1 -DirectClang
#
# Exit:
#   0 = window valid AND gate medians (spike-filtered) all <= 0.85 for every scenario×baseline
#   1 = window valid but gate median misses crush band (or unfiltered sanity miss)
#   2 = window invalid (too many spikes / parse fail / missing SDK / arc fail)
#   3 = suggests NOT checking H2 even if gate<=0.85 (unstable: unfiltered > 0.85)
#   4 = protocol aborted by runtime hang (watchdog; task_spawn_wait intermittent —
#       see report; belongs to RFC 099/102 reliability sprint)
#
# Does NOT claim industry leadership. Does NOT auto-check H2 checkbox.

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$Crush = 0.85,
    [string]$Scenarios = 'list_add_get,dict_set_get,hashset_add_contains,stringbuilder_append,task_spawn_wait,file_io_throughput,concurrent_dict_1t',
    [switch]$DirectClang,   # reuse target/e2e/cbench_*.exe if present (faster; still -O2)
    [switch]$SkipRust,      # skip the Rust baseline (e.g. cargo unavailable)
    [int]$ExeTimeoutSec = 90  # 每 exe 看门狗（挂起检测：> 超时 → kill + 终止协议）
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

# 场景 → Arc DirectClang exe 名（与 std_hotpath_bench_e2e.rs e2e_output 一致）
$ArcExe = @{
    'list_add_get'          = 'cbench_list_add_get'
    'dict_set_get'          = 'cbench_dict_set_get'
    'hashset_add_contains'  = 'cbench_hashset_add_contains'
    'stringbuilder_append'  = 'cbench_sb_append'
    'task_spawn_wait'       = 'cbench_task_spawn_wait'
    'file_io_throughput'    = 'cbench_file_io_throughput'
    'concurrent_dict_1t'    = 'cbench_concurrent_dict_1t'
}
# Rust（std::collections）覆盖场景；std 无 async 任务/并发集合 → 如实留空
$RustCovered = @('list_add_get','dict_set_get','hashset_add_contains','stringbuilder_append','file_io_throughput')

$scenarioList = @($Scenarios -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' })
foreach ($s in $scenarioList) {
    if (-not $ArcExe.ContainsKey($s)) {
        Write-Host "unknown scenario: $s (known: $($ArcExe.Keys -join ', '))"
        exit 2
    }
}

function Parse-Ok([string[]]$lines) {
    $m = @{}
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

function Median([double[]]$a) {
    if (-not $a -or $a.Count -eq 0) { return [double]::NaN }
    $s = @($a | Sort-Object { [double]$_ })
    return [double]$s[[int][Math]::Floor(($s.Count - 1) / 2.0)]
}

# 带看门狗的执行：Start-Process + WaitForExit(超时)；超时 → kill + 抛错终止协议。
# 背景：task_spawn_wait 偶发挂起（2026-08-03 实测 ~5% @50k；root cause 见报告，
# 归属 RFC 099/102 底座可靠性 Sprint）。不静默重试——挂起即如实终止并退出 4。
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arcbench_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcbench_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
    $p = Start-Process -FilePath $exe -NoNewWindow -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $p.WaitForExit($timeoutSec * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
        throw "HANG: $exe exceeded ${timeoutSec}s (label=$label) — runtime reliability finding (see report); killed."
    }
    $lines = @(Get-Content $outFile -ErrorAction SilentlyContinue)
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    return $lines
}

function Run-ArcOnce {
    if ($DirectClang) {
        $out = Join-Path $Root 'target\e2e'
        $lines = @()
        foreach ($s in $scenarioList) {
            $exe = Join-Path $out "$($ArcExe[$s]).exe"
            if (-not (Test-Path $exe)) {
                throw "DirectClang requested but missing $exe — run cargo test -p arc-integration --test std_hotpath_bench_e2e once first"
            }
            $lines += Invoke-ExeWatchdog $exe $ExeTimeoutSec $s
        }
        return (Parse-Ok $lines)
    }
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & cargo test -p arc-integration --test std_hotpath_bench_e2e -- --nocapture 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { throw "Arc hotpath e2e failed (exit $code)" }
    return (Parse-Ok ($raw | ForEach-Object { "$_" }))
}

function Run-NetOnce {
    $dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
    if (-not $dotnetCmd) { throw "dotnet SDK missing" }
    $proj = Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & dotnet run -c Release --project $proj --no-launch-profile 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { throw ".NET hotpath failed (exit $code)" }
    return (Parse-Ok ($raw | ForEach-Object { "$_" }))
}

function Run-RustOnce {
    $cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargoCmd) { throw "cargo missing" }
    $rustProj = Join-Path $Root 'scripts/bench/rust-hotpath'
    $rustTarget = Join-Path $rustProj 'target'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    # --target-dir 强制本地 target（避免继承外部 CARGO_TARGET_DIR）
    $buildRaw = & cargo build --release --manifest-path (Join-Path $rustProj 'Cargo.toml') --target-dir $rustTarget 2>&1
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($buildExit -ne 0) { throw "Rust harness build failed (exit $buildExit)" }
    $rustExe = Join-Path $rustTarget 'release\arc_rust-hotpath.exe'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $rustRaw = & $rustExe 2>&1
    $rustExit = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($rustExit -ne 0) { throw "Rust harness failed (exit $rustExit)" }
    return (Parse-Ok ($rustRaw | ForEach-Object { "$_" }))
}

Write-Host '===== H2 gate env ====='
Write-Host ("date: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("os: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
try {
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    Write-Host ("cpu: {0}" -f $cpu)
} catch { Write-Host 'cpu: (unavailable)' }
Write-Host ("protocol: paired n={0}; spike=side>={1}x median(ns); minKept={2}; crush<={3}" -f $Passes, $SpikeFactor, $MinKept, $Crush)
Write-Host ("mode: {0}" -f $(if ($DirectClang) { 'direct clang exe' } else { 'cargo test e2e' }))
Write-Host ("scenarios: {0}" -f ($scenarioList -join ', '))
Write-Host ("baselines: NET (all) + Rust ({0}){1}" -f ($RustCovered -join ','), $(if ($SkipRust) { ' [SKIPPED]' } else { '' }))

# 预构建 Rust（一次性；--target-dir 本地），并确保 DirectClang exes 已存在
if (-not $SkipRust) {
    $null = Run-RustOnce
}
if ($DirectClang) {
    $out = Join-Path $Root 'target\e2e'
    foreach ($s in $scenarioList) {
        $exe = Join-Path $out "$($ArcExe[$s]).exe"
        if (-not (Test-Path $exe)) {
            throw "DirectClang requested but missing $exe — run cargo test -p arc-integration --test std_hotpath_bench_e2e once first"
        }
    }
}

Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
[void](Run-ArcOnce)
[void](Run-NetOnce)

# 每个 scenario 存：arc / net / rust 三路样本 + 相对两基线的 r_i
$arc = @{}; $net = @{}; $rust = @{}; $ratNet = @{}; $ratRust = @{}
foreach ($s in $scenarioList) {
    $arc[$s] = New-Object System.Collections.Generic.List[double]
    $net[$s] = New-Object System.Collections.Generic.List[double]
    $rust[$s] = New-Object System.Collections.Generic.List[double]
    $ratNet[$s] = New-Object System.Collections.Generic.List[double]
    $ratRust[$s] = New-Object System.Collections.Generic.List[double]
}

for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $a = Run-ArcOnce
        $b = Run-NetOnce
        $r = if ($SkipRust) { $null } else { Run-RustOnce }
    } catch {
        # HANG（或 SDK 缺失）：如实终止；exit 4 = 协议被运行时挂起中断，不可签
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host 'H2 window: INVALID (runtime hang — do not sign; see report for reliability finding).'
        exit 4
    }
    foreach ($s in $scenarioList) {
        if (-not $a.ContainsKey($s) -or -not $b.ContainsKey($s)) {
            throw "missing scenario $s on pass $i"
        }
        $rn = $a[$s] / $b[$s]
        [void]$arc[$s].Add($a[$s])
        [void]$net[$s].Add($b[$s])
        [void]$ratNet[$s].Add($rn)
        Write-Host ("{0}: Arc={1:0.00} NET={2:0.00} r_net={3:0.00}x" -f $s, $a[$s], $b[$s], $rn)
        if (-not $SkipRust -and $RustCovered -contains $s) {
            if (-not $r.ContainsKey($s)) {
                throw "missing rust scenario $s on pass $i"
            }
            $rr = $a[$s] / $r[$s]
            [void]$rust[$s].Add($r[$s])
            [void]$ratRust[$s].Add($rr)
            Write-Host ("{0}: Rust={1:0.00} r_rust={2:0.00}x" -f $s, $r[$s], $rr)
        }
    }
}

Write-Host ''
Write-Host '===== protocol result ====='
Write-Host '| scenario | baseline | Arc med | base med | ratio raw med | kept | ratio gate med | spikes | crush? |'
Write-Host '|----------|----------|---------|----------|---------------|------|----------------|--------|--------|'

$windowOk = $true
$crushOk = $true
$stableOk = $true

foreach ($s in $scenarioList) {
    $arcArr = $arc[$s].ToArray()
    $netArr = $net[$s].ToArray()
    $ratNetArr = $ratNet[$s].ToArray()
    $mA = Median $arcArr
    $mN = Median $netArr
    $rawNetMed = Median $ratNetArr

    $keptR = New-Object System.Collections.Generic.List[double]
    $spikeIdx = New-Object System.Collections.Generic.List[int]
    for ($j = 0; $j -lt $Passes; $j++) {
        $spike = ($arcArr[$j] -gt ($SpikeFactor * $mA)) -or ($netArr[$j] -gt ($SpikeFactor * $mN))
        if ($spike) { [void]$spikeIdx.Add($j + 1) } else { [void]$keptR.Add($ratNetArr[$j]) }
    }
    $kN = $keptR.Count
    if ($kN -lt $MinKept) { $windowOk = $false }
    $gateNetMed = Median $keptR.ToArray()
    $passNet = ($gateNetMed -le $Crush)
    if (-not $passNet) { $crushOk = $false }
    if ($rawNetMed -gt $Crush) { $stableOk = $false }

    $spikeStr = if ($spikeIdx.Count -eq 0) { '-' } else { ($spikeIdx -join ',') }
    $markN = if (-not ($kN -ge $MinKept)) { 'INVALID' } elseif ($passNet) { 'PASS' } else { 'FAIL' }
    Write-Host ("| {0} | NET | {1:0.00} | {2:0.00} | {3:0.00}x | {4}/{5} | {6:0.00}x | {7} | {8} |" -f `
        $s, $mA, $mN, $rawNetMed, $kN, $Passes, $gateNetMed, $spikeStr, $markN)

    if (-not $SkipRust -and $RustCovered -contains $s) {
        $rustArr = $rust[$s].ToArray()
        $ratRustArr = $ratRust[$s].ToArray()
        $mR = Median $rustArr
        $rawRustMed = Median $ratRustArr

        $keptRR = New-Object System.Collections.Generic.List[double]
        $spikeIdxR = New-Object System.Collections.Generic.List[int]
        for ($j = 0; $j -lt $Passes; $j++) {
            $spike = ($arcArr[$j] -gt ($SpikeFactor * $mA)) -or ($rustArr[$j] -gt ($SpikeFactor * $mR))
            if ($spike) { [void]$spikeIdxR.Add($j + 1) } else { [void]$keptRR.Add($ratRustArr[$j]) }
        }
        $kR = $keptRR.Count
        if ($kR -lt $MinKept) { $windowOk = $false }
        $gateRustMed = Median $keptRR.ToArray()
        $passRust = ($gateRustMed -le $Crush)
        if (-not $passRust) { $crushOk = $false }
        if ($rawRustMed -gt $Crush) { $stableOk = $false }

        $spikeStrR = if ($spikeIdxR.Count -eq 0) { '-' } else { ($spikeIdxR -join ',') }
        $markR = if (-not ($kR -ge $MinKept)) { 'INVALID' } elseif ($passRust) { 'PASS' } else { 'FAIL' }
        Write-Host ("| {0} | Rust | {1:0.00} | {2:0.00} | {3:0.00}x | {4}/{5} | {6:0.00}x | {7} | {8} |" -f `
            $s, $mA, $mR, $rawRustMed, $kR, $Passes, $gateRustMed, $spikeStrR, $markR)
    }
}

Write-Host ''
Write-Host 'Policy: microbench anchors only. Do NOT claim industry leadership.'
if (-not $windowOk) {
    Write-Host 'H2 window: INVALID (too many spikes — re-run; do not sign).'
    exit 2
}
if (-not $crushOk) {
    Write-Host 'H2 gate: MISS crush band (spike-filtered median). Keep H2 unchecked.'
    exit 1
}
if (-not $stableOk) {
    Write-Host 'H2 gate: filtered PASS but unfiltered median > 0.85 — still noisy. Keep H2 unchecked (exit 3).'
    exit 3
}
Write-Host 'H2 gate: STABLE PASS (filtered + unfiltered medians all <= 0.85 across every scenario). May suggest checking H2.'
exit 0
