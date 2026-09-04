# fetch-quic-native.ps1 - vendor the C QUIC base for RFC 033 S4 (HTTP/3 minimal client)
#
# RFC 033 §1.2.b 裁决为 ngtcp2 + BoringSSL `SSL_QUIC_METHOD`；Phase 1 可行性调查
# （2026-08-05）确认 BoringSSL/AWS-LC 无官方 Windows 预构建且源码构建缺
# cmake/perl/go，因此走「第三方 Windows 预构建」路径（RFC 039 §1.1 选项 a：
# 双栈，S0 mbedTLS 不动 + S4 独立 QUIC-TLS 底座）：
#
#   ngtcp2 1.25.0（MIT，RFC 9000 传输层）
#     + MSYS2 预构建 libngtcp2_crypto_ossl（ngtcp2 官方 OpenSSL QUIC-TLS 适配，
#       RFC 9001；经 SSL_set_quic_tls_cbs 把 OpenSSL 3.5 作为外部 QUIC 栈的 TLS
#       backend，语义等价于 BoringSSL SSL_QUIC_METHOD 的 set_encryption_secrets /
#       add_handshake_data / flush_flight / send_alert）
#     + OpenSSL 3.5.4（Apache-2.0，仅作 ngtcp2 的 TLS 后端；OpenSSL 原生 QRL
#       全栈未使用）
#
# 本脚本下载 + SHA256 校验 MSYS2 预构建包 + clang 编译 shim（rt_quic_* ABI 面），
# 产物落到 crates/runtime-quic/bin/windows/，供 rt_quic_* 运行时动态加载。
#
# Usage: powershell -File .\scripts\fetch-quic-native.ps1          (default versions)
#        powershell -File .\scripts\fetch-quic-native.ps1 -Force    (re-download + re-build)
#        powershell -File .\scripts\fetch-quic-native.ps1 -Force -PkgDir <已下载包目录> (离线复用)
#
# Idempotent: skips when quic_native.dll already present unless -Force is given.
# Hygiene: download/build entirely under $env:TEMP; only final artifacts go to the
#          vendor dir. Never writes scratch files into the source tree.
#
# Requires: clang + llvm-lib + llvm-readobj (LLVM toolchain), tar (zstd), network
#           (same soft-skip convention as e2e tests).

