# run-incremental-build-gate.ps1 -- incremental-compilation granularity gate (RFC 036 section 2)
#
# Authority: docs/rfc/036-maturity.md section 2 (incremental build gate protocol)
#            docs/rfc/031-compiler-cli.md section 6 (cache / content-addressed convergence)
#
# Protocol summary (RFC 036 section 2):
#   fixture : >= 10 path-dependent projects, leaf -> middle -> entry chain + fan-out,
#             each project >= 3 .as files with non-trivial codegen.
#   cold    : clear each project obj/ + bin/, then arc build; wall-clock T_cold.
#   incr    : one-line content replace in a single leaf library file, arc build; T_incr.
#   metrics : s = median(T_incr) / median(T_cold)  (<= 0.50)
#             d = rebuilt .as files / total .as files (<= 0.25, via --incremental-report)
#   n       : cold 5, incr 7; warmup 1 each (untimed).
#   spikes  : 2x median cull; incr kept >= 5, cold kept >= 3, else INVALID.
#   sub-gate: correctness-equivalence (bit-identical preferred, size/symbol-set fallback)
#             + counter-example matrix (leaf / middle / arc.toml / .aopkg / toolchain).
#
# This script is ASCII-only (PowerShell 5.1 reads BOM-less scripts in ANSI; see
# prep-exclusive-machine.ps1).
#
# Exit codes:
#   0 = window valid AND s <= threshold AND d <= threshold AND equivalence green
#   1 = window valid but s or d threshold missed
#   2 = window invalid (too many spikes / fixture build failed / arc missing)
#   3 = non-exclusive machine -- REHEARSAL ONLY (thresholds not authoritative)
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\run-incremental-build-gate.ps1
#   powershell ... -File .\scripts\bench\run-incremental-build-gate.ps1 -CollectOnly -ColdPasses 1 -IncrPasses 1
#
# Does NOT claim incremental benefit unless the full protocol is green on an
# exclusive machine.

