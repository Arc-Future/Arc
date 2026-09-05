# scripts/sdk-stage.ps1 —— Phase 0 判别性验证：打一个可重定位 SDK 目录。
#
# 把 `arc[.exe]`（命名随宿主） + 运行期资源排布为安装态布局（bin + lib/{std,rt,native}），
# 供「复制到任意目录仍能 `arc build`」验收（开发用；正式分发包见 arc-pack.ps1）：
#
#   <OutDir>/bin/arc(.exe)
#   <OutDir>/lib/std/                  ← 标准库源码树
#   <OutDir>/lib/rt/runtime/           ← runtime C 源码（crates/runtime）
#   <OutDir>/lib/rt/runtime-ui/        ← crates/runtime-ui（含 platform/、wgpu-native/）
#   <OutDir>/lib/rt/runtime-drawing/   ← crates/runtime-drawing
#   <OutDir>/lib/rt/runtime-sqlite/    ← crates/runtime-sqlite
#   <OutDir>/lib/rt/runtime-crypto/    ← crates/runtime-crypto（vendored 底座 + shim）
#   <OutDir>/lib/native/               ← 内置 .ani 契约（crates/arc/native）
#
# 用法（PowerShell 5.1 / pwsh core）：
#   ./scripts/sdk-stage.ps1 -OutDir "$env:TEMP\arc-sdk-test"
#   ./scripts/sdk-stage.ps1 -OutDir <dir> -Binary <arc(.exe) 路径> [-SkipBuild]
#
# 产物落点在调用方指定目录（默认 $env:TEMP），不写仓库。
param(
    [string]$OutDir = (Join-Path $env:TEMP "arc-sdk-test"),
    [string]$Binary = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

# 宿主判定（PS 5.1 无 $IsLinux/$IsMacOS——求值得 $null → Windows 语义，安全）。
$isUnixHost = ($IsLinux -or $IsMacOS) -eq $true
$exeName = if ($isUnixHost) { "arc" } else { "arc.exe" }

if (-not $Binary) {
    if (-not $SkipBuild) {
        & cargo build --release -p arc --manifest-path (Join-Path $repo "Cargo.toml")
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release -p arc failed" }
    }
    $Binary = Join-Path $repo ("target/release/" + $exeName)
}
if (-not (Test-Path $Binary)) { throw "$exeName not found: $Binary" }

if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "bin") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutDir "lib") | Out-Null

$binTarget = Join-Path $OutDir ("bin/" + $exeName)
Copy-Item $Binary $binTarget
if ($isUnixHost) { & chmod +x $binTarget }

# std
Copy-Item (Join-Path $repo "std") (Join-Path $OutDir "lib\std") -Recurse
# runtime C 源码（rt/ 子目录）
Copy-Item (Join-Path $repo "crates\runtime")        (Join-Path $OutDir "lib\rt\runtime") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-ui")     (Join-Path $OutDir "lib\rt\runtime-ui") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-drawing") (Join-Path $OutDir "lib\rt\runtime-drawing") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-sqlite")  (Join-Path $OutDir "lib\rt\runtime-sqlite") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-crypto")  (Join-Path $OutDir "lib\rt\runtime-crypto") -Recurse
# 内置 native 契约
Copy-Item (Join-Path $repo "crates\arc\native")     (Join-Path $OutDir "lib\native") -Recurse

Write-Host "SDK staged at: $OutDir"
Write-Host "  bin/$exeName : $((Get-Item $binTarget).Length) bytes"
