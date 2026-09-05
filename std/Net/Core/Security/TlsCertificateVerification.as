// TlsCertificateVerification —— 拆分自 TlsClientSession.as（一文件一公开类型）。
namespace Arc.Net.Security;
using Arc.Collections;
using Arc.Text;
using Arc.Net;
using Arc.Security.Cryptography;
using Arc.Threading;

/// <summary>TLS 证书校验策略（S5：`TrustAnchor` 语义从「null=不校验」升级为显式策略）。</summary>
public enum TlsCertificateVerification {
    /// <summary>不校验对端证书（仅测试面；显式设置时覆盖锚）。</summary>
    None,
    /// <summary>信任锚最小校验（单 DER 锚；等同 M3 行为）。</summary>
    Anchor,
    /// <summary>完整链校验（根+中间 PEM 链；含有效期/主机名/吊销 CRL 最小面）。</summary>
    FullChain
}
