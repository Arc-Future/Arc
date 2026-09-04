// RequestMethodAttribute —— 请求方法特性基类（RFC 040 §1.5/§1.7）：自描述路由。
// 拆分维护：请求方法特性族按「一文件一类型」置于 Attributes/，与 AuthorizeAttribute 统一管理。
namespace Arc.Web;
using Arc;

/// <summary>
/// 请求方法特性基类：Method（HTTP 方法）+ Template（路由模板）。
/// 派生 [Get]/[Post]/[Put]/[Delete]/[Patch] 分别固化 HTTP 方法，仅需传模板。
/// </summary>
[AttributeUsage(AttributeTargets.Class)]
public class RequestMethodAttribute : Attribute {
    public string Method;
    public string Template;

    public RequestMethodAttribute(string method, string template) {
        this.Method = method;
        this.Template = template;
    }
}
