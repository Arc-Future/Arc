// RFC 038 上下文成体系：AISkillContextProvider — Skill 激活提示的内置上下文源。
//
// 将 Skill 的激活提示封装为 AIContextProvider：按注册序把每个 Skill 的 ActivationPrompt
// 产成为「Rules 层」上下文块（同类同优先级；空提示跳过）。RFC 038：由 Host 侧经
// AddProvider 作为普通 provider 注册（skills 非空才注册）。静态源——忽略 query/session，
// 返回已完成 Task；不实现调用后方向。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 内置上下文源：Skill 激活提示。产出每个已注册 Skill 的 ActivationPrompt
/// 为一个 Rules 层上下文块（注册序稳定；空提示不产出）。由 <see cref="AIContextEngine"/>
/// （Host 级组合根）统一排序合并。RFC 038 收编为普通 provider：由宿主（AIHost）注册，
/// 开发者可整体替换 / 移除。
/// </summary>
public class AISkillContextProvider : AIContextProvider {
    private AISkillSet _skills;

    public AISkillContextProvider(AISkillSet skills) {
        _skills = skills != null ? skills : new AISkillSet();
    }

    public override string GetName() { return "skills"; }

    public override string GetDescription() {
        return "Skill activation prompts (Rules layer).";
    }

    /// <summary>静态源：忽略 query/session，返回已完成 Task。</summary>
    public override Task<List<AIContextBlock>> ProvideContextAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> list = new List<AIContextBlock>();
        List<string> names = _skills.Names();
        int n = names.Count;
        int i = 0;
        while (i < n) {
            AISkill s = _skills.Find(names[i]);
            string prompt = s != null && s.ActivationPrompt != null ? s.ActivationPrompt : "";
            if (prompt != "") {
                AIContextBlock blk = new AIContextBlock("skills", "Rules", 0, prompt);
                blk.Title = "Activation: " + (s != null && s.Name != null ? s.Name : "");
                list.Add(blk);
            }
            i = i + 1;
        }
        return Task.FromResult(list);
    }
}