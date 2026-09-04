# build-iree-shim.ps1 - compile Arc.AI.Iree shim DLL against vendored IREE Runtime
#
# 把 `crates/runtime-iree/iree_shim.cpp`（extern "C" 包 iree/runtime/api.h）编译并链接
# vendored `target/iree-native/`，产出 `target/iree-native/iree_shim.dll`，与 IREE
# runtime DLL 同目录，使 iree_shim 加载时能解析 IREE 符号。
#
# 前置：先运行 `scripts/fetch-iree-native.ps1`（生成 target/iree-native/）。
# 运行时 iree.ani `load="auto"` + `ARC_IREE_LIB` 环境变量指向 target/iree-native/。
#
# Usage: powershell -File .\scripts\build-iree-shim.ps1          (clang++，默认)
#        powershell -File .\scripts\build-iree-shim.ps1 -Force
#
# Requires: clang++（默认；soft-skip 惯例同 e2e）。

param(
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Native = Join-Path $Root "target/iree-native"
$DllTarget = Join-Path $Native "iree_shim.dll"
$ShimSrc = Join-Path $Root "crates/runtime-iree/iree_shim.cpp"

if (!(Test-Path (Join-Path $Native "iree_runtime.dll"))) {
    throw "missing target/iree-native/iree_runtime.dll; run scripts\fetch-iree-native.ps1 first"
}
if (!(Test-Path $ShimSrc)) { throw "missing shim source: $ShimSrc" }
if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "iree_shim.dll already present ($DllTarget); use -Force to rebuild"
    exit 0
}

$Inc = Join-Path $Native "include"
$Log = Join-Path $Native "shim-build.log"

if (!(Get-Command clang++ -ErrorAction SilentlyContinue)) {
    throw "clang++ not found; install LLVM"
}

# 优先用现成 import lib（iree.lib）；缺失则经 lld-link 从 DLL 生成 import lib。
$ImportLib = Join-Path $Native "iree.lib"
if (!(Test-Path $ImportLib)) {
    $found = Get-ChildItem $Native -Filter "*.lib" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) { $ImportLib = $found.FullName }
}

Write-Host "Building iree_shim.dll with clang++ ..."

$ErrorActionPreference = "Continue"
if (Test-Path $ImportLib) {
    & clang++ -std=c++17 -O2 -shared -D_SCL_SECURE_NO_WARNINGS -D_CRT_SECURE_NO_WARNINGS `
        -D_CRT_NONSTDC_NO_WARNINGS -DNOMINMAX "-I$Inc" `
        $ShimSrc $ImportLib -o $DllTarget 2> $Log
} else {
    # 无 import lib：直链 DLL（clang++ 支持直接传 .dll；符号经 lld 解析）。
    & clang++ -std=c++17 -O2 -shared -D_SCL_SECURE_NO_WARNINGS -D_CRT_SECURE_NO_WARNINGS `
        -D_CRT_NONSTDC_NO_WARNINGS -DNOMINMAX "-I$Inc" `
        $ShimSrc (Join-Path $Native "iree_runtime.dll") -o $DllTarget 2> $Log
}
$ExitCode = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($ExitCode -ne 0) {
    Get-Content $Log | ForEach-Object { Write-Host "  $_" }
    throw "clang++ build failed (exit $ExitCode)"
}

if (!(Test-Path $DllTarget)) { throw "build did not produce $DllTarget" }

Write-Host "Built: $DllTarget ($(Get-Item $DllTarget).Length bytes)"
Write-Host ""
Write-Host "Verify exports (expect the iree_* symbols):"
if (Get-Command llvm-objdump -ErrorAction SilentlyContinue) {
    & llvm-objdump -p $DllTarget 2>$null |
        Select-String -Pattern "iree_create_runtime|iree_invoke|iree_create_buffer_float" |
        ForEach-Object { Write-Host "  $_" }
}
Write-Host ""
Write-Host "Run (all DLLs in ARC_IREE_LIB dir):"
Write-Host "  `$env:ARC_IREE_LIB = '$Native'"
