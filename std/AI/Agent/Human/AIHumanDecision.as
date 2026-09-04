// RFC 038 — HITL 决策种类（单一枚举）。
namespace Arc.Agent;

/// <summary>人类门闩关闭方式。</summary>
public enum AIHumanDecision {
    /// <summary>批准（可带编辑后的工具草稿）。</summary>
    Approved = 0,
    /// <summary>拒绝；Session 回合 Failed（M1 锁死）。</summary>
    Rejected = 1,
    /// <summary>补充输入；Session 回到 Completing。</summary>
    InputProvided = 2,
    /// <summary>取消。</summary>
    Cancelled = 3,
}
