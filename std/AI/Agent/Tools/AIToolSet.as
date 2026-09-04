// RFC 038: AIToolSet — compile-time/register-time tool list + named dispatch (no reflection).
namespace Arc.Agent;
using Arc;
using Arc.Text;

/// <summary>
/// Tool registry. Handlers stored as AIToolHandler class refs on AIToolEntry nodes
/// (same storage discipline as AIChatClient on AIHost).
/// </summary>
public class AIToolSet {
    private AIToolEntry _head;
    public int Count { get; set; }
    // schema JSON 缓存：请求间字节级稳定 → tools 数组稳定 → LLM prompt cache 前缀可命中（仅注册变更时失效）。
    private string _schemasJson;
    private bool _schemasDirty;

    public AIToolSet() {
        _head = null;
        Count = 0;
        _schemasJson = "";
        _schemasDirty = true;
    }


    public void Add(AIToolHandler handler) {
        if (handler == null) {
            return;
        }
        // Build descriptor from Name/Capability (avoid virtual Descriptor property AV).
        AIToolDescriptor desc = new AIToolDescriptor(handler.Name, handler.Capability);
        this.Add(desc, handler);
    }

    public void Add(AIToolDescriptor descriptor, AIToolHandler handler) {
        if (handler == null || descriptor == null) {
            return;
        }
        string name = descriptor.Name;
        AIToolEntry prev = null;
        AIToolEntry cur = _head;
        while (cur != null) {
            if (cur.Descriptor != null && cur.Descriptor.Name == name) {
                cur.Descriptor = descriptor;
                cur.Handler = handler;
                return;
            }
            prev = cur;
            cur = cur.Next;
        }
        AIToolEntry entry = new AIToolEntry(descriptor, handler);
        if (prev == null) {
            _head = entry;
        } else {
            prev.Next = entry;
        }
        Count = Count + 1;
    }

    public AIToolDescriptor FindDescriptor(string name) {
        AIToolEntry e = this.FindEntry(name);
        if (e == null) {
            return null;
        }
        return e.Descriptor;
    }

    public AIToolHandler FindHandler(string name) {
        AIToolEntry e = this.FindEntry(name);
        if (e == null) {
            return null;
        }
        return e.Handler;
    }

    internal AIToolEntry FindEntry(string name) {
        AIToolEntry cur = _head;
        while (cur != null) {
            if (cur.Descriptor != null && cur.Descriptor.Name == name) {
                return cur;
            }
            cur = cur.Next;
        }
        return null;
    }

    /// <summary>遍历全部工具（描述符 + 处理器）；供聚合/审计/复制（禁反射）。</summary>
    public void ForEach(Action<AIToolDescriptor, AIToolHandler> visitor) {
        if (visitor == null) { return; }
        AIToolEntry cur = _head;
        while (cur != null) {
            if (cur.Descriptor != null && cur.Handler != null) {
                visitor(cur.Descriptor, cur.Handler);
            }
            cur = cur.Next;
        }
    }

    /// <summary>聚合工具 schema 为 OpenAI 兼容 tools 数组 JSON（Provider 发射用；无工具返回空串）。</summary>
    public string BuildSchemasJson() {
        if (Count == 0) {
            return "";
        }
        string json = "[";
        AIToolEntry cur = _head;
        bool first = true;
        while (cur != null) {
            if (cur.Descriptor != null) {
                if (!first) {
                    json = json + ",";
                }
                first = false;
                AIToolDescriptor d = cur.Descriptor;
                string paramsJson = d.ParametersSchema != null && d.ParametersSchema != ""
                    ? d.ParametersSchema
                    : "{\"type\":\"object\"}";
                json = json + "{\"type\":\"function\",\"function\":{"
                    + "\"name\":\"" + AIToolSet.JsonEsc(d.Name) + "\","
                    + "\"description\":\"" + AIToolSet.JsonEsc(d.Description) + "\","
                    + "\"parameters\":" + paramsJson
                    + "}}";
            }
            cur = cur.Next;
        }
        json = json + "]";
        _schemasJson = json;
        _schemasDirty = false;
        return json;
    }

    private static string JsonEsc(string s) {
        if (s == null) { return ""; }
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = s.Length;
        while (i < n) {
            char ch = s[i];
            if (ch == '"') { sb.Append("\\\""); }
            else if (ch == '\\') { sb.Append("\\\\"); }
            else if (ch == '\n') { sb.Append("\\n"); }
            else if (ch == '\r') { sb.Append("\\r"); }
            else if (ch == '\t') { sb.Append("\\t"); }
            else { sb.Append(ch); }
            i = i + 1;
        }
        return sb.ToString();
    }
}
