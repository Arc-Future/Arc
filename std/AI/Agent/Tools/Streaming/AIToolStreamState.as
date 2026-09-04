// RFC 038：当前工具流的流式状态（内部会话机制；非开发者契约面）。
namespace Arc.Agent;
internal class AIToolStreamState {
    public string CallId;
    public string Name;
    public AIToolStreamDisposition Disposition;
    public AIToolStreamState() {
        this.CallId = ""; this.Name = ""; this.Disposition = AIToolStreamDisposition.Buffer;
    }
    public AIToolStreamState(string callId, string name) {
        this.CallId = callId != null ? callId : "";
        this.Name = name != null ? name : "";
        this.Disposition = AIToolStreamDisposition.Buffer;
    }
}