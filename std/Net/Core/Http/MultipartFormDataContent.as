// RFC 025 M4: Arc.Net — Multipart/form-data 请求体内容。
namespace Arc.Net;

/// <summary>Multipart/form-data 请求体——适用于文件上传场景。</summary>
public class MultipartFormDataContent : HttpContent {
    /// <summary>创建 multipart/form-data 请求体。</summary>
    /// <param name="boundary">分隔边界字符串。</param>
    public MultipartFormDataContent(string boundary) {
        this.Body = "";
        this.ContentType = "multipart/form-data; boundary=" + boundary;
    }

    /// <summary>添加文本字段。</summary>
    /// <param name="name">字段名。</param>
    /// <param name="value">字段值。</param>
    public void AddField(string name, string value) {
        string boundary = this.ContentType.Substring(
            this.ContentType.IndexOf("boundary=") + 9,
            this.ContentType.Length - this.ContentType.IndexOf("boundary=") - 9);
        this.Body = this.Body + "--" + boundary + "\r\n";
        this.Body = this.Body + "Content-Disposition: form-data; name=\"" + name + "\"\r\n\r\n";
        this.Body = this.Body + value + "\r\n";
    }

    /// <summary>完成 multipart 体构造（添加终止边界）。</summary>
    public void Finish() {
        string boundary = this.ContentType.Substring(
            this.ContentType.IndexOf("boundary=") + 9,
            this.ContentType.Length - this.ContentType.IndexOf("boundary=") - 9);
        this.Body = this.Body + "--" + boundary + "--\r\n";
    }
}
