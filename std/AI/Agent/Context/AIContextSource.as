// RFC 038 上下文成体系：AIContextSource — 上下文组合的不可变结果快照。
//
// Agent = LLM + Context + Tools。AIContextEngine.BuildContext 统一产出本组合结果：
//   Messages = 全部 AIContextProvider 按注册序合并的 system 上下文消息（Context 轴）；
//   Tools    = 主 AIToolSet + 激活 Skill 工具聚合（Tools 轴）。
// 单一组装点 → 前缀稳定 → LLM 上下文缓存可命中。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 上下文组合结果：请求前一次组装，同时承载「上下文消息」与「聚合工具」两个维度，
/// 供 AISession.BuildRequest 直接消费，避免会话层重复组装。
/// </summary>
public class AIContextSource {
    /// <summary>按 provider 注册序合并的 system 上下文消息（空 = 无附加上下文）。</summary>
    public List<AIMessage> Messages;
    /// <summary>聚合工具集（主 AIToolSet + 激活 Skill 工具；无工具 = 空 AIToolSet）。</summary>
    public AIToolSet Tools;
    /// <summary>预算裁剪丢弃的块数（审计；0 = 未裁剪 / 未设预算）。</summary>
    public int DroppedBlocks;

    public AIContextSource() {
        this.Messages = new List<AIMessage>();
        this.Tools = new AIToolSet();
        this.DroppedBlocks = 0;
    }
}