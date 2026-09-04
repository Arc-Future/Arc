# Remove test/debug pollution from the Arc repo working tree.
# Does NOT modify .gitignore — fixes the source of writes, not git tracking.

param(
    [switch]$WhatIf,
    [switch]$SkipProcessKill
)

$ErrorActionPreference = 'Stop'

function Get-RepoRoot {
    $dir = $PSScriptRoot
    while ($dir) {
        if (Test-Path (Join-Path $dir 'Cargo.toml') -PathType Leaf) {
            $crates = Join-Path $dir 'crates'
            if (Test-Path $crates -PathType Container) { return $dir }
        }
        $parent = Split-Path $dir -Parent
        if (-not $parent -or $parent -eq $dir) { break }
        $dir = $parent
    }
    throw 'Arc repo root not found'
}

function Test-IsPollutionWriter {
    param([string]$CommandLine, [string]$RepoRoot)
    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return $false }
    if ($CommandLine -notlike "*$RepoRoot*") { return $false }
    # Only this repo root — not other worktrees that share a parent path
    $escaped = [regex]::Escape($RepoRoot)
    if ($CommandLine -notmatch $escaped) { return $false }

    $patterns = @(
        '(?i)Tee-Object\b',
        '(?i)(Out-File|Set-Content|Add-Content)\b',
        '(?i)(\*?>\s*|\|\s*Out-File\s+)',
        '(?i)--obj-dir\s+\S*(?:[/\\])(obj-|bin-|target-|\.)',
        '(?i)--obj-dir\s+(obj-|bin-|target-|\.)',
        '(?i)[/\\]\.tmp_',
        '(?i)[/\\]tmp_[\w.-]*',
        '(?i)[/\\]obj-[a-zA-Z]',
        '(?i)[/\\]bin-[a-zA-Z]',
        '(?i)(^|[\s''"`>])(target-h\d+|/target-h\d+|\\target-h\d+)',
        '(?i)[/\\]target-[a-zA-Z]'
    )
    foreach ($p in $patterns) {
        if ($CommandLine -match $p) { return $true }
    }
    return $false
}

function Stop-PollutionWriters {
    param([string]$RepoRoot)
    $killed = 0
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | ForEach-Object {
        if (-not (Test-IsPollutionWriter -CommandLine $_.CommandLine -RepoRoot $RepoRoot)) { return }
        Write-Host "Stopping pollution writer PID $($_.ProcessId) ($($_.Name))"
        if (-not $WhatIf) {
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
        $killed++
    }
    return $killed
}

function Measure-Pollution {
    param([string]$RepoRoot)
    $items = [System.Collections.Generic.List[string]]::new()

    function Add-IfExists([string]$Path) {
        if (Test-Path -LiteralPath $Path) {
            $items.Add($Path) | Out-Null
        }
    }

    # Root dot-prefixed and plain tmp_
    Get-ChildItem -LiteralPath $RepoRoot -Force -ErrorAction SilentlyContinue | ForEach-Object {
        if ($_.PSIsContainer) {
            if ($_.Name -like '.tmp-*' -or $_.Name -like 'target-*' -or $_.Name -eq '_repro') { Add-IfExists $_.FullName }
        }
        elseif ($_.Name -like '.tmp_*' -or $_.Name -like 'tmp_*' -or $_.Name -like 'tmp-_*') {
            Add-IfExists $_.FullName
        }
        elseif ($_.Name -like 'debug-*.md') {
            # 根目录一次性调试会话备忘（非正式文档）
            Add-IfExists $_.FullName
        }
        elseif ($_.Name -match '^(test_|arc_|stderr|stdout|err\.txt|out\.txt|\.test_output)' -and $_.Extension -in '.txt', '.err', '.log', '') {
            Add-IfExists $_.FullName
        }
        elseif ($_.Extension -in '.txt', '.err', '.log' -and $_.Name -notin @('Cargo.toml', 'LICENSE.txt', 'README.txt')) {
            # Stray debug redirect at repo root (not tracked docs)
            $rel = $_.FullName.Substring($RepoRoot.Length + 1)
            if ($rel -notmatch '[/\\]') { Add-IfExists $_.FullName }
        }
        elseif ($_.Name -match '^(test_|tmp_)' -and $_.Extension -eq '.as') {
            Add-IfExists $_.FullName
        }
        elseif ($_.Name -like '+$*') {
            Add-IfExists $_.FullName
        }
    }

    # Non-standard obj-*/bin-* under source roots
    @($RepoRoot, (Join-Path $RepoRoot 'examples'), (Join-Path $RepoRoot 'crates'), (Join-Path $RepoRoot 'std')) | ForEach-Object {
        if (-not (Test-Path $_)) { return }
        Get-ChildItem -LiteralPath $_ -Directory -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -notmatch '\\\.git\\' -and ($_.Name -like 'obj-*' -or $_.Name -like 'bin-*') } |
            ForEach-Object { Add-IfExists $_.FullName }
    }

    # Stray *.bin in crates/
    $crates = Join-Path $RepoRoot 'crates'
    if (Test-Path $crates) {
        Get-ChildItem -LiteralPath $crates -Filter '*.bin' -Recurse -File -ErrorAction SilentlyContinue |
            ForEach-Object { Add-IfExists $_.FullName }
    }

    # examples/UnitTest non-standard exe names (UnitTest.h1.exe etc.)
    $ut = Join-Path $RepoRoot 'examples\UnitTest'
    if (Test-Path $ut) {
        Get-ChildItem -LiteralPath $ut -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Extension -eq '.exe' -and $_.Name -match '^UnitTest\..+\.exe$' } |
            ForEach-Object { Add-IfExists $_.FullName }
    }

    return @{ Count = $items.Count; Items = $items }
}

