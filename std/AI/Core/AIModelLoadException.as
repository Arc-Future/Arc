// AIModelLoadException — 模型加载/初始化失败（RFC 041 §7.3）。
//
// 注册表 EnsureLoaded 的工厂调用抛非 AI 异常时收敛为本类型（包装底层错误）。
// 加载失败后模型槽状态落 Failed，可再次 Acquire 重试。
namespace Arc.AI;

using Arc;

/// <summary>模型加载/初始化失败（RFC 041 §7.3）。</summary>
public class AIModelLoadException : AIModelException {
    public AIModelLoadException() : base() { }
    public AIModelLoadException(string message) : base(message) { }
    public AIModelLoadException(string message, Exception? innerException) : base(message, innerException) { }
}
