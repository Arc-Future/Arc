namespace Arc.Agent;

using Arc.Collections;

// RFC 038: AIToolArgsReader 的命名条目（内部存储；非契约公开面）。
internal class AIToolArgEntry {
    public string Name;
    public AIToolArgKind Kind;
    /// <summary>String/Number 原文或 Bool 的 "true"/"false"；StringArray/Object/JsonArray/Null 恒为空串。</summary>
    public string Text;
    public List<string> Array;
    /// <summary>Object/JsonArray 的原始 JSON 文本（GetObjectJson / GetChild 取用；其余恒为空串）。</summary>
    public string RawJson;

    public AIToolArgEntry(string name, AIToolArgKind kind, string text) {
        this.Name = name != null ? name : "";
        this.Kind = kind;
        this.Text = text != null ? text : "";
        this.Array = null;
        this.RawJson = "";
    }
}
