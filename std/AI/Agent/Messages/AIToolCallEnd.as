namespace Arc.Agent;
public class AIToolCallEnd {
    public string CallId;
    public AIToolCallEnd() { this.CallId = ""; }
    public AIToolCallEnd(string callId) { this.CallId = callId != null ? callId : ""; }
}