# scripts/packaging/arc-pack.ps1 — Phase 1/3：Arc SDK 分发包打包（宿主感知）
#
# 演进自 scripts/sdk-stage.ps1（Phase 0 判别性 staging）：release 构建 → 打安装态
# 目录（bin/ + lib/{std,rt,native}）→ 产出容器 + SHA256 → 判别验收（解包异地
# 冷构建，离线、无仓库依赖）。
#
# 产物（默认 $repo/target/dist，已在 /target/ 忽略下，不污染源码树）：
#   Windows 宿主：arc-<ver>-<triple>.zip
#   Unix 宿主（Linux/macOS，pwsh core）：arc-<ver>-<triple>.tar.xz
#   均附 .sha256 清单。
#
# 容器内布局（安装态 SDK，与 codegen::sdk_layout 契约一致；exe 名随平台）：
#   arc-<ver>-<triple>/
#   ├── bin/arc[.exe]
#   ├── lib/std/                     ← 标准库源码树
#   ├── lib/rt/runtime/ … runtime-ui/ runtime-drawing/ runtime-sqlite/ runtime-crypto/
#   │                                 ← runtime C 源码 + vendored native DLL
#   ├── lib/native/                  ← 内置 .ani 契约
#   ├── lib/llvm/                    ← 仅 -BundleLlm：瘦身版 LLVM（clang + lld 子集）
#   ├── install.ps1 / install.sh     ← 就地安装器（随宿主；仓库脚本同源嵌入）
#   ├── version.txt                  ← 版本/目标/提交
#   └── arc.env                      ← 环境变量说明模板
#
# Unix 说明：tar.xz 需系统 `tar`（含 xz 支持；macOS 需自装 xz：brew install xz）；
# bin/arc、lib/llvm/bin/* 与 install.sh 在归档前显式 chmod +x（tar 保留权限位，
# 解压即得可执行 SDK）。产线应在目标宿主上执行（或 CI 对应 OS job）。
#
# 用法（仓库根；PowerShell 5.1 / pwsh core）：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -SkipBuild
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -SkipVerify -OutDir D:\dist
#   # Phase 3：捆绑瘦身版 LLVM（clang + lld 子集，供离线用户）到 lib/llvm/
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -BundleLlm
#   # Unix 宿主（Linux/macOS）示例：
#   pwsh -NoProfile -File scripts/packaging/arc-pack.ps1 -SkipVerify
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

# 宿主判定（PS 5.1 无 $IsLinux/$IsMacOS——求值得 $null → Windows 语义，安全）。
$isUnixHost = ($IsLinux -or $IsMacOS) -eq $true
$exeName = if ($isUnixHost) { "arc" } else { "arc.exe" }
$exeSuffix = if ($isUnixHost) { "" } else { ".exe" }
$containerExt = if ($isUnixHost) { "tar.xz" } else { "zip" }
$installScriptName = if ($isUnixHost) { "install.sh" } else { "install.ps1" }

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
# 定位链：`ARC_CLANG` 环境变量 → 标准安装位（随宿主）→ PATH 上 `clang`。
function Find-ClangBinary {
    if ($env:ARC_CLANG) {
        $p = $env:ARC_CLANG.Trim()
        if ($p -and (Test-Path $p)) { return $p }
        if ($p -eq "clang") { return "clang" }
        Write-Warning "ARC_CLANG=$p does not exist; falling back to standard install locations"
    }
    $candidates = if ($isUnixHost) {
        @(
            "/usr/local/opt/llvm/bin/clang",          # Homebrew（macOS / Linux）
            "/opt/homebrew/opt/llvm/bin/clang",        # Apple Silicon Homebrew
            "/usr/lib/llvm-18/bin/clang",
            "/usr/lib/llvm-17/bin/clang",
            "/usr/bin/clang"
        )
    } else {
        @(
            "C:\Program Files\LLVM\bin\clang.exe",
            "C:\Program Files (x86)\LLVM\bin\clang.exe"
        )
    }
    foreach ($cand in $candidates) {
        if (Test-Path $cand) { return $cand }
    }
    $onPath = Get-Command clang -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    return $null
}

