// AgentContext —— 上下文工程层：Wiki 结构化记忆持久化 + 知识面注入 + 上下文源工厂 + 会话日志。
//
// 职责边界（分层：Context 层）：
//   - 记忆：AIWiki 落盘持久化（SaveAsync/LoadAsync），跨会话保留项目知识与约定。
//   - 知识面：KnowledgePaths → WikiPathsToAttach 注入系统上下文（消费桥，RFC 038）。
//   - 约定源：NewConventionsProvider 工厂——项目约定上下文源（.arcagent/conventions.md）。
//   - 会话日志：NewSessionLog 工厂——append-only 事件日志（.arcagent/sessions/），
//     /sessions 清单与 /resume 恢复的单一事实源（对齐 dsh session log）。
namespace ArcAgent.Context;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.IO;
using ArcAgent.SessionLog;

/// <summary>上下文工程：持久化记忆 + 知识面注入 + 会话事件日志（组合根之外的上下文能力面）。</summary>
public class AgentContext {
    private string _memoryFile;
    private string _sessionDir;

    public AgentContext(string memoryFile) {
        _memoryFile = memoryFile != null ? memoryFile : "";
        _sessionDir = "";
    }

    /// <summary>记忆落盘文件路径（JSON 知识图）。</summary>
    public string MemoryFile {
        get { return _memoryFile; }
    }

    /// <summary>加载记忆知识图（文件不存在 → 空图）。</summary>
    public async Task<AIWiki> LoadWikiAsync(CancellationToken cancellationToken) {
        return await AIWiki.LoadAsync(_memoryFile, cancellationToken);
    }

    /// <summary>保存记忆知识图（自动建目录；成功返回 true）。</summary>
    public async Task<bool> SaveWikiAsync(AIWiki wiki, CancellationToken cancellationToken) {
        if (wiki == null || _memoryFile == "") {
            return false;
        }
        string dir = Path.GetDirectoryName(_memoryFile);
        if (dir != "" && !Directory.Exists(dir)) {
            bool created = await Directory.CreateDirectoryAsync(dir);
            if (!created) {
                return false;
            }
        }
        return await wiki.SaveAsync(_memoryFile, cancellationToken);
    }

    /// <summary>已登记知识页路径（注入 WikiPathsToAttach 的知识库面）。</summary>
    public List<string> KnowledgePaths(AIWiki wiki) {
        if (wiki == null) {
            return new List<string>();
        }
        return wiki.List("");
    }

    /// <summary>项目约定上下文源工厂（读 .arcagent/conventions.md → Rules 层）。</summary>
    public static AIContextProvider NewConventionsProvider(string workspaceRoot) {
        return new ProjectConventionsProvider(workspaceRoot);
    }

    /// <summary>会话日志管理器工厂（.arcagent/sessions/，append-only JSONL；无活动会话）。</summary>
    public SessionEventLog NewSessionLog() {
        if (_sessionDir == "") {
            string dir = Path.GetDirectoryName(_memoryFile);
            _sessionDir = (dir != "" ? dir : ".") + "/sessions";
        }
        return new SessionEventLog(_sessionDir);
    }
}
