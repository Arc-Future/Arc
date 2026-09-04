# fetch-wgpu-native.ps1 - vendor wgpu-native prebuilt binaries
#
# Downloads the official wgpu-native Windows x86_64 msvc release and places it at
#   crates/runtime-ui/wgpu-native/bin/windows/
# so that codegen vendor-lib injection (effective_native_lib_paths) and DLL copy
# (copy_wgpu_native_dll_if_needed) take effect, and Arc examples referencing wgpu
# (ArmlDemo etc.) can link and run.
#
# Usage: powershell -File .\scripts\fetch-wgpu-native.ps1          (default version)
#        powershell -File .\scripts\fetch-wgpu-native.ps1 -Version v29.0.1.1
#        powershell -File .\scripts\fetch-wgpu-native.ps1 -Force    (re-download)
#
# Asset: wgpu-windows-x86_64-msvc-release.zip (GitHub release, ~16 MB)
# Output: crates/runtime-ui/wgpu-native/bin/windows/wgpu_native.dll
#         crates/runtime-ui/wgpu-native/bin/windows/wgpu_native.lib
#
# Idempotent: skips when both DLL/lib already exist unless -Force is given.
# Hygiene: download/extract entirely under $env:TEMP; only final artifacts go to
#          the vendor dir. Never writes scratch files into the source tree.

param(
    [string]$Version = "v29.0.1.1",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$AssetName = "wgpu-windows-x86_64-msvc-release.zip"
$DownloadUrl = "https://github.com/gfx-rs/wgpu-native/releases/download/$Version/$AssetName"
$VendorDir = Join-Path $Root "crates/runtime-ui/wgpu-native/bin/windows"
$DllTarget = Join-Path $VendorDir "wgpu_native.dll"
$LibTarget = Join-Path $VendorDir "wgpu_native.lib"

if (!$Force -and (Test-Path $DllTarget) -and (Test-Path $LibTarget)) {
    Write-Host "wgpu-native $Version already present ($DllTarget); use -Force to re-download"
    exit 0
}

Write-Host "Downloading wgpu-native $Version (Windows x86_64 msvc)..."
Write-Host "  URL: $DownloadUrl"

# Hygiene: all download/extract under $env:TEMP, never in the source tree.
$Work = Join-Path $env:TEMP "wgpu-native-fetch-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $Work -Force | Out-Null
$Zip = Join-Path $Work $AssetName

try {
    curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $Zip $DownloadUrl
    if ($LASTEXITCODE -ne 0) {
        throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
    }
    $size = (Get-Item $Zip).Length
    if ($size -lt 1MB) {
        throw "downloaded file suspiciously small ($size bytes); possibly a proxy error page"
    }
    Write-Host "Download complete: $size bytes"

    Write-Host "Extracting to temp dir..."
    $Unzip = Join-Path $Work "unzip"
    Expand-Archive -Path $Zip -DestinationPath $Unzip -Force

    # Locate wgpu_native.dll / .lib (nesting varies by release; search recursively).
    $dll = Get-ChildItem -Path $Unzip -Recurse -Filter "wgpu_native.dll" | Select-Object -First 1
    $lib = Get-ChildItem -Path $Unzip -Recurse -Filter "wgpu_native.lib" | Select-Object -First 1
    if (!$dll -or !$lib) {
        throw "wgpu_native.dll and/or wgpu_native.lib not found in extracted archive"
    }

    New-Item -ItemType Directory -Path $VendorDir -Force | Out-Null
    Copy-Item $dll.FullName $DllTarget -Force
    Copy-Item $lib.FullName $LibTarget -Force

    Write-Host "Vendored:"
    Write-Host "  $DllTarget  ($(Get-Item $DllTarget).Length bytes)"
    Write-Host "  $LibTarget  ($(Get-Item $LibTarget).Length bytes)"
    Write-Host ""
    Write-Host "Next: rebuild an Arc example referencing wgpu; it will now link."
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