# 从 LLVM bin 目录复制瘦身子集到 `<pkgDir>/lib/llvm/`。
# 设计原则（RFC 031 §13.2）：仅 `arc build` 链路必需工具——clang 驱动 + lld
# 链接器族 + clang 资源目录（头文件/内建库）。工具名单随宿主：
#   Windows：clang.exe + lld-link.exe + lld.exe + llvm-rc.exe（.rc 资源编译）
#   Unix：clang + lld（多调用分派）+ ld.lld（ELF `-fuse-ld=lld` 查找名）+
#         ld64.lld（macOS；缺失仅提示，可回落系统 ld64）
# clang-cl/clang++（C++/MSVC 兼容驱动）、clangd/lldb/Flang/OpenMP 等扩展工具
# 均不打包。clang 资源目录 `lib/clang/<ver>/` 缺失会导致 SIMD 头（emmintrin.h
# 等）回退到系统头而产出外部 `_mm_*` 符号，故**必须**捆绑 `include/`（头文件）
# 与 compiler-rt 内建库（Windows `lib/windows/clang_rt.builtins-*.lib`，
# Unix `lib/**/libclang_rt.builtins-*`——保留相对布局供 clang 自定位）。
function Copy-LlvmSlimSubset([string]$LlvmBin, [string]$DestBin) {
    $required = if ($isUnixHost) { @("clang", "lld", "ld.lld") } else { @("clang.exe", "lld-link.exe", "lld.exe", "llvm-rc.exe") }
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
    # clang / lld 属硬依赖（Windows：clang.exe / lld-link.exe）；其余缺失仅提示。
    $hard = if ($isUnixHost) { @("clang", "lld") } else { @("clang.exe", "lld-link.exe") }
    foreach ($h in $hard) {
        if (-not (Test-Path (Join-Path $DestBin $h))) {
            throw "$h not found in $LlvmBin (bundle aborted)"
        }
    }
    # macOS 额外查找 ld64.lld（多调用别名；官方发行通常随 lld 提供）。
    if ($isUnixHost -and $IsMacOS) {
        if (Test-Path (Join-Path $LlvmBin "ld64.lld")) {
            Copy-Item (Join-Path $LlvmBin "ld64.lld") (Join-Path $DestBin "ld64.lld") -Force
            $copied += "ld64.lld"
        } else {
            $missing += "ld64.lld"
        }
    }
    Write-Host "==> bundled LLVM subset ($($copied.Count) tools): $($copied -join ', ')"
    if ($missing.Count -gt 0) {
        Write-Warning "LLVM bin missing optional tools: $($missing -join ', ')"
    }

    # clang 资源目录：`<LLVM_ROOT>/lib/clang/<ver>/`。clang 按相对自身路径
    # `../lib/clang/<ver>` 自定位，复制后无需任何配置。
    $llvmRoot = Split-Path $LlvmBin -Parent
    $clangRes = Join-Path $llvmRoot "lib\clang"
    if (Test-Path $clangRes) {
        foreach ($vdir in (Get-ChildItem $clangRes -Directory -ErrorAction SilentlyContinue)) {
            $destVer = Join-Path (Join-Path (Split-Path $DestBin -Parent) "lib\clang") $vdir.Name
            if (Test-Path (Join-Path $vdir.FullName "include")) {
                Copy-Item (Join-Path $vdir.FullName "include") (Join-Path $destVer "include") -Recurse -Force
            }
            if ($isUnixHost) {
                # Unix compiler-rt 内建库：`lib/<triple>/libclang_rt.builtins-*`
                #（保留相对布局；asan/ubsan 等 sanitizer 库不打包）。
                $unixLib = Join-Path $vdir.FullName "lib"
                if (Test-Path $unixLib) {
                    foreach ($b in (Get-ChildItem $unixLib -Recurse -Filter "libclang_rt.builtins-*" -ErrorAction SilentlyContinue)) {
                        $rel = $b.FullName.Substring($unixLib.Length).TrimStart("\", "/")
                        $dst = Join-Path (Join-Path $destVer "lib") $rel
                        New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
                        Copy-Item $b.FullName $dst -Force
                    }
                }
            } else {
                $winLib = Join-Path $vdir.FullName "lib\windows"
                if (Test-Path $winLib) {
                    foreach ($b in (Get-ChildItem $winLib -Filter "clang_rt.builtins-*.lib" -ErrorAction SilentlyContinue)) {
                        $dst = Join-Path (Join-Path $destVer "lib\windows") $b.Name
                        New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
                        Copy-Item $b.FullName $dst -Force
                    }
                }
            }
            Write-Host "==> bundled clang resource dir: lib\llvm\lib\clang\$($vdir.Name) (headers + builtins)"
        }
    }

    if ($isUnixHost) {
        # Copy-Item 不保留源权限位：捆绑工具统一恢复可执行位后探测。
        foreach ($f in (Get-ChildItem $DestBin -File)) {
            & chmod +x $f.FullName
        }
        & (Join-Path $DestBin "clang") --version | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "bundled clang failed its --version probe: $DestBin/clang (Unix 发行须自包含；无 DLL 兼容路径)"
        }
    } else {
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
}

function Write-EnvTemplate([string]$Path, [string]$SdkRootHint, [bool]$Bundled) {
    $clangName = if ($isUnixHost) { "clang" } else { "clang.exe" }
    $llvmSection = if ($Bundled) {
        @"

# Bundled LLVM (Phase 3, -BundleLlm): 瘦身版 clang + lld 子集位于 SDK 内
# `<sdk-root>/lib/llvm/bin`。把 `ARC_CLANG` 指向其 clang 即可完全离线构建：
#   ARC_CLANG=$SdkRootHint/lib/llvm/bin/$clangName
"@
    } else { "" }
    $content = @"
# Arc SDK environment — reference template.
#
# The SDK self-locates via arc$exeSuffix (current_exe() walk-up), so nothing here is
# required for 'arc build' to work. These knobs exist for explicit overrides
# (see docs/rfc/031-compiler-cli.md §10 环境变量清单).
#
#   ARC_SDK_ROOT=<sdk-root>    explicit SDK root (highest priority)
#   ARC_STD_ROOT=<dir>         explicit std library root (development override)
#   ARC_HOME=<dir>             user toolchain home (cache / rt_cache / keys)
#   ARC_CLANG=<clang(.exe)>    explicit clang binary$llvmSection
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

$binary = Join-Path $repo ("target/release/" + $exeName)
if (-not (Test-Path $binary)) { throw "$exeName not found: $binary (run without -SkipBuild first)" }

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

$binTarget = Join-Path $pkgDir ("bin/" + $exeName)
Copy-Item $binary $binTarget
if ($isUnixHost) { & chmod +x $binTarget }
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
# 就地安装器：SDK 根嵌入安装脚本（随宿主——解压后无参运行即就地安装：
# 指针布局 + PATH + doctor；与仓库 scripts/packaging/{install.ps1,arc-install.sh} 同源）。
if ($isUnixHost) {
    $unixInstaller = Join-Path $pkgDir "install.sh"
    Copy-Item (Join-Path $repo "scripts/packaging/arc-install.sh") $unixInstaller
    & chmod +x $unixInstaller
} else {
    Copy-Item (Join-Path $repo "scripts\packaging\install.ps1") (Join-Path $pkgDir "install.ps1")
}

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
        $clangTool = if ($isUnixHost) { "clang" } else { "clang.exe" }
        Write-Host "==> llvm bundled: $(Join-Path $pkgDir ("lib\llvm\bin\" + $clangTool))"
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
    if ($isUnixHost) { throw "-SignThumbprint (Authenticode 签名) 仅支持 Windows 宿主" }
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

# --- 5. 容器 + SHA256（Windows zip / Unix tar.xz，随宿主）---
$artifactName = "$pkgName.$containerExt"
$artifactPath = Join-Path $OutDir $artifactName
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
if (Test-Path $artifactPath) { Remove-Item $artifactPath -Force }

if ($isUnixHost) {
    # tar.xz 容器：归档前恢复 SDK 内可执行位（tar 保留权限位，解压即得可执行
    # SDK——bin/arc 启动器、install.sh、捆绑 LLVM 工具）。
    foreach ($x in @((Join-Path $pkgDir ("bin/" + $exeName)), (Join-Path $pkgDir "install.sh"))) {
        if (Test-Path $x) { & chmod +x $x }
    }
    $llvmBinDir = Join-Path $pkgDir "lib\llvm\bin"
    if (Test-Path $llvmBinDir) {
        foreach ($f in (Get-ChildItem $llvmBinDir -File)) { & chmod +x $f.FullName }
    }
    if (-not (Get-Command xz -ErrorAction SilentlyContinue)) {
        throw "tar.xz 容器需要系统 xz (Linux: xz-utils; macOS: brew install xz)"
    }
    Write-Host "==> tar -cJf -> $artifactPath"
    & tar -cJf $artifactPath -C $stageRoot $pkgName
    if ($LASTEXITCODE -ne 0) { throw "tar -cJf failed (exit $LASTEXITCODE)" }
} else {
    # Defender 实时扫描会短暂独占新拷贝的大文件（如捆绑 clang.exe），导致
    # Compress-Archive 报 "being used by another process"；先全量读一遍触发
    # 扫描完成，压缩失败则带退避重试（观察类瞬态错误，非逻辑缺陷）。
    if ($bundledLlvm) {
        $warm = Get-ChildItem $pkgDir -Recurse -File -Include clang.exe,lld-link.exe,arc.exe -ErrorAction SilentlyContinue
        foreach ($wf in $warm) { $null = [System.IO.File]::ReadAllBytes($wf.FullName) }
    }
    Write-Host "==> Compress-Archive -> $artifactPath"
    $compressed = $false
    for ($attempt = 1; $attempt -le 6 -and -not $compressed; $attempt++) {
        try {
            Compress-Archive -Path $pkgDir -DestinationPath $artifactPath -CompressionLevel Optimal -ErrorAction Stop
            $compressed = $true
        } catch {
            if ($attempt -ge 6) { throw }
            Write-Host "==> compress attempt $attempt hit a transient file lock; retrying in 5s"
            Start-Sleep -Seconds 5
        }
    }
}
$shaName = "$artifactName.sha256"
$hash = (Get-FileHash $artifactPath -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $artifactName" | Set-Content -Encoding ASCII (Join-Path $OutDir $shaName)
Write-Host "==> sha256: $hash"
Write-Host "==> package size: $((Get-Item $artifactPath).Length) bytes"

# --- 5b. Phase 2：签名发布清单（manifest.json + manifest.json.sig）---
# 需 $env:ARC_RELEASE_SIGNING_KEY（64 hex seed，`arc release keygen` 生成）。
if ($Manifest) {
    if (-not $env:ARC_RELEASE_SIGNING_KEY) {
        Write-Error "arc-pack.ps1: -Manifest requires `$env:ARC_RELEASE_SIGNING_KEY (64 hex seed; generate via `arc release keygen`)"
        exit 1
    }
    $manifestArgs = @("release", "manifest", "--version", $version, "--triple", $triple, "--archive", $artifactPath, "--output", $OutDir)
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
    if ($isUnixHost) {
        & tar -xJf $artifactPath -C $verifyRoot
        if ($LASTEXITCODE -ne 0) { throw "verify failed: tar -xJf failed (exit $LASTEXITCODE)" }
    } else {
        Expand-Archive -Path $artifactPath -DestinationPath $verifyRoot -Force
    }
    $sdkDir = Join-Path $verifyRoot $pkgName
    $arc = Join-Path $sdkDir ("bin/" + $exeName)

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
        $bundledClangName = if ($isUnixHost) { "clang" } else { "clang.exe" }
        $env:ARC_CLANG = Join-Path $sdkDir ("lib\llvm\bin\" + $bundledClangName)
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
        # 编译 + 链接探测：证明捆绑链接器能完成 Release 式 `-fuse-ld` 链路
        #（Windows: lld-link；Linux: ld.lld；macOS: 先试 lld，失败回落系统 ld64）。
        $probeC = Join-Path $verifyRoot "bundle_probe.c"
        $probeExe = Join-Path $verifyRoot ("bundle_probe" + $(if ($isUnixHost) { "" } else { ".exe" }))
        Set-Content -Path $probeC -Value "int main(void) { return 0; }"
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        $prevPath = $env:PATH
        if ($isUnixHost) {
            # clang 按 PATH 查找 `ld.lld`/`ld64.lld`：探测期前置捆绑 bin。
            $env:PATH = (Join-Path $sdkDir "lib\llvm\bin") + [System.IO.Path]::PathSeparator + $env:PATH
        }
        try {
            $probeArgs = @($probeC, "-o", $probeExe)
            if ($isUnixHost) {
                $probeArgs += @("-fuse-ld=lld")
            } else {
                $probeArgs += @("-fuse-ld=lld-link")
            }
            & $bundleClang @probeArgs 2>&1 | ForEach-Object { Write-Host "    $_" }
            $probeExit = $LASTEXITCODE
            if ($probeExit -ne 0 -and $IsMacOS) {
                # macOS：无 lld/ld64.lld 时回落系统默认链接器（仍验证捆绑 clang 驱动）。
                Write-Host "    note: -fuse-ld=lld failed on macOS; retrying with system linker"
                & $bundleClang $probeC -o $probeExe 2>&1 | ForEach-Object { Write-Host "    $_" }
                $probeExit = $LASTEXITCODE
            }
        } finally {
            $ErrorActionPreference = $prevEap
            if ($isUnixHost) { $env:PATH = $prevPath }
        }
        if ($probeExit -ne 0 -or -not (Test-Path $probeExe)) {
            throw "verify failed: bundled clang compile/link probe failed"
        }
        Write-Host "    ok: bundled clang compile+link probe passed"
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
        $projExeSuffix = if ($isUnixHost) { "" } else { ".exe" }
        $exe = Join-Path $proj ("bin\Debug\hello" + $projExeSuffix)
        if (-not (Test-Path $exe)) { throw "verify failed: expected binary $exe" }
        # stdout 捕获：Unix 走 pwsh 直接调用；PS 5.1（Windows）对经
        # WriteFile(GetStdHandle) 输出的原生程序捕获不可靠，用 cmd /c 运行。
        if ($isUnixHost) {
            $runOut = (& $exe) -join "`n"
        } else {
            $runOut = (cmd /c "`"$exe`"") -join "`n"
        }
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
        $exe2 = Join-Path $projStd ("bin\Debug\stdapp" + $projExeSuffix)
        if (-not (Test-Path $exe2)) { throw "verify failed: expected stdapp binary $exe2" }
        if ($isUnixHost) {
            $runOut2 = (& $exe2) -join "`n"
        } else {
            $runOut2 = (cmd /c "`"$exe2`"") -join "`n"
        }
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

Write-Host "==> done: $artifactPath"
