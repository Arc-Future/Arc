# scripts/packaging/arc-pack.ps1 — Phase 1/3：Arc SDK 分发包打包（Windows zip）
#
# 演进自 scripts/sdk-stage.ps1（Phase 0 判别性 staging）：release 构建 → 打安装态
# 目录（bin/ + lib/{std,rt,native}）→ 产出 zip + SHA256 → 判别验收（解包异地
# 冷构建，离线、无仓库依赖）。
#
# 产物（默认 $repo/target/dist，已在 /target/ 忽略下，不污染源码树）：
#   arc-<ver>-<triple>.zip
#   arc-<ver>-<triple>.zip.sha256
#
# zip 内布局（安装态 SDK，与 codegen::sdk_layout 契约一致）：
#   arc-<ver>-<triple>/
#   ├── bin/arc.exe
#   ├── lib/std/                     ← 标准库源码树
#   ├── lib/rt/runtime/ … runtime-ui/ runtime-drawing/ runtime-sqlite/ runtime-crypto/
#   │                                 ← runtime C 源码 + vendored native DLL
#   ├── lib/native/                  ← 内置 .ani 契约
#   ├── lib/llvm/                    ← 仅 -BundleLlm：瘦身版 LLVM（clang + lld 子集）
#   ├── version.txt                  ← 版本/目标/提交
#   └── arc.env                      ← 环境变量说明模板
#
# 用法（仓库根）：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -SkipBuild
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -SkipVerify -OutDir D:\dist
#   # Phase 3：捆绑瘦身版 LLVM（clang + lld 子集，供离线用户）到 lib/llvm/
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -BundleLlm
#   # Phase 2：同时产出签名发布清单（需 $env:ARC_RELEASE_SIGNING_KEY = 64 hex seed）
#   $env:ARC_RELEASE_SIGNING_KEY = "<seed>"
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -Manifest -ReleaseUrlPrefix https://static.arc.dev/dist

param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$SkipVerify,
    [switch]$Manifest,
    [string]$ReleaseUrlPrefix = "",
    # Phase 3：把本机 LLVM（clang + lld 子集）打迸 SDK `lib/llvm/`，并生成
    # `arc.env` 中 `ARC_CLANG=<sdk>/lib/llvm/bin/clang.exe`，供离线用户使用
    # 捆绑 clang 完成 `arc build`。从 `$env:ARC_CLANG` 或标准 LLVM 安装位定位。
    [switch]$BundleLlm,
    # Authenticode 代码签名：签名证书指纹（Cert:\CurrentUser\My 下）。
    # 设置后对包内 arc.exe 做 signtool /fd SHA256 签名（作者/版权元数据由
    # crates/arc/build.rs 的 PE 版本资源随编译嵌入）。空 = 不签名。
    [string]$SignThumbprint = ""
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $OutDir) { $OutDir = Join-Path $repo "target\dist" }

# PowerShell 5.1 的 Set-Content -Encoding UTF8 会写 BOM，而 Arc 词法器不接受 BOM；
# 所有源码/清单文件统一用无 BOM UTF-8 写入。
function Write-Utf8NoBom([string]$Path, [string]$Content) {
    [System.IO.File]::WriteAllText($Path, $Content, (New-Object System.Text.UTF8Encoding($false)))
}

# --- 0b. Bundle-LLVM 辅助 ---
# 瘦身版 LLVM 原则：仅交付 `arc build` 链路必需的可执行文件（clang 驱动 + lld
# 链接器族），不包含 clangd/lldb/Flang/OpenMP 等扩展工具，避免 SDK 包体膨胀
# 数十倍（LLVM 官方 Windows 安装器 ~455 MB）。clang.exe 在官方 Windows 发行中
# 为自包含单文件（无运行时 DLL 依赖），与本机 lld 工具同目录即可完成编译+链接。
# 定位链：`ARC_CLANG` 环境变量 → 标准 LLVM 安装位 → PATH 上 `clang`。
function Find-ClangBinary {
    if ($env:ARC_CLANG) {
        $p = $env:ARC_CLANG.Trim()
        if ($p -and (Test-Path $p)) { return $p }
        if ($p -eq "clang") { return "clang" }
        Write-Warning "ARC_CLANG=$p does not exist; falling back to standard install locations"
    }
    foreach ($cand in @(
        "C:\Program Files\LLVM\bin\clang.exe",
        "C:\Program Files (x86)\LLVM\bin\clang.exe"
    )) {
        if (Test-Path $cand) { return $cand }
    }
    $onPath = Get-Command clang -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    return $null
}

