// RFC 042 D2: Multiaddr — 自描述网络地址格式（对标 libp2p multiaddr）。
//
// 诚实边界：ctor / ToString / Encapsulate / GetValue（斜杠段查找）可证伪；
// 禁止 GetValue 恒返回空串冒充「已解析」。完整编解码 / 校验后置。
namespace Arc.Net.P2P;

public class Multiaddr {
    private string _raw;

    public Multiaddr() {
        _raw = "";
    }

    public Multiaddr(string addr) {
        _raw = addr == null ? "" : addr;
    }

    public static Multiaddr Parse(string addr) {
        return new Multiaddr(addr);
    }

    /// <summary>按协议名查找下一斜杠段（如 <c>/ip4/127.0.0.1/tcp/4001</c> → IP4 → <c>127.0.0.1</c>）。</summary>
    public string GetValue(MultiaddrProtocol proto) {
        string token = "";
        if (proto == MultiaddrProtocol.IP4) { token = "ip4"; }
        else if (proto == MultiaddrProtocol.IP6) { token = "ip6"; }
        else if (proto == MultiaddrProtocol.Tcp) { token = "tcp"; }
        else if (proto == MultiaddrProtocol.Udp) { token = "udp"; }
        else if (proto == MultiaddrProtocol.Dns) { token = "dns"; }
        else if (proto == MultiaddrProtocol.Dns4) { token = "dns4"; }
        else if (proto == MultiaddrProtocol.Dns6) { token = "dns6"; }
        else if (proto == MultiaddrProtocol.Quic) { token = "quic"; }
        else if (proto == MultiaddrProtocol.WS) { token = "ws"; }
        else if (proto == MultiaddrProtocol.Wss) { token = "wss"; }

        string raw = _raw;
        if (raw == null || raw == "" || token == "") {
            return "";
        }
        int i = 0;
        int n = raw.Length;
        while (i < n) {
            while (i < n && raw[i] == '/') {
                i = i + 1;
            }
            if (i >= n) {
                break;
            }
            int nameStart = i;
            while (i < n && raw[i] != '/') {
                i = i + 1;
            }
            string name = raw.Substring(nameStart, i - nameStart);
            while (i < n && raw[i] == '/') {
                i = i + 1;
            }
            int valStart = i;
            while (i < n && raw[i] != '/') {
                i = i + 1;
            }
            if (name == token) {
                if (valStart >= i) {
                    return "";
                }
                return raw.Substring(valStart, i - valStart);
            }
        }
        return "";
    }

    public Multiaddr Encapsulate(Multiaddr other) {
        if (other == null) { return this; }
        Multiaddr result = new Multiaddr();
        result._raw = _raw + other._raw;
        return result;
    }

    public override string ToString() {
        return _raw;
    }
}
