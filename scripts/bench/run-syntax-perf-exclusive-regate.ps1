# run-syntax-perf-exclusive-regate.ps1 - basic-syntax perf exclusive-machine paired regate
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Scenarios (mirror Arc syntax_perf_bench_e2e.rs; same N/ops/workload):
#   loop_sum              pure arithmetic loop N=5e7        - codegen loop throughput
#   string_replace_long   1MB sparse-token replace 20x      - long-text processing
#   file_concurrency      8 threads x 50 write+read 64KB    - concurrent file ops
#
# Authority:
#   docs/chapters/08-rfcs/024-maturity-perf.md (baseline / H-gates)
#   docs/chapters/08-rfcs/034-native-load-model.md (Part A perf gate restatement)
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md (claimed discipline)
#   docs/chapters/08-rfcs/047-string-replace-long-sprint.md (the known gap item)
#
# Phase 1 - exclusive machine detection (abort on busy, exit 3, no bench run):
#   1. concurrent bench/build processes matching (arc|cargo|clang|dotnet|cbench|bench|
#      rustc|roofline) exist (excluding this script / self) -> busy
#   2. CPU load: \Processor(_Total)\% Processor Time sampled N times (1s each),
#      median > CpuBusyPct -> busy
#   busy -> print requirement, abort (exit 3).
#
# Phase 2 - paired gate (after exclusive confirmed):
#   Arc side: target/e2e/cbench_loop_sum.exe / cbench_string_replace_long.exe /
#             cbench_file_concurrency.exe (produced by syntax_perf_bench_e2e)
#   .NET side: scripts/bench/syntax-perf-dotnet-cmp/SyntaxPerfDotnetCmp.csproj
#   n = Passes; r_i = Arc_ns/op / .NET_ns/op (paired, run back-to-back in same pass);
#   spike = either side > 2x median removed; kept >= MinKept;
#   gate = spike-removed raw median <= 1.2x (one-sided, not worse than .NET).
#   Record window tip SHA (RFC 099 claimed-discipline window).
#
# NOTE: string_replace_long is a KNOWN gap (Arc rt_str_replace strstr-loop vs .NET
#   SIMD single-pass; recorded ~2.9x on shared machine). This gate is expected to MISS
#   that scenario -> honest baseline that RFC 047 tracks to closure. loop_sum and
#   file_concurrency should pass.
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-syntax-perf-exclusive-regate.ps1
#   powershell ... -File .\scripts\bench\run-syntax-perf-exclusive-regate.ps1 -CpuBusyPct 10 -Rebuild
#   powershell ... -File .\scripts\bench\run-syntax-perf-exclusive-regate.ps1 -SimulateBusy
#
# Exit:
#   0 = exclusive + window valid + all scenario gate medians <= 1.2x
#   1 = window valid but gate missed (at least one scenario > 1.2x) - keep unchecked
#   2 = window invalid (too many spikes / build fail / missing exe / dotnet missing)
#   3 = machine busy (shared/busy machine) - needs exclusive; no bench run
#   4 = protocol aborted by runtime hang (watchdog)
#
# Keep this file ASCII-only (no CJK comments) - PowerShell 5.1 reads BOM-less
# scripts in the ANSI code page; ASCII avoids encoding corruption.

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$Gate = 1.2,             # one-sided: Arc/.NET median <= 1.2x
    [int]$CpuBusyPct = 15,           # CPU median busy threshold (exclusive should be far below)
    [int]$CpuSamples = 3,            # CPU sample count (1s each)
    [switch]$Rebuild,                # re-run syntax_perf_bench_e2e to (re)build cbench exes
    [switch]$SimulateBusy,           # self-test: force busy abort branch
    [switch]$ForceProceed,           # self-test: skip exclusive check (numbers NOT acceptance)
    [int]$ExeTimeoutSec = 120
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
$E2eDir = Join-Path $Root 'target\e2e'
$NetProj = Join-Path $Root 'scripts/bench/syntax-perf-dotnet-cmp\SyntaxPerfDotnetCmp.csproj'

function Median([double[]]$a) {
    if (-not $a -or $a.Count -eq 0) { return [double]::NaN }
    $s = @($a | Sort-Object { [double]$_ })
    return [double]$s[[int][Math]::Floor(($s.Count - 1) / 2.0)]
}