# 从 LLVM bin 目录复制瘦身子集到 `<pkgDir>/lib/llvm/`。
# 设计原则（RFC 031 §13.2）：仅 `arc build` 链路必需工具——clang 驱动 + lld
# 链接器族 + Windows 资源编译器 + clang 资源目录（头文件/内建库）。clang-cl/
# clang++（C++/MSVC 兼容驱动）、ld.lld/ld64.lld（ELF/Mach-O 别名，由 lld.exe
# 多调用分派覆盖）、clangd/lldb/Flang/OpenMP 等扩展工具均不打包（LLVM 官方
# Windows 安装器 ~455 MB，全量安装 ~1.5 GB）。
# clang.exe 在官方发行中自包含（无运行时 DLL）；但其资源目录 `lib/clang/<ver>/`
# 缺失会导致 SIMD 头（emmintrin.h 等）回退到 MSVC 头而产出外部 `_mm_*` 符号，
# 故**必须**捆绑 `include/`（头文件）与 `clang_rt.builtins-*.lib`（compiler-rt）。
function Copy-LlvmSlimSubset([string]$LlvmBin, [string]$DestBin) {
    $required = @(
        "clang.exe",           # 编译器驱动（arc build 恒用，Debug/Release）
        "lld-link.exe",        # MSVC 目标 Release 链接（-fuse-ld=lld-link）
        "lld.exe",             # lld 多调用分派（ELF/COFF/Mach-O 通用）
        "llvm-rc.exe"          # Windows 资源编译（clang 处理 .rc 时调用）
    )
    New-Item -ItemType Directory -Force -Path $DestBin | Out-Null
    $copied = @()
    $missing = @()
    foreach ($name in $required) {
        $src = Join-Path $LlvmBin $name
        if (Test-Path $src) {
            Copy-Item $src (Join-Path $DestBin $name) -Force
            $copied += $name
        } else {
            $missing += $name
        }
    }
    # clang.exe / lld-link.exe 属硬依赖；其余缺失（如 LLVM 无 llvm-rc）仅提示。
    foreach ($hard in @("clang.exe", "lld-link.exe")) {
        if (-not (Test-Path (Join-Path $DestBin $hard))) {
            throw "$hard not found in $LlvmBin (bundle aborted)"
        }
    }
    Write-Host "==> bundled LLVM subset ($($copied.Count) tools): $($copied -join ', ')"
    if ($missing.Count -gt 0) {
        Write-Warning "LLVM bin missing optional tools: $($missing -join ', ')"
    }

    # clang 资源目录：`<LLVM_ROOT>/lib/clang/<ver>/`。clang.exe 按相对自身路径
    # `../lib/clang/<ver>` 自定位，复制后无需任何配置。只取 `include/`（头文件）
    # 与 `clang_rt.builtins-*.lib`（compiler-rt 内建库）；asan/ubsan 等 sanitizer
    # 库不打包（仅 `-fsanitize=` 构建需要，可后续按需）。
    $llvmRoot = Split-Path $LlvmBin -Parent
    $clangRes = Join-Path $llvmRoot "lib\clang"
    if (Test-Path $clangRes) {
        foreach ($vdir in (Get-ChildItem $clangRes -Directory -ErrorAction SilentlyContinue)) {
            $destVer = Join-Path (Join-Path (Split-Path $DestBin -Parent) "lib\clang") $vdir.Name
            if (Test-Path (Join-Path $vdir.FullName "include")) {
                Copy-Item (Join-Path $vdir.FullName "include") (Join-Path $destVer "include") -Recurse -Force
            }
            $winLib = Join-Path $vdir.FullName "lib\windows"
            if (Test-Path $winLib) {
                foreach ($b in (Get-ChildItem $winLib -Filter "clang_rt.builtins-*.lib" -ErrorAction SilentlyContinue)) {
                    $dst = Join-Path (Join-Path $destVer "lib\windows") $b.Name
                    New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
                    Copy-Item $b.FullName $dst -Force
                }
            }
            Write-Host "==> bundled clang resource dir: lib\llvm\lib\clang\$($vdir.Name) (headers + builtins)"
        }
    }

    # 探测：自包含则停；失败则复制 *.dll 兼容（非官方发行）。
    & (Join-Path $DestBin "clang.exe") --version | Out-Null
    if ($LASTEXITCODE -ne 0) {
        $dlls = Get-ChildItem $LlvmBin -Filter "*.dll" -ErrorAction SilentlyContinue
        foreach ($dll in $dlls) {
            Copy-Item $dll.FullName (Join-Path $DestBin $dll.Name) -Force
        }
        if ($dlls) {
            Write-Warning "clang needs runtime DLLs (non-official build); copied $(@($dlls).Count) DLL(s)"
        }
    } else {
        Write-Host "==> bundled clang is self-contained (no runtime DLLs copied)"
    }
}

