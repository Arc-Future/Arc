# scripts/packaging/install.ps1 — Phase 1/2：Arc SDK 一行安装脚本（Windows MVP）
#
# 流程（对标 rustup）：下载 zip → SHA256 校验 → 解压到 %LOCALAPPDATA%\arc\versions\<ver>
# → 写 versions/current 标记 + 根 bin/ 启动器 → 注入用户级 PATH（HKCU\Environment，
# 指向根 bin/ 单指针，可跳过）→ 打印版本并运行 arc doctor。
#
# 指针布局与 `arc self-update` 兼容：<InstallRoot>/bin/arc.exe 是活动版本副本，
# 多版本切换只改 versions/current 与 bin 指针，PATH 永不变（见 RFC 031 §12）。
#
# 安全：HTTPS + SHA256 全量校验（下载 URL 默认占位，需显式提供）。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\install.ps1 `
#       -Url "https://static.arc.dev/dist/arc-0.1.0-x86_64-pc-windows-msvc.zip" `
#       -Sha256 "<64-hex>"
#   powershell ... -InstallRoot D:\arc -NoModifyPath
#
# 卸载 = 删除 InstallRoot 的 bin + versions + 从用户 PATH 摘除（无残留设计）。

param(
    [string]$Url = "",
    # 本地安装包（arc-pack.ps1 产物 zip）：指定后跳过下载，离线安装。
    # 与 -Url 二选一；-Archive 优先。SHA256 仍可经 -Sha256 显式校验。
    [string]$Archive = "",
    [string]$Sha256 = "",
    [string]$InstallRoot = "",
    [switch]$NoModifyPath,
    [switch]$SkipDoctor
)

$ErrorActionPreference = "Stop"
if (-not $InstallRoot) { $InstallRoot = Join-Path $env:LOCALAPPDATA "arc" }
if (-not $Archive -and -not $Url) {
    Write-Error "install.ps1: either -Archive (local zip) or -Url (HTTPS download) is required (point it at the zip produced by scripts/packaging/arc-pack.ps1)"
    exit 1
}
if ($Archive -and $Url) {
    Write-Error "install.ps1: -Archive and -Url are mutually exclusive"
    exit 1
}
if ($Archive) {
    $resolved = Resolve-Path $Archive -ErrorAction SilentlyContinue
    if (-not $resolved) { Write-Error "install.ps1: -Archive not found: $Archive"; exit 1 }
    $Archive = $resolved.Path
} elseif ($Url -notlike "https://*") {
    Write-Warning "install.ps1: URL is not HTTPS ($Url) — Phase 1 safety requires HTTPS downloads"
}

# PowerShell 5.1 的 Set-Content -Encoding UTF8 会写 BOM，而 Arc 词法器不接受 BOM；
# 指针/清单文件统一用无 BOM UTF-8 写入（与 arc-pack.ps1 同一契约）。
function Write-Utf8NoBom([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

$zipName = if ($Archive) { Split-Path $Archive -Leaf } else { Split-Path $Url -Leaf }
if (-not $zipName.EndsWith(".zip")) { Write-Error "install.ps1: expected a .zip package, got $zipName"; exit 1 }
$pkgName = $zipName.Substring(0, $zipName.Length - 4)

# --- 1. 获取安装包（-Archive 本地直用；-Url 下载到临时目录）---
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

# --- 2. SHA256 校验（-Sha256 显式或 URL 旁 .sha256 清单；-Archive 须显式给出或跳过）---
$actual = (Get-FileHash $tmpZip -Algorithm SHA256).Hash.ToLowerInvariant()
$expected = $Sha256.ToLowerInvariant()
if (-not $expected) {
    if ($Archive) {
        Write-Warning "install.ps1: local archive without -Sha256 — integrity not verified (hash: $actual)"
    } else {
        $shaUrl = "$Url.sha256"
        try {
            $expected = ((Invoke-WebRequest -Uri $shaUrl -UseBasicParsing -ErrorAction Stop).Content -split "\s+")[0].ToLowerInvariant()
        } catch {
            Write-Error "install.ps1: no -Sha256 and no .sha256 manifest at $shaUrl — refusing to install without verification"
            exit 1
        }
    }
}
if ($expected -and $actual -ne $expected) {
    Write-Error "install.ps1: SHA256 mismatch — expected $expected, got $actual (download corrupted or tampered); aborting"
    exit 1
}
if ($expected) { Write-Host "==> sha256 ok: $actual" }

# --- 3. 解压到 <InstallRoot>\versions\<pkgName> ---
$target = Join-Path (Join-Path $InstallRoot "versions") $pkgName
if (Test-Path $target) { Remove-Item $target -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
$extractTmp = Join-Path $env:TEMP "arc-install-extract-$PID"
if (Test-Path $extractTmp) { Remove-Item $extractTmp -Recurse -Force }
New-Item -ItemType Directory -Force -Path $extractTmp | Out-Null
Expand-Archive -Path $tmpZip -DestinationPath $extractTmp -Force
Move-Item (Join-Path $extractTmp $pkgName) $target
Remove-Item $extractTmp -Recurse -Force

$arc = Join-Path $target "bin\arc.exe"
if (-not (Test-Path $arc)) { Write-Error "install.ps1: $pkgName has no bin\arc.exe (broken package)"; exit 1 }

# --- 3b. Phase 2 指针布局：versions/current 标记 + 根 bin/ 启动器（唯一 PATH 注入点）---
# 版本目录保持完整 SDK（可回滚）；<InstallRoot>/bin/arc.exe = 活动版本副本，
# 启动时按 versions/current re-exec（见 crates/arc/src/self_update.rs）。
$ver = $pkgName.Substring(4, $pkgName.IndexOf("-", 4) - 4)
$markers = Join-Path $InstallRoot "versions"
Write-Utf8NoBom (Join-Path $markers "current") "$ver`n"
$rootBin = Join-Path $InstallRoot "bin"
New-Item -ItemType Directory -Force -Path $rootBin | Out-Null
Copy-Item $arc (Join-Path $rootBin "arc.exe") -Force
Write-Host "==> pointer layout: versions/current=$ver, $rootBin\arc.exe"

# --- 4. PATH 注入（用户级 HKCU\Environment；指向根 bin/ 单指针）---
if (-not $NoModifyPath) {
    $binDir = $rootBin
    $regPath = "HKCU:\Environment"
    $current = (Get-ItemProperty -Path $regPath -Name Path -ErrorAction SilentlyContinue).Path
    if (-not $current) { $current = "" }
    if ($current -split ";" -notcontains $binDir) {
        $newPath = if ($current) { "$current;$binDir" } else { $binDir }
        Set-ItemProperty -Path $regPath -Name Path -Value $newPath
        Write-Host "==> PATH updated (user-level): $binDir"
        Write-Host "    (new terminals only — current shell needs restart)"
    } else {
        Write-Host "==> PATH already contains $binDir"
    }
} else {
    Write-Host "==> -NoModifyPath: PATH not modified (use: <InstallRoot>\bin)"
}

# --- 5. 版本 + doctor ---
Write-Host "==> installed: $target (active via $rootBin)"
& $arc --version
if ($LASTEXITCODE -ne 0) { Write-Error "install.ps1: arc.exe failed to run"; exit 1 }
if (-not $SkipDoctor) {
    & $arc doctor
    if ($LASTEXITCODE -ne 0) { Write-Error "install.ps1: arc doctor reported failures"; exit 1 }
}
Write-Host "==> install complete. Uninstall: remove $InstallRoot\bin and $markers, and remove $rootBin from user PATH."
exit 0
