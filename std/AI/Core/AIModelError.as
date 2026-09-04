// AIModelError — 后端错误收敛载体（RFC 041 §7.3 错误契约）。
//
// 对齐 OpenAI error object：{"error": {"message", "type", "param", "code"}}。
// 门面/服务层据此映射统一异常层次（ToException 实现 §7.3 映射表）；映射表之外的
// Type 一律收敛为 AIModelException（不臆造子类）。Param 指错误字段（如 "input"）、
// Code 携带服务端错误码。
namespace Arc.AI;

/// <summary>
/// 后端错误收敛载体（RFC 041 §7.3，对齐 OpenAI error object）。Message/Type/
/// Code/Param 四字段承载，经 <see cref="ToException"/> 映射统一异常层次。
/// </summary>
public class AIModelError {
    /// <summary>人类可读错误描述。</summary>
    public string Message;

    /// <summary>错误类型（OpenAI error type，如 rate_limit_error / server_error / timeout）。</summary>
    public string Type;

    /// <summary>服务端错误码（无码可为空串）。</summary>
    public string Code;

    /// <summary>出错字段（如 "input"）；null = 无特定字段。</summary>
    public string? Param;

    public AIModelError() {
        this.Message = "";
        this.Type = "";
        this.Code = "";
        this.Param = null;
    }

    public AIModelError(string message, string type, string code, string? param) {
        this.Message = message != null ? message : "";
        this.Type = type != null ? type : "";
        this.Code = code != null ? code : "";
        this.Param = param;
    }

    /// <summary>
    /// 从底层异常构造错误对象（服务层收敛后端错误；Type 取 inference_error 惯例）。
    /// </summary>
    public static AIModelError FromException(Exception? ex) {
        string msg = "inference error";
        if (ex != null && ex.Message != null) {
            msg = ex.Message;
        }
        return new AIModelError(msg, "inference_error", "", null);
    }

    /// <summary>
    /// 按 RFC 041 §7.3 映射表把错误对象映射为统一异常层次：
    /// rate_limit_error / insufficient_quota → BudgetExceeded；server_error →
    /// Inference（可重试）；timeout → Timeout；其余一律收敛为 AIModelException。
    /// </summary>
    public AIModelException ToException() {
        if (this.Type == "rate_limit_error" || this.Type == "insufficient_quota") {
            return new AIModelBudgetExceededException(this.Message);
        }
        if (this.Type == "server_error") {
            return new AIModelInferenceException(this.Message, this);
        }
        if (this.Type == "timeout") {
            return new AIModelTimeoutException(this.Message);
        }
        return new AIModelException(this.Message);
    }
}
