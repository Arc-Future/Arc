// RoleAuthorizationBehavior —— 默认角色鉴权（RFC 040 §1.10）。
// 端点声明角色（逗号分隔），主体含任一声明角色即放行；否则抛 UnauthorizedException。
namespace Arc.Web;
using Arc;
using Arc.Collections;

/// <summary>
/// 默认角色鉴权行为（RFC 040 §1.10）：比对主体角色集与端点声明角色（任一匹配即放行）。
/// 空声明（无需鉴权）直接放行；无主体或角色不足抛 UnauthorizedException。
/// 可扩展（动态鉴权/DB 支撑）经 IAuthorizationBehavior 派生实现。
/// </summary>
public class RoleAuthorizationBehavior : IAuthorizationBehavior {
    public Task AuthorizeAsync(RequestContext context, string requiredRoles) {
        if (requiredRoles == null || requiredRoles == "") {
            return Task.CompletedTask;
        }
        if (context.User == null) {
            throw new UnauthorizedException("Authentication required: " + requiredRoles);
        }
        List<string> roles = this.SplitRoles(requiredRoles);
        for (int i = 0; i < roles.Count; i++) {
            if (context.User.IsInRole(roles[i])) {
                return Task.CompletedTask;
            }
        }
        throw new UnauthorizedException("Insufficient role, required: " + requiredRoles);
    }

    /// <summary>按逗号拆分角色声明并去空白。</summary>
    private List<string> SplitRoles(string s) {
        List<string> result = new List<string>();
        int start = 0;
        for (int i = 0; i <= s.Length; i++) {
            if (i == s.Length || s.Substring(i, 1) == ",") {
                string part = s.Substring(start, i - start).Trim();
                if (part != "") {
                    result.Add(part);
                }
                start = i + 1;
            }
        }
        return result;
    }
}
