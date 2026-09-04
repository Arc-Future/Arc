# github-export.ps1 - GitHub open-source export: produce a clean tree ready to
# push to a public repository.
#
# Semantics:
#   1. Export **git-tracked files only** (untracked local artifacts / IDE notes
#      never leak into the public tree);
#   2. Remove internal-process assets per the exclusion list ($ExcludeExact /
#      $ExcludeDirs);
#   3. Pre-publish safety scan over the EXPORTED tree: file:// absolute links,
#      personal email leaks, 64-hex key material, personal absolute paths,
#      files > 50MB (GitHub warning line), and the dev placeholder release key.
#   4. -InitGit runs `git init` + a single clean initial commit inside the
#      exported tree (fresh history recommended: the internal repo history
#      contains deleted process files and internal commit messages).
#
# Exclusion list rationale:
#   - docs/plan.md            internal implementation planning (iteration ledger)
#   - docs/discuss.md         internal discussion draft
#   - docs/reviews/           internal review records
#   - docs/rfc/proposals/     internal proposals / rejection memos
#   - .git-blame-ignore-revs  depends on the internal full git history
#   - AGENTS.md / .cursor/    KEPT: CI spec-guard gate inputs (dev governance)
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-export.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-export.ps1 -InitGit
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-export.ps1 -OutDir D:\export\arc

param(
    [string]$OutDir = "",
    [switch]$InitGit,
    [string]$CommitMessage = "chore: Arc 1.0.0 initial public release",
    [string]$AuthorName = "LUSIDA (Start)",
    [string]$AuthorEmail = "474309146@qq.com"
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $OutDir) { $OutDir = Join-Path $repo "target\github-export\arc" }

$ExcludeExact = @(
    "docs/plan.md",
    "docs/discuss.md",
    ".git-blame-ignore-revs"
)
$ExcludeDirs = @(
    "docs/reviews/",
    "docs/rfc/proposals/"
)

# --- 1. Export (committed index state via checkout-index; immune to in-flight
#        working-tree changes from parallel sessions) ---
if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$prefix = ($OutDir -replace '\\', '/')
if (-not $prefix.EndsWith('/')) { $prefix += '/' }
git checkout-index -a -f --prefix="$prefix"
if ($LASTEXITCODE -ne 0) { throw "git checkout-index failed - commit your changes first" }