function Write-EnvTemplate([string]$Path, [string]$SdkRootHint, [bool]$Bundled) {
    $llvmSection = if ($Bundled) {
        @"

# Bundled LLVM (Phase 3, -BundleLlm): 瘦身版 clang + lld 子集位于 SDK 内
# `<sdk-root>/lib/llvm/bin`。把 `ARC_CLANG` 指向其 clang 即可完全离线构建：
#   ARC_CLANG=$SdkRootHint/lib/llvm/bin/clang.exe
"@
    } else { "" }
    $content = @"
# Arc SDK environment — reference template.
#
# The SDK self-locates via arc.exe (current_exe() walk-up), so nothing here is
# required for 'arc build' to work. These knobs exist for explicit overrides
# (see docs/rfc/031-compiler-cli.md §10 环境变量清单).
#
#   ARC_SDK_ROOT=<sdk-root>    explicit SDK root (highest priority)
#   ARC_STD_ROOT=<dir>         explicit std library root (development override)
#   ARC_HOME=<dir>             user toolchain home (cache / rt_cache / keys)
#   ARC_CLANG=<clang.exe>      explicit clang binary$llvmSection
#   PATH=<sdk-root>/bin;...    PATH entry for this SDK's bin directory
"@
    Write-Utf8NoBom $Path $content
}

# --- 1. release 构建 ---
if (-not $SkipBuild) {
    Write-Host "==> cargo build --release -p arc"
    & cargo build --release -p arc --manifest-path (Join-Path $repo "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release -p arc failed" }
}

$binary = Join-Path $repo "target\release\arc.exe"
if (-not (Test-Path $binary)) { throw "arc.exe not found: $binary (run without -SkipBuild first)" }

# --- 2. 版本 / 目标三元组 ---
$version = (& $binary --version) -replace "^arc ", ""
if (-not $version) { throw "failed to read arc version" }
$envJson = & $binary env --json | ConvertFrom-Json
$triple = $envJson.HOST_TRIPLE
if (-not $triple) { throw "failed to read host triple from arc env --json" }
$commit = & git -C $repo rev-parse --short HEAD
if ($LASTEXITCODE -ne 0) { $commit = "unknown" }
$pkgName = "arc-$version-$triple"
Write-Host "==> packing $pkgName (commit $commit)"

# --- 3. 打安装态目录 ---
$stageRoot = Join-Path (Join-Path $repo "target") "staging"
$pkgDir = Join-Path $stageRoot $pkgName
if (Test-Path $pkgDir) { Remove-Item $pkgDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $pkgDir "bin")  | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $pkgDir "lib")  | Out-Null

Copy-Item $binary (Join-Path $pkgDir "bin\arc.exe")
# std
Copy-Item (Join-Path $repo "std") (Join-Path $pkgDir "lib\std") -Recurse
# runtime C 源码 + vendored 底座（crypto_native / wgpu_native DLL 随包）
Copy-Item (Join-Path $repo "crates\runtime")         (Join-Path $pkgDir "lib\rt\runtime") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-ui")      (Join-Path $pkgDir "lib\rt\runtime-ui") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-drawing") (Join-Path $pkgDir "lib\rt\runtime-drawing") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-sqlite")  (Join-Path $pkgDir "lib\rt\runtime-sqlite") -Recurse
Copy-Item (Join-Path $repo "crates\runtime-crypto")  (Join-Path $pkgDir "lib\rt\runtime-crypto") -Recurse
# 内置 native 契约
Copy-Item (Join-Path $repo "crates\arc\native")      (Join-Path $pkgDir "lib\native") -Recurse

