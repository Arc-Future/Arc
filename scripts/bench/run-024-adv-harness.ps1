# run-024-adv-harness.ps1 — RFC 034 §1.1.4 结构性优势场景协议跑批（预演 harness）
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority:
#   docs/chapters/08-rfcs/034-native-load-model.md §1.1.4（新增优势场景）
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md §3.3（极致性能验收协议）
#   docs/chapters/08-rfcs/024-maturity-perf.md（性能门禁文档）
#
# 场景（RFC 034 §1.1.4，4 项）：
#   adv_async_io_throughput   异步批量 IO 吞吐（Reactor submit/flush/poll 整周期）
#                             门禁：剔尖峰后中位 req/s >= 1,000,000（RFC 016 L51 预算）
#   adv_zero_copy_*           零拷贝 IO 管线（rt_iobuf_pool acquire/release 借还周期，
#                             含注册 reactor 的 pipeline 形态）—— 锚点（RFC 034 未定数值
#                             目标，由维护者签收时定，本脚本只记录不判定）
#   adv_soa_simd_sum          SoA·SIMD（rt_soa_array 字段求和，clang 自动向量化）
#   adv_soa_aos_sum           AoS 对照 —— 门禁：SoA 中位 ns/op 不得慢于 AoS（结构性优势保持）
#   adv_aot_startup_arc       AOT 冷启动（arc build -c Release；spawn→exit 墙钟 ms）——
#                             锚点（「无 JIT 预热」为编译型结构性定位，数值目标维护者定）
#
# 协议（对齐 099 §3.3，绝对场景适配）：
#   n = 11（warmup 各 1 次不计）；尖峰 = 侧 > 2×中位剔除；kept >= 7；
#   gate = 剔尖峰后 raw 中位；看门狗 = 每 exe 超时 kill + 终止协议（exit 4）。
#   共享机数字只作预演记录，不宣布达标（099 §2.1 宣称纪律；验收须独占机 + 协议绿 + 维护者宣布）。
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-024-adv-harness.ps1
#   powershell ... -File .\scripts\bench\run-024-adv-harness.ps1 -Passes 11 -AsyncGate 1000000
#   powershell ... -File .\scripts\bench\run-024-adv-harness.ps1 -SkipBuild
#   powershell ... -File .\scripts\bench\run-024-adv-harness.ps1 -SkipAot
#
# Exit:
#   0 = 窗口有效且所有硬门禁通过（async >= AsyncGate req/s；SoA 快于 AoS）
#   1 = 窗口有效但门禁未过（如实记录差距；不宣布达标）
#   2 = 窗口无效（尖峰过多 / 构建失败 / exe 缺失 / clang 缺失）
#   4 = 协议被运行时挂起中断（看门狗）

param(
    [int]$Passes = 11,
    [double]$SpikeFactor = 2.0,
    [int]$MinKept = 7,
    [double]$AsyncGate = 1000000.0,   # >=1M req/s（RFC 016 L51 预算）
    [switch]$SkipBuild,               # 复用 target/e2e 既有 exe（跳过 cargo e2e 构建）
    [switch]$SkipAot,                 # 跳过 AOT 启动测量（节省时间）
    [int]$ExeTimeoutSec = 120
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$E2eDir = Join-Path $Root 'target\e2e'
$ArcExe = @{
    'adv_async_io_throughput'  = 'cbench_adv_async_io'
    'adv_zero_copy_acquire_release' = 'cbench_adv_zero_copy'
    'adv_zero_copy_pipeline'    = 'cbench_adv_zero_copy'
    'adv_soa_simd_sum'          = 'cbench_adv_soa_simd'
    'adv_soa_aos_sum'           = 'cbench_adv_soa_simd'
    'adv_aot_startup_arc'       = 'adv_aot_startup_arc'
}

function Parse-Ok([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+ops=([\d.]+)\s+ns_total=([\d.]+)\s+ns_per_op=([\d.]+)\s+ops_per_s=([\d.]+)') {
            $m[$Matches[1]] = [pscustomobject]@{
                NsPerOp = [double]$Matches[4]
                NsTotal = [double]$Matches[3]
                OpsPerS = [double]$Matches[5]
                Ops     = [double]$Matches[2]
            }
        }
    }
    return $m
}

function Median([double[]]$a) {
    if (-not $a -or $a.Count -eq 0) { return [double]::NaN }
    $s = @($a | Sort-Object { [double]$_ })
    return [double]$s[[int][Math]::Floor(($s.Count - 1) / 2.0)]
}

