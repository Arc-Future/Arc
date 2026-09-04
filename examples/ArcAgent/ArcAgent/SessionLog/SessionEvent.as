// SessionEvent —— 会话事件数据模型（append-only 事件日志的最小单元）。
//
// 对齐 dsh core/session 的「Model-visible means logged」：模型可见的事实
// （用户消息 / 助手回复 / 工具调用与结果 / 审批决策 / 用量）各记为一行事件，
// 按发生序追加写入 .jsonl。日志即审计面，恢复（/resume）从事件重放重建 transcript。
//
// 事件类型（t 字段）与承载字段：
//   meta       id/title/created —— 会话元数据（首行）
//   user       text             —— 用户消息
//   assistant  text             —— 助手回复
//   tool       name/args/err/result —— 工具调用 + 结果（err=true 为错误）
//   approval   tool/decision/reason —— 人机审批决策（approved/rejected）
//   decision   dkind/detail/reason  —— 决策轨迹（airfc:*/checkpoint:*/work_summary；经 Agent 会话事件面）
//   usage      prompt/completion/total —— 回合 token 用量
//   error      kind/message     —— 回合错误
namespace ArcAgent.SessionLog;
using Arc;
using Arc.Text.Json;

/// <summary>会话事件类型。</summary>
public enum SessionEventKind {
    /// <summary>会话元数据（首行）。</summary>
    Meta,
    /// <summary>用户消息。</summary>
    User,
    /// <summary>助手回复。</summary>
    Assistant,
    /// <summary>工具调用与结果。</summary>
    Tool,
    /// <summary>人机审批决策。</summary>
    Approval,
    /// <summary>决策轨迹事件（airfc:*/checkpoint:*/work_summary；RFC 043 M5–M6 单轨）。</summary>
    Decision,
    /// <summary>回合 token 用量。</summary>
    Usage,
    /// <summary>回合错误。</summary>
    Error,
}

/// <summary>会话事件：一行 JSON 的自描述记录（模型可见即记录）。</summary>
public class SessionEvent {
    public SessionEventKind Kind;
    public string Ts;
    // meta
    public string SessionId;
    public string Title;
    // user / assistant
    public string Text;
    // tool / approval
    public string ToolName;
    public string Args;
    public bool IsError;
    public string Result;
    public string Decision;
    public string Reason;
    // decision
    public string TrailKind;
    public string Detail;
    // usage
    public int PromptTokens;
    public int CompletionTokens;
    public int TotalTokens;
    // error
    public string ErrorKind;

    public SessionEvent() {
        this.Kind = SessionEventKind.Meta;
        this.Ts = "";
        this.SessionId = "";
        this.Title = "";
        this.Text = "";
        this.ToolName = "";
        this.Args = "";
        this.IsError = false;
        this.Result = "";
        this.Decision = "";
        this.Reason = "";
        this.TrailKind = "";
        this.Detail = "";
        this.PromptTokens = 0;
        this.CompletionTokens = 0;
        this.TotalTokens = 0;
        this.ErrorKind = "";
    }

    /// <summary>当前时间戳（毫秒，自 0001-01-01）。</summary>
    private static string NowMs() {
        DateTime now = DateTime.Now;
        long ms = now.Ticks / 10000;
        return "" + ms;
    }

