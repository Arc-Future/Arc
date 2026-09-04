// SessionEventLog —— 会话事件日志（append-only JSONL）+ 重放恢复。
//
// 对齐 dsh core/session 的设计：会话史是 append-only 事件日志（单一事实源），
// LLM 消息历史从事件重放派生（Model-visible means logged），不另存副本。
// 职责边界（分层：SessionLog 层）：
//   - StartAsync：新建会话 → 写 meta 首行（id/created）。
//   - AppendAsync：任一模型可见事实（user/assistant/tool/approval/usage/error）追加一行。
//   - ListAsync：扫描 *.jsonl 生成会话清单（id/标题/事件数/起止时间）；标题取自首条用户消息。
//   - ReplayAsync / BuildSnapshotAsync：按会话重放事件重建 transcript（/resume 恢复来源）。
namespace ArcAgent.SessionLog;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.IO;

/// <summary>会话清单条目（/sessions 展示面）。</summary>
public class SessionInfo {
    public string Id;
    public string Title;
    public int EventCount;
    public string Created;
    public string Updated;

    public SessionInfo() {
        this.Id = "";
        this.Title = "";
        this.EventCount = 0;
        this.Created = "";
        this.Updated = "";
    }
}

/// <summary>会话事件日志管理器（append-only JSONL + 重放恢复）。</summary>
public class SessionEventLog {
    private string _dir;
    private string _activeId;
    private string _activePath;
    private string _activeTitle;

    public SessionEventLog(string sessionDir) {
        _dir = sessionDir != null ? sessionDir : "";
        _activeId = "";
        _activePath = "";
        _activeTitle = "";
    }

    /// <summary>会话目录（.arcagent/sessions/）。</summary>
    public string Directory {
        get { return _dir; }
    }

    /// <summary>当前活动会话 id（无活动会话为空串）。</summary>
    public string ActiveId {
        get { return _activeId; }
    }

    /// <summary>当前活动会话标题（无活动会话为空串）。</summary>
    public string ActiveTitle {
        get { return _activeTitle; }
    }

    public bool HasActive {
        get { return _activeId != ""; }
    }

    /// <summary>新建会话：建目录、生成 id、写 meta 首行。返回是否成功。</summary>
    public async Task<bool> StartAsync(string title, CancellationToken cancellationToken) {
        if (_dir == "") {
            return false;
        }
        if (!Directory.Exists(_dir)) {
            bool ok = await Directory.CreateDirectoryAsync(_dir);
            if (!ok) {
                return false;
            }
        }
        _activeId = this.GenerateId();
        _activeTitle = title != null ? title : "";
        _activePath = this.FilePathFor(_activeId);
        return await this.AppendAsync(SessionEvent.Meta(_activeId, _activeTitle), cancellationToken);
    }

    /// <summary>追加一行事件到活动会话日志（append-only）。</summary>
    public async Task<bool> AppendAsync(SessionEvent evt, CancellationToken cancellationToken) {
        if (_activePath == "" || evt == null) {
            return false;
        }
        string line = evt.ToJson() + "\n";
        return await File.AppendAllTextAsync(_activePath, line);
    }

    /// <summary>恢复既有会话为活动会话（/resume）：后续 AppendAsync 追加到该会话文件。</summary>
    public async Task<bool> ResumeAsync(string id, CancellationToken cancellationToken) {
        if (_dir == "" || id == null || id == "") {
            return false;
        }
        string path = this.FilePathFor(id);
        if (!File.Exists(path)) {
            return false;
        }
        _activeId = id;
        _activePath = path;
        _activeTitle = "";
        return true;
    }

    /// <summary>扫描会话目录生成清单（标题取首条用户消息前 40 字符，单行化；按创建时间倒序）。</summary>
    public async Task<List<SessionInfo>> ListAsync(CancellationToken cancellationToken) {
        List<SessionInfo> result = new List<SessionInfo>();
        if (_dir == "" || !Directory.Exists(_dir)) {
            return result;
        }
        string[] files = Directory.GetFiles(_dir, "*.jsonl");
        int n = files.Length;
        int i = 0;
        while (i < n) {
            string text = await File.ReadAllTextAsync(files[i]);
            SessionInfo info = this.Scan(text);
            if (info != null) {
                result.Add(info);
            }
            i = i + 1;
        }
        this.SortByNewest(result);
        return result;
    }

    /// <summary>按创建时间倒序（ms 时间戳转 long 比较；空/非法视为最早）。</summary>
    private void SortByNewest(List<SessionInfo> list) {
        int n = list.Count;
        int i = 0;
        while (i < n) {
            int best = i;
            long bestMs = this.ToMs(list[i].Created);
            int j = i + 1;
            while (j < n) {
                long ms = this.ToMs(list[j].Created);
                if (ms > bestMs) {
                    best = j;
                    bestMs = ms;
                }
                j = j + 1;
            }
            if (best != i) {
                SessionInfo tmp = list[i];
                list[i] = list[best];
                list[best] = tmp;
            }
            i = i + 1;
        }
    }

