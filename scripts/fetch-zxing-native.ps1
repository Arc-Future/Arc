# fetch-zxing-native.ps1 - build the zxing-cpp shared library for RFC 036 M5 (real semantics)
#
# RFC 036 §1.1/§1.6/§1.7：zxing-cpp（Apache-2.0）**不 vendored**、以外部可选依赖
# 形态走 `.ani` 契约 `load="auto"` + `ARC_ZXING_LIB` 环境变量运行时加载。本脚本
# 负责**外部构建/分发**：下载 release 源码 + SHA256 校验 + clang++ 直编 reader-only
# 共享库（zxing.dll / libzxing.so / libzxing.dylib），产物落
#   target/zxing-native/<zxing.dll|libzxing.so|libzxing.dylib>
# （工作区卫生 G″：仅落 `target/`，不进源码树；对齐 fetch-boringssl-native.ps1
#  形态，但 zxing 明确不 vendored → 产物不入 `crates/`）。
#
# 编译面：reader-only（1D/AZTEC/DATAMATRIX/MAXICODE/PDF417/QRCODE 全解码器 +
# core reader 路径 + libzueci），**排除 writer 路径**（CreateBarcode/WriteBarcode
# 走 stub、MultiFormatWriter/TextEncoder 不编、libzint 不编）——产物只导出
# `zxing_decode_c` 单符号（桥接 `crates/runtime-drawing/shim/zxing_shim.cpp`），
# 干净、小、符号面最小。
#
# Usage: powershell -File .\scripts\fetch-zxing-native.ps1            (default version)
#        powershell -File .\scripts\fetch-zxing-native.ps1 -Version v3.1.1
#        powershell -File .\scripts\fetch-zxing-native.ps1 -Force     (re-build)
#        powershell -File .\scripts\fetch-zxing-native.ps1 -Force -SourceDir <已解压源码树> (离线复用)
#
# Idempotent: skips when the target lib already present unless -Force is given.
# Hygiene: download/build entirely under $env:TEMP; only the final lib goes to target/.
#
# Requires: clang / clang++ (same soft-skip convention as e2e tests).

