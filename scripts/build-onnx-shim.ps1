# build-onnx-shim.ps1 - compile Arc.AI.Onnx shim DLL against vendored ONNX Runtime
#
# 把 `crates/runtime-onnx/onnx_shim.cpp`（extern "C" 包 onnxruntime_cxx_api.h）编译
# 并链接 vendored `target/onnx-native/onnxruntime.lib`，产出
#   target/onnx-native/onnx.dll
# 与 onnxruntime.dll 同目录，使 onnx.dll 加载时能解析 onnxruntime 符号。
# 命名约定（对齐 zxing 先例）：`.ani` 模块名 `onnx` = 产物库名 `onnx.dll`
# （源码名 `onnx_shim.cpp` ≠ 产物名）。
#
# 前置：先运行 `scripts/fetch-onnx-native.ps1`（生成 target/onnx-native/）。
# 运行时 onnx.ani `load="auto"` + `ARC_ONNX_LIB` 环境变量指向 target/onnx-native/
# （含 onnx.dll + onnxruntime.dll 的目录）。
#
# Usage: powershell -File .\scripts\build-onnx-shim.ps1          (clang++，默认)
#        powershell -File .\scripts\build-onnx-shim.ps1 -Toolchain msvc
#        powershell -File .\scripts\build-onnx-shim.ps1 -Force
#
# Requires: clang++（默认；soft-skip 惯例同 e2e）或 MSVC cl（-Toolchain msvc）。

param(
    [ValidateSet("clang", "msvc")]
    [string]$Toolchain = "clang",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Native = Join-Path $Root "target/onnx-native"
$DllTarget = Join-Path $Native "onnx.dll"
$ShimSrc = Join-Path $Root "crates/runtime-onnx/onnx_shim.cpp"

if (!(Test-Path (Join-Path $Native "onnxruntime.lib"))) {
    throw "missing target/onnx-native/onnxruntime.lib; run scripts\fetch-onnx-native.ps1 first"
}
if (!(Test-Path $ShimSrc)) { throw "missing shim source: $ShimSrc" }
if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "onnx.dll already present ($DllTarget); use -Force to rebuild"
    exit 0
}

$Inc = Join-Path $Native "include"
$Log = Join-Path $Native "shim-build.log"

Write-Host "Building onnx.dll with $Toolchain ..."

# clang++：直接传 MSVC import lib；-std=c++17 为 ONNX Runtime 头所要求。
if ($Toolchain -eq "clang") {
    if (!(Get-Command clang++ -ErrorAction SilentlyContinue)) {
        throw "clang++ not found; install LLVM or use -Toolchain msvc"
    }
    $ErrorActionPreference = "Continue"
    & clang++ -std=c++17 -O2 -shared -D_SCL_SECURE_NO_WARNINGS -D_CRT_SECURE_NO_WARNINGS `
        -D_CRT_NONSTDC_NO_WARNINGS -DNOMINMAX "-I$Inc" `
        $ShimSrc (Join-Path $Native "onnxruntime.lib") -o $DllTarget 2> $Log
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($ExitCode -ne 0) {
        Get-Content $Log | ForEach-Object { Write-Host "  $_" }
        throw "clang++ build failed (exit $ExitCode)"
    }
}
else {
    # MSVC：使用环境中的 cl.exe（需 Developer Command Prompt 或 vsdevcmd）。
    if (!(Get-Command cl -ErrorAction SilentlyContinue)) {
        throw "cl.exe not found; run inside a Developer Command Prompt or use -Toolchain clang"
    }
    $ErrorActionPreference = "Continue"
    & cl -nologo -O2 -EHsc -std:c++17 /D_SCL_SECURE_NO_WARNINGS /D_CRT_SECURE_NO_WARNINGS `
        /DNOMINMAX "/I$Inc" $ShimSrc (Join-Path $Native "onnxruntime.lib") `
        /Fe:onnx.dll /link /DLL 2> $Log
    $ExitCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($ExitCode -ne 0) {
        Get-Content $Log | ForEach-Object { Write-Host "  $_" }
        throw "MSVC build failed (exit $ExitCode)"
    }
    # MSVC 默认在 CWD 产出；移动到目标目录。
    if (Test-Path (Join-Path (Get-Location) "onnx.dll")) {
        Copy-Item (Join-Path (Get-Location) "onnx.dll") $DllTarget -Force
    }
}

if (!(Test-Path $DllTarget)) { throw "build did not produce $DllTarget" }

Write-Host "Built: $DllTarget ($(Get-Item $DllTarget).Length bytes)"
Write-Host ""
Write-Host "Verify exports (expect the onnx_* symbols):"
if (Get-Command llvm-objdump -ErrorAction SilentlyContinue) {
    & llvm-objdump -p $DllTarget 2>$null |
        Select-String -Pattern "onnx_create_session|onnx_run|onnx_create_tensor_float" |
        ForEach-Object { Write-Host "  $_" }
}
Write-Host ""
Write-Host "Run (both DLLs in ARC_ONNX_LIB dir):"
Write-Host "  \$env:ARC_ONNX_LIB = '$Native'"