$root = Get-RepoRoot
$removed = 0

function Remove-IfExists {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return }
    if ($WhatIf) {
        Write-Host "Would remove: $Path"
    }
    else {
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "Removed: $Path"
    }
    $script:removed++
}

Write-Host "Repo: $root"
$before = Measure-Pollution -RepoRoot $root
Write-Host "Pollution before: $($before.Count) item(s)"
foreach ($p in $before.Items) { Write-Host "  - $p" }

if (-not $SkipProcessKill) {
    $killed = Stop-PollutionWriters -RepoRoot $root
    Write-Host "Pollution writer processes stopped: $killed"
}

# Root .tmp_* / tmp_* / tmp-* (agent Tee-Object / redirect pollution)
Get-ChildItem -LiteralPath $root -Force -ErrorAction SilentlyContinue | ForEach-Object {
    if ($_.PSIsContainer) {
        if ($_.Name -like '.tmp-*' -or $_.Name -like 'target-*' -or $_.Name -eq '_repro') { Remove-IfExists $_.FullName }
    }
    elseif ($_.Name -like '.tmp_*' -or $_.Name -like 'tmp_*' -or $_.Name -like 'tmp-_*') {
        Remove-IfExists $_.FullName
    }
    elseif ($_.Name -like 'debug-*.md') {
        # 根目录一次性调试会话备忘（非正式文档）
        Remove-IfExists $_.FullName
    }
    elseif ($_.Name -match '^(test_|arc_|stderr|stdout|err\.txt|out\.txt|\.test_output)' -and ($_.Extension -in '.txt', '.err', '.log' -or $_.Extension -eq '')) {
        Remove-IfExists $_.FullName
    }
    elseif ($_.Extension -in '.txt', '.err', '.log') {
        $rel = $_.FullName.Substring($root.Length + 1)
        if ($rel -notmatch '[/\\]' -and $_.Name -notin @('Cargo.toml')) {
            Remove-IfExists $_.FullName
        }
    }
    elseif ($_.Name -match '^(test_|tmp_)' -and $_.Extension -eq '.as') {
        Remove-IfExists $_.FullName
    }
    elseif ($_.Name -like '+$*') {
        Remove-IfExists $_.FullName
    }
}

# obj-*/bin-* under repo root, examples/, crates/, std/
@($root, (Join-Path $root 'examples'), (Join-Path $root 'crates'), (Join-Path $root 'std')) | ForEach-Object {
    if (-not (Test-Path $_)) { return }
    Get-ChildItem -LiteralPath $_ -Directory -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\\.git\\' -and ($_.Name -like 'obj-*' -or $_.Name -like 'bin-*') } |
        ForEach-Object { Remove-IfExists $_.FullName }
}

# Stray *.bin in crates/
Get-ChildItem -LiteralPath (Join-Path $root 'crates') -Filter '*.bin' -Recurse -File -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-IfExists $_.FullName }

# examples/UnitTest non-standard exe (UnitTest.h1.exe etc.)
$ut = Join-Path $root 'examples\UnitTest'
if (Test-Path $ut) {
    Get-ChildItem -LiteralPath $ut -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Extension -eq '.exe' -and $_.Name -match '^UnitTest\..+\.exe$' } |
        ForEach-Object { Remove-IfExists $_.FullName }
}

$after = Measure-Pollution -RepoRoot $root
Write-Host "Done. Items touched: $removed"
Write-Host "Pollution after: $($after.Count) item(s)"
if ($after.Count -gt 0) {
    foreach ($p in $after.Items) { Write-Host "  REMAINING: $p" }
    exit 1
}