param(
    [string]$Version = "v3.1.1",
    [switch]$Force,
    [string]$SourceDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

# release tag 形如 v3.1.1 → 资产名 zxing-cpp-3.1.1.zip
$AssetVersion = $Version.TrimStart('v')
$AssetName = "zxing-cpp-$AssetVersion.zip"
$DownloadUrl = "https://github.com/zxing-cpp/zxing-cpp/releases/download/$Version/$AssetName"
$Sha256 = "C914877D2598E6C748725E9BC0DAD9B667710537C6B91A3D8B8A7214834EE1D5"

# 产物落 target/（工作区卫生 G″；zxing 不 vendored、不入 crates/）
$LibDir = Join-Path $Root "target/zxing-native"
if ($IsWindows -or $env:OS -eq "Windows_NT") {
    $LibName = "zxing.dll"
} elseif ($IsMacOS) {
    $LibName = "libzxing.dylib"
} else {
    $LibName = "libzxing.so"
}
$LibTarget = Join-Path $LibDir $LibName

if (!$Force -and (Test-Path $LibTarget)) {
    Write-Host "zxing lib already present ($LibTarget); use -Force to re-build"
    exit 0
}

Write-Host "Fetching zxing-cpp $Version (Apache-2.0, external optional dep)..."
Write-Host "  URL: $DownloadUrl"

# Hygiene: all download/build under $env:TEMP, never in the source tree.
if ($SourceDir) {
    $SourceDir = (Resolve-Path $SourceDir).Path
    $Base = $SourceDir
    if (!(Test-Path (Join-Path $Base "core/src"))) {
        throw "-SourceDir 不是有效的 zxing-cpp 源码树: $Base (缺少 core/src/)"
    }
} else {
    $Work = Join-Path $env:TEMP "zxing-native-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $Work -Force | Out-Null
    $Zip = Join-Path $Work $AssetName
}

try {
    if (!$SourceDir) {
        curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $Zip $DownloadUrl
        if ($LASTEXITCODE -ne 0) {
            throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
        }
        $actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $Sha256.ToLower()) {
            throw "SHA256 mismatch: expected $Sha256, got $actual"
        }
        Write-Host "SHA256 verified: $($actual.ToUpper())"

        Write-Host "Extracting to temp dir..."
        Expand-Archive -Path $Zip -DestinationPath $Work -Force
        $Base = Join-Path $Work "zxing-cpp-$AssetVersion"
    } else {
        Write-Host "Reusing source tree: $Base"
    }

    $SrcDir = Join-Path $Base "core/src"
    $BuildScratch = Join-Path $Base "build"   # Version.h 生成处
    New-Item -ItemType Directory -Path $BuildScratch -Force | Out-Null

    # 手动生成 Version.h（CMake 的 configure_file 产物）：reader 全开、writer 关。
    $VersionH = Join-Path $BuildScratch "Version.h"
    @"
#pragma once
#define ZXING_READERS
#define ZXING_ENABLE_1D 1
#define ZXING_ENABLE_AZTEC 1
#define ZXING_ENABLE_DATAMATRIX 1
#define ZXING_ENABLE_MAXICODE 1
#define ZXING_ENABLE_PDF417 1
#define ZXING_ENABLE_QRCODE 1
#define ZXING_VERSION_MAJOR $($AssetVersion.Split('.')[0])
#define ZXING_VERSION_MINOR $($AssetVersion.Split('.')[1])
#define ZXING_VERSION_PATCH $($AssetVersion.Split('.')[2])
#define ZXING_VERSION_SUFFIX ""
#define ZXING_VERSION_STR "$AssetVersion"
"@ | Set-Content -Path $VersionH -Encoding utf8

    # reader-only 源文件集（对齐 core/CMakeLists.txt ZXING_READERS=ON / WRITERS=OFF）。
    $Cpp = @(
        # COMMON_FILES（writer 相关 CreateBarcode/WriteBarcode 在守卫下走 stub，安全）
        "Barcode.cpp","BarcodeFormat.cpp","BitMatrix.cpp","BitMatrixIO.cpp",
        "CharacterSet.cpp","Content.cpp","ECI.cpp","Error.cpp","GTIN.cpp",
        "JSON.cpp","ReadBarcode.cpp","ReedSolomon.cpp","Utf.cpp","ZXingCpp.cpp",
        "CreateBarcode.cpp","WriteBarcode.cpp",
        # READERS 分支附加
        "BitArray.cpp","HRI.cpp","BinaryBitmap.cpp","BitSource.cpp",
        "ConcentricFinder.cpp","GlobalHistogramBinarizer.cpp","GridSampler.cpp",
        "LocalGrid.cpp","HybridBinarizer.cpp","MultiFormatReader.cpp",
        "PerspectiveTransform.cpp","ResultPoint.cpp","TextDecoder.cpp",
        "WhiteRectDetector.cpp"
    )
    $Cpp += @(
        "aztec/AZDecoder.cpp","aztec/AZDetector.cpp","aztec/AZReader.cpp"
    )
    $Cpp += @(
        "datamatrix/DMBitLayout.cpp","datamatrix/DMVersion.cpp",
        "datamatrix/DMDataBlock.cpp","datamatrix/DMDecoder.cpp",
        "datamatrix/DMDetector.cpp","datamatrix/DMReader.cpp"
    )
    $Cpp += @(
        "maxicode/MCBitMatrixParser.cpp","maxicode/MCDecoder.cpp","maxicode/MCReader.cpp"
    )
    $Cpp += @(
        "oned/ODUPCEANCommon.cpp","oned/ODCodabarReader.cpp","oned/ODCode39Reader.cpp",
        "oned/ODCode93Reader.cpp","oned/ODCode128Reader.cpp","oned/ODDataBarCommon.cpp",
        "oned/ODDataBarReader.cpp","oned/ODDataBarExpandedBitDecoder.cpp",
        "oned/ODDataBarExpandedReader.cpp","oned/ODDataBarLimitedReader.cpp",
        "oned/ODDXFilmEdgeReader.cpp","oned/ODITFReader.cpp","oned/ODTelepenReader.cpp",
        "oned/ODMultiUPCEANReader.cpp","oned/ODReader.cpp"
    )
    $Cpp += @(
        "pdf417/ZXBigInteger.cpp","pdf417/PDF417.cpp","pdf417/PDFBarcodeValue.cpp",
        "pdf417/PDFBoundingBox.cpp","pdf417/PDFCodewordDecoder.cpp",
        "pdf417/PDFDecoder.cpp","pdf417/PDFDetectionResult.cpp",
        "pdf417/PDFDetectionResultColumn.cpp","pdf417/PDFDetector.cpp",
        "pdf417/PDFReader.cpp","pdf417/PDFScanningDecoder.cpp",
        "pdf417/MicroPDFReader.cpp"
    )
    $Cpp += @(
        "qrcode/QRCodecMode.cpp","qrcode/QRErrorCorrectionLevel.cpp",
        "qrcode/QRVersion.cpp","qrcode/QRBitMatrixParser.cpp","qrcode/QRDataBlock.cpp",
        "qrcode/QRDecoder.cpp","qrcode/QRDetector.cpp","qrcode/QRFormatInformation.cpp",
        "qrcode/QRReader.cpp"
    )

    $Files = @()
    foreach ($rel in $Cpp) {
        $f = Join-Path $SrcDir $rel
        if (!(Test-Path $f)) { throw "missing source: $f" }
        $Files += $f
    }
    # 桥接 shim（仓库内 crates/runtime-drawing/shim/zxing_shim.cpp）
    $Shim = Join-Path $Root "crates/runtime-drawing/shim/zxing_shim.cpp"
    if (!(Test-Path $Shim)) { throw "missing shim: $Shim" }
    $Files += $Shim
    # libzueci（reader 字符集转换，C 源）
    $Zueci = Join-Path $SrcDir "libzueci/zueci.c"
    if (!(Test-Path $Zueci)) { throw "missing source: $Zueci" }

    Write-Host "Compiling $($Files.Count + 1) TUs with clang++ (reader-only, C++20)..."
    $Log = Join-Path $BuildScratch "compile.log"
    $ErrorActionPreference = "Continue"
    & clang++ -std=c++20 -O2 -shared -DZXING_INTERNAL -D_SCL_SECURE_NO_WARNINGS `
        -D_CRT_SECURE_NO_WARNINGS -D_CRT_NONSTDC_NO_WARNINGS -DNOMINMAX `
        "-I$SrcDir" "-I$BuildScratch" "-I$SrcDir/libzueci" `
        @($Files) $Zueci -o $LibTarget 2> $Log
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($ExitCode -ne 0) {
        Get-Content $Log | ForEach-Object { Write-Host "  $_" }
        throw "clang++ compile failed (exit $ExitCode)"
    }
    if (!(Test-Path $LibTarget)) {
        throw "link did not produce $LibTarget"
    }

    Write-Host "Built: $LibTarget ($((Get-Item $LibTarget).Length) bytes)"
    Write-Host "Verify exports (expect exactly 'zxing_decode_c'):"
    if (Get-Command llvm-objdump -ErrorAction SilentlyContinue) {
        & llvm-objdump -p $LibTarget 2>$null |
            Select-String -Pattern "zxing_decode_c" | ForEach-Object { Write-Host "  $_" }
    }
} finally {
    if (!$SourceDir) {
        Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue
    }
}