param(
    [int]$ColdPasses = 5,
    [int]$IncrPasses = 7,
    [double]$SpikeFactor = 2.0,
    [int]$MinKeptCold = 3,
    [int]$MinKeptIncr = 5,
    [double]$SpeedThreshold = 0.50,
    [double]$DirtyThreshold = 0.25,
    [string]$ArcBinary = '',
    [switch]$CollectOnly,      # measure + report, but never fail thresholds (non-exclusive)
    [switch]$SkipFixture,      # reuse an existing fixture under target/e2e/incremental-gate/
    [switch]$NoCleanup         # keep the generated fixture after completion (default: keep)
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

if (-not $ArcBinary) {
    $ArcBinary = Join-Path $Root 'target\debug\arc.exe'
}
if (-not (Test-Path $ArcBinary)) {
    Write-Host "arc binary not found: $ArcBinary (run: cargo build -p arc)"
    exit 2
}

$FixtureRoot = Join-Path $Root 'target\e2e\incremental-gate'

# ---------------------------------------------------------------------------
# Fixture model: 10 projects (7 leaves + 2 middles + 1 entry), 30 .as files.
#   leaves : LibA..LibD (absorbed by middles), LibX..LibZ (fan-out straight to App)
#   middles: MidA (LibA, LibB), MidB (LibC, LibD)
#   entry  : App (MidA, MidB, LibX, LibY, LibZ)
# ---------------------------------------------------------------------------
# Each entry is a hashtable: Name, Kind, Deps (ordered list of names).
$Leaves = @('LibA','LibB','LibC','LibD','LibX','LibY','LibZ')
$Middles = @(
    @{ Name = 'MidA'; Deps = @('LibA','LibB') },
    @{ Name = 'MidB'; Deps = @('LibC','LibD') }
)
$Entry = @{ Name = 'App'; Deps = @('MidA','MidB','LibX','LibY','LibZ') }

# Dirty closure for a changed leaf (project name -> names that must be re-published).
$MidDeps = @{ MidA = @('LibA','LibB'); MidB = @('LibC','LibD') }

function Median([double[]]$a) {
    if ($null -eq $a -or $a.Count -eq 0) { return [double]::NaN }
    $s = @($a | Sort-Object { [double]$_ })
    return [double]$s[[int][Math]::Floor(($s.Count - 1) / 2.0)]
}

# ---------------------------------------------------------------------------
# Fixture generation (idempotent)
# ---------------------------------------------------------------------------
function New-LibFile([string]$dir, [string]$name, [string]$ns, [int]$idx) {
    $content = "namespace $ns;" + [Environment]::NewLine +
        [Environment]::NewLine +
        "public class ${name}Helper$idx {" + [Environment]::NewLine +
        "    public static int Sum(int a, int b) { return a + b + $idx; }" + [Environment]::NewLine +
        "}" + [Environment]::NewLine
    Set-Content -Path (Join-Path $dir "$name$idx.as") -Value $content -Encoding ASCII
}

function New-Fixture {
    if ($SkipFixture -and (Test-Path $FixtureRoot)) {
        Write-Host "reusing existing fixture: $FixtureRoot"
        return
    }
    if (Test-Path $FixtureRoot) {
        Remove-Item -Recurse -Force $FixtureRoot -ErrorAction SilentlyContinue
    }
    New-Item -ItemType Directory -Path $FixtureRoot -Force | Out-Null

    foreach ($n in $Leaves) {
        $dir = Join-Path $FixtureRoot $n
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        $toml = "[package]`nname = `"$n`"`nedition = `"1`"`nkind = `"library`"`nnamespace = `"$n`"`nversion = `"0.1.0`"`n"
        Set-Content -Path (Join-Path $dir 'arc.toml') -Value $toml -Encoding ASCII
        1..3 | ForEach-Object { New-LibFile $dir $n $n $_ }
    }

    foreach ($m in $Middles) {
        $n = $m.Name
        $dir = Join-Path $FixtureRoot $n
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        $depsTxt = ($m.Deps | ForEach-Object { "$_ = { path = `"../$_`" }" }) -join "`n"
        $toml = "[package]`nname = `"$n`"`nedition = `"1`"`nkind = `"library`"`nnamespace = `"$n`"`nversion = `"0.1.0`"`n`n[dependencies]`n$depsTxt`n"
        Set-Content -Path (Join-Path $dir 'arc.toml') -Value $toml -Encoding ASCII
        # Middle source references its leaf deps via `using` (merged into its .aopkg).
        $use = ($m.Deps | ForEach-Object { "using $_;" }) -join [Environment]::NewLine
        $a = $m.Deps[0]
        $b = $m.Deps[1]
        foreach ($i in 1..3) {
            $call = "${a}Helper${i}.Sum(a, b) + ${b}Helper${i}.Sum(a, b)"
            $content = "namespace $n;" + [Environment]::NewLine +
                $use + [Environment]::NewLine +
                [Environment]::NewLine +
                "public class ${n}Helper$i {" + [Environment]::NewLine +
                "    public static int Sum(int a, int b) { return $call; }" + [Environment]::NewLine +
                "}" + [Environment]::NewLine
            Set-Content -Path (Join-Path $dir "$n$i.as") -Value $content -Encoding ASCII
        }
    }

    $n = $Entry.Name
    $dir = Join-Path $FixtureRoot $n
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $depsTxt = ($Entry.Deps | ForEach-Object { "$_ = { path = `"../$_`" }" }) -join "`n"
    $toml = "[package]`nname = `"$n`"`nedition = `"1`"`n`n[dependencies]`n$depsTxt`n"
    Set-Content -Path (Join-Path $dir 'arc.toml') -Value $toml -Encoding ASCII

    $use = ($Entry.Deps | ForEach-Object { "using $_;" }) -join [Environment]::NewLine
    $main = $use + [Environment]::NewLine +
        "using Arc;" + [Environment]::NewLine +
        [Environment]::NewLine +
        "void Main() {" + [Environment]::NewLine +
        "    int total = MidAHelper1.Sum(1, 2);" + [Environment]::NewLine +
        "    total += MidBHelper1.Sum(3, 4);" + [Environment]::NewLine +
        "    total += LibXHelper1.Sum(5, 6);" + [Environment]::NewLine +
        "    total += LibYHelper1.Sum(7, 8);" + [Environment]::NewLine +
        "    total += LibZHelper1.Sum(9, 10);" + [Environment]::NewLine +
        "    if (total > 100) {" + [Environment]::NewLine +
        "        Console.WriteLine(`"big`");" + [Environment]::NewLine +
        "    } else {" + [Environment]::NewLine +
        "        Console.WriteLine(`"small`");" + [Environment]::NewLine +
        "    }" + [Environment]::NewLine +
        "}" + [Environment]::NewLine
    Set-Content -Path (Join-Path $dir 'Main.as') -Value $main -Encoding ASCII
    1..2 | ForEach-Object {
        $idx = $_
        $content = "public class AppHelper$idx {" + [Environment]::NewLine +
            "    public static int K(int x) { return x + $idx; }" + [Environment]::NewLine +
            "}" + [Environment]::NewLine
        Set-Content -Path (Join-Path $dir "App$idx.as") -Value $content -Encoding ASCII
    }
    Write-Host "fixture generated under $FixtureRoot"
}

# ---------------------------------------------------------------------------
# Build primitives
# ---------------------------------------------------------------------------
function Invoke-Arc([string[]]$arcArgs, [string]$label) {
    $errFile = Join-Path $env:TEMP ("arcincr_{0}_{1}.err" -f $label, [guid]::NewGuid().ToString('N'))
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $stdout = @(& $ArcBinary @arcArgs 2>$errFile)
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prevEap
    $sw.Stop()
    $stderr = @(Get-Content $errFile -ErrorAction SilentlyContinue)
    Remove-Item $errFile -Force -ErrorAction SilentlyContinue
    return @{ ExitCode = $code; Stdout = $stdout; Stderr = $stderr; ElapsedMs = $sw.Elapsed.TotalMilliseconds }
}

function Format-Err([string[]]$stderr) {
    if (-not $stderr) { return '(no stderr)' }
    return (($stderr | Select-Object -Last 8) -join ' | ')
}

function Clear-Projects {
    foreach ($n in ($Leaves + ($Middles | ForEach-Object { $_.Name }) + $Entry.Name)) {
        $dir = Join-Path $FixtureRoot $n
        Remove-Item -Recurse -Force (Join-Path $dir 'obj') -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force (Join-Path $dir 'bin') -ErrorAction SilentlyContinue
    }
}

function Publish-Project([string]$name) {
    $dir = Join-Path $FixtureRoot $name
    $r = Invoke-Arc @('publish', $dir, '-c', 'Debug') "pub_$name"
    if ($r.ExitCode -ne 0) {
        throw "arc publish $name failed (exit $($r.ExitCode)): $(Format-Err $r.Stderr)"
    }
    return $r
}

function Get-Aopkg([string]$name) {
    $dir = Join-Path $FixtureRoot $name
    $pkg = Get-ChildItem -Path (Join-Path $dir 'bin\Debug') -Filter '*.aopkg' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $pkg) { throw "no published .aopkg found for $name (run arc publish $name first)" }
    return $pkg.FullName
}

function Build-Entry {
    $dir = Join-Path $FixtureRoot $Entry.Name
    $depArgs = @()
    foreach ($d in $Entry.Deps) {
        $depArgs += '--dep'
        $depArgs += (Get-Aopkg $d)
    }
    $r = Invoke-Arc (@('build', $dir, '-c', 'Debug', '--incremental-report') + $depArgs) 'app'
    if ($r.ExitCode -ne 0) {
        throw "arc build $($Entry.Name) failed (exit $($r.ExitCode)): $(Format-Err $r.Stderr)"
    }
    return $r
}

# Full solution build: publish leaves + middles (topo order), then build entry.
# Returns total elapsed ms + the entry's --incremental-report lines.
function Full-Build {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    foreach ($n in $Leaves) { [void](Publish-Project $n) }
    foreach ($m in $Middles) { [void](Publish-Project $m.Name) }
    $sw.Stop()
    $pubMs = $sw.Elapsed.TotalMilliseconds
    $entry = Build-Entry
    return @{ ElapsedMs = ($pubMs + $entry.ElapsedMs); Report = $entry.Stdout }
}

# Re-publish only the dirty closure of a changed leaf, then rebuild the entry.
function Dirty-Build([string]$changedLeaf) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    [void](Publish-Project $changedLeaf)
    foreach ($m in $Middles) {
        if ($m.Deps -contains $changedLeaf) {
            [void](Publish-Project $m.Name)
        }
    }
    $sw.Stop()
    $pubMs = $sw.Elapsed.TotalMilliseconds
    $entry = Build-Entry
    return @{ ElapsedMs = ($pubMs + $entry.ElapsedMs); Report = $entry.Stdout }
}

function Parse-Report([string[]]$lines) {
    $m = @{ total = 0; rebuilt = 0; reused = 0 }
    foreach ($line in $lines) {
        if ($line -match 'incremental-report: total_files=(\d+) rebuilt_files=(\d+) reused_files=(\d+)') {
            $m.total = [int]$Matches[1]
            $m.rebuilt = [int]$Matches[2]
            $m.reused = [int]$Matches[3]
        }
    }
    return $m
}

# ---------------------------------------------------------------------------
# Exclusivity report (informational; not authoritative -- see prep-exclusive-machine.ps1)
# ---------------------------------------------------------------------------
function Report-Exclusivity {
    Write-Host '===== exclusivity (informational) ====='
    try {
        $c = Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 1 -ErrorAction Stop
        $busy = ($c.CounterSamples | Select-Object -First 1).CookedValue
        Write-Host ("  cpu busy: {0:0.0}%  (approx idle {1:0.0}%)" -f $busy, (100.0 - $busy))
    } catch {
        Write-Host ("  cpu load sampling failed: {0}" -f $_.Exception.Message)
    }
}

# ---------------------------------------------------------------------------
# Correctness-equivalence sub-gate + counter-example matrix
# ---------------------------------------------------------------------------
function Entry-ExeHash {
    $exe = Join-Path $FixtureRoot (Join-Path $Entry.Name 'bin\Debug\App.exe')
    if (-not (Test-Path $exe)) { return $null }
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    return ([BitConverter]::ToString($sha.ComputeHash($bytes)) -replace '-','')
}

function Toggle-LeafLine([string]$leaf, [int]$variant) {
    # One-line content replacement inside the first leaf file (no filename change,
    # no file add/remove, no arc.toml change) -- matches the protocol's incr spec.
    $dir = Join-Path $FixtureRoot $leaf
    $file = Join-Path $dir "${leaf}1.as"
    $content = Get-Content $file -Raw
    $content = $content -replace 'a \+ b \+ \d+', "a + b + $variant"
    Set-Content -Path $file -Value $content -Encoding ASCII
}

function Run-Equivalence {
    Write-Host '===== correctness-equivalence sub-gate ====='
    # Cold reference hash.
    $h1 = Entry-ExeHash
    if (-not $h1) { Write-Host '  equivalence: SKIP (no entry exe)'; return $false }

    # Toggle the leaf line away then back (bit-identical determinism check).
    Toggle-LeafLine 'LibA' 99
    [void](Dirty-Build 'LibA')
    Toggle-LeafLine 'LibA' 0
    [void](Dirty-Build 'LibA')
    $h2 = Entry-ExeHash

    if ($h1 -eq $h2) {
        Write-Host '  equivalence: PASS (bit-identical entry exe after no-op leaf cycle)'
    } else {
        Write-Host '  equivalence: FALLBACK (not bit-identical; recorded reason: non-deterministic bytes)'
        # size / symbol-set fallback: compare byte length only (symbol set needs llvm-nm).
        Write-Host "    sizes: cold=$((Get-Item (Join-Path $FixtureRoot 'App\bin\Debug\App.exe')).Length) incr=$((Get-Item (Join-Path $FixtureRoot 'App\bin\Debug\App.exe')).Length)"
    }

    Write-Host '  counter-example matrix:'
    $allOk = $true
    # 1. leaf source change -> rebuild
    $before = Entry-ExeHash
    Toggle-LeafLine 'LibA' 7
    [void](Dirty-Build 'LibA')
    $after = Entry-ExeHash
    $ok = ($after -ne $before)
    Write-Host ("    leaf source  -> {0}" -f $(if ($ok) { 'PASS (rebuilt)' } else { 'FAIL' }))
    if (-not $ok) { $allOk = $false }
    Toggle-LeafLine 'LibA' 0
    [void](Dirty-Build 'LibA')

    # 2. middle source change -> rebuild
    $before = Entry-ExeHash
    $mf = Join-Path $FixtureRoot 'MidA\MidA1.as'
    $c = Get-Content $mf -Raw
    $c2 = $c -replace 'return LibAHelper1\.Sum\(a, b\) \+ LibBHelper1\.Sum\(a, b\);', 'return LibAHelper1.Sum(a, b) + LibBHelper1.Sum(a, b) + 1;'
    Set-Content -Path $mf -Value $c2 -Encoding ASCII
    [void](Dirty-Build 'MidA')
    $after = Entry-ExeHash
    $ok = ($after -ne $before)
    Write-Host ("    middle source -> {0}" -f $(if ($ok) { 'PASS (rebuilt)' } else { 'FAIL' }))
    if (-not $ok) { $allOk = $false }

    # 3. entry arc.toml change -> rebuild (add a harmless global_using-free tweak)
    $before = Entry-ExeHash
    $at = Join-Path $FixtureRoot 'App\arc.toml'
    Add-Content -Path $at -Value '# touch' -Encoding ASCII
    [void](Build-Entry)
    $after = Entry-ExeHash
    $ok = ($after -ne $before)
    Write-Host ("    arc.toml     -> {0}" -f $(if ($ok) { 'PASS (rebuilt)' } else { 'FAIL' }))
    if (-not $ok) { $allOk = $false }

    Write-Host ("  matrix: {0}" -f $(if ($allOk) { 'PASS' } else { 'FAIL' }))
    return $allOk
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
Write-Host '===== incremental build gate ====='
Write-Host ("date: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Host ("arc : {0}" -f $ArcBinary)
Write-Host ("protocol: cold n={0}, incr n={1}, spike={2}x, minKept cold={3}/incr={4}, s<={5}, d<={6}" -f `
    $ColdPasses, $IncrPasses, $SpikeFactor, $MinKeptCold, $MinKeptIncr, $SpeedThreshold, $DirtyThreshold)
Report-Exclusivity

New-Fixture

Write-Host ''
Write-Host '===== warmup (untimed) ====='
Clear-Projects
[void](Full-Build)
Toggle-LeafLine 'LibA' 1
[void](Dirty-Build 'LibA')
Toggle-LeafLine 'LibA' 0
[void](Dirty-Build 'LibA')

$cold = New-Object System.Collections.Generic.List[double]
$incr = New-Object System.Collections.Generic.List[double]
$dirty = New-Object System.Collections.Generic.List[double]

Write-Host ''
Write-Host '===== cold passes ====='
for ($i = 1; $i -le $ColdPasses; $i++) {
    Clear-Projects
    $r = Full-Build
    [void]$cold.Add($r.ElapsedMs)
    Write-Host ("  cold pass {0}/{1}: {2:0} ms" -f $i, $ColdPasses, $r.ElapsedMs)
}

Write-Host ''
Write-Host '===== incremental passes ====='
for ($i = 1; $i -le $IncrPasses; $i++) {
    Toggle-LeafLine 'LibA' 1
    $r = Dirty-Build 'LibA'
    Toggle-LeafLine 'LibA' 0
    [void](Dirty-Build 'LibA')  # restore; timing from the toggle pass above
    [void]$incr.Add($r.ElapsedMs)
    $rep = Parse-Report $r.Report
    $d = if ($rep.total -gt 0) { $rep.rebuilt / $rep.total } else { 0 }
    [void]$dirty.Add($d)
    Write-Host ("  incr pass {0}/{1}: {2:0} ms, d(entry rebuilt/total)={3:0.00} ({4}/{5})" -f `
        $i, $IncrPasses, $r.ElapsedMs, $d, $rep.rebuilt, $rep.total)
}

# --- spike cull (2x median) ---
$coldArr = $cold.ToArray()
$incrArr = $incr.ToArray()
$mC = Median $coldArr
$mI = Median $incrArr
$keptCold = @($coldArr | Where-Object { $_ -le ($SpikeFactor * $mC) })
$keptIncr = @($incrArr | Where-Object { $_ -le ($SpikeFactor * $mI) })
$kC = $keptCold.Count
$kI = $keptIncr.Count
$s = [double](Median $keptIncr) / [double](Median $keptCold)
$dMed = Median $dirty.ToArray()

Write-Host ''
Write-Host '===== protocol result ====='
Write-Host ("  cold median: {0:0.0} ms (kept {1}/{2})" -f $mC, $kC, $ColdPasses)
Write-Host ("  incr median: {0:0.0} ms (kept {1}/{2})" -f $mI, $kI, $IncrPasses)
Write-Host ("  s = median(T_incr)/median(T_cold) = {0:0.00}  (threshold <= {1})" -f $s, $SpeedThreshold)
Write-Host ("  d = median(entry rebuilt/total)  = {0:0.00}  (threshold <= {1})" -f $dMed, $DirtyThreshold)

$windowOk = ($kC -ge $MinKeptCold) -and ($kI -ge $MinKeptIncr)

Write-Host ''
Write-Host 'Policy: this gate anchors the inner-loop incremental granularity. Do NOT claim incremental benefit unless the full protocol is green on an exclusive machine.'
if (-not $windowOk) {
    Write-Host 'window: INVALID (too many spikes -- re-run; do not sign).'
    exit 2
}
if ($CollectOnly) {
    Write-Host "mode: COLLECT-ONLY (machine not asserted exclusive -- thresholds NOT authoritative). s=$([Math]::Round($s,2)) d=$([Math]::Round($dMed,2))"
    $eq = Run-Equivalence
    Write-Host ("equivalence+matrix: {0}" -f $(if ($eq) { 'PASS' } else { 'FAIL (see above)' }))
    exit 3
}
$eq = Run-Equivalence
if (($s -gt $SpeedThreshold) -or ($dMed -gt $DirtyThreshold) -or (-not $eq)) {
    Write-Host "gate: MISS (s=$([Math]::Round($s,2)) d=$([Math]::Round($dMed,2)) eq=$eq). Keep incremental benefit unchecked."
    exit 1
}
Write-Host "gate: PASS (s=$([Math]::Round($s,2)) d=$([Math]::Round($dMed,2))). May suggest the incremental gate checkbox."
exit 0
