# fetch-onnx-native.ps1 - vendor ONNX Runtime (CPU + DirectML) for Arc.AI.Onnx
#
# Arc.AI.Onnx 封装 ONNX Runtime C++（`crates/runtime-onnx/onnx_shim.{h,cpp}` 把
# `onnxruntime_cxx_api.h` 包成 extern "C" C ABI，见 onnx.ani 契约）。ONNX Runtime 是
# 重量级外部依赖，**不 vendored 进仓库**（工作区卫生 G″，对齐 zxing-cpp 先例）：
# 本脚本只负责**外部下载/分发**，产物落
#   target/onnx-native/
#     include/            （onnxruntime_cxx_api.h 等头）
#     onnxruntime.dll     （DirectML 构建：CPU 基线 + DirectML GPU EP）
#     onnxruntime.lib     （MSVC import lib，供 onnx_shim 链接）
#     SHA256.txt          （下载包 SHA256 记录，版本锁定）
#
# 默认取 `Microsoft.ML.OnnxRuntime.DirectML` NuGet 包——它含 DirectML EP 符号
# （`OrtSessionOptionsAppendExecutionProvider_DML`），CPU-only 包不导出该符号，
# 会使 onnx_shim 链接失败；选 DirectML 构建可同时支撑 CPU + DirectML（RFC 决策）。
#
# Usage: powershell -File .\scripts\fetch-onnx-native.ps1                 (default version)
#        powershell -File .\scripts\fetch-onnx-native.ps1 -Version 1.20.1
#        powershell -File .\scripts\fetch-onnx-native.ps1 -Package Microsoft.ML.OnnxRuntime.DirectML
#        powershell -File .\scripts\fetch-onnx-native.ps1 -Force          (re-download)
#
# Idempotent: skips when target/onnx-native/onnxruntime.dll already present unless -Force.
# Hygiene: download/extract entirely under $env:TEMP; only final artifacts go to target/.
#
# 宣称纪律（RFC 025 §1.1）：首次运行时本脚本计算并记录下载包 SHA256 到 SHA256.txt。
# 正式版本锁定前，请将记录的哈希与上游 release 公告核验后回填进 VENDOR.md；未核验
# 不得宣称"已固定"。

param(
    [string]$Version = "1.20.1",
    [string]$Package = "Microsoft.ML.OnnxRuntime.DirectML",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$OutDir = Join-Path $Root "target/onnx-native"
$DllTarget = Join-Path $OutDir "onnxruntime.dll"

if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "onnxruntime.dll already present ($OutDir); use -Force to re-fetch"
    exit 0
}

# NuGet 包名须小写用于 flat-container 路径；包名本身保留原始大小写（DLL 名不变）。
$PkgLower = $Package.ToLowerInvariant()
$AssetName = "$PkgLower.$Version.nupkg"
$DownloadUrl = "https://api.nuget.org/v3-flatcontainer/$PkgLower/$Version/$AssetName"

Write-Host "Fetching $Package $Version (ONNX Runtime, CPU + DirectML)..."
Write-Host "  URL: $DownloadUrl"

$Work = Join-Path $env:TEMP "onnx-native-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $Work -Force | Out-Null
$Nupkg = Join-Path $Work $AssetName

try {
    curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $Nupkg $DownloadUrl
    if ($LASTEXITCODE -ne 0) {
        throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
    }

    $actual = (Get-FileHash $Nupkg -Algorithm SHA256).Hash.ToUpper()
    Write-Host "SHA256: $actual  (recorded to SHA256.txt; verify against upstream release)"

    Write-Host "Extracting NuGet package..."
    $Extract = Join-Path $Work "extract"
    # PS 5.1 的 Expand-Archive 只认 .zip 扩展名；.nupkg/.whl 本质是 zip，改走 .NET ZipFile
    # （兼容任意扩展名）。
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($Nupkg, $Extract)

    # 定位 DirectML NuGet 的 win-x64 原生资产与头。
    $Native = Join-Path $Extract "runtimes/win-x64/native/onnxruntime.dll"
    if (!(Test-Path $Native)) { $Native = Join-Path $Extract "runtimes/win-x86_64/native/onnxruntime.dll" }
    if (!(Test-Path $Native)) {
        $cand = Get-ChildItem (Join-Path $Extract "runtimes") -Recurse -Filter onnxruntime.dll -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($cand) { $Native = $cand.FullName }
    }
    if (!(Test-Path $Native)) { throw "onnxruntime.dll not found in package (unexpected layout)" }

    # import lib：优先取与 win-x64 DLL 匹配的 x64 库。DirectML 包含多架构
    # （win-arm64/win-x64...），纯递归搜索会用 `Select-Object -First 1` 误选 arm64，
    # 导致 clang 链接报 "machine type arm64 conflicts with x64"。
    $Lib = Join-Path $Extract "runtimes/win-x64/native/onnxruntime.lib"
    if (!(Test-Path $Lib)) { $Lib = Join-Path $Extract "runtimes/win-x86_64/native/onnxruntime.lib" }
    if (!(Test-Path $Lib)) {
        $cand = Get-ChildItem $Extract -Recurse -Filter onnxruntime.lib -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match "x64|x86_64" } | Select-Object -First 1
        if ($cand) { $Lib = $cand.FullName }
    }
    if (!(Test-Path $Lib)) {
        $cand = Get-ChildItem $Extract -Recurse -Filter onnxruntime.lib -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($cand) { $Lib = $cand.FullName }
    }
    if (!(Test-Path $Lib)) { throw "onnxruntime.lib not found in package (unexpected layout)" }

    # 头目录：build/native/include/
    $Inc = Join-Path $Extract "build/native/include"
    if (!(Test-Path (Join-Path $Inc "onnxruntime_cxx_api.h"))) { $Inc = "" }

    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    Copy-Item $Native $DllTarget -Force
    Copy-Item $Lib (Join-Path $OutDir "onnxruntime.lib") -Force
    if ($Inc) {
        Copy-Item $Inc (Join-Path $OutDir "include") -Recurse -Force
    } else {
        # 无头则从包内 rl 扫描拷贝 onnxruntime_cxx_api.h 所在头目录。
        $head = Get-ChildItem $Extract -Recurse -Filter onnxruntime_cxx_api.h -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($head) { Copy-Item $head.Directory (Join-Path $OutDir "include") -Recurse -Force }
        else { throw "onnxruntime_cxx_api.h not found in package" }
    }

    # 版本 / 哈希 / 许可记录（版本锁定证据）。
    Set-Content (Join-Path $OutDir "version.txt") $Version -Encoding ascii
    Set-Content (Join-Path $OutDir "package.txt") $Package -Encoding ascii
    Set-Content (Join-Path $OutDir "SHA256.txt") $actual -Encoding ascii
    $License = Get-ChildItem $Extract -Recurse -Filter "LICENSE*" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($License) { Copy-Item $License.FullName (Join-Path $OutDir "LICENSE") -Force }

    Write-Host "Vendored to $OutDir :"
    Write-Host "  onnxruntime.dll  ($(Get-Item $DllTarget).Length bytes)"
    Write-Host "  onnxruntime.lib  ($(Get-Item (Join-Path $OutDir 'onnxruntime.lib')).Length bytes)"
    Write-Host "  include\onnxruntime_cxx_api.h"
    Write-Host "  SHA256.txt = $actual"
    Write-Host ""
    Write-Host "Next: powershell -File .\scripts\build-onnx-shim.ps1"
    Write-Host "Then set ARC_ONNX_LIB=$OutDir and run the ONNX inference e2e."
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
