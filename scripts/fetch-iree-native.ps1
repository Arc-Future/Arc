# fetch-iree-native.ps1 - vendor IREE Runtime for Arc.AI.Iree
#
# Arc.AI.Iree 封装 IREE Runtime 的高层 C API（`crates/runtime-iree/iree_shim.{h,cpp}`
# 把 `iree/runtime/api.h` 包成 extern "C" C ABI，见 iree.ani 契约）。IREE Runtime 是
# 重量级外部依赖，**不 vendored 进仓库**（工作区卫生 G″，对齐 onnx/zxing 先例）：
# 本脚本只负责**外部下载/分发**，产物落
#   target/iree-native/
#     include/            （iree/runtime/api.h 等头）
#     iree.dll            （IREE runtime native DLL）
#     iree.lib            （import lib，供 iree_shim 链接）
#     SHA256.txt          （下载包 SHA256 记录，版本锁定）
#
# 来源：`iree-runtime` PyPI wheel（官方发布 IREE runtime 的 Python 分发，内含
# Windows win-amd64 的 runtime DLL + 头 + import lib）。真实 IREE Runtime 依赖多个
# DLL（iree_runtime.dll 及其组件），fetch 时一并抽取到同一目录。
#
# Usage: powershell -File .\scripts\fetch-iree-native.ps1                    (default version)
#        powershell -File .\scripts\fetch-iree-native.ps1 -Version 3.2.0
#        powershell -File .\scripts\fetch-iree-native.ps1 -Force            (re-download)
#
# Idempotent: skips when target/iree-native/iree_runtime.dll already present unless -Force.
# Hygiene: download/extract entirely under $env:TEMP; only final artifacts go to target/.
#
# 宣称纪律（RFC 025 §1.1）：首次运行时本脚本计算并记录下载包 SHA256 到 SHA256.txt。
# 正式版本锁定前，请将记录的哈希与上游 release 公告核验后回填进 VENDOR.md；未核验
# 不得宣称"已固定"。

param(
    [string]$Version = "3.2.0",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$OutDir = Join-Path $Root "target/iree-native"
$DllTarget = Join-Path $OutDir "iree_runtime.dll"

if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "iree_runtime.dll already present ($OutDir); use -Force to re-fetch"
    exit 0
}

Write-Host "Fetching iree-runtime $Version (IREE Runtime)..."
Write-Host "  source: PyPI iree-runtime wheel"

$Work = Join-Path $env:TEMP "iree-native-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $Work -Force | Out-Null

try {
    # 经 PyPI JSON API 解析 iree-runtime wheel 直链（确定性、无平台猜测）。
    $jsonUrl = "https://pypi.org/pypi/iree-runtime/$Version/json"
    $release = Invoke-RestMethod -Uri $jsonUrl
    $winWheel = $release.urls | Where-Object {
        $_.packagetype -eq "bdist_wheel" -and $_.filename -match "win_amd64" -and $_.filename -notmatch "manylinux|macos"
    } | Select-Object -First 1
    if (!$winWheel) {
        throw "no Windows win_amd64 wheel for iree-runtime $Version (verify version exists on PyPI)"
    }

    $WheelUrl = $winWheel.url
    $AssetName = $winWheel.filename
    $Nupkg = Join-Path $Work $AssetName

    Write-Host "  URL: $WheelUrl"
    curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $Nupkg $WheelUrl
    if ($LASTEXITCODE -ne 0) {
        throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
    }

    $actual = (Get-FileHash $Nupkg -Algorithm SHA256).Hash.ToUpper()
    Write-Host "SHA256: $actual  (recorded to SHA256.txt; verify against upstream release)"

    # wheel 即 zip：抽到 TEMP 后定位 win-amd64 runtime 资产。
    $Extract = Join-Path $Work "extract"
    Expand-Archive -Path $Nupkg -DestinationPath $Extract -Force

    $NativeRoot = Join-Path $Extract "iree/runtime"
    $Candidates = @(
        (Join-Path $NativeRoot "iree_runtime.dll"),
        (Get-ChildItem $Extract -Recurse -Filter iree_runtime.dll -ErrorAction SilentlyContinue | Select-Object -First 1)
    )
    $Dll = $Candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (!$Dll) { throw "iree_runtime.dll not found in wheel (unexpected layout)" }
    $Dll = if ($Dll -is [string]) { $Dll } else { $Dll.FullName }

    # import lib：wheel 通常不含 .lib；若缺则由 build-iree-shim.ps1 用 clang 直链 DLL
    # 生成（dlltool / lld-link -dll）。此处尝试定位现成 .lib。
    $Lib = Get-ChildItem $Extract -Recurse -Filter "iree*.lib" -ErrorAction SilentlyContinue |
        Select-Object -First 1

    # 头目录：iree/runtime/ 或顶层 include/。
    $Inc = Join-Path $Extract "include"
    if (!(Test-Path (Join-Path $Inc "iree/runtime/api.h"))) { $Inc = $NativeRoot }

    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    Copy-Item $Dll $DllTarget -Force
    if ($Lib) { Copy-Item $Lib.FullName (Join-Path $OutDir $Lib.Name) -Force }

    # 抽取同目录所有 IREE runtime 依赖 DLL（组件较多，一并拷贝保证加载完整）。
    Get-ChildItem $Extract -Recurse -Filter "*.dll" -ErrorAction SilentlyContinue |
        ForEach-Object { Copy-Item $_.FullName (Join-Path $OutDir $_.Name) -Force }

    if (Test-Path (Join-Path $Inc "iree/runtime/api.h")) {
        Copy-Item $Inc (Join-Path $OutDir "include") -Recurse -Force
    } else {
        throw "iree/runtime/api.h not found in wheel"
    }

    # 版本 / 哈希 / 许可记录（版本锁定证据）。
    Set-Content (Join-Path $OutDir "version.txt") $Version -Encoding ascii
    Set-Content (Join-Path $OutDir "source.txt") "PyPI iree-runtime" -Encoding ascii
    Set-Content (Join-Path $OutDir "SHA256.txt") $actual -Encoding ascii
    $License = Get-ChildItem $Extract -Recurse -Filter "LICENSE*" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($License) { Copy-Item $License.FullName (Join-Path $OutDir "LICENSE") -Force }

    Write-Host "Vendored to $OutDir :"
    Write-Host "  iree_runtime.dll  ($(Get-Item $DllTarget).Length bytes)"
    Write-Host "  include\iree\runtime\api.h"
    Write-Host "  SHA256.txt = $actual"
    Write-Host ""
    Write-Host "Next: powershell -File .\scripts\build-iree-shim.ps1"
    Write-Host "Then set ARC_IREE_LIB=$OutDir and run the IREE inference e2e."
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
