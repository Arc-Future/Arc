// RFC 026 M1: Rsa — RSA-PSS 签名/验签 + SPKI/PKCS#8 导入导出 facade（S0 TLS 1.3 原语面）。
//
// 对标 C# System.Security.Cryptography.RSA（去 PKCS#1 v1.5 糟粕，RFC 026 §1.2 ②）：
// S0 固定 2048-bit；TLS 1.3 中 RSA 角色 = 证书链签名/验签。实例方法经 codegen
// 拦截直射 vendored crypto_native.dll 的 `rt_crypto_rsa_*` ABI（mbedTLS 4.1.1）。

namespace Arc.Security.Cryptography;

public class Rsa {
    private long _handle;

    private Rsa(long handle) {
        this._handle = handle;
    }

    /// <summary>生成指定位数 RSA 密钥对（S0 固定 2048）。</summary>
    [Builtin(ABI = "rt_crypto_rsa_keygen")]
    private static long _Keygen(int bits) { return 0; }

    /// <summary>SPKI DER 导入 → 密钥句柄。</summary>
    [Builtin(ABI = "rt_crypto_rsa_spki_import")]
    private static long _ImportSpki(byte[] der) { return 0; }

    /// <summary>生成 2048-bit RSA 密钥对。</summary>
    public static Rsa Create() {
        return new Rsa(_Keygen(2048));
    }

    /// <summary>内部工厂：包装既有 opaque 密钥句柄（X509Certificate2.PublicKey 等）。</summary>
    internal static Rsa FromHandle(long handle) {
        return new Rsa(handle);
    }

    /// <summary>从 DER 编码 SubjectPublicKeyInfo 导入公钥。</summary>
    public static Rsa ImportSubjectPublicKeyInfo(byte[] der) {
        return new Rsa(_ImportSpki(der));
    }

    /// <summary>导出 DER 编码 SubjectPublicKeyInfo（X.509 公钥）。</summary>
    [Builtin(ABI = "rt_crypto_rsa_spki_export")]
    public byte[] ExportSubjectPublicKeyInfo() { return null; }

    /// <summary>导出 DER 编码 PKCS#8 私钥。</summary>
    [Builtin(ABI = "rt_crypto_rsa_pkcs8_export")]
    public byte[] ExportPkcs8PrivateKey() { return null; }

    /// <summary>RSASSA-PSS-SHA256 签名（2048-bit → 256 字节签名）。</summary>
    [Builtin(ABI = "rt_crypto_rsa_sign_pss")]
    public byte[] Sign(byte[] data) { return null; }

    /// <summary>RSASSA-PSS-SHA256 验签；失败（含数据被篡改）返回 false。</summary>
    [Builtin(ABI = "rt_crypto_rsa_verify_pss")]
    public bool Verify(byte[] data, byte[] signature) { return false; }
}
