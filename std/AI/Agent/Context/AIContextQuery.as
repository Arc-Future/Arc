// RFC 038 上下文成体系：AIContextQuery — 查询/任务上下文（动态源检索触发）。
//
// 真实上下文工程中，RAG / 时间感知 / 记忆等动态源需"当前这一步"的信息才能产出相关
// 上下文——查询/任务上下文随每次 BuildAsync 注入，承载会话、当前用户请求与回合
// 序号。静态源（Skill / Wiki 全量）忽略该参数。
namespace Arc.Agent;

/// <summary>
/// 查询/任务上下文：随每次 <see cref="AIContextProvider.BuildAsync"/> 注入，
/// 供动态提供方按需检索（如以 <see cref="Prompt"/> 作 RAG 查询、以回合序号区分
/// 首轮/续轮）。静态源无须消费。
/// </summary>
public class AIContextQuery {
    /// <summary>会话标识（记忆 / 审计归属）。</summary>
    public string SessionId;
    /// <summary>当前用户请求文本（RAG / 意图检索的触发输入；空 = 无用户输入）。</summary>
    public string Prompt;
    /// <summary>回合序号（首轮回合起始；供时序感知源区分阶段）。</summary>
    public int TurnIndex;

    public AIContextQuery() {
        this.SessionId = "";
        this.Prompt = "";
        this.TurnIndex = 0;
    }

    public AIContextQuery(string sessionId, string prompt, int turnIndex) {
        this.SessionId = sessionId != null ? sessionId : "";
        this.Prompt = prompt != null ? prompt : "";
        this.TurnIndex = turnIndex;
    }
}