# --- 3b. -BundleLlm：瘦身版 LLVM（clang + lld 子集）→ lib/llvm/ ---
$bundledLlvm = $false
if ($BundleLlm) {
    $clang = Find-ClangBinary
    if (-not $clang) {
        Write-Warning "-BundleLlm requested but no clang found (ARC_CLANG / C:\Program Files\LLVM / PATH). Skipping LLVM bundle."
    } else {
        $clangBin = Split-Path $clang -Parent
        & $clang --version | Select-Object -First 1
        if ($LASTEXITCODE -ne 0) { throw "bundled clang failed its --version probe: $clang" }
        Copy-LlvmSlimSubset $clangBin (Join-Path $pkgDir "lib\llvm\bin")
        $bundledLlvm = $true
        Write-Host "==> llvm bundled: $(Join-Path $pkgDir "lib\llvm\bin\clang.exe")"
    }
}

# --- 4. version.txt / arc.env（无 BOM UTF-8）---
# 作者/版权信息与 crates/arc/build.rs 的 PE 版本资源同源（单一事实：Cargo authors）。
$versionContent = @"
arc=$version
author=LUSIDA (Start)
copyright=Copyright (C) 2026 LUSIDA (Start)
triple=$triple
commit=$commit
layout=installed
llvm=$(if ($bundledLlvm) { "bundled" } else { "external" })
signed=$(if ($SignThumbprint) { "authenticode" } else { "none" })
"@
Write-Utf8NoBom (Join-Path $pkgDir "version.txt") $versionContent

# --- 4b. Authenticode 代码签名（可选，-SignThumbprint）---
# 对包内 arc.exe 做 /fd SHA256 签名——须在 Compress-Archive **之前**（zip 内
# 的字节即已含签名）。正式发布请使用 CA 签发的 OV/EV 证书；本地测试可用
# New-SelfSignedCertificate -Type CodeSigningCert（验证链须入受信任存储）。
if ($SignThumbprint) {
    $signtool = $null
    $kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
    if (Test-Path $kits) {
        $signtool = Get-ChildItem $kits -Directory | Sort-Object Name -Descending | ForEach-Object {
            Join-Path $_.FullName 'x64\signtool.exe'
        } | Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $signtool) { throw "signtool.exe not found under $kits (install Windows SDK)" }
    $pkgExe = Join-Path $pkgDir "bin\arc.exe"
    Write-Host "==> signing arc.exe (thumbprint $SignThumbprint)"
    & $signtool sign /fd SHA256 /sha1 $SignThumbprint /d 'Arc Compiler CLI' $pkgExe 2>&1 | ForEach-Object { Write-Host "    $_" }
    if ($LASTEXITCODE -ne 0) { throw "signtool sign failed (exit $LASTEXITCODE)" }
    $sigStatus = (Get-AuthenticodeSignature $pkgExe).Status
    if ($sigStatus -ne 'Valid') { throw "signature status after sign: $sigStatus" }
    Write-Host "==> signature ok: $sigStatus"
}

Write-EnvTemplate (Join-Path $pkgDir "arc.env") "<sdk-root>" $bundledLlvm

# --- 5. zip + SHA256 ---
$zipPath = Join-Path $OutDir "${pkgName}.zip"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Write-Host "==> Compress-Archive -> $zipPath"
Compress-Archive -Path $pkgDir -DestinationPath $zipPath -CompressionLevel Optimal
$hash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
"${hash}  ${pkgName}.zip" | Set-Content -Encoding ASCII (Join-Path $OutDir "${pkgName}.zip.sha256")
Write-Host "==> sha256: $hash"
Write-Host "==> package size: $((Get-Item $zipPath).Length) bytes"

# --- 5b. Phase 2：签名发布清单（manifest.json + manifest.json.sig）---
# 需 $env:ARC_RELEASE_SIGNING_KEY（64 hex seed，`arc release keygen` 生成）。
if ($Manifest) {
    if (-not $env:ARC_RELEASE_SIGNING_KEY) {
        Write-Error "arc-pack.ps1: -Manifest requires `$env:ARC_RELEASE_SIGNING_KEY (64 hex seed; generate via `arc release keygen`)"
        exit 1
    }
    $manifestArgs = @("release", "manifest", "--version", $version, "--triple", $triple, "--archive", $zipPath, "--output", $OutDir)
    if ($ReleaseUrlPrefix) { $manifestArgs += @("--url-prefix", $ReleaseUrlPrefix) }
    Write-Host "==> generating signed manifest -> $OutDir"
    & $binary @manifestArgs
    if ($LASTEXITCODE -ne 0) { throw "arc release manifest failed (exit $LASTEXITCODE)" }
    Write-Host "==> manifest.json + manifest.json.sig written to $OutDir"
}

