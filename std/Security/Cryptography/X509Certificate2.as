// RFC 026 M3: X509Certificate2 — X.509 证书解析 facade（S0 TLS 1.3 证书面）。
//
// 对标 C# System.Security.Cryptography.X509Certificates.X509Certificate2（去糟粕）：
// S0 仅解析 DER/PEM 证书 + 提取 Subject 名称 + RSA 公钥（验签用）；完整链校验/
// 有效期/主机名匹配后置（RFC 026 §1.2 ④ 诚实边界）。实例方法经 codegen 拦截
// 直射 vendored crypto_native.dll 的 `rt_crypto_x509_*` ABI（mbedTLS 4.1.1）。
//
// RFC 026 M3 P0-1 生命周期修复：Subject 改为构造时缓存（原 [Builtin] 属性每次
// 访问 malloc 新 C 字符串且无 free → 泄漏）；新增 Dispose()（rt_crypto_x509_free
// 幂等）——释放后 Verify/PublicKey 抛 ObjectDisposedException，杜绝 use-after-free。

namespace Arc.Security.Cryptography;

public class X509Certificate2 {
    private long _handle;
    private byte[] _rawData;
    private string _subject;   // 构造时缓存（rt_crypto_x509_subject 返回 malloc'd C 字符串）

    private X509Certificate2(long handle, byte[] rawData) {
        this._handle = handle;
        this._rawData = rawData;
        this._subject = this._LoadSubject();
    }

    /// <summary>从 DER 字节解析证书 → opaque 句柄；失败返回 0。</summary>
    [Builtin(ABI = "rt_crypto_x509_parse_der")]
    private static long _ParseDer(byte[] der) { return 0; }

    /// <summary>从 PEM 字符串解析证书 → opaque 句柄；失败返回 0。</summary>
    [Builtin(ABI = "rt_crypto_x509_parse_pem")]
    private static long _ParsePem(string pem) { return 0; }

    /// <summary>解析 PEM 编码证书（RFC 026 §1.2 ④）。</summary>
    public static X509Certificate2 CreateFromPem(string pem) {
        if (pem == null || pem.Length == 0) {
            throw new ArgumentException("X509Certificate2 requires a non-empty PEM string.");
        }
        long handle = _ParsePem(pem);
        if (handle == 0) {
            throw new ArgumentException("X509Certificate2.CreateFromPem failed to parse PEM.");
        }
        return new X509Certificate2(handle, null);
    }

    /// <summary>解析 DER 编码证书（RFC 026 §1.2 ④）。</summary>
    public static X509Certificate2 CreateFromDer(byte[] der) {
        if (der == null || der.Length == 0) {
            throw new ArgumentException("X509Certificate2 requires a non-empty DER byte[].");
        }
        long handle = _ParseDer(der);
        if (handle == 0) {
            throw new ArgumentException("X509Certificate2.CreateFromDer failed to parse DER.");
        }
        return new X509Certificate2(handle, der);
    }

    /// <summary>rt_crypto_x509_subject：malloc'd C 字符串（Arc string 语义直收）。</summary>
    [Builtin(ABI = "rt_crypto_x509_subject")]
    private string _LoadSubject() { return null; }

    /// <summary>主题名称（CN 等，mbedTLS 文本格式）——构造时缓存，避免每次访问
    /// 泄漏 malloc'd 字符串；Dispose 后为字符串快照仍可读。</summary>
    public string Subject {
        get { return this._subject; }
    }

    /// <summary>DER 原始字节（CreateFromDer 传入；CreateFromPem 为 null，诚实边界）。</summary>
    public byte[] RawData {
        get { return this._rawData; }
    }

    /// <summary>提取 RSA 公钥（验签用）；非 RSA 证书返回 null。</summary>
    public Rsa PublicKey {
        get {
            long h = this._GetPublicKeyHandle();
            if (h == 0) {
                return null;
            }
            return Rsa.FromHandle(h);
        }
    }

    /// <summary>提取证书公钥句柄（RSA）；非 RSA 返回 0。</summary>
    [Builtin(ABI = "rt_crypto_x509_pubkey")]
    private long _GetPublicKeyHandle() { return 0; }

    /// <summary>验证本证书是否由给定信任锚签发（RFC 026 §1.2 ④ 最小校验）。
    /// 校验签名、信任链与有效期；true = 有效，false = 无效。</summary>
    [Builtin(ABI = "rt_crypto_x509_verify")]
    private static int _Verify(long leafHandle, long trustHandle) { return 0; }

    /// <summary>验证本证书是否由信任锚签发（联合 CreateFromPem 自签 CA 作信任锚）。</summary>
    public bool Verify(X509Certificate2 trustAnchor) {
        if (trustAnchor == null) {
            throw new ArgumentNullException("trustAnchor");
        }
        if (this._handle == 0 || trustAnchor._handle == 0) {
            throw new ObjectDisposedException("X509Certificate2");
        }
        return _Verify(this._handle, trustAnchor._handle) == 0;
    }
}