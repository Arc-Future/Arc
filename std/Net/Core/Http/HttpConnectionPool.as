// RFC 033 §1.0: Arc.Net — HTTP 连接池（对齐 C# HttpConnectionPool）。
//
// 按 host:port 维护空闲连接的复用与淘汰。逻辑由原 HttpClient.as 的
// AcquireConnection/StorePool/DiscardPool/ShouldKeepAlive 迁移至此（单一职责）。
// 同步当面；异步待 §1.4。禁止迭代中改容器（NLL 无条件启用）。
namespace Arc.Net;

using Arc.Collections;

/// <summary>
/// HTTP 连接池——按 scheme+authority（host:port）复用空闲 TCP 连接。
/// 对齐 C# System.Net.Http.HttpConnectionPool。上限 _maxPoolSize 内 LRU 淘汰。
/// </summary>
public class HttpConnectionPool {
    private List<string> _poolKeys;
    private List<TcpClient> _poolClients;
    private int _maxPoolSize;
    private int _timeout;

    public HttpConnectionPool() {
        _poolKeys = new List<string>();
        _poolClients = new List<TcpClient>();
        _maxPoolSize = 8;
        _timeout = 30000;
    }

    /// <summary>连接超时（毫秒），新建连接时应用。</summary>
    public int Timeout {
        get { return _timeout; }
        set { _timeout = value; }
    }

    /// <summary>从池中取用 host:port 的有效连接；无则新建并连接。失败返回 null。</summary>
    public TcpClient AcquireConnection(string host, int port) {
        string key = this._poolKey(host, port);
        int found = -1;
        for (int i = 0; i < _poolKeys.Count; i++) {
            if (_poolKeys[i] == key && _poolClients[i].Connected) {
                found = i;
                break;
            }
        }
        if (found >= 0) {
            var c = _poolClients[found];
            _poolKeys.RemoveAt(found);
            _poolClients.RemoveAt(found);
            return c;
        }
        // 清理失效连接：先收集索引再统一移除（迭代中禁改容器）。
        List<int> stale = new List<int>();
        for (int i = 0; i < _poolClients.Count; i++) {
            if (!_poolClients[i].Connected) {
                stale.Add(i);
                _poolClients[i].Close();
            }
        }
        for (int k = stale.Count - 1; k >= 0; k--) {
            int idx = stale[k];
            _poolKeys.RemoveAt(idx);
            _poolClients.RemoveAt(idx);
        }
        var cl = new TcpClient();
        cl.SetReceiveTimeout(_timeout);
        cl.SetSendTimeout(_timeout / 3);
        cl.SetNoDelay(true);
        if (!cl.Connect(host, port)) { cl.Close(); return null; }
        return cl;
    }

    /// <summary>将连接归还池中（去重 + 容量淘汰最旧）。</summary>
    public void StorePool(string host, int port, TcpClient cl) {
        string key = this._poolKey(host, port);
        // 去重：同一 host:port 已有连接则关闭旧连接（先找索引，退出迭代后再移除）。
        int dup = -1;
        for (int i = 0; i < _poolKeys.Count; i++) {
            if (_poolKeys[i] == key) {
                dup = i;
                break;
            }
        }
        if (dup >= 0) {
            _poolClients[dup].Close();
            _poolKeys.RemoveAt(dup);
            _poolClients.RemoveAt(dup);
        }
        // 容量超限时淘汰最旧的连接（索引 0）。
        if (_poolKeys.Count >= _maxPoolSize) {
            _poolClients[0].Close();
            _poolKeys.RemoveAt(0);
            _poolClients.RemoveAt(0);
        }
        _poolKeys.Add(key);
        _poolClients.Add(cl);
    }

    /// <summary>清空并关闭所有池化连接。</summary>
    public void ClearConnectionPool() {
        for (int i = 0; i < _poolClients.Count; i++) {
            _poolClients[i].Close();
        }
        _poolKeys.Clear();
        _poolClients.Clear();
    }

    /// <summary>判断响应连接是否可复用（keep-alive 语义，RFC 7230 §6.3）。</summary>
    public bool ShouldKeepAlive(HttpResponseMessage r) {
        if (r.StatusCode >= 400) { return false; }
        // 响应体无明确分帧（无 Content-Length 亦无 Transfer-Encoding）→ 读到 EOF，
        // 不能复用于持久连接。
        if (!r._keepAlive) { return false; }
        string c = r.Headers.Get("Connection");
        if (c != "" && c.ToLower() == "close") { return false; }
        // HTTP/1.0 默认短连接，须显式 Connection: keep-alive 才可复用。
        if (r.Version == "HTTP/1.0" && c.ToLower() != "keep-alive") { return false; }
        return true;
    }

    private string _poolKey(string host, int port) {
        return host + ":" + Convert.ToString(port);
    }
}