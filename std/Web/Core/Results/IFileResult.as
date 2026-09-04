// IFileResult —— 文件/二进制结果（RFC 040 §5）：下载 / 资源响应载体。
namespace Arc.Web;

/// <summary>文件/二进制结果：Http 契约的 ContentType/Data 复用 IWebResult；本面仅追加下载文件名。</summary>
public interface IFileResult : IWebResult {
    /// <summary>下载文件名（Content-Disposition 用；空表示内联展示）。</summary>
    string FileName { get; }
}