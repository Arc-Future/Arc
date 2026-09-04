# prep-exclusive-machine.ps1 - Prepare this machine for a v1.0 exclusive-machine
# performance re-test (RFC 099 / RFC 024 / RFC 034). Reduces scheduling & thermal
# noise so the machine is as close to "exclusive benchmark host" as possible.
#
# Authority:
#   docs/chapters/08-rfcs/topics/099-foundation-reliability-charter.md (宣称纪律)
#   docs/chapters/08-rfcs/024-maturity-perf.md (exclusive-machine protocol)
#   scripts/bench/run-024-exclusive-regate.ps1 (Phase 1 exclusive detection)
#
# What it does:
#   1. Switch the active Windows power plan to "High performance" (reduces
#      frequency-scaling / boost variance) when admin is available.
#   2. Optionally stop known noise services (PC Manager) that spin CPU in the
#      background. Default = dry-run report only; use -Apply to actually stop.
#   3. Report current CPU idle load and top CPU consumers so you can judge
#      whether the machine qualifies as exclusive (the regate Phase 1 check is
#      the authoritative gate; this is informational).
#
# Usage (repo root, PowerShell as admin for power-plan / service changes):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\bench\prep-exclusive-machine.ps1 -Apply
#   powershell ... -File .\scripts\bench\prep-exclusive-machine.ps1            # report only
#
# NOTE: Keep this file ASCII-only (no CJK comments). PowerShell 5.1 reads
# BOM-less scripts in the ANSI code page; ASCII avoids encoding corruption.

param(
    [switch]$Apply,          # actually apply High Performance + stop noise services
    [switch]$RestoreBalanced # switch back to Balanced power plan (cleanup)
)

$ErrorActionPreference = 'Continue'
$prevEap = $ErrorActionPreference

Write-Host '===== exclusive machine prep ====='
Write-Host ("date: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))

# --- power plan ---
$balanced  = '381b4222-f694-41f0-9685-ff5bb260df2e'  # Balanced (standard GUID)
$highPerf  = '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c'  # High performance (standard GUID)
$current = (powercfg /getactivescheme) -join ' '

if ($RestoreBalanced) {
    Write-Host 'Restoring Balanced power plan ...'
    powercfg /setactive $balanced
    Write-Host ("  now: {0}" -f ((powercfg /getactivescheme) -join ' '))
    exit 0
}

if ($Apply) {
    Write-Host 'Applying High performance power plan ...'
    $null = powercfg /setactive $highPerf 2>&1
    Write-Host ("  now: {0}" -f ((powercfg /getactivescheme) -join ' '))
} else {
    Write-Host ("  active: {0}" -f $current)
    Write-Host '  (dry-run; use -Apply to switch to High performance)'
}

# --- noise services (report or stop) ---
# PC Manager spins background CPU; not needed for benchmarking.
$noiseServices = @('MSPCManagerService')
foreach ($svc in $noiseServices) {
    $s = Get-Service -Name $svc -ErrorAction SilentlyContinue
    if (-not $s) { continue }
    if ($Apply) {
        if ($s.Status -eq 'Running') {
            Stop-Service -Name $svc -Force -ErrorAction SilentlyContinue
            Write-Host ("  service {0}: requested stop (was {1})" -f $svc, $s.Status)
        } else {
            Write-Host ("  service {0}: not running ({1})" -f $svc, $s.Status)
        }
    } else {
        Write-Host ("  service {0}: {1} (dry-run; -Apply to stop)" -f $svc, $s.Status)
    }
}

# --- CPU idle load sample (informational; authoritative gate is regate Phase 1) ---
Write-Host ''
Write-Host '===== current CPU load ===='
try {
    $loads = New-Object System.Collections.Generic.List[double]
    for ($i = 0; $i -lt 3; $i++) {
        $c = Get-Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 1 -ErrorAction Stop
        foreach ($smp in $c.CounterSamples) { [void]$loads.Add([double]$smp.CookedValue) }
    }
    $idle = 100.0 - (($loads | Measure-Object -Average).Average)
    Write-Host ("  avg busy: {0:0.0}%  (approx idle {1:0.0}%)" -f (100.0 - $idle), $idle)
} catch {
    Write-Host ("  cpu load sampling failed: {0}" -f $_.Exception.Message)
}

Write-Host ''
Write-Host '===== top CPU consumers (informational) ===='
Get-Process | Sort-Object CPU -Descending | Select-Object -First 10 `
    ProcessName,Id,@{n='CPU(s)';e={[math]::Round($_.CPU,1)}},@{n='WS(MB)';e={[math]::Round($_.WorkingSet64/1MB,0)}} |
    Format-Table -AutoSize

Write-Host ''
Write-Host 'NOTE: the authoritative exclusive-machine gate is run-024-exclusive-regate.ps1'
Write-Host 'Phase 1 (process + CPU load). A shared/dev laptop with the IDE + agent host'
Write-Host 'running may NOT qualify. If it aborts (exit 3), the data is REHEARSAL ONLY.'
