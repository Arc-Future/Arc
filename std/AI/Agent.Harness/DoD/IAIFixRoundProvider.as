// RFC 043 场景 2.3/4.3：L2 修复回合注入点 — 基座只定义契约；修复实现由宿主（REPL 模型回合）
// 或领域提供。禁基座写死修复策略（模型 / 脚本 / 人工修复均为一轮 FixAsync）。
namespace Arc.Agent.Harness;

/// <summary>
/// L2 自动迭代的修复回合提供者：输入结构化失败回喂（<see cref="AIDoDFixFeedback"/>），
/// 执行一轮修复并返回修复说明文本（做了什么 / 结果）。由调用方（如 REPL 方向环）以
/// 模型回合实现；e2e 以脚本化修复验证机器闭环。
/// </summary>
public interface IAIFixRoundProvider {
    /// <summary>执行一轮修复（基于失败回喂修码），返回修复说明（可空字符串）。</summary>
    Task<string> FixAsync(AIDoDFixFeedback feedback, CancellationToken cancellationToken);
}
