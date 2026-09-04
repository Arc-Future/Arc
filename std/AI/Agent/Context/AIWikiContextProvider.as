// RFC 038 上下文成体系：AIWikiContextProvider — Wiki 知识页的内置上下文源。
//
// 将 Wiki 消费桥封装为 AIContextProvider：按配置路径把对应 AIWikiPage 产成为
// 「Knowledge 层」上下文块（路径序稳定；空页跳过）。RFC 038：由 Host 侧经 AddProvider
// 作为普通 provider 注册（wikiPaths 非空才注册）。静态源——忽略 query/session，返回已完成
// Task；不实现调用后方向。开发者可完全替换为自定义记忆源。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 内置上下文源：Wiki 知识页（消费桥）。按配置路径把对应 <see cref="AIWikiPage"/>
/// 产成为 Knowledge 层上下文块（路径序稳定；空 body 不产出）。由 <see cref="AIContextEngine"/>
/// （Host 级组合根）统一排序合并。RFC 038 收编为普通 provider：由宿主（AIHost）注册，
/// 开发者可整体替换 / 移除 / 自定义扩展。
/// </summary>
public class AIWikiContextProvider : AIContextProvider {
    private List<string> _wikiPaths;
    private AIWiki _wiki;

    public AIWikiContextProvider(List<string> wikiPaths, AIWiki wiki) {
        _wikiPaths = wikiPaths != null ? wikiPaths : new List<string>();
        _wiki = wiki;
    }

    public override string GetName() { return "wiki"; }

    public override string GetDescription() {
        return "Wiki knowledge pages (Knowledge layer).";
    }

    /// <summary>静态源：忽略 query/session，返回已完成 Task。</summary>
    public override Task<List<AIContextBlock>> ProvideContextAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> list = new List<AIContextBlock>();
        if (_wiki == null) {
            return Task.FromResult(list);
        }
        int i = 0;
        int n = _wikiPaths.Count;
        while (i < n) {
            string p = _wikiPaths[i];
            AIWikiPage page = _wiki.Get(p);
            if (page != null && page.Body != null && page.Body != "") {
                AIContextBlock blk = new AIContextBlock("wiki", "Knowledge", 0, page.Body);
                blk.Title = "Wiki: " + p;
                blk.TokenEstimate = page.Body.Length / 4;
                list.Add(blk);
            }
            i = i + 1;
        }
        return Task.FromResult(list);
    }
}