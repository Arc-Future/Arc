# run-async-io-cmp.ps1 - Arc vs .NET / Rust / Go async IO (batch file read) same-machine compare
#
# NOTE (2026-08-29): Arc 侧 bench 原属 crates/arc-integration，已随该 crate 退场（a2627a0f）。
#   本脚本保留为历史性能记录：Arc 侧数据不可再生，其余语言侧 harness 仍可独立运行。
#
# Authority: RFC 016 L51 IO throughput budget (>=1M req/s was judged to need Linux io_uring).
#           Maintainer ruling (2026-08-08): async IO validation = same-machine Windows compare
#           against popular languages, not insisting on Linux absolute threshold.
#
# Each language mirrors Arc `adv_async_io_throughput_e2e` workload (64MiB file; K=4096 x 4KB
# offset reads/round; ROUNDS=256; ops=1,048,576; 4x warmup). Idiomatic async-read path:
#   - Arc   : true IOCP reactor (cbench_adv_async_io)
#   - .NET  : RandomAccess.ReadAsync + FileOptions.Asynchronous (true IOCP)
#   - Rust  : std thread pool + FileExt::seek_read (Windows has no true async file IO; tokio::fs-like)
#   - Go    : goroutine + os.File.ReadAt (Windows has no true async file IO)
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-async-io-cmp.ps1
#
# Exit:
#   0 = compare produced (Arc + at least one reference; missing ones printed honestly)
#   2 = no reference succeeded / Arc side failed
#
# Policy: same-machine anchors only. No industry-leadership claim.

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

function Parse-ReqS([string[]]$lines) {
    $m = @{}
    foreach ($line in $lines) {
        if ($line -match 'OK:\s+(\S+)\s+ops=[\d.]+\s+ns_total=[\d.]+\s+ns_per_op=[\d.]+\s+ops_per_s=([\d.]+)') {
            $m[$Matches[1]] = [double]$Matches[2]
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
Write-Host 'workload: 64MiB file, K=4096 x 4KB offset reads/round, ROUNDS=256, ops=1,048,576, 4x warmup'

$results = @{}
$errors = @()

# ---- 1. Arc ----
Write-Host ''
Write-Host '===== 1. Arc (cbench_adv_async_io / IOCP reactor) ====='
$arcExe = Join-Path $Root 'target\e2e\cbench_adv_async_io.exe'
if (-not (Test-Path $arcExe)) {
    Write-Host 'Arc exe missing - run `cargo test -p arc-integration --test adv_async_io_throughput_e2e` first.'
    $errors += 'Arc missing'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $arcOut = (& $arcExe 2>&1) | ForEach-Object { "$_" }
    $ErrorActionPreference = $prev
    $arcOut | ForEach-Object { Write-Host $_ }
    $arc = Parse-ReqS $arcOut
    if ($arc.ContainsKey('adv_async_io_throughput')) {
        $results['Arc'] = $arc['adv_async_io_throughput']
    } else {
        $errors += 'Arc parse failed'
    }
}

# ---- 2. .NET ----
Write-Host ''
Write-Host '===== 2. .NET (RandomAccess.ReadAsync / true IOCP) ====='
$dotnetCmd = Get-Command dotnet -ErrorAction SilentlyContinue
if (-not $dotnetCmd) {
    Write-Host 'Missing SDK: dotnet not found.'
    $errors += '.NET missing'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $proj = Join-Path $Root 'scripts/bench/async-io-dotnet-cmp\AsyncIoDotnetCmp.csproj'
    $netOut = (& dotnet run -c Release --project $proj --no-launch-profile 2>&1) | ForEach-Object { "$_" }
    $ErrorActionPreference = $prev
    $netOut | ForEach-Object { Write-Host $_ }
    $net = Parse-ReqS $netOut
    if ($net.ContainsKey('async_io_dotnet')) {
        $results['.NET'] = $net['async_io_dotnet']
    } else {
        $errors += '.NET parse failed'
    }
}

# ---- 3. Rust ----
Write-Host ''
Write-Host '===== 3. Rust (std thread pool + FileExt::seek_read) ====='
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    Write-Host 'Missing toolchain: cargo not found.'
    $errors += 'Rust missing'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $rustProj = Join-Path $Root 'scripts/bench/async-io-rust-cmp'
    $buildRaw = (& cargo build --release --manifest-path (Join-Path $rustProj 'Cargo.toml') 2>&1) | ForEach-Object { "$_" }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($buildExit -ne 0) {
        $buildRaw | ForEach-Object { Write-Host $_ }
        $errors += 'Rust build failed'
    } else {
        $rustExe = Join-Path $rustProj 'target\release\arc_async_io_rust.exe'
        $prev = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $rustOut = (& $rustExe 2>&1) | ForEach-Object { "$_" }
        $ErrorActionPreference = $prev
        $rustOut | ForEach-Object { Write-Host $_ }
        $rust = Parse-ReqS $rustOut
        if ($rust.ContainsKey('async_io_rust')) {
            $results['Rust'] = $rust['async_io_rust']
        } else {
            $errors += 'Rust parse failed'
        }
    }
}

# ---- 4. Go ----
Write-Host ''
Write-Host '===== 4. Go (goroutine + os.File.ReadAt) ====='
$goCmd = Get-Command go -ErrorAction SilentlyContinue
if (-not $goCmd) {
    Write-Host 'Missing toolchain: go not found.'
    $errors += 'Go missing'
} else {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $goProj = Join-Path $Root 'scripts/bench/async-io-go-cmp'
    $goOut = (& go run (Join-Path $goProj 'main.go') 2>&1) | ForEach-Object { "$_" }
    $ErrorActionPreference = $prev
    $goOut | ForEach-Object { Write-Host $_ }
    $go = Parse-ReqS $goOut
    if ($go.ContainsKey('async_io_go')) {
        $results['Go'] = $go['async_io_go']
    } else {
        $errors += 'Go parse failed'
    }
}

# ---- 5. summary table ----
Write-Host ''
Write-Host '===== 5. Arc vs .NET / Rust / Go (same machine, Windows) ====='
Write-Host '| language | async model | req/s | vs Arc |'
Write-Host '|----------|-------------|-------|--------|'
if (-not $results.ContainsKey('Arc')) {
    Write-Host 'Arc baseline missing - cannot compute ratios.'
} else {
    $arc = $results['Arc']
    foreach ($lang in @('.NET', 'Rust', 'Go')) {
        if ($results.ContainsKey($lang)) {
            $reqs = $results[$lang]
            $ratio = if ($reqs -gt 0) { $arc / $reqs } else { [double]::PositiveInfinity }
            $model = switch ($lang) {
                '.NET' { 'true IOCP' }
                'Rust' { 'thread pool' }
                'Go'   { 'goroutine+ReadAt' }
            }
            Write-Host ("| {0} | {1} | {2:0} | {3:0.00}x |" -f $lang, $model, $reqs, $ratio)
        } else {
            Write-Host ("| {0} | - | (missing) | - |" -f $lang)
        }
    }
}

Write-Host ''
Write-Host "Arc baseline: $([int]($results['Arc'])) req/s (IOCP, this machine)"
Write-Host 'Policy: same-machine Windows anchors only. No industry-leadership claim.'
Write-Host 'Interpretation: Arc and .NET both use true IOCP (directly comparable); Rust/Go use blocking thread pools on Windows (their runtime file-IO model).'
if ($errors.Count -gt 0 -and $results.Count -le 1) {
    Write-Host ("errors: {0}" -f ($errors -join '; '))
    exit 2
}
exit 0
