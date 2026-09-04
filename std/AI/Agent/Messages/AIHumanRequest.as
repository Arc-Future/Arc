namespace Arc.Agent;

/// <summary>
/// HITL 门闩载荷（PendingHuman）。工具草稿含 Arguments 供 Approve 编辑。
/// </summary>
public class AIHumanRequest {
    public string Reason;
    public string Prompt;
    public string ToolCallId;
    public string ToolName;
    /// <summary>工具参数草稿（JSON/原文）；Approve 可编辑。</summary>
    public string ToolArguments;

    public AIHumanRequest() {
        this.Reason = "";
        this.Prompt = "";
        this.ToolCallId = "";
        this.ToolName = "";
        this.ToolArguments = "";
    }

    public AIHumanRequest(string reason, string prompt) {
        this.Reason = reason != null ? reason : "";
        this.Prompt = prompt != null ? prompt : "";
        this.ToolCallId = "";
        this.ToolName = "";
        this.ToolArguments = "";
    }
}
