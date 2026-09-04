// RFC 026 M1: AesGcm — AES-256-GCM AEAD facade（S0 TLS 1.3 原语面）。
//
// 对标 C# System.Security.Cryptography.AesGcm（去 CBC 糟粕）。单一惯用法
// （RFC 003 / 035 §1.2 ①）：非就地、返回新 byte[]；tag 与密文分离（Decrypt
// 显式收 tag）。实例方法经 codegen 拦截直射 vendored crypto_native.dll 的
// `rt_crypto_aesgcm_*` ABI（mbedTLS 4.1.1 PSA）。
//
// 诚实边界（RFC 026 §1.2 ①）：仅 AES-256-GCM；AAD 不暴露；nonce 固定 12 字节。

namespace Arc.Security.Cryptography;

public class AesGcm {
    private byte[] _key;

    private AesGcm(byte[] key) {
        this._key = key;
    }

    /// <summary>CSPRNG 生成 32 字节随机密钥。</summary>
    [Builtin(ABI = "rt_crypto_aesgcm_new_key")]
    private static byte[] _GenerateKey() { return null; }

    /// <summary>密钥 = CSPRNG 生成 32 字节。</summary>
    public static AesGcm Create() {
        return new AesGcm(_GenerateKey());
    }

    /// <summary>显式密钥（32 字节，非 32 抛 ArgumentException）。</summary>
    public static AesGcm Create(byte[] key) {
        if (key == null || key.Length != 32) {
            throw new ArgumentException("AesGcm requires a 32-byte key.");
        }
        return new AesGcm(key);
    }

    /// <summary>读写密钥（值替换即生效）。</summary>
    public byte[] Key {
        get { return this._key; }
        set { this._key = value; }
    }

    /// <summary>恒 16（128-bit 标签）。</summary>
    public int TagSize {
        get { return 16; }
    }

    /// <summary>加密：nonce 固定 12 字节（违反抛 ArgumentException）。
    /// 返回密文（含附加 16 字节 tag 的封装形态，RFC 026 §1.2 ①）。</summary>
    public byte[] Encrypt(byte[] nonce, byte[] plaintext) {
        if (nonce == null || nonce.Length != 12) {
            throw new ArgumentException("AesGcm nonce must be exactly 12 bytes.");
        }
        return this._Encrypt(nonce, plaintext);
    }

    /// <summary>解密：认证失败（篡改 tag/密文）返回 null。</summary>
    public byte[] Decrypt(byte[] nonce, byte[] ciphertext, byte[] tag) {
        if (nonce == null || nonce.Length != 12) {
            throw new ArgumentException("AesGcm nonce must be exactly 12 bytes.");
        }
        return this._Decrypt(nonce, ciphertext, tag);
    }

    [Builtin(ABI = "rt_crypto_aesgcm_encrypt")]
    private byte[] _Encrypt(byte[] nonce, byte[] plaintext) { return null; }

    [Builtin(ABI = "rt_crypto_aesgcm_decrypt")]
    private byte[] _Decrypt(byte[] nonce, byte[] ciphertext, byte[] tag) { return null; }
}
