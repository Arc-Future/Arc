# github-sync.ps1 - incremental mirror of the internal repository into the
# public GitHub repo (normal fast-forward commits; no history rewrite).
#
# Complements github-export.ps1: use -InitGit once for the clean bootstrap
# snapshot, then run this script to publish internal changes as regular
# commits. Only git-tracked files are mirrored; scripts/release/export-
# exclusions.txt (shared with github-export.ps1) removes internal-process
# assets (docs/plan.md, docs/discuss.md, docs/reviews/, docs/rfc/proposals/).
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-sync.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-sync.ps1 -Message "sync: release infra update"

param(
    [string]$Repo = "Arc-Future/Arc",
    [string]$SyncDir = "",
    [string]$Message = "",
    [string]$AuthorName = "LUSIDA (Start)",
    [string]$AuthorEmail = "209404271+lusida2026@users.noreply.github.com"
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $SyncDir) { $SyncDir = Join-Path $repoRoot "target\github-sync\arc" }

# --- shared exclusion list (same file as github-export.ps1) ---
$exclusionFile = Join-Path $PSScriptRoot "export-exclusions.txt"
$ExcludeExact = @()
$ExcludeDirs = @()
foreach ($line in [System.IO.File]::ReadAllLines($exclusionFile)) {
    $l = $line.Trim()
    if ($l -eq '' -or $l.StartsWith('#')) { continue }
    if ($l.EndsWith('/')) { $ExcludeDirs += $l } else { $ExcludeExact += $l }
}
function Test-Excluded([string]$f) {
    foreach ($d in $ExcludeDirs) { if ($f.StartsWith($d)) { return $true } }
    foreach ($e in $ExcludeExact) { if ($f -eq $e) { return $true } }
    return $false
}

# --- 1. clone (first run) or refresh the public clone ---
# PS 5.1: git writes progress to stderr; under EAP=Stop that becomes a
# terminating NativeCommandError even on success - quiet flags + Continue
# locally, with explicit $LASTEXITCODE checks.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
if (-not (Test-Path (Join-Path $SyncDir ".git"))) {
    if (Test-Path $SyncDir) { Remove-Item $SyncDir -Recurse -Force }
    git clone -q "https://github.com/$Repo.git" $SyncDir 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { $ErrorActionPreference = $prevEap; throw "clone https://github.com/$Repo.git failed" }
}
git -C $SyncDir fetch -q origin 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { $ErrorActionPreference = $prevEap; throw "git fetch failed in $SyncDir" }
git -C $SyncDir reset -q --hard origin/main 2>&1 | Out-Null
git -C $SyncDir clean -q -fd 2>&1 | Out-Null
git -C $SyncDir config user.name $AuthorName
git -C $SyncDir config user.email $AuthorEmail
$ErrorActionPreference = $prevEap

# --- 2. materialize internal tracked files (minus exclusions) ---
$wanted = @()
foreach ($f in (git ls-files)) {
    if (-not (Test-Excluded $f)) { $wanted += $f }
}
foreach ($f in $wanted) {
    $src = Join-Path $repoRoot $f
    $dst = Join-Path $SyncDir ($f -replace '/', '\')
    $dstDir = Split-Path $dst -Parent
    if (-not (Test-Path $dstDir)) { New-Item -ItemType Directory -Force -Path $dstDir | Out-Null }
    Copy-Item $src $dst -Force
}

# --- 3. orphan removal: still tracked in the public clone but gone internally ---
$orphans = @(git -C $SyncDir ls-files | Where-Object { -not ($wanted -contains $_) })
foreach ($f in $orphans) {
    git -C $SyncDir rm -q -f -- $f | Out-Null
    Write-Host "==> removed orphan from public repo: $f"
}

# --- 4. commit + push (no-op when nothing changed) ---
git -C $SyncDir add -A
$dirty = git -C $SyncDir status --porcelain
if (-not $dirty) {
    Write-Host "==> public repo up to date with the internal snapshot"
    return
}
if (-not $Message) { $Message = "sync: internal snapshot $(Get-Date -Format 'yyyy-MM-dd HH:mm')" }
$ErrorActionPreference = 'Continue'
git -C $SyncDir commit -q -m $Message 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { $ErrorActionPreference = 'Stop'; throw "git commit failed in $SyncDir" }
$pushOut = git -C $SyncDir push origin main 2>&1
if ($LASTEXITCODE -ne 0) { $ErrorActionPreference = 'Stop'; throw "git push failed: $pushOut" }
$ErrorActionPreference = 'Stop'
$hash = git -C $SyncDir rev-parse --short HEAD
Write-Host "==> synced to github.com/$Repo (main @ $hash)"
