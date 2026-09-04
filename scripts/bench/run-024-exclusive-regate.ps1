# run-024-exclusive-regate.ps1 — RFC 034 §1.1.2 独占机器复测协议（concurrent_dict/hashset vs .NET）
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority:
#   docs/chapters/08-rfcs/034-native-load-model.md §1.1.2
#     （concurrent_dict/hashset-vs-.NET：独占机复测 + 单侧界 ≤1.2×，0.83–1.24× 摆动
#       由 .NET 侧方差主导 → 独占机剔除共享 CI 抖动）
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md §3.3（成对协议）
#   docs/chapters/08-rfcs/024-maturity-perf.md（H4 基线上调记录）
#
# Phase 1 · 独占机检测（超阈值即 abort，exit 3，不跑任何基准）：
#   1. 并发基准进程：匹配 (arc|cargo|clang|dotnet|cbench|bench|rustc|roofline) 的进程
#      存在（排除本脚本自身）→ busy
#   2. CPU 负载：\Processor(_Total)\% Processor Time 采样 N 次（各 1s），
#      均值 > CpuBusyPct → busy
#   busy → 打印独占机要求说明，abort（exit 3）。
#
# Phase 2 · 成对门禁（独占机确认后）：
#   场景：concurrent_dict_1t / hashset_add_contains（Arc `std_hotpath_bench_e2e` 既有
#         cbench exe）vs .NET（scripts/bench/std-hotpath-dotnet-cmp/StdHotpathDotnetCmp）。
#   n = 11；r_i = Arc_ns/op ÷ .NET_ns/op（同 pass 紧挨执行）；尖峰 = 任一侧 > 2×中位剔除；
#   kept >= 7；gate = 剔尖峰后 raw 中位 ≤ 1.2×（单侧界，不劣于 .NET）。
#   记录窗 tip SHA（099 §3.1 声明窗纪律）。
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-024-exclusive-regate.ps1
#   powershell ... -File .\scripts\bench\run-024-exclusive-regate.ps1 -CpuBusyPct 10 -Rebuild
#   powershell ... -File .\scripts\bench\run-024-exclusive-regate.ps1 -SimulateBusy   # 自测 busy 分支
#
# Exit:
#   0 = 独占机确认 + 窗口有效 + 两场景 gate 中位均 ≤ 1.2×
#   1 = 窗口有效但门禁未过（保持未签收）
#   2 = 窗口无效（尖峰过多 / 构建失败 / 缺 exe / dotnet 缺失）
#   3 = 机器忙（共享/繁忙机器）——需要独占机，未运行基准
#   4 = 协议被运行时挂起中断（看门狗）
#
# 注意：Arc 侧复用 `std_hotpath_bench_e2e` 的 cbench exe（A1 worker 在改该测试文件，
# 默认不触发重建，仅校验既有 exe；-Rebuild 才重新跑该 e2e 生成）。

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$Gate = 1.2,             # 单侧界：Arc/.NET 中位 ≤ 1.2×
    [int]$CpuBusyPct = 15,           # CPU 均值忙碌阈值（独占机应远低于此）
    [int]$CpuSamples = 3,            # CPU 采样次数（各 1s）
    [switch]$Rebuild,                # 重新跑 std_hotpath_bench_e2e 生成 cbench exe
    [switch]$SimulateBusy,           # 自测：强制走 busy abort 分支
    [switch]$ForceProceed,           # 自测：跳过独占机检测，仅验证门禁逻辑（共享机数字不可作验收）
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

function Parse-Ok([string[]]$lines) {
    # Handles BOTH output formats:
    #   simple:      OK: <name> ops=N ns_total=T ns_per_op=P
    #   statistical: OK: <name> iters=K ops=N  ... then  claim: min_per_op=Xns
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

# 带看门狗执行，返回 stdout 行。正确性以 stdout OK 行为准（exe 内置断言；Start-Process
# 重定向时 $p.ExitCode 不可靠——与 run-std-hotpath-h2-gate.ps1 同套路）。
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arcrgt_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcrgt_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
    $p = Start-Process -FilePath $exe -NoNewWindow -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $p.WaitForExit($timeoutSec * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
        throw "HANG: $exe exceeded ${timeoutSec}s (label=$label) — killed; protocol aborted."
    }
    $lines = @(Get-Content $outFile -ErrorAction SilentlyContinue)
    $errs = @(Get-Content $errFile -ErrorAction SilentlyContinue)
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    if ($lines.Count -eq 0) {
        throw "EXE FAIL: $exe produced no stdout (label=$label) ERR: $($errs -join '; ')"
    }
    return $lines
}

