// AIModelRegistryEvents — 注册表生命周期事件回调（RFC 041 §7.2 统计/审计）。
//
// 与 AIPlanApprovalHandler 同构（Action 字段回调类 + SetEvents 注入）。应用订阅
// 写日志/审计/通知；null = 不通知（默认零开销）。回调仅反映注册/加载/驱逐/
// 加载失败四类事件，调用统计经 AIModelRegistry.GetStats 读取。
namespace Arc.AI;

/// <summary>模型注册表生命周期事件回调（RFC 041 §7.2；应用经
/// <see cref="AIModelRegistry.SetEvents"/> 订阅）。</summary>
public class AIModelRegistryEvents {
    /// <summary>模型注册（Register 调用，按名覆盖时亦触发）。</summary>
    public Action<AIModelRegistration> OnModelRegistered;

    /// <summary>模型加载完成（runner 就绪）。</summary>
    public Action<AIModelRegistration> OnModelLoaded;

    /// <summary>模型被策略驱逐卸载。</summary>
    public Action<AIModelRegistration> OnModelEvicted;

    /// <summary>模型加载失败。</summary>
    public Action<AIModelRegistration> OnLoadFailed;

    public AIModelRegistryEvents() {
        this.OnModelRegistered = null;
        this.OnModelLoaded = null;
        this.OnModelEvicted = null;
        this.OnLoadFailed = null;
    }
}
