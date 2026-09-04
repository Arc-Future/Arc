namespace Arc.Agent;
using Arc.Collections;
public class AIRequest {
    public List<AIMessage> Messages;
    /// <summary>OpenAI 兼容 tools 数组 JSON（空串 = 不发射 tools；由 Host 从 AIToolSet 注入）。</summary>
    public string ToolsJson;
    /// <summary>响应格式契约（null = 不约束；由 Host/Session 注入；Provider 按协议映射）。</summary>
    public AIResponseFormat ResponseFormat;
    public AIRequest() {
        this.Messages = new List<AIMessage>();
        this.ToolsJson = "";
        this.ResponseFormat = null;
    }
}