# 带看门狗执行：Start-Process + WaitForExit(超时)；超时 → kill + 抛错终止协议。
# 注意：Start-Process + 重定向输出时 $p.ExitCode 不可靠（PowerShell 长期 quirk），
# 与 run-std-hotpath-h2-gate.ps1 一致，正确性以 stdout OK 行 / 哨兵行为准（各 exe 内置断言）。
function Invoke-ExeWatchdog([string]$exe, [int]$timeoutSec, [string]$label) {
    $outFile = Join-Path $env:TEMP ("arcadv_{0}_{1}.out" -f $label, [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcadv_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
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

# AOT 冷启动：spawn→exit 墙钟（毫秒）。带看门狗。正确性以哨兵行为准。
function Measure-AotOnce([string]$exe, [int]$timeoutSec) {
    $outFile = Join-Path $env:TEMP ("arcadv_aot_{0}.out" -f [guid]::NewGuid().ToString('N'))
    $errFile = Join-Path $env:TEMP ("arcadv_aot_{0}.err" -f [guid]::NewGuid().ToString('N'))
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $p = Start-Process -FilePath $exe -NoNewWindow -PassThru `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    if (-not $p.WaitForExit($timeoutSec * 1000)) {
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 200
        throw "HANG: AOT exe exceeded ${timeoutSec}s — killed; protocol aborted."
    }
    $sw.Stop()
    $lines = @(Get-Content $outFile -ErrorAction SilentlyContinue)
    $errs = @(Get-Content $errFile -ErrorAction SilentlyContinue)
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    if (-not ($lines -match 'adv-aot-startup-ready')) {
        throw "AOT exe failed (missing sentinel) ERR: $($errs -join '; ')"
    }
    return $sw.Elapsed.TotalMilliseconds
}

# 每 pass 跑一次全部场景，返回指标字典
function Run-PassOnce {
    $out = @{}
    $aio = Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_adv_async_io.exe') $ExeTimeoutSec 'aio'
    $zc  = Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_adv_zero_copy.exe') $ExeTimeoutSec 'zc'
    $soa = Invoke-ExeWatchdog (Join-Path $E2eDir 'cbench_adv_soa_simd.exe') $ExeTimeoutSec 'soa'

    $a = Parse-Ok $aio
    if (-not $a.ContainsKey('adv_async_io_throughput')) { throw "async scenario missing in pass" }
    $out['async_ns_total'] = $a['adv_async_io_throughput'].NsTotal
    $out['async_req_s']    = $a['adv_async_io_throughput'].OpsPerS

    $z = Parse-Ok $zc
    if (-not $z.ContainsKey('adv_zero_copy_acquire_release') -or -not $z.ContainsKey('adv_zero_copy_pipeline')) {
        throw "zero-copy scenario missing in pass"
    }
    $out['zc_ar_ns_total']  = $z['adv_zero_copy_acquire_release'].NsTotal
    $out['zc_pipe_ns_total'] = $z['adv_zero_copy_pipeline'].NsTotal
    $out['zc_ar_ops']       = $z['adv_zero_copy_acquire_release'].Ops
    $out['zc_pipe_ops']     = $z['adv_zero_copy_pipeline'].Ops

    $s = Parse-Ok $soa
    if (-not $s.ContainsKey('adv_soa_simd_sum') -or -not $s.ContainsKey('adv_soa_aos_sum')) {
        throw "soa scenario missing in pass"
    }
    $out['soa_ns_total'] = $s['adv_soa_simd_sum'].NsTotal
    $out['aos_ns_total'] = $s['adv_soa_aos_sum'].NsTotal

    if (-not $SkipAot) {
        $out['aot_ms'] = Measure-AotOnce (Join-Path $E2eDir 'adv_aot_startup_arc.exe') $ExeTimeoutSec
    }
    return $out
}

Write-Host '===== 024 advantage scenarios harness ====='
Write-Host ("date: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("os: {0}" -f [System.Runtime.InteropServices.RuntimeInformation]::OSDescription)
try {
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    Write-Host ("cpu: {0}" -f $cpu)
} catch { Write-Host 'cpu: (unavailable)' }
Write-Host ("protocol: n={0}; spike=side>={1}x median; minKept={2}; asyncGate>={3} req/s" -f $Passes, $SpikeFactor, $MinKept, $AsyncGate)
Write-Host ("scenarios: async_io / zero_copy / soa_simd / aot_startup{0}" -f $(if ($SkipAot) { ' [SKIPPED]' } else { '' }))
Write-Host 'Policy: shared-machine numbers are REHEARSAL ONLY — no pass claims (099 §2.1).'

# ---- 构建（一次性）：跑各 e2e 生成 target/e2e/cbench_adv_*.exe / adv_aot_startup_arc.exe ----
if (-not $SkipBuild) {
    Write-Host ''
    Write-Host '===== build harnesses (cargo e2e, once) ====='
    foreach ($t in @('adv_async_io_throughput_e2e', 'adv_zero_copy_io_e2e', 'adv_soa_simd_e2e', 'adv_aot_startup_e2e')) {
        Write-Host ("building {0} ..." -f $t)
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $raw = & cargo test -p arc-integration --test $t 2>&1
        $code = $LASTEXITCODE
        $ErrorActionPreference = $prev
        if ($code -ne 0) { throw "cargo e2e build failed for $t (exit $code)" }
    }
}

foreach ($f in @('cbench_adv_async_io.exe', 'cbench_adv_zero_copy.exe', 'cbench_adv_soa_simd.exe')) {
    if (-not (Test-Path (Join-Path $E2eDir $f))) { throw "missing $f — run without -SkipBuild first" }
}
if (-not $SkipAot -and -not (Test-Path (Join-Path $E2eDir 'adv_aot_startup_arc.exe'))) {
    throw "missing adv_aot_startup_arc.exe — run without -SkipBuild / -SkipAot first"
}

Write-Host ''
Write-Host '===== warmup (untimed; not scored) ====='
$null = Run-PassOnce

# 每场景存样本
$samples = @{
    'async_ns_total' = New-Object System.Collections.Generic.List[double]
    'async_req_s'    = New-Object System.Collections.Generic.List[double]
    'zc_ar_ns_total' = New-Object System.Collections.Generic.List[double]
    'zc_pipe_ns_total' = New-Object System.Collections.Generic.List[double]
    'zc_ar_ops'      = New-Object System.Collections.Generic.List[double]
    'zc_pipe_ops'    = New-Object System.Collections.Generic.List[double]
    'soa_ns_total'   = New-Object System.Collections.Generic.List[double]
    'aos_ns_total'   = New-Object System.Collections.Generic.List[double]
    'aot_ms'         = New-Object System.Collections.Generic.List[double]
}

for ($i = 1; $i -le $Passes; $i++) {
    Write-Host ("=== pass {0}/{1} ===" -f $i, $Passes)
    try {
        $r = Run-PassOnce
    } catch {
        Write-Host ("PROTOCOL ABORTED on pass {0}: {1}" -f $i, $_.Exception.Message)
        Write-Host '024 advantage window: INVALID (runtime hang or exe fail — do not treat as pass).'
        exit 4
    }
    foreach ($k in $samples.Keys) {
        if ($r.ContainsKey($k)) { [void]$samples[$k].Add($r[$k]) }
    }
    $aotMs = if ($r.ContainsKey('aot_ms')) { $r['aot_ms'] } else { -1 }
    $zcArCyc = [double]$r['zc_ar_ops'] * 1e9 / [double]$r['zc_ar_ns_total']
    $zcPipeCyc = [double]$r['zc_pipe_ops'] * 1e9 / [double]$r['zc_pipe_ns_total']
    $soaNsOp = [double]$r['soa_ns_total'] / 8e6
    $aosNsOp = [double]$r['aos_ns_total'] / 8e6
    Write-Host ("  async: {0:0} req/s | zc_ar: {1:0} cycles/s | zc_pipe: {2:0} cycles/s | soa: {3:0.00} ns/op | aos: {4:0.00} ns/op | aot: {5:0.0} ms" -f `
        [double]$r['async_req_s'], $zcArCyc, $zcPipeCyc, $soaNsOp, $aosNsOp, $aotMs)
}

Write-Host ''
Write-Host '===== protocol result ====='

function SpikeFilter([string]$metric, [double[]]$arr, [int]$total) {
    $med = Median $arr
    $keptIdx = @()
    for ($j = 0; $j -lt $arr.Count; $j++) {
        if ($arr[$j] -le ($SpikeFactor * $med)) { $keptIdx += $j }
    }
    return $keptIdx
}

$windowOk = $true
$gateOk = $true

# ---- async 吞吐 ----
$arr = @($samples['async_ns_total'])
$mA = Median $arr
$kept = SpikeFilter 'async_ns_total' $arr $Passes
if ($kept.Count -lt $MinKept) { $windowOk = $false }
$keptReqS = @()
foreach ($j in $kept) { $keptReqS += [double]$samples['async_req_s'][$j] }
$gateReqS = Median $keptReqS
$passAsync = ($gateReqS -ge $AsyncGate)
if (-not $passAsync) { $gateOk = $false }
Write-Host ("| async_batch_io | req/s | {0:0} | kept {1}/{2} | spikes {3} | gate>={4:0} req/s | {5} |" -f `
    $gateReqS, $kept.Count, $Passes, ($Passes - $kept.Count), $AsyncGate, $(if ($kept.Count -lt $MinKept) { 'INVALID' } elseif ($passAsync) { 'PASS' } else { 'FAIL' }))

# ---- zero-copy（锚点，无硬门禁） ----
$arrZc = @($samples['zc_ar_ns_total'])
$keptZc = SpikeFilter 'zc_ar_ns_total' $arrZc $Passes
if ($keptZc.Count -lt $MinKept) { $windowOk = $false }
$zcOps = Median @($samples['zc_ar_ops'])
$zcMed = $zcOps * 1e9 / (Median $arrZc)
Write-Host ("| zero_copy_acquire_release | cycles/s (anchor) | {0:0} | kept {1}/{2} | spikes {3} | gate: TBD(maintainer) | anchor |" -f `
    $zcMed, $keptZc.Count, $Passes, ($Passes - $keptZc.Count))
$arrZp = @($samples['zc_pipe_ns_total'])
$keptZp = SpikeFilter 'zc_pipe_ns_total' $arrZp $Passes
if ($keptZp.Count -lt $MinKept) { $windowOk = $false }
$zpOps = Median @($samples['zc_pipe_ops'])
$zpMed = $zpOps * 1e9 / (Median $arrZp)
Write-Host ("| zero_copy_pipeline | cycles/s (anchor) | {0:0} | kept {1}/{2} | spikes {3} | gate: TBD(maintainer) | anchor |" -f `
    $zpMed, $keptZp.Count, $Passes, ($Passes - $keptZp.Count))

# ---- SoA vs AoS ----
$arrSoa = @($samples['soa_ns_total'])
$arrAos = @($samples['aos_ns_total'])
$mSoa = Median $arrSoa
$mAos = Median $arrAos
$keptSoa = @()
for ($j = 0; $j -lt $arrSoa.Count; $j++) {
    if (($arrSoa[$j] -le ($SpikeFactor * $mSoa)) -and ($arrAos[$j] -le ($SpikeFactor * $mAos))) { $keptSoa += $j }
}
if ($keptSoa.Count -lt $MinKept) { $windowOk = $false }
$ratio = if ($mSoa -gt 0) { $mAos / $mSoa } else { 0 }
$passSoa = ($mSoa -lt $mAos)
if (-not $passSoa) { $gateOk = $false }
Write-Host ("| soa_simd_sum vs aos | ratio aos/soa | {0:0.00}x | kept {1}/{2} | spikes {3} | gate: soa median < aos median | {4} |" -f `
    $ratio, $keptSoa.Count, $Passes, ($Passes - $keptSoa.Count), $(if ($keptSoa.Count -lt $MinKept) { 'INVALID' } elseif ($passSoa) { 'PASS' } else { 'FAIL' }))

# ---- AOT 启动（锚点） ----
if (-not $SkipAot -and $samples['aot_ms'].Count -gt 0) {
    $arrAot = @($samples['aot_ms'])
    $mAot = Median $arrAot
    $keptAot = SpikeFilter 'aot_ms' $arrAot $Passes
    if ($keptAot.Count -lt $MinKept) { $windowOk = $false }
    Write-Host ("| aot_startup | spawn->exit ms (anchor) | {0:0.0} | kept {1}/{2} | spikes {3} | gate: TBD(maintainer) | anchor |" -f `
        $mAot, $keptAot.Count, $Passes, ($Passes - $keptAot.Count))
}

Write-Host ''
Write-Host 'Policy: absolute advantage anchors on shared machine — REHEARSAL, not acceptance.'
if (-not $windowOk) {
    Write-Host '024 advantage window: INVALID (too many spikes — re-run; do not treat as pass).'
    exit 2
}
if (-not $gateOk) {
    Write-Host '024 advantage gate: MISS (record current gap honestly; keep unchecked).'
    exit 1
}
Write-Host '024 advantage gate: STABLE PASS (async req/s gate + SoA advantage hold; zero-copy/AOT are anchors awaiting maintainer targets).'
exit 0
