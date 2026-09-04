# run-ws-dispatch-exclusive-regate.ps1 - RFC 034 s1.1.2 exclusive-machine re-test
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
# (ws dispatch / spawn+drain latency <= 250ns, Arc-only ABSOLUTE gate)
#
# Authority:
#   docs/chapters/08-rfcs/013-async-model.md s1.5
#     (ws dispatch = spawn+drain scheduling overhead; RFC 034 new-caliber gate
#      <=250ns; shared-machine rehearsal ~308ns / quiet median ~239ns; formal
#      acceptance requires exclusive machine + protocol green)
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md s3.3 (protocol)
#   docs/chapters/08-rfcs/024-maturity-perf.md (H4 baseline upgrade record)
#
# DIFFERENCE from the paired regates (concurrent / task_spawn_wait): this gate is
# ABSOLUTE and Arc-only. "ws dispatch" has NO .NET counterpart (no Task.Run-style
# baseline); the criterion is the measured spawn+drain ns/op must be <= 250ns.
# Measurement: `spawn_drain_latency_bench` (roofline_bench.rs) -> spawns N work
# items on a single-worker pool and drains them; single-pass ns_per_op.
#
# Phase 1 - exclusive-machine detection (abort on busy, exit 3, no benchmark run):
#   1. concurrent bench/build processes matching (arc|cargo|clang|dotnet|cbench|bench|
#      rustc|roofline) present (excluding this script) -> busy
#   2. CPU load: sample \Processor(_Total)\% Processor Time N times (1s each); median
#      > CpuBusyPct -> busy
#   busy -> print exclusive-machine requirement, abort (exit 3).
#
# Phase 2 - absolute gate (after exclusive machine confirmed):
#   n = 11 runs of `spawn_drain_latency_bench.exe`; ns_per_op per run; spike = value
#   > 2x median removed; kept >= 7; gate = median of kept <= 250ns. Records window
#   tip SHA (099 s3.1 window discipline).
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-ws-dispatch-exclusive-regate.ps1
#   powershell ... -File .\scripts\bench\run-ws-dispatch-exclusive-regate.ps1 -CpuBusyPct 10 -Rebuild
#   powershell ... -File .\scripts\bench\run-ws-dispatch-exclusive-regate.ps1 -SimulateBusy   # self-test busy branch
#
# Exit:
#   0 = exclusive machine + window valid + median ns/op <= 250
#   1 = window valid but gate MISS (median > 250ns; keep unchecked)
#   2 = window invalid (too many spikes / build fail / missing exe)
#   3 = machine busy (shared/busy machine) - need exclusive machine; no benchmark run
#   4 = protocol interrupted by runtime hang (watchdog)

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$Gate = 250,             # ABSOLUTE ns: median ns/op <= 250 (no .NET baseline)
    [int]$CpuBusyPct = 15,           # median CPU busy threshold (exclusive machine far below)
    [int]$CpuSamples = 3,            # CPU sample count (1s each)
    [switch]$Rebuild,                # re-run roofline_bench spawn_drain_latency_bench to build exe
    [switch]$SimulateBusy,           # self-test: force busy abort branch
    [switch]$ForceProceed,           # self-test: skip exclusive check (shared numbers NOT acceptance)
    [int]$ExeTimeoutSec = 120
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
$E2eDir = Join-Path $Root 'target\e2e'

function Median([double[]]$a) {
    if (-not $a -or $a.Count -eq 0) { return [double]::NaN }
    $s = @($a | Sort-Object { [double]$_ })
    return [double]$s[[int][Math]::Floor(($s.Count - 1) / 2.0)]
}

# Parse spawn_drain_latency_bench output:
#   ok: spawn_drain_latency N=100000
#     total=12.838ms ns_per_op=128ns
#     PASS
# Returns ns_per_op as double (key 'ws_dispatch').
function Parse-Ws([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'ns_per_op=(\d+)ns') {
            $m['ws_dispatch'] = [double]$Matches[1]
        }
    }
    return $m
}

