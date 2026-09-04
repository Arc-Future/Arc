// UserPrincipal —— 请求主体（RFC 040 §1.11）：最小面——身份 + 角色集。
// 无 IPrincipal/IIdentity 继承（Arc 禁用接口继承依赖）。可扩展（Claims/Attributes 后置）。
namespace Arc.Web;

/// <summary>请求主体（RFC 040 §1.11）：已验证用户的身份与角色集。null 表示未认证。</summary>
public class UserPrincipal {
    /// <summary>用户标识（用户名/ID/邮箱等）。认证方设置，null 表示匿名。</summary>
    public string? Identity { get; }

    /// <summary>角色名列表（空列表表示无角色）。</summary>
    public List<string> Roles { get; }

    /// <summary>是否在指定角色中（大小写敏感；空角色列表恒 false）。</summary>
    public bool IsInRole(string role) {
        for (int i = 0; i < this.Roles.Count; i++) {
            if (this.Roles[i] == role) {
                return true;
            }
        }
        return false;
    }

    public UserPrincipal(string? identity) {
        this.Identity = identity;
        this.Roles = new List<string>();
    }

    public UserPrincipal(string? identity, List<string> roles) {
        this.Identity = identity;
        this.Roles = roles;
    }
}