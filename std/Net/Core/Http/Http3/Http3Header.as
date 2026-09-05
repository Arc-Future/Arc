// Http3Header —— 拆分自 Http3Types.as（一文件一公开类型）。
namespace Arc.Net;

/// <summary>单个 HTTP/3 头字段（伪头与常规头统一表示）。</summary>
public class Http3Header {
    public string Name;
    public string Value;

    public Http3Header(string name, string value) {
        Name = name;
        Value = value;
    }
}