# ---- Phase 1 · 独占机检测 ----
function Test-ExclusiveMachine {
    if ($SimulateBusy) {
        Write-Host 'SimulateBusy: forcing busy branch (self-test).'
        return $false
    }
    $busy = @()
    # 1. 并发基准/构建进程
    $pat = '^(arc|cargo|clang|dotnet|cbench|bench|rustc|roofline|arc-git-sync)'
    $procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.ProcessName -match $pat -and $_.Id -ne $PID
    })
    foreach ($p in $procs) {
        $busy += ("process {0} (pid {1})" -f $p.ProcessName, $p.Id)
    }
    # 2. CPU 负载
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
        Write-Host ("WARN: cpu load sampling failed ({0}) — falling back to process detection only" -f $_.Exception.Message)
    }
    if ($busy.Count -gt 0) {
        Write-Host ''
        Write-Host '===== EXCLUSIVE-MACHINE CHECK: BUSY — ABORT ====='
        foreach ($b in $busy) { Write-Host ("  busy: {0}" -f $b) }
        Write-Host ''
        Write-Host 'This protocol requires an EXCLUSIVE benchmark machine (RFC 034 §1.1.2):'
        Write-Host '  - no other benchmark/build processes running'
        Write-Host '  - CPU load below threshold (idle machine; shared CI / dev box is disqualified)'
        Write-Host 'Move to a dedicated machine and re-run. No benchmark was executed.'
        return $false
    }
    Write-Host 'exclusive machine: CLEAN (no bench/build processes; CPU within threshold)'
    return $true
}

Write-Host '===== 024 exclusive regate (concurrent_dict/hashset vs .NET) ====='
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
Write-Host ("scenarios: concurrent_dict_1t / hashset_add_contains (Arc cbench vs .NET StdHotpathDotnetCmp)")

# ---- pre-flight ----
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) { Write-Host 'dotnet SDK missing — .NET baseline unavailable.'; exit 2 }

# ---- Phase 1 · 独占机检测（在跑任何基准之前） ----
if ($ForceProceed) {
    Write-Host 'ForceProceed: SKIPPING exclusive-machine check (self-test only — numbers NOT acceptance).'
} elseif (-not (Test-ExclusiveMachine)) {
    exit 3
}
Write-Host ''

# ---- Arc 侧 exe 准备 ----
$arcExes = @('cbench_concurrent_dict_1t.exe', 'cbench_hashset_add_contains.exe')
if ($Rebuild) {
    Write-Host 'rebuilding Arc cbench exes via std_hotpath_bench_e2e ...'
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $raw = & cargo test -p arc-integration --test std_hotpath_bench_e2e 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { Write-Host 'std_hotpath_bench_e2e build failed (may be WIP in another worker); exiting.'; exit 2 }
}
foreach ($f in $arcExes) {
    if (-not (Test-Path (Join-Path $E2eDir $f))) {
        Write-Host "missing $f — re-run with -Rebuild (or run std_hotpath_bench_e2e once)."
        exit 2
    }
}

# ---- warmup（不计分） ----
Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
$prev = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$null = & dotnet run -c Release --project (Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj') --no-launch-profile 2>&1
$ErrorActionPreference = $prev

# ---- n=11 成对 ----
$scenarios = @('concurrent_dict_1t', 'hashset_add_contains')
$arc = @{}; $net = @{}; $rat = @{}
foreach ($s in $scenarios) {
    $arc[$s] = New-Object System.Collections.Generic.List[double]
    $net[$s] = New-Object System.Collections.Generic.List[double]
    $rat[$s] = New-Object System.Collections.Generic.List[double]
}

for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $a = Parse-Ok (Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_concurrent_dict_1t.exe') $ExeTimeoutSec 'arc_cd')
        $a2 = Parse-Ok (Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_hashset_add_contains.exe') $ExeTimeoutSec 'arc_hs')
        foreach ($s in @('concurrent_dict_1t', 'hashset_add_contains')) {
            if ($s -eq 'concurrent_dict_1t' -and -not $a.ContainsKey($s)) { throw "missing arc $s" }
            if ($s -eq 'hashset_add_contains' -and -not $a2.ContainsKey($s)) { throw "missing arc $s" }
        }
        $arc['concurrent_dict_1t'].Add($a['concurrent_dict_1t'])
        $arc['hashset_add_contains'].Add($a2['hashset_add_contains'])

        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $netRaw = & dotnet run -c Release --project (Join-Path $Root 'scripts/bench/std-hotpath-dotnet-cmp\StdHotpathDotnetCmp.csproj') --no-launch-profile 2>&1
        $netCode = $LASTEXITCODE
        $ErrorActionPreference = $prev
        if ($netCode -ne 0) { throw ".NET harness failed (exit $netCode)" }
        $b = Parse-Ok ($netRaw | ForEach-Object { "$_" })
    } catch {
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host '024 regate window: INVALID (runtime hang or harness fail — do not treat as pass).'
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
Write-Host 'RFC 099 §2.1: acceptance requires exclusive machine + protocol green + maintainer announcement.'
if (-not $windowOk) {
    Write-Host '024 regate window: INVALID (too many spikes — re-run on exclusive machine; do not sign).'
    exit 2
}
if (-not $gateOk) {
    Write-Host '024 regate gate: MISS (keep unchecked).'
    exit 1
}
Write-Host '024 regate gate: PASS (both scenarios gate medians <= 1.2x on exclusive machine).'
exit 0
