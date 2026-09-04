// RFC 038 上下文成体系：AIContextBlock — 自描述上下文块（组合流水线的最小单元）。
//
// provider 产出的不是裸消息，而是带元数据的「块」——来源、分层标签、布局优先级、标题、
// 引用与 token 估算。引擎据此稳定排序、预算裁剪、扁平化为最终消息面 → 结构传达重要性
// + 前缀稳定 → KV cache 可命中。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 上下文块：<see cref="AIContextProvider"/> 产出的最小自描述单元。除正文外，携带来源
/// <see cref="Provider"/>、层次化布局标签 <see cref="Kind"/>、布局优先级
/// <see cref="Priority"/>、标题 <see cref="Title"/>、来源引用 <see cref="Refs"/> 与
/// token 估算 <see cref="TokenEstimate"/>。引擎按 (Kind 固定序 → Priority) 稳定排序、
/// 预算裁剪后，经 <see cref="ToMessage"/> 扁平化为 system 消息。
/// </summary>
public class AIContextBlock {
    /// <summary>产出源名（审计 / 去重归属）。</summary>
    public string Provider;
    /// <summary>层次化布局标签（开放字符串，如 "Rules"/"Task"/"UserProfile"/"Knowledge"/"ToolOutputs"）。</summary>
    public string Kind;
    /// <summary>同层内布局优先级（小值越靠前；决定前缀位置）。</summary>
    public int Priority;
    /// <summary>块标题（渲染为 system 消息标题行；空 = 无标题）。</summary>
    public string Title;
    /// <summary>块正文（system 上下文内容）。</summary>
    public string Content;
    /// <summary>来源/引用（RAG 检索块附引文；空 = 无）。</summary>
    public List<AIContextSourceRef> Refs;
    /// <summary>token 估算（预算裁剪依据；&lt;=0 视为 0 成本）。</summary>
    public int TokenEstimate;
    /// <summary>是否注入请求面（预算裁剪置 false；默认 true）。</summary>
    public bool Enabled;

    public AIContextBlock() {
        this.Provider = "";
        this.Kind = "";
        this.Priority = 0;
        this.Title = "";
        this.Content = "";
        this.Refs = new List<AIContextSourceRef>();
        this.TokenEstimate = 0;
        this.Enabled = true;
    }

    public AIContextBlock(string provider, string kind, int priority, string content) {
        this.Provider = provider != null ? provider : "";
        this.Kind = kind != null ? kind : "";
        this.Priority = priority;
        this.Title = "";
        this.Content = content != null ? content : "";
        this.Refs = new List<AIContextSourceRef>();
        this.TokenEstimate = 0;
        this.Enabled = true;
    }

    /// <summary>扁平化为 system 消息（标题行 + 正文 + 来源注脚）。</summary>
    public AIMessage ToMessage() {
        string body = "";
        if (this.Title != "") {
            body = "## " + this.Title + "\n\n";
        }
        body = body + this.Content;
        if (this.Refs != null && this.Refs.Count > 0) {
            body = body + "\n\nSources:";
            int i = 0;
            int n = this.Refs.Count;
            while (i < n) {
                AIContextSourceRef r = this.Refs[i];
                string t = r != null && r.Title != null ? r.Title : "";
                string u = r != null && r.Uri != null ? r.Uri : "";
                body = body + "\n- " + t + (u != "" ? (" (" + u + ")") : "");
                i = i + 1;
            }
        }
        return new AIMessage(AIRole.System, body);
    }
}