$tracked = @(git ls-files)
$excluded = 0
foreach ($f in $tracked) {
    $skip = $false
    foreach ($d in $ExcludeDirs) { if ($f.StartsWith($d)) { $skip = $true; break } }
    if (-not $skip) { foreach ($e in $ExcludeExact) { if ($f -eq $e) { $skip = $true; break } } }
    if (-not $skip) { continue }
    $excluded++
    $dst = Join-Path $OutDir ($f -replace '/', '\')
    if (Test-Path $dst) { Remove-Item $dst -Force }
}
# prune now-empty directories left by the exclusions (git cannot track them anyway)
foreach ($d in $ExcludeDirs) {
    $dir = Join-Path $OutDir ($d -replace '/', '\')
    if ((Test-Path $dir) -and -not (Get-ChildItem $dir -Recurse -File)) { Remove-Item $dir -Recurse -Force }
}
$exported = $tracked.Count - $excluded
Write-Host "==> exported $exported committed files (excluded $excluded internal) -> $OutDir"

# --- 2. Pre-publish safety scan over the exported tree (informational; WARN
#        items must be resolved or explicitly accepted) ---
$warn = 0
function Warn([string]$msg) {
    $script:warn++
    Write-Host "WARN: $msg"
}
$scanExtensions = @('.md', '.ps1', '.rs', '.c', '.h', '.toml', '.json', '.yml', '.txt', '.as', '.sh', '.ani', '.arml')

function Find-InExport([string]$Pattern) {
    Get-ChildItem $OutDir -Recurse -File |
        Where-Object { $scanExtensions -contains $_.Extension.ToLowerInvariant() } |
        Select-String -Pattern $Pattern -List |
        ForEach-Object {
            $p = $_.Path
            if ($p.StartsWith($OutDir)) { $p = $p.Substring($OutDir.Length) }
            ($p.TrimStart('\', '/') -replace '\\', '/')
        }
}

# 2a. file:// absolute links with a drive letter (local machine paths leaking).
#     Code that HANDLES the file:// URI scheme and protocol-syntax docs are
#     legitimate; scan prose-like files only and require a drive prefix.
$hits = @(Find-InExport 'file:///[a-zA-Z]:[/\\]')
if ($hits.Count -gt 0) { Warn ("file:// drive links found in: " + ($hits -join ', ')) }

# 2b. Personal email (README/LICENSE/Cargo.toml are the intentional signature
#     spots; anything else must be reviewed)
$hits = @(Find-InExport '[a-zA-Z0-9._%+-]+@(qq|gmail|outlook|163|hotmail)\.(com|net)' |
    Where-Object { $_ -notmatch '^(README\.md|README\.en\.md|LICENSE|Cargo\.toml)$' -and $_ -notmatch '^crates/runtime-' })
if ($hits.Count -gt 0) { Warn ("personal email outside signature/vendored files: " + ($hits -join ', ')) }

# 2c. Personal absolute paths (Windows / macOS drive & user dirs)
$hits = @(Find-InExport 'C:\\Users\\|D:\\GitCode|/Users/[a-z]+/')
if ($hits.Count -gt 0) { Warn ("personal absolute paths found in: " + ($hits -join ', ')) }

# 2d. 64-hex key material (an Ed25519 seed is a PRIVATE key to keep offline;
#     public keys are fine to embed). Known-safe: hash.rs (SHA-256 test
#     vector), components.json (component checksums).
$hits = @(Find-InExport '"[0-9a-fA-F]{64}"' |
    Where-Object { $_ -match '^crates/arc/src/' -and $_ -notmatch '^(crates/arc/src/hash\.rs|crates/arc/src/components\.json)$' })
if ($hits.Count -gt 0) { Warn ("64-hex literals in compiler sources (verify none is a signing seed): " + ($hits -join ', ')) }

# 2e. Release signing placeholder key: if the public repo still embeds the dev
#     placeholder public key (its seed appeared in prior internal history), the
#     self-update trust anchor is effectively void - rotate before distribution.
$releaseSrc = Join-Path $OutDir "crates\arc\src\release.rs"
if (Test-Path $releaseSrc) {
    $relText = [System.IO.File]::ReadAllText($releaseSrc)
    if ($relText -match '2152f8d19b791d24453242e15f2eab6cb7cffa7b6a5ed30097960e069881db12') {
        Warn "release.rs still embeds the DEV PLACEHOLDER public key - rotate via 'arc release keygen' before any real distribution!"
    }
}

# 2f. Large files (GitHub warns > 50MB, rejects > 100MB)
foreach ($f in (Get-ChildItem $OutDir -Recurse -File)) {
    if ($f.Length -gt 50MB) {
        $rel = $f.FullName.Substring($OutDir.Length).TrimStart('\', '/') -replace '\\', '/'
        Write-Host ("INFO: large file {0:N1} MB: {1}" -f ($f.Length / 1MB), $rel)
    }
}

# --- 3. Optional: clean history starting point (signature identity applied
#        repo-locally; global git config is never touched) ---
if ($InitGit) {
    Push-Location $OutDir
    try {
        git init -b main 2>$null | Out-Null
        git add -A
        if ($LASTEXITCODE -ne 0) { throw "git add failed in $OutDir" }
        git config user.name $AuthorName
        git config user.email $AuthorEmail
        git commit -m $CommitMessage | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "git commit failed in $OutDir" }
        Write-Host "==> git repository initialized with a single clean commit (author: $AuthorName <$AuthorEmail>)"
    } finally {
        Pop-Location
    }
}

Write-Host "==> done ($warn warning(s)). Next steps:"
Write-Host "    1. Review WARN items above (they must be resolved or explicitly accepted)."
Write-Host "    2. Rotate the release signing key if the placeholder warning is present;"
Write-Host "       keep the new seed OFFLINE (never commit it)."
Write-Host "    3. Create the public GitHub repo and push from $OutDir"
Write-Host "       (fresh history recommended; do NOT push the internal repo's history)."
