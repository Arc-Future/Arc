# fetch-boringssl-native.ps1 - vendor the C cryptography base for RFC 035 (S0 TLS 1.3)
#
# RFC 035 §1.1 首选 BoringSSL/AWS-LC 预构建；S0 本环境走 mbedTLS 4.1 LTS 备选
# （"clang 直编最省事"）。本脚本下载 + SHA256 校验源码包 + clang 直编
# crypto_native.dll（含 M0 探针 shim + M1 `rt_crypto_*` ABI 实现面），产物落到
#   crates/runtime-crypto/bin/windows/
# 使 M0 加载探针可动态加载并解析三枚核心符号
# （EVP_aead_aes_256_gcm / SSL_CTX_new / X509_parse）；
# 使 M1 验收面（AEAD / RSA 签名 / X25519 用例）可经
# `rt_crypto_aesgcm_*` / `rt_crypto_rsa_*` / `rt_crypto_x25519_*`
# 走真实 mbedTLS 语义（AES-256-GCM · RSASSA-PSS-SHA256 · X25519）。
#
# Usage: powershell -File .\scripts\fetch-boringssl-native.ps1          (default version)
#        powershell -File .\scripts\fetch-boringssl-native.ps1 -Version mbedtls-4.1.1
#        powershell -File .\scripts\fetch-boringssl-native.ps1 -Force    (re-build)
#        powershell -File .\scripts\fetch-boringssl-native.ps1 -Force -SourceDir <已解压源码树> (离线复用)
#
# Idempotent: skips when crypto_native.dll already present unless -Force is given.
# Hygiene: download/build entirely under $env:TEMP; only final artifacts go to the
#          vendor dir. Never writes scratch files into the source tree.
#
# Requires: clang (same soft-skip convention as e2e tests).

