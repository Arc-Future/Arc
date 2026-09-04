// AIModelTimeoutException — 调用超时（RFC 041 §7.3）。
//
// 服务骨架执行超时（TimeoutMs 内未完成）时抛本类型；可重试（幂等推理默认允许，
// 指数退避；TTS 等非幂等默认 MaxRetries = 0 不重试）。请求级超时 / OpenAI 错误
// Type = timeout 亦映射本类型。
namespace Arc.AI;

using Arc;

/// <summary>模型调用超时（可重试；RFC 041 §7.3）。</summary>
public class AIModelTimeoutException : AIModelException {
    public AIModelTimeoutException() : base() { }
    public AIModelTimeoutException(string message) : base(message) { }
    public AIModelTimeoutException(string message, Exception? innerException) : base(message, innerException) { }
}
