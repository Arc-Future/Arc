// RFC 038：AIWikiLinter —— 编译期 Lint 体检报告。
//
// AIWiki.Lint() 据此返回知识图体检结果，可证伪四类门禁：
//   - broken_links：互链指向不存在的页面（G8）
//   - dup_aliases：重复页面别名 / PageId / SourceId / 断言 Id（G9）
//   - orphan_claims：未被任何页面引用的断言（G10）
//   - weak_citations：低可信或未核验的断言（L6 编译期体检辅助）
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 编译期 Lint 报告：四类体检发现 + 是否含问题。空报告 = 知识图体检健康。
/// </summary>
public class AIWikiLinter {
    /// <summary>失效互链（目标页面不存在）。</summary>
    public List<string> BrokenLinks;
    /// <summary>重复别名（页面 Path / PageId / SourceId / 断言 Id）。</summary>
    public List<string> DupAliases;
    /// <summary>孤儿断言（未被任何页面引用）。</summary>
    public List<string> OrphanClaims;
    /// <summary>弱引用断言（低可信 / 未核验）。</summary>
    public List<string> WeakCitations;

    public AIWikiLinter() {
        this.BrokenLinks = new List<string>();
        this.DupAliases = new List<string>();
        this.OrphanClaims = new List<string>();
        this.WeakCitations = new List<string>();
    }

    /// <summary>是否含任一问题（健康 = false）。</summary>
    public bool HasIssues {
        get {
            return this.BrokenLinks.Count > 0
                || this.DupAliases.Count > 0
                || this.OrphanClaims.Count > 0
                || this.WeakCitations.Count > 0;
        }
    }

    public int BrokenLinkCount {
        get { return this.BrokenLinks.Count; }
    }
    public int DupAliasCount {
        get { return this.DupAliases.Count; }
    }
    public int OrphanClaimCount {
        get { return this.OrphanClaims.Count; }
    }
    public int WeakCitationCount {
        get { return this.WeakCitations.Count; }
    }
}
