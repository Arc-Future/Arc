// RFC 025 M4: Arc.Net — 表单 URL 编码请求体内容。
namespace Arc.Net;
using Arc.Text;

/// <summary>表单 URL 编码请求体——适用于 application/x-www-form-urlencoded。</summary>
public class FormUrlEncodedContent : HttpContent {
    /// <summary>创建表单编码请求体。</summary>
    /// <param name="formData">已编码的 key1=value1&amp;key2=value2 字符串。</param>
    public FormUrlEncodedContent(string formData) {
        this.Body = formData;
        this.ContentType = "application/x-www-form-urlencoded";
    }

    /// <summary>从键值对构建表单体（键与值经 <see cref="Url.Encode"/> 百分号编码）。</summary>
    public FormUrlEncodedContent(string key, string value) {
        this.Body = Url.Encode(key) + "=" + Url.Encode(value);
        this.ContentType = "application/x-www-form-urlencoded";
    }
}
