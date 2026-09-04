// UnauthorizedException —— 鉴权失败异常（RFC 040 §1.10）。
// 宿主捕获后映射 HTTP 401 Unauthorized；扩展行为（自定义拒绝原因）可派生。
namespace Arc.Web;
using Arc;

/// <summary>鉴权失败异常（RFC 040 §1.10）：主体缺失或角色不足。宿主捕获映射 HTTP 401。</summary>
public class UnauthorizedException : Exception {
    public UnauthorizedException(string message) : base(message) { }
}
