# run-task-spawn-wait-exclusive-regate.ps1 - RFC 034 s1.1.2 exclusive-machine re-test (task_spawn_wait vs .NET)
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority:
#   docs/chapters/08-rfcs/034-native-load-model.md s1.1.2
#     (task_spawn_wait is a swinging concurrent item: exclusive-machine re-test +
#      one-sided bound <=1.2x; the 0.83-1.24x swing is dominated by .NET-side
#      variance -> exclusive machine removes shared-CI jitter)
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md s3.3 (paired protocol)
#   docs/chapters/08-rfcs/013-async-model.md s1.5 (min_per_op as falsifiable lower bound)
#
# Difference from run-024-exclusive-regate.ps1: scenario is task_spawn_wait; the Arc
# side uses the STATISTICAL benchmark `cbench_task_spawn_wait_statistical` (30 iters,
# p50) to remove the bimodal scheduling noise of the single-pass `cbench_task_spawn_wait`.
# The paired gate uses Arc p50_per_op vs .NET ns_per_op (both central tendency,
# like-for-like); min_per_op (RFC 013 falsifiable lower bound) is also recorded as
# claim evidence.
#
# Phase 1 - exclusive-machine detection (abort on busy, exit 3, no benchmark run):
#   1. concurrent bench/build processes matching (arc|cargo|clang|dotnet|cbench|bench|
#      rustc|roofline) present (excluding this script) -> busy
#   2. CPU load: sample \Processor(_Total)\% Processor Time N times (1s each); median
#      > CpuBusyPct -> busy
#   busy -> print exclusive-machine requirement, abort (exit 3).
#
# Phase 2 - paired gate (after exclusive machine confirmed):
#   scenario: task_spawn_wait (Arc `cbench_task_spawn_wait_statistical`) vs .NET
#             (scripts/bench/std-hotpath-dotnet-cmp/StdHotpathDotnetCmp).
#   n = 11; r_i = Arc p50/op / .NET ns/op (same pass, back-to-back); spike = either
#   side > 2x median removed; kept >= 7; gate = raw median of kept <= 1.2x
#   (one-sided, not worse than .NET). Records window tip SHA (099 s3.1 window discipline).
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-task-spawn-wait-exclusive-regate.ps1
#   powershell ... -File .\scripts\bench\run-task-spawn-wait-exclusive-regate.ps1 -CpuBusyPct 10 -Rebuild
#   powershell ... -File .\scripts\bench\run-task-spawn-wait-exclusive-regate.ps1 -SimulateBusy   # self-test busy branch
#
# Exit:
#   0 = exclusive machine + window valid + gate median <= 1.2x
#   1 = window valid but gate MISS (keep unchecked)
#   2 = window invalid (too many spikes / build fail / missing exe / dotnet missing)
#   3 = machine busy (shared/busy machine) - need exclusive machine; no benchmark run
#   4 = protocol interrupted by runtime hang (watchdog)
#
# NOTE: Arc side reuses the `std_hotpath_bench_e2e` cbench exe (an A1 worker may edit
# that test file; by default we do NOT rebuild, only check the existing exe; -Rebuild
# re-runs that e2e to regenerate it).

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$Gate = 1.2,             # one-sided: Arc p50 / .NET median <= 1.2x
    [int]$CpuBusyPct = 15,           # median CPU busy threshold (exclusive machine far below)
    [int]$CpuSamples = 3,            # CPU sample count (1s each)
    [switch]$Rebuild,                # re-run std_hotpath_bench_e2e to regenerate cbench exe
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

