// DispatchContext —— 分发上下文（RFC 040 §1.7 · internal）。
namespace Arc.Web;
using Arc.DI;

/// <summary>
/// 分发上下文（internal）：一次请求分发的输入载体——每请求 DI 服务提供者 +
/// 绑定 JSON（由 Binder 按 method 构造）。
/// </summary>
internal class DispatchContext {
    public IServiceProvider Sp;
    public string BindJson;

    public DispatchContext() {
        Sp = null;
        BindJson = "";
    }
}
