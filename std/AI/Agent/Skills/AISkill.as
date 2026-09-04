// RFC 038 基础设施：AISkill — 命名能力单元（可复用能力封装）。
//
// 定位：与声明式 [AITool]（单工具）和 MCP（外部工具源）同级的基础设施。
// Skill = 描述 + 能力工具集(AIToolSet) + 激活提示(ActivationPrompt)。挂载到会话后，
// 激活提示注入 system 上下文（模型可理解该能力何时/如何用），能力工具并入请求
// tools 数组（模型可调用）。开发者按命名单元复用一组能力，而非逐个拼工具。
//
// RFC 038：对齐官方 Agent Skills 规范（agentskills.io, spec 1.0）三级渐进披露：
//   发现层 = Name + Description（常驻 system，~100 tokens）；
//   激活层 = ActivationPrompt / Body（SKILL.md body，命中后注入）；
//   执行层 = references/scripts/assets（按需加载，经 SourcePath 定位）。
// 额外字段（License / Compatibility / AllowedTools）由 AISkillLoader 从 YAML frontmatter
// 反序列化填充（复用 Arc.Text.Yaml，见 AISkillLoader.as）。
//
// 诚实边界：Skill 是「能力封装/复用」基础设施，**非**应用侧 Multi-Agent 编排
// （RFC 004/028 非目标边界保持）。Skill 不管理 Agent 生命周期、不做 RAG、不
// 跨会话编排——这些仍属显式排除。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 命名能力单元——一组能力工具 + 激活提示的封装。<see cref="Tools"/> 的 schema
/// 并入请求 tools 数组；<see cref="ActivationPrompt"/> 注入 system 上下文（会话
/// 建立时稳定注入，保持 KV cache 前缀稳定）。
/// </summary>
public class AISkill {
    /// <summary>Skill 唯一名（注册/查找键；模型与开发者共用）。</summary>
    public string Name;
    /// <summary>模型可见描述（能力何时/如何用的提示原材料；发现层）。</summary>
    public string Description;
    /// <summary>激活提示：注入 system 上下文，说明本 Skill 的能力边界与用法（激活层）。</summary>
    public string ActivationPrompt;
    /// <summary>本 Skill 的能力工具集（schema 并入会话 tools 数组）。空 = 无工具。</summary>
    public AIToolSet Tools;
    /// <summary>许可标识（官方 frontmatter `license`；可选）。</summary>
    public string License;
    /// <summary>兼容性说明（官方 frontmatter `compatibility`；可选，≤500 字符）。</summary>
    public string Compatibility;
    /// <summary>允许的工具白名单（官方 frontmatter `allowed-tools`；可选；空 = 未约束）。</summary>
    public List<string> AllowedTools;
    /// <summary>Skill 根目录（用于执行层 references/scripts/assets 按需定位；加载时填充）。</summary>
    public string SourcePath;

    public AISkill() {
        this.Name = "";
        this.Description = "";
        this.ActivationPrompt = "";
        this.Tools = new AIToolSet();
        this.License = "";
        this.Compatibility = "";
        this.AllowedTools = new List<string>();
        this.SourcePath = "";
    }

    public AISkill(string name, string description, string activationPrompt, AIToolSet tools) {
        this.Name = name != null ? name : "";
        this.Description = description != null ? description : "";
        this.ActivationPrompt = activationPrompt != null ? activationPrompt : "";
        this.Tools = tools != null ? tools : new AIToolSet();
        this.License = "";
        this.Compatibility = "";
        this.AllowedTools = new List<string>();
        this.SourcePath = "";
    }
}