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
    [string]$AuthorEmail = "209404271+lusida2026@users.noreply.github.com",
    [string]$Token = ""
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $SyncDir) { $SyncDir = Join-Path $repoRoot "target\github-sync\arc" }

# Non-interactive GitHub auth: never pop GCM account picker / credential UI.
$env:GCM_INTERACTIVE = 'never'
$env:GIT_TERMINAL_PROMPT = '0'

function Resolve-GitHubToken {
    param([string]$Explicit)
    if ($Explicit) { return $Explicit }
    if ($env:GH_TOKEN) { return $env:GH_TOKEN }
    if ($env:GITHUB_TOKEN) { return $env:GITHUB_TOKEN }
    $session = Join-Path $env:TEMP 'arc-github-pat.session'
    if (Test-Path $session) {
        $t = [System.IO.File]::ReadAllText($session).Trim()
        if ($t) { return $t }
    }
    $credIn = Join-Path $env:TEMP 'arc-github-sync-cred-fill.txt'
    [System.IO.File]::WriteAllText($credIn, "protocol=https`nhost=github.com`n`n")
    $fill = cmd /c "git credential fill < `"$credIn`" 2>nul"
    Remove-Item $credIn -Force -ErrorAction SilentlyContinue
    foreach ($line in ($fill -split "`r?`n")) {
        if ($line -match '^password=(.+)$') { return $Matches[1] }
    }
    return ''
}

function Invoke-GitHubPush {
    param([string]$Dir, [string]$Tok)
    if (-not $Tok) {
        throw "no GitHub token: pass -Token, set GH_TOKEN/GITHUB_TOKEN, or run 'gh auth login --with-token' (non-interactive)"
    }
    # Bypass credential helpers / account UI: Authorization header only.
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("x-access-token:$Tok"))
    cmd /c "git -C `"$Dir`" -c credential.helper= -c http.extraHeader=`"Authorization: Basic $basic`" push origin main"
    if ($LASTEXITCODE -ne 0) { throw "git push failed (exit $LASTEXITCODE)" }
}

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

$gitHubToken = Resolve-GitHubToken -Explicit $Token

# --- 1. clone (first run) or refresh the public clone ---
if (-not (Test-Path (Join-Path $SyncDir ".git"))) {
    if (Test-Path $SyncDir) { Remove-Item $SyncDir -Recurse -Force }
    if ($gitHubToken) {
        $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("x-access-token:$gitHubToken"))
        cmd /c "git -c credential.helper= -c http.extraHeader=`"Authorization: Basic $basic`" clone https://github.com/$Repo.git `"$SyncDir`""
    } else {
        git clone "https://github.com/$Repo.git" $SyncDir
    }
    if ($LASTEXITCODE -ne 0) { throw "clone https://github.com/$Repo.git failed" }
}
if ($gitHubToken) {
    $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("x-access-token:$gitHubToken"))
    cmd /c "git -C `"$SyncDir`" -c credential.helper= -c http.extraHeader=`"Authorization: Basic $basic`" fetch origin >nul 2>&1"
} else {
    cmd /c "git -C `"$SyncDir`" fetch origin >nul 2>&1"
}
if ($LASTEXITCODE -ne 0) { throw "git fetch origin failed in $SyncDir" }
cmd /c "git -C `"$SyncDir`" reset --hard origin/main >nul 2>&1"
cmd /c "git -C `"$SyncDir`" clean -fd >nul 2>&1"
git -C $SyncDir config user.name $AuthorName
git -C $SyncDir config user.email $AuthorEmail
# 公开仓一律 LF 入库，避免 Windows Copy-Item 工作区 CRLF 被原样提交后
# 与 generate()/rustfmt（Unix）漂移；与仓库 .gitattributes eol=lf 对齐。
git -C $SyncDir config core.autocrlf false
git -C $SyncDir config core.eol lf

# --- 2. materialize internal tracked files (minus exclusions) ---
$wanted = @()
foreach ($f in (git ls-files)) {
    if (-not (Test-Excluded $f)) { $wanted += $f }
}
$textExt = [System.Collections.Generic.HashSet[string]]::new(
    [string[]]@('.rs','.as','.arml','.toml','.md','.yml','.yaml','.json','.cjs','.ps1','.sh','.c','.h','.cpp','.hpp','.wgsl','.txt','.gitignore','.gitattributes')
)
foreach ($f in $wanted) {
    $src = Join-Path $repoRoot $f
    $dst = Join-Path $SyncDir ($f -replace '/', '\')
    $dstDir = Split-Path $dst -Parent
    if (-not (Test-Path $dstDir)) { New-Item -ItemType Directory -Force -Path $dstDir | Out-Null }
    $ext = [System.IO.Path]::GetExtension($f).ToLowerInvariant()
    if ($textExt.Contains($ext) -or ($f -notmatch '\.')) {
        # 读入后统一 LF 写出（UTF-8 无 BOM），杜绝 CRLF 金丝雀测试在公开仓红灯
        $bytes = [System.IO.File]::ReadAllBytes($src)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            $text = [System.Text.Encoding]::UTF8.GetString($bytes, 3, $bytes.Length - 3)
        } else {
            $text = [System.Text.Encoding]::UTF8.GetString($bytes)
        }
        $text = $text -replace "`r`n", "`n" -replace "`r", "`n"
        $utf8NoBom = New-Object System.Text.UTF8Encoding $false
        [System.IO.File]::WriteAllText($dst, $text, $utf8NoBom)
    } else {
        Copy-Item $src $dst -Force
    }
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
git -C $SyncDir commit -q -m $Message
if ($LASTEXITCODE -ne 0) { throw "git commit failed in $SyncDir" }
# Push via token header only — never GCM account-picker UI.
# Progress on stderr must not become a terminating ErrorRecord (use cmd).
Invoke-GitHubPush -Dir $SyncDir -Tok $gitHubToken
$hash = git -C $SyncDir rev-parse --short HEAD
Write-Host "==> synced to github.com/$Repo (main @ $hash)"