function Parse-Ok([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+ops=[\d.]+\s+ns_total=[\d.]+\s+ns_per_op=([\d.]+)\s+ops_per_s=[\d.]+') {
            $m[$Matches[1]] = [double]$Matches[2]
        }
    }
    return $m
}

# Watchdog execution; correctness judged by stdout OK lines (exe has internal asserts).
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arcspt_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcspt_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
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

# ---- Phase 1 - exclusive machine detection ----
function Test-ExclusiveMachine {
    if ($SimulateBusy) {
        Write-Host 'SimulateBusy: forcing busy branch (self-test).'
        return $false
    }
    $busy = @()
    $pat = '^(arc|cargo|clang|dotnet|cbench|bench|rustc|roofline|arc-git-sync)'
    $procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.ProcessName -match $pat -and $_.Id -ne $PID
    })
    foreach ($p in $procs) {
        $busy += ("process {0} (pid {1})" -f $p.ProcessName, $p.Id)
    }
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
        Write-Host 'This protocol requires an EXCLUSIVE benchmark machine:'
        Write-Host '  - no other benchmark/build processes running'
        Write-Host '  - CPU load below threshold (idle machine; shared CI / dev box is disqualified)'
        Write-Host 'Move to a dedicated machine and re-run. No benchmark was executed.'
        return $false
    }
    Write-Host 'exclusive machine: CLEAN (no bench/build processes; CPU within threshold)'
    return $true
}

