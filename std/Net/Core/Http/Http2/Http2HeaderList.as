// Http2HeaderList —— 拆分自 Http2Types.as（一文件一公开类型）。
namespace Arc.Net;
using Arc.Collections;

/// <summary>HTTP/2 头字段列表（顺序保持接收次序；`Get` 不区分大小写）。</summary>
public class Http2HeaderList {
    private List<Http2Header> _items;

    public Http2HeaderList() {
        _items = new List<Http2Header>();
    }

    /// <summary>头部数量。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>追加头字段（请求侧构造 / 响应侧解析共用）。</summary>
    public void Add(string name, string value) {
        _items.Add(new Http2Header(name, value));
    }

    /// <summary>按名取值（不区分大小写）；未命中返回空串。</summary>
    public string Get(string name) {
        int i = 0;
        while (i < _items.Count) {
            Http2Header h = _items[i];
            if (SameName(h.Name, name)) { return h.Value; }
            i = i + 1;
        }
        return "";
    }

    /// <summary>第 i 个头字段名。</summary>
    public string GetName(int i) { return _items[i].Name; }

    /// <summary>第 i 个头字段值。</summary>
    public string GetValue(int i) { return _items[i].Value; }

    private static bool SameName(string a, string b) {
        if (a.Length != b.Length) { return false; }
        int i = 0;
        while (i < a.Length) {
            char ca = a[i];
            char cb = b[i];
            if (ca >= 'A' && ca <= 'Z') { ca = (char)(ca + 32); }
            if (cb >= 'A' && cb <= 'Z') { cb = (char)(cb + 32); }
            if (ca != cb) { return false; }
            i = i + 1;
        }
        return true;
    }
}
