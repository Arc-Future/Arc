// RFC 038 上下文成体系：AIContextHost — 宿主环境（MAF AddInEnvironment 的 Arc 对应）。
//
// MAF 激活外接程序时宿主向其注入运行环境句柄（AddInEnvironment），外接程序经
// IServiceProviderContract 检索宿主服务。本类型承袭该精髓：组合根（AIContextEngine）
// 在激活每个 provider 时注入宿主环境，provider 经 Host 访问宿主共享服务（Wiki 记忆区
// 与 token 预算）。封闭契约：只暴露宿主已知服务，不泄漏引擎内部。
namespace Arc.Agent;

/// <summary>
/// 宿主环境句柄（MAF AddInEnvironment / IServiceProviderContract 的 Arc 对应）。
/// 组合根激活 <see cref="AIContextProvider"/> 时注入；提供方经 Host 访问宿主共享
/// 服务——结构化记忆 <see cref="Wiki"/> 与上下文 token 预算（<see cref="TryReserve"/>）。
/// </summary>
public class AIContextHost {
    private AIWiki _wiki;
    private int _maxTokens;
    private int _usedTokens;

    public AIContextHost(AIWiki wiki, int maxTokens) {
        _wiki = wiki;
        _maxTokens = maxTokens > 0 ? maxTokens : 0;
        _usedTokens = 0;
    }

    /// <summary>宿主共享结构化记忆（只读消费；空 = 宿主未提供）。</summary>
    public AIWiki Wiki {
        get { return _wiki; }
    }

    /// <summary>上下文 token 预算上限（0 = 不设限）。</summary>
    public int MaxTokens {
        get { return _maxTokens; }
    }

    /// <summary>本回合已预留 token（预算水位；审计）。</summary>
    public int UsedTokens {
        get { return _usedTokens; }
    }

    /// <summary>预留 token 预算（超上限返回 false；max=0 时永不超限）。</summary>
    public bool TryReserve(int tokens) {
        if (tokens < 0) {
            return false;
        }
        if (_maxTokens > 0 && _usedTokens + tokens > _maxTokens) {
            return false;
        }
        _usedTokens = _usedTokens + tokens;
        return true;
    }

    /// <summary>归零预算水位（每次 BuildAsync 前由组合根调用）。</summary>
    public void ResetBudget() {
        _usedTokens = 0;
    }
}