param(
    [string]$Version = "mbedtls-4.1.1",
    [switch]$Force,
    [string]$SourceDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

$AssetName = "$Version.tar.bz2"
$DownloadUrl = "https://github.com/Mbed-TLS/mbedtls/releases/download/$Version/$AssetName"
$Sha256 = "3359a349e23db3d5536fcee032ae7b2ecbfc08972fab643089b5cbf2a375c98c"
$VendorDir = Join-Path $Root "crates/runtime-crypto/bin/windows"
$DllTarget = Join-Path $VendorDir "crypto_native.dll"

if (!$Force -and (Test-Path $DllTarget)) {
    Write-Host "crypto_native.dll already present ($DllTarget); use -Force to re-build"
    exit 0
}

Write-Host "Fetching crypto base $Version (mbedTLS LTS, Apache-2.0)..."
Write-Host "  URL: $DownloadUrl"

# Hygiene: all download/build under $env:TEMP, never in the source tree.
# -SourceDir 复用已解压源码树（跳过下载/校验/解压），用于离线重建。
if ($SourceDir) {
    $SourceDir = (Resolve-Path $SourceDir).Path
    $Base = $SourceDir
    if (!(Test-Path (Join-Path $Base "library"))) {
        throw "-SourceDir 不是有效的 mbedtls 源码树: $Base (缺少 library/)"
    }
} else {
    $Work = Join-Path $env:TEMP "crypto-vendor-$([guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $Work -Force | Out-Null
    $Tar = Join-Path $Work $AssetName
}

try {
    if (!$SourceDir) {
        curl.exe -L --retry 3 --connect-timeout 30 --retry-delay 2 -o $Tar $DownloadUrl
        if ($LASTEXITCODE -ne 0) {
            throw "curl download failed (exit $LASTEXITCODE); check network or proxy"
        }
        $actual = (Get-FileHash $Tar -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $Sha256) {
            throw "SHA256 mismatch: expected $Sha256, got $actual"
        }
        Write-Host "SHA256 verified: $actual"

        Write-Host "Extracting to temp dir..."
        # tf-psa-crypto mldsa-native 示例树含符号链接，tar.exe 无法直解；排除。
        tar.exe -xf $Tar -C $Work --exclude "*mldsa-native*"
        if ($LASTEXITCODE -ne 0) {
            throw "tar extraction failed (exit $LASTEXITCODE)"
        }
        $Base = Join-Path $Work $Version
    } else {
        Write-Host "Reusing source tree: $Base"
    }

    Write-Host "Compiling with clang (mbedtls library + tf-psa-crypto + ABI shim)..."
    $Include = @(
        "-I$Base/include",
        "-I$Base/tf-psa-crypto/include",
        "-I$Base/tf-psa-crypto/core",
        "-I$Base/tf-psa-crypto/dispatch",
        "-I$Base/tf-psa-crypto/extras",
        "-I$Base/tf-psa-crypto/platform",
        "-I$Base/tf-psa-crypto/utilities",
        "-I$Base/tf-psa-crypto/drivers/builtin/include",
        "-I$Base/tf-psa-crypto/drivers/builtin/src",
        "-I$Base/tf-psa-crypto/drivers/everest/include",
        "-I$Base/tf-psa-crypto/drivers/p256-m"
    )
    # RFC 035 M3：X.509 证书解析需 DER + PEM 双格式。mbedTLS 默认配置不含
    # PEM/BASE64，须显式 -D 启用（否则 mbedtls_x509_crt_parse 只认 DER，
    # CreateFromPem 会以 MBEDTLS_ERR_X509_INVALID_FORMAT 失败）。
    # RFC 035 S5：0-RTT 早数据（MBEDTLS_SSL_EARLY_DATA）默认关闭，须显式启用
    # （否则 mbedtls_ssl_write/read_early_data 等符号不编译；实测差异见
    # rt_crypto_native.c S5 段注记）。
    $Cflags = @(
        "-c", "-O2", "-ffunction-sections", "-fdata-sections",
        "-D_CRT_SECURE_NO_WARNINGS",
        "-DMBEDTLS_PEM_PARSE_C",
        "-DMBEDTLS_BASE64_C",
        "-DMBEDTLS_SSL_EARLY_DATA",
        # RFC 035 S5：证书有效期/吊销时间检查需系统时钟（MBEDTLS_HAVE_TIME_DATE
        # 默认关 → 过期证书校验不触发；S5 完整链校验 e2e 依赖其生效）。
        "-DMBEDTLS_HAVE_TIME",
        "-DMBEDTLS_HAVE_TIME_DATE"
    )
    # 编译/链接 scratch 目录（-SourceDir 模式下 $Work 未定义，独立建 scratch）。
    if ($SourceDir) {
        $BuildScratch = Join-Path $env:TEMP "crypto-vendor-build-$([guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $BuildScratch -Force | Out-Null
    } else {
        $BuildScratch = $Work
    }
    $Obj = Join-Path $BuildScratch "obj"
    New-Item -ItemType Directory -Path $Obj -Force | Out-Null
    Push-Location $Obj
    try {
        $Sources = @()
        $Sources += Get-ChildItem (Join-Path $Base "library") -Filter *.c | ForEach-Object FullName
        $Sources += Get-ChildItem (Join-Path $Base "tf-psa-crypto/core") -Filter *.c | ForEach-Object FullName
        $Sources += Get-ChildItem (Join-Path $Base "tf-psa-crypto/utilities") -Filter *.c | ForEach-Object FullName
        $Sources += Get-ChildItem (Join-Path $Base "tf-psa-crypto/platform") -Filter *.c | ForEach-Object FullName
        $Sources += Get-ChildItem (Join-Path $Base "tf-psa-crypto/extras") -Filter *.c | ForEach-Object FullName
        $Sources += Get-ChildItem (Join-Path $Base "tf-psa-crypto/drivers/builtin/src") -Filter *.c | ForEach-Object FullName
        $Sources += Join-Path $Base "tf-psa-crypto/drivers/everest/library/x25519.c"
        $Sources += Join-Path $Base "tf-psa-crypto/drivers/everest/library/Hacl_Curve25519_joined.c"
        $Sources += Join-Path $Base "tf-psa-crypto/drivers/p256-m/p256-m_driver_entrypoints.c"
        $Sources += Join-Path $Base "tf-psa-crypto/drivers/p256-m/p256-m/p256-m.c"
        $Sources += Join-Path $Root "crates/runtime-crypto/shim/openssl_compat.c"
        $Sources += Join-Path $Root "crates/runtime-crypto/shim/rt_crypto_native.c"
        foreach ($src in $Sources) {
            if (!(Test-Path $src)) { throw "missing source: $src" }
        }
        # 注意：$ErrorActionPreference="Stop" 下 clang 的 stderr 警告会被 PowerShell 当终止性
        # 错误（NativeCommandError），因此 clang 调用需临时切回 Continue（脚本已显式检查
        # $LASTEXITCODE）；stderr 重定向到文件，失败时再回显。
        $CompileLog = Join-Path $Obj "clang-stderr.log"
        $ErrorActionPreference = "Continue"
        & clang.exe @Cflags @Include $Sources 2> $CompileLog
        $ExitCode = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($ExitCode -ne 0) {
            Get-Content $CompileLog | ForEach-Object { Write-Host "  $_" }
            throw "clang compile failed (exit $ExitCode)"
        }
        $Objs = Get-ChildItem $Obj -Filter *.o | ForEach-Object FullName
        Write-Host "Compiled $($Objs.Count) objects"
    } finally {
        Pop-Location
    }

    Write-Host "Linking crypto_native.dll..."
    $Dll = Join-Path $BuildScratch "crypto_native.dll"
    Push-Location $Obj
    try {
        $LinkLog = Join-Path $Obj "clang-link-stderr.log"
        $ErrorActionPreference = "Continue"
        & clang.exe -shared -O2 -o $Dll @($Objs) -lbcrypt -lcrypt32 2> $LinkLog
        $ExitCode = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($ExitCode -ne 0) {
            Get-Content $LinkLog | ForEach-Object { Write-Host "  $_" }
            throw "clang link failed (exit $ExitCode)"
        }
    } finally {
        Pop-Location
    }
    $Lib = Join-Path $BuildScratch "crypto_native.lib"

    New-Item -ItemType Directory -Path $VendorDir -Force | Out-Null
    Copy-Item $Dll $DllTarget -Force
    Copy-Item $Lib (Join-Path $VendorDir "crypto_native.lib") -Force

    # Best-effort MinGW import lib (备用；wgpu 模式下 codegen 以 MinGW 惯例链接)。
    $MinGW = Join-Path $VendorDir "libcrypto_native.dll.a"
    if (Get-Command llvm-dlltool.exe -ErrorAction SilentlyContinue) {
        $Def = Join-Path $BuildScratch "crypto_native.def"
        @(
            "LIBRARY crypto_native",
            "EXPORTS",
            "  EVP_aead_aes_256_gcm",
            "  SSL_CTX_new",
            "  X509_parse",
            "  rt_crypto_aesgcm_new_key",
            "  rt_crypto_aesgcm_encrypt",
            "  rt_crypto_aesgcm_decrypt",
            "  rt_crypto_aesgcm_encrypt_aad",
            "  rt_crypto_aesgcm_decrypt_aad",
            "  rt_crypto_rsa_keygen",
            "  rt_crypto_rsa_spki_export",
            "  rt_crypto_rsa_spki_import",
            "  rt_crypto_rsa_pkcs8_export",
            "  rt_crypto_rsa_sign_pss",
            "  rt_crypto_rsa_verify_pss",
            "  rt_crypto_x25519_keygen",
            "  rt_crypto_x25519_pubkey",
            "  rt_crypto_x25519_import_private",
            "  rt_crypto_x25519_derive",
            "  rt_crypto_tls_hkdf_extract",
            "  rt_crypto_tls_hkdf_expand_label",
            "  rt_crypto_tls_derive_secret",
            "  rt_crypto_tls_record_seal",
            "  rt_crypto_tls_record_open",
            "  rt_crypto_x509_parse_der",
            "  rt_crypto_x509_parse_pem",
            "  rt_crypto_x509_subject",
            "  rt_crypto_x509_pubkey",
            "  rt_crypto_x509_verify",
            "  rt_crypto_x509_free",
            "  rt_crypto_tls_client_new",
            "  rt_crypto_tls_server_new",
            "  rt_crypto_tls_handshake",
            "  rt_crypto_tls_write",
            "  rt_crypto_tls_read",
            "  rt_crypto_tls_alpn",
            "  rt_crypto_tls_free",
            "  rt_crypto_tls_set_verify",
            "  rt_crypto_tls_load_system_roots",
            "  rt_crypto_tls_set_crl",
            "  rt_crypto_tls_verify_result",
            "  rt_crypto_tls_set_client_cert",
            "  rt_crypto_tls_session_save",
            "  rt_crypto_tls_session_load",
            "  rt_crypto_tls_server_new_ex",
            "  rt_crypto_tls_drain",
            "  rt_crypto_tls_enable_early_data",
            "  rt_crypto_tls_write_early_data",
            "  rt_crypto_tls_early_data_status",
            "  rt_crypto_tls_read_early_data"
        ) | Set-Content $Def -Encoding ascii
        llvm-dlltool.exe -m i386:x86-64 -d $Def -l $MinGW 2>$null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "llvm-dlltool failed; skipping MinGW import lib"
        }
    } else {
        Write-Warning "llvm-dlltool not found; skipping MinGW import lib"
    }

    Write-Host "Vendored:"
    Write-Host "  $DllTarget  ($(Get-Item $DllTarget).Length bytes)"
    Write-Host "  $(Join-Path $VendorDir 'crypto_native.lib')  ($(Get-Item (Join-Path $VendorDir 'crypto_native.lib')).Length bytes)"
    Write-Host ""
    Write-Host "Next: load crypto_native.dll and probe the three core symbols (M0 acceptance)."
} finally {
    Remove-Item -Recurse -Force $BuildScratch -ErrorAction SilentlyContinue
    if ($Work) {
        Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
    }
}