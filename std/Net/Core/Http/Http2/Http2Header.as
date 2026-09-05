// Http2Header —— 拆分自 Http2Types.as（一文件一公开类型）。
namespace Arc.Net;
using Arc.Collections;

/// <summary>单个 HTTP/2 头字段。</summary>
public class Http2Header {
    public string Name;
    public string Value;

    public Http2Header(string name, string value) {
        Name = name;
        Value = value;
    }
}
