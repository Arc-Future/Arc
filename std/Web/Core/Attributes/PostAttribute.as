// PostAttribute —— POST 路由特性（RFC 040 §1.5/§1.7）：自描述路由。
// 拆分维护：请求方法特性族按「一文件一类型」置于 Attributes/，与基类 RequestMethodAttribute 分文件。
namespace Arc.Web;
using Arc;

/// <summary>POST 路由：`[Post("/api/users")]`。</summary>
[AttributeUsage(AttributeTargets.Class)]
public class PostAttribute : RequestMethodAttribute {
    public PostAttribute(string template) : base("POST", template) { }
}