    public static SessionEvent Meta(string id, string title) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Meta;
        e.Ts = SessionEvent.NowMs();
        e.SessionId = id != null ? id : "";
        e.Title = title != null ? title : "";
        return e;
    }

    public static SessionEvent User(string text) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.User;
        e.Ts = SessionEvent.NowMs();
        e.Text = text != null ? text : "";
        return e;
    }

    public static SessionEvent Assistant(string text) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Assistant;
        e.Ts = SessionEvent.NowMs();
        e.Text = text != null ? text : "";
        return e;
    }

    public static SessionEvent Tool(string name, string args, bool isError, string result) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Tool;
        e.Ts = SessionEvent.NowMs();
        e.ToolName = name != null ? name : "";
        e.Args = args != null ? args : "";
        e.IsError = isError;
        e.Result = result != null ? result : "";
        return e;
    }

    public static SessionEvent Approval(string tool, string decision, string reason) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Approval;
        e.Ts = SessionEvent.NowMs();
        e.ToolName = tool != null ? tool : "";
        e.Decision = decision != null ? decision : "";
        e.Reason = reason != null ? reason : "";
        return e;
    }

    /// <summary>决策轨迹事件（airfc:*/checkpoint:*/work_summary；经 Agent 会话事件面落盘）。</summary>
    public static SessionEvent Decision(string trailKind, string detail, string reason) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Decision;
        e.Ts = SessionEvent.NowMs();
        e.TrailKind = trailKind != null ? trailKind : "";
        e.Detail = detail != null ? detail : "";
        e.Reason = reason != null ? reason : "";
        return e;
    }

    public static SessionEvent Usage(int prompt, int completion, int total) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Usage;
        e.Ts = SessionEvent.NowMs();
        e.PromptTokens = prompt;
        e.CompletionTokens = completion;
        e.TotalTokens = total;
        return e;
    }

    public static SessionEvent Error(string kind, string message) {
        SessionEvent e = new SessionEvent();
        e.Kind = SessionEventKind.Error;
        e.Ts = SessionEvent.NowMs();
        e.ErrorKind = kind != null ? kind : "";
        e.Reason = message != null ? message : "";
        return e;
    }

    /// <summary>序列化为单行 JSON。</summary>
    public string ToJson() {
        JsonWriter w = new JsonWriter();
        w.WriteStartObject();
        w.WriteString("t", this.KindName());
        w.WriteString("ts", this.Ts);
        if (this.SessionId != "") { w.WriteString("id", this.SessionId); }
        if (this.Title != "") { w.WriteString("title", this.Title); }
        if (this.Text != "") { w.WriteString("text", this.Text); }
        if (this.ToolName != "") { w.WriteString("name", this.ToolName); }
        if (this.Args != "") { w.WriteString("args", this.Args); }
        if (this.Kind == SessionEventKind.Tool) { w.WriteBoolean("err", this.IsError); }
        if (this.Result != "") { w.WriteString("result", this.Result); }
        if (this.Decision != "") { w.WriteString("decision", this.Decision); }
        if (this.Reason != "") { w.WriteString("reason", this.Reason); }
        if (this.TrailKind != "") { w.WriteString("dkind", this.TrailKind); }
        if (this.Detail != "") { w.WriteString("detail", this.Detail); }
        if (this.Kind == SessionEventKind.Usage) {
            w.WriteNumber("prompt", this.PromptTokens);
            w.WriteNumber("completion", this.CompletionTokens);
            w.WriteNumber("total", this.TotalTokens);
        }
        if (this.ErrorKind != "") { w.WriteString("kind", this.ErrorKind); }
        w.WriteEndObject();
        return w.ToString();
    }

    /// <summary>从单行 JSON 解析；非法行返回 null。</summary>
    public static SessionEvent Parse(string line) {
        if (line == null || line == "") {
            return null;
        }
        SessionEvent e = new SessionEvent();
        JsonReader r = new JsonReader(line);
        bool ok = false;
        while (r.Read()) {
            if (r.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string name = r.GetString();
            r.Read();
            if (name == "t") {
                e.Kind = SessionEvent.KindOf(r.GetString());
                ok = true;
            } else if (name == "ts") {
                e.Ts = r.GetString();
            } else if (name == "id") {
                e.SessionId = r.GetString();
            } else if (name == "title") {
                e.Title = r.GetString();
            } else if (name == "text") {
                e.Text = r.GetString();
            } else if (name == "name") {
                e.ToolName = r.GetString();
            } else if (name == "args") {
                e.Args = r.GetString();
            } else if (name == "err") {
                e.IsError = r.GetBoolean();
            } else if (name == "result") {
                e.Result = r.GetString();
            } else if (name == "decision") {
                e.Decision = r.GetString();
            } else if (name == "reason") {
                e.Reason = r.GetString();
            } else if (name == "dkind") {
                e.TrailKind = r.GetString();
            } else if (name == "detail") {
                e.Detail = r.GetString();
            } else if (name == "prompt") {
                e.PromptTokens = r.GetInt32();
            } else if (name == "completion") {
                e.CompletionTokens = r.GetInt32();
            } else if (name == "total") {
                e.TotalTokens = r.GetInt32();
            } else if (name == "kind") {
                e.ErrorKind = r.GetString();
            }
        }
        if (!ok) {
            return null;
        }
        return e;
    }

    private string KindName() {
        switch (this.Kind) {
            case SessionEventKind.Meta: {
                return "meta";
            }
            case SessionEventKind.User: {
                return "user";
            }
            case SessionEventKind.Assistant: {
                return "assistant";
            }
            case SessionEventKind.Tool: {
                return "tool";
            }
            case SessionEventKind.Approval: {
                return "approval";
            }
            case SessionEventKind.Decision: {
                return "decision";
            }
            case SessionEventKind.Usage: {
                return "usage";
            }
            case SessionEventKind.Error: {
                return "error";
            }
            default: {
                return "meta";
            }
        }
    }

    private static SessionEventKind KindOf(string name) {
        if (name == "user") { return SessionEventKind.User; }
        if (name == "assistant") { return SessionEventKind.Assistant; }
        if (name == "tool") { return SessionEventKind.Tool; }
        if (name == "approval") { return SessionEventKind.Approval; }
        if (name == "decision") { return SessionEventKind.Decision; }
        if (name == "usage") { return SessionEventKind.Usage; }
        if (name == "error") { return SessionEventKind.Error; }
        return SessionEventKind.Meta;
    }
}
