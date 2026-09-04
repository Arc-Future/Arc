# github-release.ps1 - publish Arc 1.0 artifacts to a GitHub Release.
#
# Flow:
#   1. Locate the four dist artifacts in $DistDir:
#        arc-<Version>-<triple>.zip / .zip.sha256 / manifest.json / manifest.json.sig
#   2. RE-SIGN the release manifest with the REAL GitHub download URLs
#      (https://github.com/<Repo>/releases/download/v<Version>/...) so that
#      `arc self-update` can consume the release endpoint as ARC_RELEASE_BASE.
#      Signing key resolution: $env:ARC_RELEASE_SIGNING_KEY > offline key file
#      ~/.arc/keys/release-signing-key-<Version>.txt (NEVER committed).
#   3. Generate release notes (downloads table + sha256 + verify instructions)
#      unless -NotesFile is given.
#   4. Resolve a GitHub token: -Token > $env:GH_TOKEN / $env:GITHUB_TOKEN >
#      Git Credential Manager entry for github.com (the same credential that
#      pushed the repo; needs repo scope).
#   5. Create the release (tag auto-created on the default branch); if the
#      tag already exists the existing release is reused and its body updated.
#   6. Upload the four assets idempotently (existing same-name assets are
#      replaced), then read the release back and report the asset count.
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-release.ps1 -DryRun
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-release.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-release.ps1 -Version 1.0.1 -Repo Arc-Future/Arc

param(
    [string]$Version = "1.0.0",
    [string]$Repo = "Arc-Future/Arc",
    [string]$DistDir = "",
    [string]$ArcExe = "",
    [string]$Token = "",
    [string]$NotesFile = "",
    [switch]$SkipResign,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $DistDir) { $DistDir = Join-Path $repoRoot "target\dist" }
if (-not $ArcExe) {
    $candidate = Join-Path $repoRoot "target\release\arc.exe"
    if (Test-Path $candidate) { $ArcExe = $candidate } else { $ArcExe = "arc" }
}
$tag = "v$Version"
$downloadBase = "https://github.com/$Repo/releases/download/$tag"
$apiBase = "https://api.github.com/repos/$Repo"

# --- 1. Locate artifacts ---
$zip = Get-ChildItem $DistDir -Filter "arc-$Version-*.zip" -ErrorAction Stop | Select-Object -First 1
if (-not $zip) { throw "artifact arc-$Version-*.zip not found in $DistDir" }
$zipName = $zip.Name
$triple = ($zipName -replace "^arc-$Version-", '') -replace '\.zip$', ''
$zipShaPath = "$($zip.FullName).sha256"
$manifestPath = Join-Path $DistDir "manifest.json"
$manifestSigPath = Join-Path $DistDir "manifest.json.sig"
foreach ($f in @($zipShaPath, $manifestPath, $manifestSigPath)) {
    if (-not (Test-Path $f)) { throw "missing artifact: $f" }
}
Write-Host "==> artifacts: $zipName (+sha256, manifest.json, manifest.json.sig), triple=$triple"

# --- 2. Re-sign manifest with the real release download URLs ---
if (-not $SkipResign) {
    if (-not $env:ARC_RELEASE_SIGNING_KEY) {
        $keyFile = Join-Path $env:USERPROFILE ".arc\keys\release-signing-key-$Version.txt"
        if (Test-Path $keyFile) {
            $line = [System.IO.File]::ReadAllLines($keyFile) | Select-String 'ARC_RELEASE_SIGNING_KEY='
            $env:ARC_RELEASE_SIGNING_KEY = ($line.ToString() -split '=', 2)[1]
        } else {
            throw "signing key not found: set `$env:ARC_RELEASE_SIGNING_KEY or create $keyFile (offline file, never commit it)"
        }
    }
    & $ArcExe release manifest --version $Version --triple $triple --archive $zip.FullName --output $DistDir --url-prefix $downloadBase
    if ($LASTEXITCODE -ne 0) { throw "arc release manifest failed" }
    Write-Host "==> manifest re-signed with URL prefix $downloadBase"
}

# --- 3. Release notes (downloads + integrity + verify instructions) ---
if (-not $NotesFile) {
    $sha = ((Get-Content $zipShaPath -TotalCount 1) -split '\s+')[0].Trim()
    $pubkey = "(set `$_env:ARC_RELEASE_SIGNING_KEY to embed the signing pubkey here)"
    if ($env:ARC_RELEASE_SIGNING_KEY) {
        $kg = & $ArcExe release keygen --seed $env:ARC_RELEASE_SIGNING_KEY 2>$null
        $publine = $kg | Select-String 'ARC_RELEASE_PUBKEY'
        if ($publine) { $pubkey = ($publine.ToString() -split '= ', 2)[1].Trim() }
    }
    $notesPath = Join-Path $DistDir "release-notes.md"
    # Single-quoted here-string: backticks must survive verbatim (markdown
    # inline code + fences); dynamic values injected via token replacement.
    $notes = @'
# Arc {VERSION}

First stable release of the Arc language, compiler, standard library and runtime: a single `arc` executable, source-distributed standard library, bundled slim LLVM (clang + lld subset) for fully offline builds, AOT compilation to native machine code - no JIT runtime.

## Downloads

| Asset | Purpose |
|-------|---------|
| `{ZIP}` | Windows x64 SDK (compiler + std + runtime C + bundled LLVM) |
| `{ZIP}.sha256` | Integrity checksum |
| `manifest.json` / `manifest.json.sig` | Ed25519-signed release manifest (consumed by `arc self-update`) |

## Verify integrity

```bash
sha256sum -c {ZIP}.sha256
```

or, with the Arc CLI:

```bash
arc release verify <download-dir> --version {VERSION} --archive {ZIP}
```

Signed release manifest (Ed25519); trust anchor embedded in the compiler:

```
{PUBKEY}
```
'@
    $notes = $notes.Replace('{VERSION}', $Version).Replace('{ZIP}', $zipName).Replace('{SHA}', $sha).Replace('{PUBKEY}', $pubkey)
    [System.IO.File]::WriteAllText($notesPath, $notes, (New-Object System.Text.UTF8Encoding($false)))
    $NotesFile = $notesPath
    Write-Host "==> notes generated: $NotesFile"
}

