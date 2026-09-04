// RFC 042 M5 P0-2: Noise XK 安全传输——byte[] 全接线（N3 债清偿）。
//
// 真实面（regular class，AesGcm 模式：私有 [Builtin] `_XxxArr` 经 codegen
// 拦截直射 vendored crypto_native.dll 的 `rt_noise_*_arr` ABI——RtArray
// byte[] 语义见 rt_abi.h；公开方法为真实体，负责参数校验与对象语义）：
//   - SecureSession.Create → rt_noise_session_create_arr（32B X25519 静态
//     密钥装载 + 协议名 MixHash；返回对象，不透明会话句柄经 Handle 导出）。
//   - NoiseTransport.Initiate → rt_noise_initiate_handshake_arr：msg1 =
//     e.pub(32) + 空 payload AEAD tag(16) = 48 字节（CSPRNG 临时密钥生成）。
//   - NoiseTransport.Respond → rt_noise_respond_handshake_arr：msg1 →
//     msg2 = e.pub(32) + tag(16) = 48 字节。
//   - NoiseTransport.Finalize → rt_noise_initiate_finalize_arr：msg2 →
//     Split + msg3 = s.pub(32) + tag(16) = 64 字节（本端静态密钥加密传输，
//     交由调用方投递给响应方）。
//   - NoiseTransport.RespondFinalize → rt_noise_respond_finalize_arr：
//     msg3 → Split（此后双端 transport 密钥就绪）。
//   - SecureSession.Encrypt/Decrypt → rt_noise_session_encrypt_arr/
//     decrypt_arr：传输阶段 AEAD。Encrypt 出参为「密文‖tag」合并数组
//     （pt_len+16）；Decrypt 显式分离收 16 字节 tag，认证失败返回 null
//     （镜像 AesGcm 语义）。
//   - NoiseTransport.HandshakeHash → rt_noise_session_handshake_hash_arr：
//     32 字节握手 hash（Split 后可用；双端一致即握手表成，测试断言面）。
//
// 诚实边界（byte[] 债 N3 完成后剩余）：
//   - 会话销毁（rt_noise_session_destroy）未暴露——句柄生命周期即进程生命周期；
//   - 密钥为裸 32 字节 X25519 材料，须经带外途径（如 Security 包
//     ECDiffieHellman）生成与分发，本类不做编码/持久化；
//   - Encrypt/Decrypt 仅在握手 Split 完成后可用，未就绪返回 null；
//   - 握手入参长度由 C 侧按 RtArray 实长处理（协议长度不符 → 失败 null/false）。
namespace Arc.Net.P2P;

public class SecureSession {
    private string _handle;   // 不透明会话句柄（C 侧 malloc 指针，按 string 穿透）

    private SecureSession(string handle) {
        this._handle = handle;
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
        get { return this._handle; }
    }

    /// <summary>传输加密：返回「密文‖tag」合并 byte[]（pt_len+16）；会话未就绪返回 null。</summary>
    public byte[] Encrypt(byte[] plaintext) {
        if (plaintext == null) { throw new ArgumentNullException("plaintext"); }
        return this._EncryptArr(plaintext);
    }

    /// <summary>传输解密：认证失败（篡改 tag/密文）返回 null。</summary>
    public byte[] Decrypt(byte[] ciphertext, byte[] tag) {
        if (ciphertext == null) { throw new ArgumentNullException("ciphertext"); }
        if (tag == null || tag.Length != 16) {
            throw new ArgumentException("Noise transport tag must be exactly 16 bytes.");
        }
        return this._DecryptArr(ciphertext, tag);
    }
}

public class NoiseTransport {
    /// <summary>initiate_arr：msg1 = e.pub(32) + 空 payload AEAD tag(16) = 48 字节。</summary>
    [Builtin(ABI = "rt_noise_initiate_handshake_arr")]
    private static byte[] _InitiateArr(string handle) { return null; }

    /// <summary>respond_arr：msg1 → msg2 = e.pub(32) + tag(16) = 48 字节。</summary>
    [Builtin(ABI = "rt_noise_respond_handshake_arr")]
    private static byte[] _RespondArr(string handle, byte[] inMsg) { return null; }

    /// <summary>finalize_arr：msg2 → Split + msg3 = s.pub(32) + tag(16) = 64 字节。</summary>
    [Builtin(ABI = "rt_noise_initiate_finalize_arr")]
    private static byte[] _FinalizeArr(string handle, byte[] inMsg) { return null; }

    /// <summary>respond_finalize_arr：msg3 → Split；false = 握手失败。</summary>
    [Builtin(ABI = "rt_noise_respond_finalize_arr")]
    private static bool _RespondFinalizeArr(string handle, byte[] inMsg) { return false; }

    /// <summary>handshake_hash_arr：握手 hash（32 字节，Split 后可用）。</summary>
    [Builtin(ABI = "rt_noise_session_handshake_hash_arr")]
    private static byte[] _HandshakeHashArr(string handle) { return null; }

    /// <summary>发起握手首条消息 → msg1（48 字节；真实 X25519 临时密钥生成）。</summary>
    public static byte[] Initiate(string handle) {
        if (handle == null) { throw new ArgumentNullException("handle"); }
        return _InitiateArr(handle);
    }

    /// <summary>响应握手：msg1 → msg2（48 字节）。</summary>
    public static byte[] Respond(string handle, byte[] inMsg) {
        if (handle == null) { throw new ArgumentNullException("handle"); }
        if (inMsg == null) { throw new ArgumentNullException("inMsg"); }
        return _RespondArr(handle, inMsg);
    }

    /// <summary>发起方完成握手：msg2 → Split 并返回 msg3（64 字节，投递给响应方）。</summary>
    public static byte[] Finalize(string handle, byte[] inMsg) {
        if (handle == null) { throw new ArgumentNullException("handle"); }
        if (inMsg == null) { throw new ArgumentNullException("inMsg"); }
        return _FinalizeArr(handle, inMsg);
    }

    /// <summary>响应方完成握手：msg3 → Split；false = 握手失败（长度/认证不符）。</summary>
    public static bool RespondFinalize(string handle, byte[] inMsg) {
        if (handle == null) { throw new ArgumentNullException("handle"); }
        if (inMsg == null) { throw new ArgumentNullException("inMsg"); }
        return _RespondFinalizeArr(handle, inMsg);
    }

    /// <summary>握手 hash（32 字节；Split 后可用，双端一致即握手表成）。</summary>
    public static byte[] HandshakeHash(string handle) {
        if (handle == null) { throw new ArgumentNullException("handle"); }
        return _HandshakeHashArr(handle);
    }
}