Write-Host '===== syntax perf exclusive regate (loop / long-text / file conc vs .NET) ====='
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
Write-Host ("protocol: exclusive machine + paired n={0}; spike=side>={1}x median; minKept={2}; gate<= {3}x (one-sided)" -f `
    $Passes, $SpikeFactor, $MinKept, $Gate)
Write-Host 'scenarios: loop_sum / string_replace_long / file_concurrency (Arc cbench vs .NET SyntaxPerfDotnetCmp)'
Write-Host 'NOTE: string_replace_long is a KNOWN gap (see RFC 047); expected MISS on this gate.'
Write-Host '      loop_sum / file_concurrency expected PASS. This run solidifies the baseline.'

# ---- pre-flight ----
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) { Write-Host 'dotnet SDK missing - .NET baseline unavailable.'; exit 2 }

# ---- Phase 1 - exclusive machine detection (before any bench) ----
if ($ForceProceed) {
    Write-Host 'ForceProceed: SKIPPING exclusive-machine check (self-test only - numbers NOT acceptance).'
} elseif (-not (Test-ExclusiveMachine)) {
    exit 3
}
Write-Host ''

# ---- Arc side exe prep ----
$arcExes = @('cbench_loop_sum.exe', 'cbench_string_replace_long.exe', 'cbench_file_concurrency.exe')
if ($Rebuild) {
    Write-Host 'rebuilding Arc cbench exes via syntax_perf_bench_e2e ...'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & cargo test -p arc-integration --test syntax_perf_bench_e2e 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { Write-Host 'syntax_perf_bench_e2e build failed; exiting.'; exit 2 }
}
foreach ($f in $arcExes) {
    if (-not (Test-Path (Join-Path $E2eDir $f))) {
        Write-Host "missing $f - re-run with -Rebuild (or run syntax_perf_bench_e2e once)."
        exit 2
    }
}

# ---- warmup (untimed; not scored) ----
Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
$prev = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$null = & dotnet run -c Release --project $NetProj --no-launch-profile 2>&1
$ErrorActionPreference = $prev

# ---- n=Passes paired ----
$scenarios = @('loop_sum', 'string_replace_long', 'file_concurrency')
$arc = @{}; $net = @{}; $rat = @{}
foreach ($s in $scenarios) {
    $arc[$s] = New-Object System.Collections.Generic.List[double]
    $net[$s] = New-Object System.Collections.Generic.List[double]
    $rat[$s] = New-Object System.Collections.Generic.List[double]
}

for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $a  = Parse-Ok (Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_loop_sum.exe') $ExeTimeoutSec 'arc_lp')
        $a2 = Parse-Ok (Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_string_replace_long.exe') $ExeTimeoutSec 'arc_rp')
        $a3 = Parse-Ok (Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_file_concurrency.exe') $ExeTimeoutSec 'arc_fc')
        if (-not $a.ContainsKey('loop_sum')) { throw 'missing arc loop_sum' }
        if (-not $a2.ContainsKey('string_replace_long')) { throw 'missing arc string_replace_long' }
        if (-not $a3.ContainsKey('file_concurrency')) { throw 'missing arc file_concurrency' }
        $arc['loop_sum'].Add($a['loop_sum'])
        $arc['string_replace_long'].Add($a2['string_replace_long'])
        $arc['file_concurrency'].Add($a3['file_concurrency'])

        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $netRaw = & dotnet run -c Release --project $NetProj --no-launch-profile 2>&1
        $netCode = $LASTEXITCODE
        $ErrorActionPreference = $prev
        if ($netCode -ne 0) { throw ".NET harness failed (exit $netCode)" }
        $b = Parse-Ok ($netRaw | ForEach-Object { "$_" })
    } catch {
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host 'syntax perf regate window: INVALID (runtime hang or harness fail - do not treat as pass).'
        exit 4
    }
    foreach ($s in $scenarios) {
        if (-not $b.ContainsKey($s)) { throw "missing .NET scenario $s on pass $i" }
        $r = $arc[$s][$arc[$s].Count - 1] / $b[$s]
        [void]$net[$s].Add($b[$s])
        [void]$rat[$s].Add($r)
        Write-Host ("  {0}: Arc={1:0.00} NET={2:0.00} r={3:0.00}x" -f $s, $arc[$s][$arc[$s].Count - 1], $b[$s], $r)
    }
}

Write-Host ''
Write-Host '===== protocol result ====='
Write-Host '| scenario | Arc med | NET med | raw med | kept | gate med | spikes | gate<=1.2x |'
Write-Host '|----------|---------|---------|---------|------|----------|--------|------------|'
$windowOk = $true
$gateOk = $true
foreach ($s in $scenarios) {
    $arcArr = $arc[$s].ToArray()
    $netArr = $net[$s].ToArray()
    $ratArr = $rat[$s].ToArray()
    $mA = Median $arcArr
    $mN = Median $netArr
    $rawMed = Median $ratArr

    $kept = New-Object System.Collections.Generic.List[double]
    $spikeIdx = New-Object System.Collections.Generic.List[int]
    for ($j = 0; $j -lt $Passes; $j++) {
        $spike = ($arcArr[$j] -gt ($SpikeFactor * $mA)) -or ($netArr[$j] -gt ($SpikeFactor * $mN))
        if ($spike) { [void]$spikeIdx.Add($j + 1) } else { [void]$kept.Add($ratArr[$j]) }
    }
    $k = $kept.Count
    if ($k -lt $MinKept) { $windowOk = $false }
    $gateMed = Median $kept.ToArray()
    $pass = ($gateMed -le $Gate)
    if (-not $pass) { $gateOk = $false }
    $spikeStr = if ($spikeIdx.Count -eq 0) { '-' } else { ($spikeIdx -join ',') }
    $mark = if (-not ($k -ge $MinKept)) { 'INVALID' } elseif ($pass) { 'PASS' } else { 'FAIL' }
    Write-Host ("| {0} | {1:0.00} | {2:0.00} | {3:0.00}x | {4}/{5} | {6:0.00}x | {7} | {8} |" -f `
        $s, $mA, $mN, $rawMed, $k, $Passes, $gateMed, $spikeStr, $mark)
}

Write-Host ''
Write-Host 'Policy: exclusive-machine one-sided bound (not worse than .NET <=1.2x). No industry leadership claims.'
Write-Host 'string_replace_long gap is tracked by RFC 047; a FAIL here is expected baseline, not a false pass.'
Write-Host 'RFC 099 claimed-discipline: acceptance requires exclusive machine + protocol green + maintainer announcement.'
if (-not $windowOk) {
    Write-Host 'syntax perf regate window: INVALID (too many spikes - re-run on exclusive machine; do not sign).'
    exit 2
}
if (-not $gateOk) {
    Write-Host 'syntax perf regate gate: MISS (at least one scenario > 1.2x; loop/string/file baseline solidified).'
    exit 1
}
Write-Host 'syntax perf regate gate: PASS (all scenarios gate medians <= 1.2x on exclusive machine).'
exit 0
