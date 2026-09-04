// IRedirectResult —— 重定向结果（RFC 040 §5）：PRG（Post-Redirect-Get）模式载体。
namespace Arc.Web;

/// <summary>重定向结果：携带目标 URL（响应 302/303 + Location）。</summary>
public interface IRedirectResult : IWebResult {
    /// <summary>重定向目标 URL。</summary>
    string Url { get; }
}
