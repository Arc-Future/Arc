// SecureSession —— 拆分自 NoiseTransport.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class SecureSession {
    private string _handle;   // 不透明会话句柄（C 侧 malloc 指针，按 string 穿透）

    private SecureSession(string handle) {
        _handle = handle;
    }

    /// <summary>create_arr：装载 32B X25519 静态密钥（initiator=1/0）→ opaque 句柄。</summary>
    [Builtin(ABI = "rt_noise_session_create_arr")]
    private static string _CreateArr(byte[] localSk, byte[] remotePk, int initiator) { return null; }

    /// <summary>encrypt_arr：明文 → 「密文‖tag」合并 byte[]（pt_len+16）；失败 null。</summary>
    [Builtin(ABI = "rt_noise_session_encrypt_arr")]
    private byte[] _EncryptArr(byte[] plaintext) { return null; }

    /// <summary>decrypt_arr：密文 + 16B tag 分离 → 明文 byte[]；认证失败 null。</summary>
    [Builtin(ABI = "rt_noise_session_decrypt_arr")]
    private byte[] _DecryptArr(byte[] ciphertext, byte[] tag) { return null; }

    /// <summary>
    /// 创建 Noise_XK 会话。localSk 为本端 32 字节静态私钥、remotePk 为对端
    /// 32 字节静态公钥（initiator=1 发起方 / 0 响应方）。返回 SecureSession；
    /// 装载失败返回 null。
    /// </summary>
    public static SecureSession Create(byte[] localSk, byte[] remotePk, int initiator) {
        if (localSk == null || localSk.Length != 32) {
            throw new ArgumentException("SecureSession requires a 32-byte local static key.");
        }
        if (remotePk == null || remotePk.Length != 32) {
            throw new ArgumentException("SecureSession requires a 32-byte remote static key.");
        }
        if (initiator != 0 && initiator != 1) {
            throw new ArgumentException("SecureSession initiator must be 0 or 1.");
        }
        string handle = _CreateArr(localSk, remotePk, initiator);
        if (handle == null) { return null; }
        return new SecureSession(handle);
    }

    /// <summary>不透明会话句柄（供 NoiseTransport 握手流程传递）。</summary>
    public string Handle {
        get { return _handle; }
    }

    /// <summary>传输加密：返回「密文‖tag」合并 byte[]（pt_len+16）；会话未就绪返回 null。</summary>
    public byte[] Encrypt(byte[] plaintext) {
        if (plaintext == null) { throw new ArgumentNullException("plaintext"); }
        return _EncryptArr(plaintext);
    }

    /// <summary>传输解密：认证失败（篡改 tag/密文）返回 null。</summary>
    public byte[] Decrypt(byte[] ciphertext, byte[] tag) {
        if (ciphertext == null) { throw new ArgumentNullException("ciphertext"); }
        if (tag == null || tag.Length != 16) {
            throw new ArgumentException("Noise transport tag must be exactly 16 bytes.");
        }
        return _DecryptArr(ciphertext, tag);
    }
}
