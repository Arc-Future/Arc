// IEndpointDispatcher —— 端点分发器契约（RFC 040 §1.7 · internal）。
// 输入绑定 JSON（DispatchContext），输出 handler 分发的响应对象（object 多态面）。
// 宿主按类型桥接：IWebResult 走 HTTP 契约；其余类型回退 JSON 序列化。
namespace Arc.Web;

/// <summary>
/// 端点分发器契约（internal）：承载一次请求的分发——绑定 JSON → 构造请求 →
/// 解析 handler → 调用，返回响应对象（IWebResult 或普通响应对象的装箱视图）。
/// 非泛型面，由具体泛型分发器实现，便于注册表以统一类型持有各类型端点。
/// </summary>
internal interface IEndpointDispatcher {
    object Dispatch(DispatchContext ctx);
}
