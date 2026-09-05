# scripts/packaging/install.ps1 - Arc SDK installer (Windows; embedded at the SDK root when packaging)
#
# Three mutually exclusive install sources:
#   -Archive <zip>   local distribution zip (arc-pack.ps1 output)
#   -Url <https...>  remote distribution zip (HTTPS download)
#   -FromDir <dir>   an already-extracted SDK directory (in-place install; when
#                    this script is embedded inside the extracted SDK root,
#                    running `.\install.ps1` with no arguments auto-selects this)
#
# Flow: integrity check (SHA256 only for zip/url sources) -> place under
# <InstallRoot>\versions\<pkgName> -> write versions/current marker + root bin/
# launcher -> append the user PATH (HKCU\Environment, points at the single root
# bin/ pointer; can be skipped) -> print version and run `arc doctor`.
#
# Pointer layout is compatible with `arc self-update`: <InstallRoot>/bin/arc.exe
# is a copy of the active version; switching versions only rewrites the
# versions/current marker and the bin/ pointer, PATH never changes (RFC 031 s12).
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1          # in-place (script inside an extracted SDK root)
#   powershell ... -File install.ps1 -FromDir D:\sdk\arc-1.0.0-x86_64-pc-windows-msvc
#   powershell ... -File install.ps1 -Archive arc-1.0.0-x86_64-pc-windows-msvc.zip [-Sha256 <64-hex>]
#   powershell ... -InstallRoot D:\arc -NoModifyPath
#
# Uninstall = delete InstallRoot's bin + versions and remove the root bin entry
# from the user PATH (no residue by design).
#
# NOTE: keep this file ASCII-only. It runs under Windows PowerShell 5.1 too,
# which reads BOM-less scripts with the ANSI codepage; non-ASCII comments can
# then garble parsing.

param(
    [string]$Url = "",
    # Local distribution zip (arc-pack.ps1 output): skips download, offline
    # install. Mutually exclusive with -Url/-FromDir; SHA256 still honored when
    # -Sha256 is given.
    [string]$Archive = "",
    # Already-extracted SDK directory (must contain bin\arc.exe): in-place
    # install, skips download and SHA256 (file is local; integrity by source).
    [string]$FromDir = "",
    [string]$Sha256 = "",
    [string]$InstallRoot = "",
    [switch]$NoModifyPath,
    [switch]$SkipDoctor
)

$ErrorActionPreference = "Stop"
if (-not $InstallRoot) { $InstallRoot = Join-Path $env:LOCALAPPDATA "arc" }

# --- Source resolution: one of -Url/-Archive/-FromDir; no args while the
#     script sits in an extracted SDK root defaults to in-place install. ---
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Url -and -not $Archive -and -not $FromDir) {
    if (Test-Path (Join-Path $scriptDir "bin\arc.exe")) {
        $FromDir = (Resolve-Path $scriptDir).Path
    } else {
        Write-Error "install.ps1: need one of -Url, -Archive or -FromDir (or run from an extracted SDK root); point it at the zip produced by scripts/packaging/arc-pack.ps1"
        exit 1
    }
}
$modes = @()
if ($Url) { $modes += "-Url" }
if ($Archive) { $modes += "-Archive" }
if ($FromDir) { $modes += "-FromDir" }
if ($modes.Count -gt 1) {
    Write-Error "install.ps1: -Url, -Archive and -FromDir are mutually exclusive (got: $($modes -join ', '))"
    exit 1
}
if ($Archive) {
    $resolved = Resolve-Path $Archive -ErrorAction SilentlyContinue
    if (-not $resolved) { Write-Error "install.ps1: -Archive not found: $Archive"; exit 1 }
    $Archive = $resolved.Path
} elseif ($Url -and $Url -notlike "https://*") {
    Write-Warning "install.ps1: URL is not HTTPS ($Url) - Phase 1 safety requires HTTPS downloads"
}

