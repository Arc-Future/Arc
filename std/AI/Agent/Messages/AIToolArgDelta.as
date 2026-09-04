namespace Arc.Agent;
public class AIToolArgDelta {
    public string CallId;
    public string Name;
    public string Delta;
    public AIToolArgDelta() { this.CallId = ""; this.Name = ""; this.Delta = ""; }
    public AIToolArgDelta(string callId, string name, string delta) {
        this.CallId = callId != null ? callId : "";
        this.Name = name != null ? name : "";
        this.Delta = delta != null ? delta : "";
    }
}