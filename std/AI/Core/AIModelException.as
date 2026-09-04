// AIModelException — 小模型统一异常基类（RFC 041 §7.3）。
//
// 六层统一异常层次的根（NotAvailable/Load/Timeout/BudgetExceeded/Inference/
// Cancelled）。对齐 §3 门闩降级链既有模式（OnnxException 等）：业务侧 catch
// AIModelException 统一收敛小模型错误；映射表之外的错误收敛为本类型（不臆造子类）。
namespace Arc.AI;

using Arc;

/// <summary>小模型统一异常基类（RFC 041 §7.3）。所有小模型错误收敛为本类型或其子类。</summary>
public class AIModelException : SystemException {
    public AIModelException() : base() { }
    public AIModelException(string message) : base(message) { }
    public AIModelException(string message, Exception? innerException) : base(message, innerException) { }
}