# UTF-8 without BOM for marker files (PowerShell 5.1 Set-Content -Encoding
# UTF8 writes a BOM, which the Arc lexer rejects).
function Write-Utf8NoBom([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

# --- Package name/version derivation: zip/url use the file name; in-place
#     prefers version.txt (the directory may have been renamed). ---
function Get-VersionFromVersionTxt([string]$sdkDir) {
    $vt = Join-Path $sdkDir "version.txt"
    if (-not (Test-Path $vt)) { return $null }
    $map = @{}
    foreach ($line in [System.IO.File]::ReadAllLines($vt)) {
        $kv = $line -split "=", 2
        if ($kv.Count -eq 2) { $map[$kv[0].Trim()] = $kv[1].Trim() }
    }
    if ($map["arc"] -and $map["triple"]) {
        return "arc-$($map['arc'])-$($map['triple'])"
    }
    return $null
}

$pkgName = $null
$tmpZip = $null
if ($FromDir) {
    # NOTE: $FromDir is a [string]-typed parameter. Assigning the Resolve-Path
    # PathInfo straight back would coerce it to string, and `.Path` on a string
    # yields $null (empty Join-Path arg). Resolve into an unconstrained local
    # first, then dereference .Path.
    $resolvedFrom = Resolve-Path $FromDir -ErrorAction SilentlyContinue
    if (-not $resolvedFrom) { Write-Error "install.ps1: -FromDir not found: $FromDir"; exit 1 }
    $FromDir = $resolvedFrom.Path
    if (-not (Test-Path (Join-Path $FromDir "bin\arc.exe"))) {
        Write-Error "install.ps1: $FromDir has no bin\arc.exe (not an extracted Arc SDK?)"
        exit 1
    }
    $pkgName = Get-VersionFromVersionTxt $FromDir
    if (-not $pkgName) {
        # Fallback: the directory name must follow arc-<ver>-<triple>, else refuse.
        $leaf = Split-Path $FromDir -Leaf
        if ($leaf -notmatch "^arc-[\w.]+-[\w.-]+$") {
            Write-Error "install.ps1: cannot derive package name from $leaf (missing version.txt and non-standard dir name)"
            exit 1
        }
        $pkgName = $leaf
    }
    Write-Host "==> using extracted SDK dir $FromDir (package $pkgName, integrity by source)"
} else {
    $zipName = if ($Archive) { Split-Path $Archive -Leaf } else { Split-Path $Url -Leaf }
    if (-not $zipName.EndsWith(".zip")) { Write-Error "install.ps1: expected a .zip package, got $zipName"; exit 1 }
    $pkgName = $zipName.Substring(0, $zipName.Length - 4)
    if (-not $pkgName) { Write-Error "install.ps1: cannot derive package name from $zipName"; exit 1 }

    # --- 1. Obtain the package (-Archive uses it directly; -Url downloads) ---
    if ($Archive) {
        $tmpZip = $Archive
        Write-Host "==> using local archive $Archive"
    } else {
        $tmpZip = Join-Path $env:TEMP $zipName
        if (Test-Path $tmpZip) { Remove-Item $tmpZip -Force }
        Write-Host "==> downloading $Url"
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $Url -OutFile $tmpZip -UseBasicParsing
    }

    # --- 2. SHA256 check (explicit -Sha256 or the <url>.sha256 sidecar; a local
    #        -Archive without -Sha256 only warns). ---
    $actual = (Get-FileHash $tmpZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $expected = $Sha256.ToLowerInvariant()
    if (-not $expected) {
        if ($Archive) {
            Write-Warning "install.ps1: local archive without -Sha256 - integrity not verified (hash: $actual)"
        } else {
            $shaUrl = "$Url.sha256"
            try {
                $expected = ((Invoke-WebRequest -Uri $shaUrl -UseBasicParsing -ErrorAction Stop).Content -split "\s+")[0].ToLowerInvariant()
            } catch {
                Write-Error "install.ps1: no -Sha256 and no .sha256 manifest at $shaUrl - refusing to install without verification"
                exit 1
            }
        }
    }
    if ($expected -and $actual -ne $expected) {
        Write-Error "install.ps1: SHA256 mismatch - expected $expected, got $actual (download corrupted or tampered); aborting"
        exit 1
    }
    if ($expected) { Write-Host "==> sha256 ok: $actual" }
}

# --- 3. Place <InstallRoot>\versions\<pkgName> ---
$target = Join-Path (Join-Path $InstallRoot "versions") $pkgName
if ($FromDir -and (Resolve-Path $FromDir -ErrorAction SilentlyContinue).Path -eq (Resolve-Path $target -ErrorAction SilentlyContinue).Path) {
    # In-place re-run: source is already the versioned layout, refresh pointers only.
    Write-Host "==> already in versioned layout: $target (refreshing pointers)"
} else {
    if (Test-Path $target) { Remove-Item $target -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
    if ($FromDir) {
        # Copy instead of move: the script itself may be running from inside the
        # directory being installed (zip-embedded in-place flow), and Windows
        # refuses to rename a directory holding an open file (no FILE_SHARE_DELETE
        # on the running script). Copying also keeps the user's extracted copy
        # usable. The Unix installer has no such restriction and moves instead.
        Copy-Item $FromDir $target -Recurse
        Write-Host "==> copied $FromDir -> $target"
    } else {
        $extractTmp = Join-Path $env:TEMP "arc-install-extract-$PID"
        if (Test-Path $extractTmp) { Remove-Item $extractTmp -Recurse -Force }
        New-Item -ItemType Directory -Force -Path $extractTmp | Out-Null
        Expand-Archive -Path $tmpZip -DestinationPath $extractTmp -Force
        Move-Item (Join-Path $extractTmp $pkgName) $target
        Remove-Item $extractTmp -Recurse -Force
    }
}

$arc = Join-Path $target "bin\arc.exe"
if (-not (Test-Path $arc)) { Write-Error "install.ps1: $pkgName has no bin\arc.exe (broken package)"; exit 1 }

# --- 3b. Pointer layout: versions/current marker + root bin/ launcher (the
#         single PATH injection point). Versioned dirs stay complete (rollback);
#         <InstallRoot>/bin/arc.exe is the active copy and re-execs per
#         versions/current (see crates/arc/src/self_update.rs). ---
$ver = $pkgName.Substring(4, $pkgName.IndexOf("-", 4) - 4)
$markers = Join-Path $InstallRoot "versions"
Write-Utf8NoBom (Join-Path $markers "current") "$ver`n"
$rootBin = Join-Path $InstallRoot "bin"
New-Item -ItemType Directory -Force -Path $rootBin | Out-Null
Copy-Item $arc (Join-Path $rootBin "arc.exe") -Force
Write-Host "==> pointer layout: versions/current=$ver, $rootBin\arc.exe"

# --- 4. PATH injection (user-level HKCU\Environment; points at the root bin/
#         single pointer). ---
if (-not $NoModifyPath) {
    $binDir = $rootBin
    $regPath = "HKCU:\Environment"
    $current = (Get-ItemProperty -Path $regPath -Name Path -ErrorAction SilentlyContinue).Path
    if (-not $current) { $current = "" }
    if ($current -split ";" -notcontains $binDir) {
        $newPath = if ($current) { "$current;$binDir" } else { $binDir }
        Set-ItemProperty -Path $regPath -Name Path -Value $newPath
        Write-Host "==> PATH updated (user-level): $binDir"
        Write-Host "    (new terminals only - current shell needs restart)"
    } else {
        Write-Host "==> PATH already contains $binDir"
    }
} else {
    Write-Host "==> -NoModifyPath: PATH not modified (use: <InstallRoot>\bin)"
}

# --- 5. Version + doctor ---
Write-Host "==> installed: $target (active via $rootBin)"
& $arc --version
if ($LASTEXITCODE -ne 0) { Write-Error "install.ps1: arc.exe failed to run"; exit 1 }
if (-not $SkipDoctor) {
    & $arc doctor
    if ($LASTEXITCODE -ne 0) { Write-Error "install.ps1: arc doctor reported failures"; exit 1 }
}
Write-Host "==> install complete. Uninstall: remove $InstallRoot\bin and $markers, and remove $rootBin from user PATH."
exit 0
