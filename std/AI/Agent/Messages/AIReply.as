namespace Arc.Agent;
using Arc.Collections;
/// <summary>RunAsync / CompleteAsync 出参（RFC 草图 AICompletion 与本类型同一意图；冻结名 AIReply）。</summary>
public class AIReply {
    public string Text;
    /// <summary>assistant 思维链内容（DeepSeek reasoning_content；Provider 解析回填，空 = 未产生/未解析）。</summary>
    public string ReasoningContent;
    public List<AIToolCall> ToolCalls;
    public bool IsError;
    public string ErrorKind;
    public string ErrorMessage;
    /// <summary>HITL 门闩：RunAsync 在需确认的工具执行前返回 true，载荷见 <see cref="Gate"/>。</summary>
    public bool NeedsHuman;
    public AIHumanRequest Gate;
    /// <summary>token 用量统计（Provider 上报；未上报为 null，见 <see cref="AITokenUsage"/>）。</summary>
    public AITokenUsage Usage;
    public AIReply() {
        this.Text = "";
        this.ReasoningContent = "";
        this.ToolCalls = new List<AIToolCall>();
        this.IsError = false;
        this.ErrorKind = "";
        this.ErrorMessage = "";
        this.NeedsHuman = false;
        this.Gate = null;
        this.Usage = null;
    }
    public static AIReply FromText(string text) {
        AIReply r = new AIReply();
        r.Text = text != null ? text : "";
        return r;
    }
    public static AIReply Fail(string errorKind, string message) {
        AIReply r = new AIReply();
        r.IsError = true;
        r.ErrorKind = errorKind != null ? errorKind : "";
        r.ErrorMessage = message != null ? message : "";
        return r;
    }
}