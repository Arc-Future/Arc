// RFC 025 M4 + RFC 033 §1.0.1: Arc.Net — 字节数组内容（对齐 C# ByteArrayContent）。
//
// 诚实边界：MVP 传输面为 string（N1 同步）；字节载荷经 Encoding 转 string 承载，
// 二进制保真随底层 byte[] 管线就位后递升。异步当面待 §1.4。
namespace Arc.Net;

using Arc.Text;

/// <summary>字节数组请求体内容（对齐 C# ByteArrayContent）。</summary>
public class ByteArrayContent : HttpContent {
    private byte[] _data;

    public ByteArrayContent(byte[] data) {
        _data = data;
        this.Body = data != null ? Encoding.GetString(data) : "";
        this.ContentType = "application/octet-stream";
    }

    /// <summary>取原始字节数组（不经 string 中转；null 表示空载荷）。</summary>
    public override byte[] ReadAsByteArray() {
        return _data;
    }
}