param(
    [string]$Ngtcp2Version = "1.25.0-1",
    [string]$OpenSslVersion = "3.5.4-1",
    [switch]$Force,
    [string]$PkgDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$Ngtcp2Pkg = "mingw-w64-ucrt-x86_64-ngtcp2-$Ngtcp2Version-any.pkg.tar.zst"
$OpenSslPkg = "mingw-w64-ucrt-x86_64-openssl-$OpenSslVersion-any.pkg.tar.zst"
$Mirror = "https://repo.msys2.org/mingw/ucrt64"
# SHA256（repo.msys2.org 2026-07/08 快照；升级版本时必须同步更新校验值）
$Ngtcp2Sha256 = "CCE230D80A05CD0A1EC38FA5D76C0445C9BB725C65C2136BE3D23E6DC218A89E"
$OpenSslSha256 = "F124FDA279F00B6789D3FC4EEF564ECF37B3C44D31C712A7E77E3E822B498A06"

$VendorDir = Join-Path $Root "crates/runtime-quic/bin/windows"
$ShimSrc = Join-Path $Root "crates/runtime-quic/shim/rt_quic_native.c"
$DllTarget = Join-Path $VendorDir "quic_native.dll"

if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "quic_native.dll already present ($DllTarget); use -Force to re-build"
    exit 0
}
foreach ($tool in @("clang", "llvm-lib", "llvm-readobj", "tar")) {
    if (!(Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "required tool not found: $tool (LLVM toolchain / Windows tar)"
    }
}

$Work = Join-Path $env:TEMP "quic-vendor-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $Work -Force | Out-Null
$ImpDir = Join-Path $Work "imp"
New-Item -ItemType Directory -Path $ImpDir -Force | Out-Null

try {
    # ---- 下载 + 校验（-PkgDir 离线复用） ----
    $PkgSrc = @(
        @{ Name = $Ngtcp2Pkg; Sha = $Ngtcp2Sha256 },
        @{ Name = $OpenSslPkg; Sha = $OpenSslSha256 }
    )
    foreach ($p in $PkgSrc) {
        $local = if ($PkgDir) { Join-Path $PkgDir $p.Name } else { Join-Path $Work $p.Name }
        if (!(Test-Path $local)) {
            if ($PkgDir) { throw "package not found in -PkgDir: $($p.Name)" }
            Write-Host "Downloading $($p.Name)..."
            curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $local "$Mirror/$($p.Name)"
            if ($LASTEXITCODE -ne 0) {
                throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
            }
        }
        $actual = (Get-FileHash $local -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $p.Sha.ToLower()) {
            throw "SHA256 mismatch for $($p.Name): expected $($p.Sha), got $actual"
        }
        Write-Host "SHA256 verified: $($p.Name)"
        tar.exe -xf $local -C $Work
        if ($LASTEXITCODE -ne 0) { throw "tar extraction failed for $($p.Name)" }
    }

    $Ngtcp2Root = Join-Path $Work "ucrt64"
    $OpenSslRoot = Join-Path $Work "ucrt64"

    # ---- 生成 MSVC 兼容导入库（MSYS2 只带 MinGW .dll.a） ----
    # llvm-readobj 列出 DLL 导出名 → .def（LIBRARY 名必须与真实 DLL 一致）→
    # llvm-lib 产出 MSVC .lib，供 clang/MSVC 目标链接 shim。
    $ImportTargets = @(
        @{ Dll = "libngtcp2-16.dll";        Base = "libngtcp2-16";        Root = $Ngtcp2Root },
        @{ Dll = "libngtcp2_crypto_ossl-0.dll"; Base = "libngtcp2_crypto_ossl-0"; Root = $Ngtcp2Root },
        @{ Dll = "libssl-3-x64.dll";         Base = "libssl-3-x64";        Root = $OpenSslRoot },
        @{ Dll = "libcrypto-3-x64.dll";      Base = "libcrypto-3-x64";     Root = $OpenSslRoot }
    )
    foreach ($t in $ImportTargets) {
        $dllPath = Join-Path $t.Root "bin/$($t.Dll)"
        if (!(Test-Path $dllPath)) { throw "DLL not found in package: $dllPath" }
        $def = Join-Path $ImpDir "$($t.Base).def"
        $lib = Join-Path $ImpDir "$($t.Base).lib"
        $exports = @("LIBRARY $($t.Base)", "EXPORTS")
        $obj = llvm-readobj --coff-exports $dllPath
        foreach ($line in $obj) {
            if ($line -match '^\s*Name:\s+(\w+)\s*$') { $exports += "  $($Matches[1])" }
        }
        if ($exports.Count -le 2) { throw "no exports parsed from $dllPath" }
        Set-Content -Path $def -Value $exports -Encoding Ascii
        llvm-lib -def:$def -out:$lib -machine:x64
        if ($LASTEXITCODE -ne 0) { throw "llvm-lib failed for $($t.Dll)" }
        Write-Host "import lib ready: $($t.Base).lib ($($exports.Count - 2) exports)"
    }

    # ---- 编译 shim ----
    Write-Host "Compiling rt_quic_native.c -> quic_native.dll ..."
    clang -shared -O1 -D_CRT_SECURE_NO_WARNINGS -o $DllTarget $ShimSrc `
        "-I$(Join-Path $Ngtcp2Root 'include')" `
        "-I$(Join-Path $OpenSslRoot 'include')" `
        "$(Join-Path $ImpDir 'libngtcp2-16.lib')" `
        "$(Join-Path $ImpDir 'libngtcp2_crypto_ossl-0.lib')" `
        "$(Join-Path $ImpDir 'libssl-3-x64.lib')" `
        "$(Join-Path $ImpDir 'libcrypto-3-x64.lib')" `
        -lws2_32 -ladvapi32 -lbcrypt -lcrypt32 -luser32
    if ($LASTEXITCODE -ne 0) { throw "clang shim build failed (exit $LASTEXITCODE)" }

    # ---- 拷贝依赖 DLL + 导入库到 vendor 目录 ----
    foreach ($t in $ImportTargets) {
        Copy-Item (Join-Path $t.Root "bin/$($t.Dll)") (Join-Path $VendorDir $t.Dll) -Force
        Copy-Item (Join-Path $ImpDir "$($t.Base).lib") (Join-Path $VendorDir "$($t.Base).lib") -Force
    }
    New-Item -ItemType Directory -Path $VendorDir -Force | Out-Null

    Write-Host "Done. vendored base at $VendorDir"
} finally {
    Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
}
