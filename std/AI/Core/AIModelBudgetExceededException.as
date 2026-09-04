// AIModelBudgetExceededException — 内存预算/回合预算超限（RFC 041 §7.3）。
//
// 注册表预算检查拒绝加载时抛（MemoryBudgetBytes 超限且无可驱逐空闲模型）。
// 对齐 OpenAI 错误映射：Type = rate_limit_error（429）/ insufficient_quota →
// 本类型。预算不足属可观测失败面，绝不静默降级。
namespace Arc.AI;

using Arc;

/// <summary>内存/回合预算超限（RFC 041 §7.3）。</summary>
public class AIModelBudgetExceededException : AIModelException {
    public AIModelBudgetExceededException() : base() { }
    public AIModelBudgetExceededException(string message) : base(message) { }
    public AIModelBudgetExceededException(string message, Exception? innerException) : base(message, innerException) { }
}
