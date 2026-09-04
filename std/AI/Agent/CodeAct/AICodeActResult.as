// M8 CodeAct（RFC 038 §3.4.2）：模型生成代码执行结果容器。
namespace Arc.Agent;

/// <summary>
/// CodeAct 执行结果——成功/退出码/标准输出/标准错误/错误/超时/取消/截断/耗时。
/// Success 仅在进程正常退出（ExitCode==0）且未超时/取消/被拒时为 true。
/// </summary>
public class AICodeActResult {
    public bool Success;
    public int ExitCode;
    public string StandardOutput;
    public string StandardError;
    public string Error;
    public bool TimedOut;
    public bool Cancelled;
    public bool Truncated;
    public long DurationMs;

    public AICodeActResult() {
        Success = false;
        ExitCode = -1;
        StandardOutput = "";
        StandardError = "";
        Error = "";
        TimedOut = false;
        Cancelled = false;
        Truncated = false;
        DurationMs = 0;
    }

    /// <summary>能力被拒（fail-closed）：未授权执行，无副作用。</summary>
    public static AICodeActResult CapabilityDenied(string reason) {
        AICodeActResult r = new AICodeActResult();
        r.Error = reason != null ? reason : "capability denied";
        return r;
    }

    /// <summary>执行失败（未启动/后端缺失等）。</summary>
    public static AICodeActResult Fail(string reason) {
        AICodeActResult r = new AICodeActResult();
        r.Error = reason != null ? reason : "execution failed";
        return r;
    }
}