# Watchdog execution, returns stdout lines. Correctness keyed on stdout PASS line.
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arcwsd_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcwsd_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
    $p = Start-Process -FilePath $exe -NoNewWindow -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $p.WaitForExit($timeoutSec * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
        throw "HANG: $exe exceeded ${timeoutSec}s (label=$label) - killed; protocol aborted."
    }
    $lines = @(Get-Content $outFile -ErrorAction SilentlyContinue)
    $errs = @(Get-Content $errFile -ErrorAction SilentlyContinue)
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    if ($lines.Count -eq 0) {
        throw "EXE FAIL: $exe produced no stdout (label=$label) ERR: $($errs -join '; ')"
    }
    if (-not ($lines -match 'PASS')) {
        throw "EXE FAIL: $exe did not print PASS (label=$label) ERR: $($errs -join '; ')"
    }
    return $lines
}

# ---- Phase 1 - exclusive-machine detection ----
function Test-ExclusiveMachine {
    if ($SimulateBusy) {
        Write-Host 'SimulateBusy: forcing busy branch (self-test).'
        return $false
    }
    $busy = @()
    # 1. concurrent bench/build processes
    $pat = '^(arc|cargo|clang|dotnet|cbench|bench|rustc|roofline|arc-git-sync)'
    $procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.ProcessName -match $pat -and $_.Id -ne $PID
    })
    foreach ($p in $procs) {
        $busy += ("process {0} (pid {1})" -f $p.ProcessName, $p.Id)
    }
    # 2. CPU load
    $loads = New-Object System.Collections.Generic.List[double]
    try {
        for ($i = 0; $i -lt $CpuSamples; $i++) {
            $c = Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 1 -ErrorAction Stop
            foreach ($s in $c.CounterSamples) { [void]$loads.Add([double]$s.CookedValue) }
        }
        $avgLoad = Median ($loads.ToArray())
        Write-Host ("cpu load: {0:0.0}% busy (median of {1} samples; threshold {2}%)" -f `
            $avgLoad, $CpuSamples, $CpuBusyPct)
        if ($avgLoad -gt $CpuBusyPct) {
            $busy += ("cpu busy {0:0.0}% > {1}%" -f $avgLoad, $CpuBusyPct)
        }
    } catch {
        Write-Host ("WARN: cpu load sampling failed ({0}) - falling back to process detection only" -f $_.Exception.Message)
    }
    if ($busy.Count -gt 0) {
        Write-Host ''
        Write-Host '===== EXCLUSIVE-MACHINE CHECK: BUSY - ABORT ====='
        foreach ($b in $busy) { Write-Host ("  busy: {0}" -f $b) }
        Write-Host ''
        Write-Host 'This protocol requires an EXCLUSIVE benchmark machine (RFC 034 s1.1.2):'
        Write-Host '  - no other benchmark/build processes running'
        Write-Host '  - CPU load below threshold (idle machine; shared CI / dev box is disqualified)'
        Write-Host 'Move to a dedicated machine and re-run. No benchmark was executed.'
        return $false
    }
    Write-Host 'exclusive machine: CLEAN (no bench/build processes; CPU within threshold)'
    return $true
}

Write-Host '===== ws dispatch exclusive regate (spawn+drain, Arc-only absolute <=250ns) ====='
Write-Host ("date: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("os: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
try {
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    Write-Host ("cpu: {0}" -f $cpu)
} catch { Write-Host 'cpu: (unavailable)' }
try {
    $tip = (git rev-parse HEAD 2>$null).Trim()
    Write-Host ("window tip SHA: {0}" -f $tip)
} catch { Write-Host 'window tip SHA: (unavailable)' }
Write-Host ("protocol: exclusive machine + n={0}; spike=value>={1}x median; minKept={2}; gate<= {3}ns (absolute, Arc-only)" -f `
    $Passes, $SpikeFactor, $MinKept, $Gate)
Write-Host ("scenario: ws dispatch (spawn_drain_latency_bench - Arc-only, no .NET baseline)")

# ---- Phase 1 - exclusive-machine detection (before running any benchmark) ----
if ($ForceProceed) {
    Write-Host 'ForceProceed: SKIPPING exclusive-machine check (self-test only - numbers NOT acceptance).'
} elseif (-not (Test-ExclusiveMachine)) {
    exit 3
}
Write-Host ''

# ---- Arc side exe preparation ----
$arcExe = 'spawn_drain_latency_bench.exe'
if ($Rebuild) {
    Write-Host 'rebuilding ws-dispatch exe via roofline_bench (spawn_drain_latency_bench) ...'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & cargo test -p arc-integration --test roofline_bench spawn_drain_latency_bench 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { Write-Host 'roofline_bench spawn_drain build failed; exiting.'; exit 2 }
}
if (-not (Test-Path (Join-Path $E2eDir $arcExe))) {
    Write-Host "missing $arcExe - re-run with -Rebuild (or run roofline_bench spawn_drain_latency_bench once)."
    exit 2
}

# ---- warmup (untimed; not scored) ----
Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
$null = Invoke-ExeWatchdog (Join-Path $E2eDir $arcExe) $ExeTimeoutSec 'warmup'

# ---- n=11 ----
$vals = New-Object System.Collections.Generic.List[double]
for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $a = Parse-Ws (Invoke-ExeWatchdog (Join-Path $E2eDir $arcExe) $ExeTimeoutSec ("p{0}" -f $i))
        if (-not $a.ContainsKey('ws_dispatch')) { throw "missing arc ws_dispatch on pass $i" }
        $v = $a['ws_dispatch']
        $vals.Add($v)
        Write-Host ("  ws_dispatch: {0:0} ns/op (gate<= {1}ns)" -f $v, $Gate)
    } catch {
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host 'ws dispatch regate window: INVALID (runtime hang or harness fail - do not treat as pass).'
        exit 4
    }
}

