namespace Arc.Agent;
public class AIToolCall {
    public string CallId;
    public string Name;
    public string ArgumentsJson;
    public AIToolCall() { this.CallId = ""; this.Name = ""; this.ArgumentsJson = ""; }
    public AIToolCall(string callId, string name, string argumentsJson) {
        this.CallId = callId != null ? callId : "";
        this.Name = name != null ? name : "";
        this.ArgumentsJson = argumentsJson != null ? argumentsJson : "";
    }
}