if ($DryRun) {
    Write-Host "==> DRY RUN: would publish tag $tag to $Repo with assets:"
    Write-Host "    - $zipName"
    Write-Host "    - $zipName.sha256"
    Write-Host "    - manifest.json"
    Write-Host "    - manifest.json.sig"
    Write-Host "    notes: $NotesFile"
    return
}

# --- 4. Resolve GitHub token ---
if (-not $Token) { $Token = $env:GH_TOKEN; if (-not $Token) { $Token = $env:GITHUB_TOKEN } }
if (-not $Token) {
    # Reuse the Git Credential Manager entry for github.com (the same
    # credential that pushed the repository). Fed via a temp file so the
    # credential helper receives protocol/host lines verbatim.
    $credIn = Join-Path $env:TEMP "arc-git-credential-fill.txt"
    [System.IO.File]::WriteAllLines($credIn, @("protocol=https", "host=github.com", ""))
    $fill = cmd /c "git credential fill < `"$credIn`" 2>nul"
    Remove-Item $credIn -Force -ErrorAction SilentlyContinue
    $pwline = $fill | Select-String '^password='
    if ($pwline) { $Token = ($pwline.ToString().Substring(9)).Trim() }
}
if (-not $Token) { throw "no GitHub token available: pass -Token or set GH_TOKEN (repo scope required)" }

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072
$headers = @{
    Authorization = "token $Token"
    'User-Agent'  = 'arc-github-release-script'
    Accept        = 'application/vnd.github+json'
}

# --- 5. Create release (or reuse the existing one for the tag) ---
$notesBody = [System.IO.File]::ReadAllText($NotesFile)
$createBody = @{
    tag_name     = $tag
    name         = "Arc $Version"
    body         = $notesBody
    draft        = $false
    prerelease   = $false
    make_latest  = "true"
} | ConvertTo-Json
$release = $null
try {
    $release = Invoke-RestMethod -Method Post -Uri "$apiBase/releases" -Headers $headers `
        -Body ([System.Text.Encoding]::UTF8.GetBytes($createBody)) -ContentType 'application/json'
    Write-Host "==> release created: $tag"
} catch {
    $code = 0
    if ($_.Exception.Response) { $code = [int]$_.Exception.Response.StatusCode }
    if ($code -eq 422) {
        Write-Host "==> release $tag already exists - reusing and updating notes"
        $release = Invoke-RestMethod -Method Get -Uri "$apiBase/releases/tags/$tag" -Headers $headers
        $patchBody = @{ name = "Arc $Version"; body = $notesBody } | ConvertTo-Json
        $release = Invoke-RestMethod -Method Patch -Uri "$apiBase/releases/$($release.id)" -Headers $headers `
            -Body ([System.Text.Encoding]::UTF8.GetBytes($patchBody)) -ContentType 'application/json'
    } else {
        throw
    }
}
$relId = $release.id

# --- 6. Upload assets (idempotent: replace same-name assets) ---
$assets = @($zipName, "$zipName.sha256", "manifest.json", "manifest.json.sig")
$existing = @(Invoke-RestMethod -Uri "$apiBase/releases/$relId/assets" -Headers $headers)
foreach ($a in $assets) {
    $path = Join-Path $DistDir ($a -replace '/', '\')
    if (-not (Test-Path $path)) { throw "missing asset file: $path" }
    $old = $existing | Where-Object { $_.name -eq $a }
    if ($old) {
        Invoke-RestMethod -Method Delete -Uri "$apiBase/releases/assets/$($old.id)" -Headers $headers | Out-Null
        Write-Host "==> replaced existing asset: $a"
    }
    $uploadUri = "https://uploads.github.com/repos/$Repo/releases/$relId/assets?name=$a"
    Invoke-RestMethod -Method Post -Uri $uploadUri -Headers $headers -InFile $path `
        -ContentType 'application/octet-stream' -TimeoutSec 1800 | Out-Null
    Write-Host "==> uploaded: $a"
}

# --- 7. Read back and report ---
$final = Invoke-RestMethod -Uri "$apiBase/releases/tags/$tag" -Headers $headers
Write-Host ("==> release published: {0} ({1} asset(s), {2})" -f $final.html_url, $final.assets.Count, $final.tag_name)
