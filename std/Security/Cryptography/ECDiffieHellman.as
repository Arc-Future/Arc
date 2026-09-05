// RFC 026 M1: ECDiffieHellman — X25519 固定曲线 ECDH facade（S0 TLS 1.3 原语面）。
//
// 对标 C# System.Security.Cryptography.ECDiffieHellman（RFC 026 §1.2 ③）：
// S0 固定 X25519（TLS 1.3 默认组）；DeriveKeyMaterial 返回 32 字节原始共享秘密，
// HKDF 用户面不暴露（诚实边界）。实例方法经 codegen 拦截直射 vendored
// crypto_native.dll 的 `rt_crypto_x25519_*` ABI（mbedTLS 4.1.1 PSA）。

namespace Arc.Security.Cryptography;

public class ECDiffieHellman {
    private int _handle;

    private ECDiffieHellman(int handle) {
        _handle = handle;
    }

    /// <summary>X25519 密钥对（CSPRNG 生成）。</summary>
    [Builtin(ABI = "rt_crypto_x25519_keygen")]
    private static int _Keygen() { return 0; }

    /// <summary>导入 32 字节 X25519 私钥（RFC 7748 §6.1 向量所需；S0 面扩展）。</summary>
    [Builtin(ABI = "rt_crypto_x25519_import_private")]
    private static int _ImportPrivate(byte[] privateKey) { return 0; }

    /// <summary>生成 X25519 密钥对。</summary>
    public static ECDiffieHellman Create() {
        return new ECDiffieHellman(_Keygen());
    }

    /// <summary>导入 32 字节 X25519 私钥（RFC 7748 §6.1 已知向量测试）。</summary>
    public static ECDiffieHellman ImportPrivateKey(byte[] privateKey) {
        if (privateKey == null || privateKey.Length != 32) {
            throw new ArgumentException("X25519 private key must be exactly 32 bytes.");
        }
        return new ECDiffieHellman(_ImportPrivate(privateKey));
    }

    /// <summary>32 字节 X25519 公钥。</summary>
    [Builtin(ABI = "rt_crypto_x25519_pubkey")]
    public byte[] PublicKey { get; }

    /// <summary>与对方公钥派生 32 字节共享秘密（X25519 原始共享秘密）。</summary>
    [Builtin(ABI = "rt_crypto_x25519_derive")]
    public byte[] DeriveKeyMaterial(byte[] otherPublicKey) { return null; }
}
