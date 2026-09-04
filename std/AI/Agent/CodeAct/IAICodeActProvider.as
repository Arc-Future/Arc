// M8 CodeAct（RFC 038 §3.4.2）：模型生成代码执行的可插拔后端抽象。
namespace Arc.Agent;
using Arc;

/// <summary>
/// CodeAct 执行后端（可插拔）。脚本后端（独立解释器进程）/ 原生后端（编译器 ABI）各一实现，
/// 契约稳定——<see cref="ExecuteAsync"/> 为唯一入口，宿主可替换后端而不改上层。
/// </summary>
public interface IAICodeActProvider {
    /// <summary>
    /// 执行一段模型生成代码。经独立进程/单元执行，绝不在宿主进程内跑任意代码。
    /// </summary>
    /// <param name="code">要执行的代码文本。</param>
    /// <param name="env">环境变量（可空）。</param>
    /// <param name="timeoutMs">执行超时（毫秒；超时须终止进程）。</param>
    /// <param name="maxOutputChars">标准输出/错误各自的上限字符数（超出截断标记 Truncated）。</param>
    /// <param name="cancellationToken">取消令牌（取消须终止进程）。</param>
    Task<AICodeActResult> ExecuteAsync(
        string code,
        Dictionary<string, string?> env,
        long timeoutMs,
        int maxOutputChars,
        CancellationToken cancellationToken);
}
