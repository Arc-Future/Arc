namespace Arc.Agent;
public class AIToolCallStart {
    public string CallId;
    public string ToolName;
    public AIToolCallStart() { this.CallId = ""; this.ToolName = ""; }
    public AIToolCallStart(string callId, string toolName) {
        this.CallId = callId != null ? callId : "";
        this.ToolName = toolName != null ? toolName : "";
    }
}