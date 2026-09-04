// AIModelInferenceException — 后端推理失败（RFC 041 §7.3）。
//
// 服务骨架 catch-all 把非 AI 执行异常包装为本类型；可选携带 AIModelError
// （Message/Type/Code/Param，后端错误收敛面，不裸透原生状态）。对齐 OpenAI 错误
// 映射：Type = server_error（5xx）→ 本类型（可重试）。
namespace Arc.AI;

using Arc;

/// <summary>后端推理失败（可重试；RFC 041 §7.3）。</summary>
public class AIModelInferenceException : AIModelException {
    /// <summary>收敛后的后端错误对象（Message/Type/Code/Param）；null = 无载体。</summary>
    public AIModelError? Error { get; }

    public AIModelInferenceException() : base() {
        this.Error = null;
    }

    public AIModelInferenceException(string message) : base(message) {
        this.Error = null;
    }

    public AIModelInferenceException(string message, Exception? innerException) : base(message, innerException) {
        this.Error = null;
    }

    /// <summary>携带收敛错误对象的构造。</summary>
    public AIModelInferenceException(string message, AIModelError? error) : base(message) {
        this.Error = error;
    }
}
