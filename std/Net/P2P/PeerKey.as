// RFC 042: PeerKey — Ed25519 身份密钥对（RFC 8032）。
//
// RFC 026 M3 P0-1 假面清除：原 [Builtin] 公开方法为假接线（Generate 返回裸 pk
// 指针冒充对象、Sign 产出未初始化缓冲、Verify 传 msg_len=0/pk=null）。现改
// AesGcm 模式（regular class）：私有 [Builtin] `_Xxx` 经 codegen 拦截直射
// vendored crypto_native.dll 的 `rt_crypto_ed25519_*_arr` ABI（byte[] 语义见
// rt_abi.h）；公开方法为真实体，负责参数校验与对象语义。
//
// 密钥布局：keygen_arr 返回 byte[64] = seed(32)‖pk(32)；构造器切分存储，
// Sign 直传 seed（sign_arr 要求 byte[32]），PublicKey 由 pk 小写 hex 编码。

namespace Arc.Net.P2P;

using Arc.Text;

public class PeerKey {
    private byte[] _sk;   // seed（32 字节，签名私钥）
    private byte[] _pk;   // 公钥（32 字节）

    private PeerKey(byte[] keypair) {
        this._sk = new byte[32];
        this._pk = new byte[32];
        for (int i = 0; i < 32; i++) {
            this._sk[i] = keypair[i];
            this._pk[i] = keypair[i + 32];
        }
    }

    /// <summary>keygen_arr：CSPRNG 种子 → byte[64] = seed‖pk；熵源不可用返回 null（拒绝降级）。</summary>
    [Builtin(ABI = "rt_crypto_ed25519_keygen_arr")]
    private static byte[] _KeygenArr() { return null; }

    /// <summary>seed_keygen_arr：32 字节种子确定性重建 byte[64] = seed‖pk（非 32 字节 C 侧拒绝）。</summary>
    [Builtin(ABI = "rt_crypto_ed25519_seed_keygen_arr")]
    private static byte[] _SeedKeygenArr(byte[] seed) { return null; }

    /// <summary>sign_arr：msg 须 RtArray byte[]（C 侧按 rt_array_length 取长）；
    /// sk 须 byte[32]（seed），返回 byte[64] 签名（R‖S）。</summary>
    [Builtin(ABI = "rt_crypto_ed25519_sign_arr")]
    private byte[] _SignArr(byte[] message, byte[] sk) { return null; }

    /// <summary>verify_arr：sig 须 byte[64]、pk 须 byte[32]；1/0/-1 → bool。</summary>
    [Builtin(ABI = "rt_crypto_ed25519_verify_arr")]
    private bool _VerifyArr(byte[] message, byte[] signature, byte[] pk) { return false; }

    /// <summary>生成新密钥对；系统熵源不可用时返回 null。</summary>
    public static PeerKey Generate() {
        byte[] keypair = _KeygenArr();
        if (keypair == null) { return null; }
        return new PeerKey(keypair);
    }

    /// <summary>从 32 字节种子确定性重建密钥对。</summary>
    public static PeerKey FromSeed(byte[] seed) {
        if (seed == null) { throw new ArgumentNullException("seed"); }
        byte[] keypair = _SeedKeygenArr(seed);
        if (keypair == null) { return null; }
        return new PeerKey(keypair);
    }

    /// <summary>公钥身份（Arc.Text 小写 hex 编码 pk——Net 包不依赖 Security）。</summary>
    public PeerId PublicKey {
        get { return new PeerId(Hex.ToHexString(this._pk)); }
    }

    /// <summary>对消息签名 → 64 字节签名（R‖S）。</summary>
    public byte[] Sign(string message) {
        if (message == null) { throw new ArgumentNullException("message"); }
        return this._SignArr(Encoding.GetBytes(message), this._sk);
    }

    /// <summary>验证签名；长度非 64 直接判假（不进 ABI）。</summary>
    public bool Verify(string message, byte[] signature) {
        if (message == null || signature == null || signature.Length != 64) { return false; }
        return this._VerifyArr(Encoding.GetBytes(message), signature, this._pk);
    }

    /// <summary>完整密钥对（seed‖pk，64 字节），供持久化场景。</summary>
    internal byte[] GetSecretKey() {
        byte[] keypair = new byte[64];
        for (int i = 0; i < 32; i++) {
            keypair[i] = this._sk[i];
            keypair[i + 32] = this._pk[i];
        }
        return keypair;
    }
}