Write-Host ''
Write-Host '===== protocol result ====='
$arr = $vals.ToArray()
$m = Median $arr
$rawMed = $m
$kept = New-Object System.Collections.Generic.List[double]
$spikeIdx = New-Object System.Collections.Generic.List[int]
for ($j = 0; $j -lt $Passes; $j++) {
    if ($arr[$j] -gt ($SpikeFactor * $m)) { [void]$spikeIdx.Add($j + 1) } else { [void]$kept.Add($arr[$j]) }
}
$k = $kept.Count
$windowOk = ($k -ge $MinKept)
$gateMed = Median $kept.ToArray()
$pass = ($gateMed -le $Gate)
$gateOk = $pass
$spikeStr = if ($spikeIdx.Count -eq 0) { '-' } else { ($spikeIdx -join ',') }
$mark = if (-not $windowOk) { 'INVALID' } elseif ($pass) { 'PASS' } else { 'FAIL' }
Write-Host ("| ws_dispatch | raw med {0:0} ns | kept {1}/{2} | gate med {3:0} ns | spikes {4} | gate<= {5}ns | {6} |" -f `
    $rawMed, $k, $Passes, $gateMed, $spikeStr, $Gate, $mark)
Write-Host ("  values: {0}" -f (($arr | ForEach-Object { '{0:0}' -f $_ }) -join ', '))

Write-Host ''
Write-Host 'Policy: exclusive-machine ABSOLUTE bound (ws dispatch median <= 250ns). No industry leadership claims.'
Write-Host 'RFC 099 s2.1: acceptance requires exclusive machine + protocol green + maintainer announcement.'
if (-not $windowOk) {
    Write-Host 'ws dispatch regate window: INVALID (too many spikes - re-run on exclusive machine; do not sign).'
    exit 2
}
if (-not $gateOk) {
    Write-Host 'ws dispatch regate gate: MISS (median > 250ns; keep unchecked).'
    exit 1
}
Write-Host 'ws dispatch regate gate: PASS (median ns/op <= 250 on exclusive machine).'
exit 0
