// RFC 038 —— Wiki 进程内快照（落盘后置）。
namespace Arc.Agent;

using Arc.Collections;

/// <summary>AIWiki 页面表深拷贝；非向量索引。</summary>
public class AIWikiSnapshot {
    public List<AIWikiPage> Pages { get; }

    public AIWikiSnapshot(List<AIWikiPage> pages) {
        Pages = pages != null ? pages : new List<AIWikiPage>();
    }

    public int Count {
        get { return Pages.Count; }
    }

}