# Parse Arc statistical benchmark output:
#   OK: task_spawn_wait_statistical N=50000 iters=30
#     p50=...ns (X.XXns/op)
#     claim: min_per_op=Y.YYns (falsifiable lower bound, RFC 013 s1.5)
# Returns @{ p50 = @{key->p50_per_op}; min = @{key->min_per_op} }
function Parse-Arc([string[]]$lines) {
    $p50 = @{}
    $min = @{}
    $cur = $null
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+N=\d+\s+iters=\d+') {
            $cur = $Matches[1]
        } elseif ($cur -and $line -match 'p50=\d+ns\s+\(([\d.]+)ns/op\)') {
            $p50[$cur] = [double]$Matches[1]
        } elseif ($cur -and $line -match 'claim:\s+min_per_op=([\d.]+)ns') {
            $min[$cur] = [double]$Matches[1]
            $cur = $null
        }
    }
    return @{ 'p50' = $p50; 'min' = $min }
}

# Parse .NET harness output (simple format): OK: <name> ops=N ns_total=T ns_per_op=P
function Parse-Net([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+ops=([\d.]+)\s+ns_total=([\d.]+)\s+ns_per_op=([\d.]+)') {
            $m[$Matches[1]] = [double]$Matches[4]
        }
    }
    return $m
}

# Watchdog execution, returns stdout lines. Correctness keyed on stdout OK lines
# (the exe has built-in asserts; $p.ExitCode is unreliable under Start-Process
# redirection - same approach as run-std-hotpath-h2-gate.ps1).
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arctsw_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arctsw_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
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

Write-Host '===== task_spawn_wait exclusive regate (vs .NET) ====='
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
Write-Host ("protocol: exclusive machine + paired n={0}; spike=side>={1}x median; minKept={2}; gate<= {3}x (one-sided, Arc p50 vs .NET)" -f `
    $Passes, $SpikeFactor, $MinKept, $Gate)
Write-Host ("scenario: task_spawn_wait (Arc `cbench_task_spawn_wait_statistical` vs .NET StdHotpathDotnetCmp)")

# ---- pre-flight ----
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) { Write-Host 'dotnet SDK missing - .NET baseline unavailable.'; exit 2 }

# ---- Phase 1 - exclusive-machine detection (before running any benchmark) ----
if ($ForceProceed) {
    Write-Host 'ForceProceed: SKIPPING exclusive-machine check (self-test only - numbers NOT acceptance).'
} elseif (-not (Test-ExclusiveMachine)) {
    exit 3
}
Write-Host ''

# ---- Arc side exe preparation ----
$arcExe = 'cbench_task_spawn_wait_statistical.exe'
if ($Rebuild) {
    Write-Host 'rebuilding Arc cbench exes via std_hotpath_bench_e2e ...'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & cargo test -p arc-integration --test std_hotpath_bench_e2e 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { Write-Host 'std_hotpath_bench_e2e build failed (may be WIP in another worker); exiting.'; exit 2 }
}
if (-not (Test-Path (Join-Path $E2eDir $arcExe))) {
    Write-Host "missing $arcExe - re-run with -Rebuild (or run std_hotpath_bench_e2e once)."
    exit 2
}

# ---- warmup (untimed; not scored) ----
Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
$prev = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$null = & dotnet run -c Release --project (Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj') --no-launch-profile 2>&1
$ErrorActionPreference = $prev

# ---- n=11 paired ----
$arcP50 = New-Object System.Collections.Generic.List[double]
$arcMin = New-Object System.Collections.Generic.List[double]
$net    = New-Object System.Collections.Generic.List[double]
$ratP50 = New-Object System.Collections.Generic.List[double]
$ratMin = New-Object System.Collections.Generic.List[double]

for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $a = Parse-Arc (Invoke-ExeWatchdog (Join-Path $E2eDir $arcExe) $ExeTimeoutSec 'arc_tsw')
        if (-not $a.p50.ContainsKey('task_spawn_wait_statistical') -or -not $a.min.ContainsKey('task_spawn_wait_statistical')) {
            throw "missing arc task_spawn_wait_statistical"
        }
        $ap50 = $a.p50['task_spawn_wait_statistical']
        $amnn = $a.min['task_spawn_wait_statistical']

        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $netRaw = & dotnet run -c Release --project (Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj') --no-launch-profile 2>&1
        $netCode = $LASTEXITCODE
        $ErrorActionPreference = $prev
        if ($netCode -ne 0) { throw ".NET harness failed (exit $netCode)" }
        $b = Parse-Net ($netRaw | ForEach-Object { "$_" })
        if (-not $b.ContainsKey('task_spawn_wait')) { throw "missing .NET task_spawn_wait on pass $i" }
        $nv = $b['task_spawn_wait']

        $arcP50.Add($ap50)
        $arcMin.Add($amnn)
        $net.Add($nv)
        $ratP50.Add($ap50 / $nv)
        $ratMin.Add($amnn / $nv)
        Write-Host ("  task_spawn_wait: Arc_p50={0:0.00} Arc_min={1:0.00} NET={2:0.00} r_p50={3:0.00}x r_min={4:0.00}x" -f `
            $ap50, $amnn, $nv, ($ap50 / $nv), ($amnn / $nv))
    } catch {
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host 'task_spawn_wait regate window: INVALID (runtime hang or harness fail - do not treat as pass).'
        exit 4
    }
}