# --- 6. 判别验收：解包异地冷构建（离线、无仓库依赖）---
if (-not $SkipVerify) {    Write-Host "==> verify: extract to temp + offline cold build"
    $verifyRoot = Join-Path $env:TEMP "arc-pack-verify-$PID"
    if (Test-Path $verifyRoot) { Remove-Item $verifyRoot -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $verifyRoot | Out-Null
    Expand-Archive -Path $zipPath -DestinationPath $verifyRoot -Force
    $sdkDir = Join-Path $verifyRoot $pkgName
    $arc = Join-Path $sdkDir "bin\arc.exe"

    # 判别环境隔离：清 ARC_SDK_ROOT（自定位必须权威）、ARC_HOME 指向 verifyRoot
    # 下的临时域（不触碰用户真实 ~/.arc），ARC_CLANG 仅在 bundle 判别内临时设置。
    $prevSdkRoot = $env:ARC_SDK_ROOT
    $prevArcHome = $env:ARC_HOME
    $prevClang = $env:ARC_CLANG
    $env:ARC_SDK_ROOT = $null
    $env:ARC_HOME = Join-Path $verifyRoot "home"
    $env:ARC_CLANG = $null
    # Bundle 判别：整个验证（含离线冷构建）均用捆绑 clang，证明「捆绑版 LLVM
    # 满足离线用户」——设了 ARC_CLANG 指向 lib/llvm 即可完全离线 build。
    # 非 bundle 时 ARC_CLANG 保持未设，冷构建走 codegen 标准解析序（系统 clang）。
    if ($bundledLlvm) {
        $env:ARC_CLANG = Join-Path $sdkDir "lib\llvm\bin\clang.exe"
    }
    try {

    # 0. Bundle-LLVM 判别：arc env 反映捆绑 clang；arc doctor 用捆绑 clang 全绿；
    #    直接探测捆绑 clang + lld-link 完整编译+链接（Release 链路用 `-fuse-ld=lld-link`）。
    if ($bundledLlvm) {
        $bundleClang = $env:ARC_CLANG
        if (-not (Test-Path $bundleClang)) { throw "verify failed: bundled clang missing at $bundleClang" }
        $envOut = & $arc env --json | ConvertFrom-Json
        if ($envOut.ARC_CLANG -ne $bundleClang) {
            throw "verify failed: ARC_CLANG=$($envOut.ARC_CLANG) (expected $bundleClang)"
        }
        Write-Host "    ok: arc env reports bundled clang at $bundleClang"
        & $arc doctor 2>&1 | ForEach-Object { Write-Host "    $_" }
        if ($LASTEXITCODE -ne 0) { throw "verify failed: arc doctor FAIL with bundled clang" }
        Write-Host "    ok: arc doctor passes with bundled LLVM"
        # 编译 + 链接探测：证明捆绑 lld-link 能完成 Release 式 `-fuse-ld=lld-link` 链路。
        $probeC = Join-Path $verifyRoot "bundle_probe.c"
        $probeExe = Join-Path $verifyRoot "bundle_probe.exe"
        Set-Content -Path $probeC -Value "int main(void) { return 0; }"
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            & $bundleClang $probeC -o $probeExe -fuse-ld=lld-link 2>&1 | ForEach-Object { Write-Host "    $_" }
            $probeExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $prevEap
        }
        if ($probeExit -ne 0 -or -not (Test-Path $probeExe)) {
            throw "verify failed: bundled clang + lld-link compile/link probe failed"
        }
        Write-Host "    ok: bundled clang + lld-link compile+link probe passed"
    }

    # 1. 自定位正确（SDK_LAYOUT=installed，且 ARC_SDK_ROOT 解析到解包目录）
    $envOut = & $arc env --json | ConvertFrom-Json
    if ($envOut.SDK_LAYOUT -ne "installed") { throw "verify failed: SDK_LAYOUT=$($envOut.SDK_LAYOUT) (expected installed)" }
    if ($envOut.ARC_SDK_ROOT -ne $sdkDir) { throw "verify failed: ARC_SDK_ROOT=$($envOut.ARC_SDK_ROOT) (expected $sdkDir)" }
    if ($envOut.STD_SOURCE -ne "sdk") { throw "verify failed: STD_SOURCE=$($envOut.STD_SOURCE) (expected sdk)" }
    Write-Host "    ok: arc env self-locates SDK at $($envOut.ARC_SDK_ROOT)"

    # 6b. 离线冷构建示例（不依赖仓库；std 取自包内 lib/std，runtime C 取自包内 lib/rt）
    $proj = Join-Path $verifyRoot "hello"
    New-Item -ItemType Directory -Force -Path $proj | Out-Null
    Write-Utf8NoBom (Join-Path $proj "arc.toml") "[package]`nname = `"hello`"`nedition = `"1`"`n"
    $mainAs = @"
using Arc;

void Main() {
    Console.WriteLine("hello from packaged SDK");
}
"@
    Write-Utf8NoBom (Join-Path $proj "main.as") $mainAs
    Push-Location $proj
    try {
        # 传绝对项目路径（`arc build .` 对 `.` 无文件名词干，产物会退化为 out.exe）。
        # PS 5.1 下 $ErrorActionPreference=Stop 会把 native stderr 当终止错误，
        # 故围绕 arc 调用临时降为 Continue，统一按 $LASTEXITCODE 判定。
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $buildOut = & $arc build $proj 2>&1
            $buildExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $prevEap
        }
        $buildOut | ForEach-Object { Write-Host "    $_" }
        if ($buildExit -ne 0) { throw "verify failed: arc build in extracted SDK failed (exit $buildExit)" }
        $exe = Join-Path $proj "bin\Debug\hello.exe"
        if (-not (Test-Path $exe)) { throw "verify failed: expected binary $exe" }
        # PS 5.1 对经 WriteFile(GetStdHandle) 输出的原生程序捕获不可靠（cmd/.NET
        # 管道均正常），故用 cmd /c 运行并捕获 stdout。
        $runOut = (cmd /c "`"$exe`"") -join "`n"
        if ($runOut.Trim() -ne "hello from packaged SDK") { throw "verify failed: binary output '$runOut'" }
        Write-Host "    ok: offline cold build + run succeeded in extracted SDK"
    } finally {
        Pop-Location
    }

    # 6c. Installed-SDK development acceptance: consume std beyond the implicit
    #     Arc root - Arc.Collections (List<int> generics + namespace-index source
    #     pull from lib/std), proving users can do real Arc project development
    #     right after install.
    $projStd = Join-Path $verifyRoot "stdapp"
    New-Item -ItemType Directory -Force -Path $projStd | Out-Null
    Write-Utf8NoBom (Join-Path $projStd "arc.toml") "[package]`nname = `"stdapp`"`nversion = `"0.1.0`"`nedition = `"1`"`n"
    $stdMain = @"
using Arc;
using Arc.Collections;

void Main() {
    List<int> xs = new List<int>();
    xs.Add(40);
    xs.Add(2);
    int sum = 0;
    for (int i = 0; i < xs.Count; i = i + 1) {
        sum = sum + xs[i];
    }
    Console.WriteLine("std:ok " + sum);
}
"@
    Write-Utf8NoBom (Join-Path $projStd "main.as") $stdMain
    Push-Location $projStd
    try {
        $prevEap2 = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $buildOut2 = & $arc build $projStd 2>&1
            $buildExit2 = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $prevEap2
        }
        $buildOut2 | ForEach-Object { Write-Host "    $_" }
        if ($buildExit2 -ne 0) { throw "verify failed: std-consuming build failed (exit $buildExit2)" }
        $exe2 = Join-Path $projStd "bin\Debug\stdapp.exe"
        if (-not (Test-Path $exe2)) { throw "verify failed: expected stdapp binary $exe2" }
        $runOut2 = (cmd /c "`"$exe2`"") -join "`n"
        if ($runOut2.Trim() -ne "std:ok 42") { throw "verify failed: stdapp output '$runOut2'" }
        Write-Host "    ok: installed SDK builds a std-consuming project (Arc.Collections)"
    } finally {
        Pop-Location
    }

    } finally {
        $env:ARC_SDK_ROOT = $prevSdkRoot
        $env:ARC_HOME = $prevArcHome
        $env:ARC_CLANG = $prevClang
    }
    Remove-Item $verifyRoot -Recurse -Force
}

Write-Host "==> done: $zipPath"
