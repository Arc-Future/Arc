// AuthorizeAttribute —— 鉴权标记（RFC 040 §1.10）：标识 IRequest 纳入鉴权。
// 拆分维护：Web 特性按「一文件一类型」统一置于 Attributes/。
namespace Arc.Web;
using Arc;

/// <summary>
/// 标记 IRequest 纳入鉴权；可选 Roles 声明角色需求（默认角色鉴权）。
/// 鉴权行为可扩展（IAuthorizationBehavior），默认角色鉴权，可扩展动态鉴权/API Key 等。
/// </summary>
[AttributeUsage(AttributeTargets.Class)]
public class AuthorizeAttribute : Attribute {
    public string Roles;

    public AuthorizeAttribute() {
        this.Roles = "";
    }

    public AuthorizeAttribute(string roles) {
        this.Roles = roles != null ? roles : "";
    }
}