Write-Host ''
Write-Host '===== protocol result ====='
Write-Host '| metric | Arc p50 med | NET med | raw med | kept | gate med | spikes | gate<=1.2x |'
Write-Host '|--------|-------------|---------|---------|------|----------|--------|------------|'
$windowOk = $true
$gateOk = $true
$arrP50 = $arcP50.ToArray()
$arrMin = $arcMin.ToArray()
$arrNet = $net.ToArray()
$arrRat = $ratP50.ToArray()
$mA = Median $arrP50
$mN = Median $arrNet
$rawMed = Median $arrRat
$arcMinMed = Median $arrMin

$kept = New-Object System.Collections.Generic.List[double]
$spikeIdx = New-Object System.Collections.Generic.List[int]
for ($j = 0; $j -lt $Passes; $j++) {
    $spike = ($arrP50[$j] -gt ($SpikeFactor * $mA)) -or ($arrNet[$j] -gt ($SpikeFactor * $mN))
    if ($spike) { [void]$spikeIdx.Add($j + 1) } else { [void]$kept.Add($arrRat[$j]) }
}
$k = $kept.Count
if ($k -lt $MinKept) { $windowOk = $false }
$gateMed = Median $kept.ToArray()
$pass = ($gateMed -le $Gate)
if (-not $pass) { $gateOk = $false }
$spikeStr = if ($spikeIdx.Count -eq 0) { '-' } else { ($spikeIdx -join ',') }
$mark = if (-not ($k -ge $MinKept)) { 'INVALID' } elseif ($pass) { 'PASS' } else { 'FAIL' }
Write-Host ("| task_spawn_wait | {0:0.00} | {1:0.00} | {2:0.00}x | {3}/{4} | {5:0.00}x | {6} | {7} |" -f `
    $mA, $mN, $rawMed, $k, $Passes, $gateMed, $spikeStr, $mark)

Write-Host ''
Write-Host ("Arc min_per_op (RFC 013 falsifiable lower bound) median: {0:0.00} ns/op" -f $arcMinMed)
Write-Host ("Arc p50/op median: {0:0.00} ns/op vs .NET median {1:0.00} ns/op" -f $mA, $mN)
Write-Host 'Policy: exclusive-machine one-sided bound (not worse than .NET <=1.2x). No industry leadership claims.'
Write-Host 'RFC 099 s2.1: acceptance requires exclusive machine + protocol green + maintainer announcement.'
if (-not $windowOk) {
    Write-Host 'task_spawn_wait regate window: INVALID (too many spikes - re-run on exclusive machine; do not sign).'
    exit 2
}
if (-not $gateOk) {
    Write-Host 'task_spawn_wait regate gate: MISS (keep unchecked).'
    exit 1
}
Write-Host 'task_spawn_wait regate gate: PASS (p50 gate median <= 1.2x on exclusive machine).'
exit 0