    /// <summary>毫秒时间戳字符串转 long（非法返回 0）。</summary>
    private static long ToMs(string ts) {
        if (ts == null || ts == "") {
            return 0;
        }
        return Convert.ToInt64(ts);
    }

    /// <summary>按会话 id 重放事件，重建模型可见消息历史（/resume 的 transcript 来源）。</summary>
    public async Task<List<AIMessage>> ReplayAsync(string id, CancellationToken cancellationToken) {
        List<AIMessage> msgs = new List<AIMessage>();
        string path = this.FilePathFor(id);
        if (!File.Exists(path)) {
            return msgs;
        }
        string text = await File.ReadAllTextAsync(path);
        if (text == null) {
            return msgs;
        }
        string[] lines = text.Split("\n");
        int n = lines.Length;
        int i = 0;
        int callSeq = 0;
        while (i < n) {
            SessionEvent evt = SessionEvent.Parse(lines[i]);
            if (evt != null) {
                if (evt.Kind == SessionEventKind.User && evt.Text != "") {
                    msgs.Add(new AIMessage(AIRole.User, evt.Text));
                } else if (evt.Kind == SessionEventKind.Assistant && evt.Text != "") {
                    msgs.Add(new AIMessage(AIRole.Assistant, evt.Text));
                } else if (evt.Kind == SessionEventKind.Tool) {
                    callSeq = callSeq + 1;
                    string callId = "call-" + callSeq;
                    List<AIToolCall> calls = new List<AIToolCall>();
                    calls.Add(new AIToolCall(callId, evt.ToolName, evt.Args));
                    msgs.Add(new AIMessage(AIRole.Assistant, "", "", calls));
                    string resultText = evt.Result;
                    if (evt.IsError) {
                        resultText = "[tool error] " + resultText;
                    }
                    msgs.Add(new AIMessage(AIRole.Tool, resultText, callId, null));
                }
            }
            i = i + 1;
        }
        return msgs;
    }

    /// <summary>按会话 id 构造可恢复快照（transcript 重放 + Idle 回合态；/resume 直接 Restore）。</summary>
    public async Task<AISessionSnapshot> BuildSnapshotAsync(string id, CancellationToken cancellationToken) {
        List<AIMessage> msgs = await this.ReplayAsync(id, cancellationToken);
        AISessionSnapshot snap = new AISessionSnapshot();
        snap.SessionId = id;
        snap.Turn = AITurnState.Idle;
        snap.Transcript = msgs;
        return snap;
    }

    /// <summary>会话事件文件路径（<paramref name="id"/>.jsonl）。</summary>
    public string FilePathFor(string id) {
        if (_dir == "" || id == null || id == "") {
            return "";
        }
        return _dir + "/" + id + ".jsonl";
    }

    /// <summary>扫描单文件文本：提取清单条目（id/created 取 meta，标题取首条用户消息，updated 取末条 ts）。</summary>
    private SessionInfo Scan(string text) {
        if (text == null || text == "") {
            return null;
        }
        SessionInfo info = new SessionInfo();
        string firstUser = "";
        string[] lines = text.Split("\n");
        int n = lines.Length;
        int i = 0;
        while (i < n) {
            SessionEvent evt = SessionEvent.Parse(lines[i]);
            if (evt != null) {
                info.EventCount = info.EventCount + 1;
                if (evt.Kind == SessionEventKind.Meta) {
                    if (info.Id == "") { info.Id = evt.SessionId; }
                    if (info.Created == "") { info.Created = evt.Ts; }
                } else if (evt.Kind == SessionEventKind.User) {
                    if (firstUser == "") { firstUser = evt.Text; }
                }
                if (evt.Ts != "") { info.Updated = evt.Ts; }
            }
            i = i + 1;
        }
        if (info.Id == "") {
            return null;
        }
        info.Title = this.MakeTitle(firstUser);
        return info;
    }

    /// <summary>标题生成：首条用户消息单行化截断（≤40 字符；空 = "(no messages)"）。</summary>
    private string MakeTitle(string firstUser) {
        if (firstUser == null || firstUser == "") {
            return "(no messages)";
        }
        string one = firstUser.Replace("\n", " ").Replace("\r", " ");
        string t = one.Trim();
        if (t.Length > 40) {
            t = t.Substring(0, 40) + "...";
        }
        return t;
    }

    private string GenerateId() {
        DateTime now = DateTime.Now;
        long ms = now.Ticks / 10000;
        return "s" + ms;
    }
}
