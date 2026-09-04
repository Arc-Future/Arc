// RFC 038 上下文工程管理基座：AIContextEngine — Host 级上下文源组合根（Loader）。
//
// 对齐 Karpathy「context engineering」（上下文即一等公民）与微软 MAF 的组合根设计：
// 把构成模型上下文的各种输入源（系统指令 / Skill 激活提示 / Wiki 知识页 / 开发者自定义源）
// 统一为「AIContextProvider 可插拔上下文源」，由本引擎（组合根 / Loader）注册、去重、
// 生命周期编排、预算裁剪、按层次化布局稳定排序、扁平化为请求的 system 上下文面，并对
// 工具 schema 聚合（主 AIToolSet + 激活 Skill 工具）。单一组装点 → 前缀稳定 → LLM 上下文
// 缓存（KV cache）命中。
//
// RFC 038：引擎升级为 **Host 级组合根**——provider 实例跨会话共享（Host 注册一次，
// 各会话只传自身 AIContextSession），不再每会话 new 一套 engine/providers；删除对
// Instructions/Skill/Wiki 的硬编码特判，内置源（AIInstructionContextProvider /
// AISkillContextProvider / AIWikiContextProvider）一律为普通 provider，由 Host 侧经
// AddProvider 注册，开发者可整体替换 / 移除 / 注入自定义源，无需改动引擎。
//
// 诚实边界：本引擎只做「请求前上下文面组装 + 调用后消息分发」，不做 App 侧 Multi-Agent
// 编排 / RAG 检索编排（RFC 004/028 非目标边界保持）。源顺序稳定、字节稳定是缓存友好前提。
namespace Arc.Agent;
using Arc.Collections;
using Arc;

/// <summary>
/// 上下文工程 Host 级组合根（Loader）。持有按名去重的有序 <see cref="AIContextProvider"/>
/// 集合，负责生命周期编排（激活全部源）、预算裁剪、收集块 → 按 (Kind 固定序 → Priority
/// 稳定序) 排序 → 扁平化为 system 消息、聚合工具集，并把调用后消息分发给全部源——一次
/// BuildAsync 产出 <see cref="AIContextSource"/> 供会话消费。provider 实例跨会话共享；
/// 会话态经 <see cref="AIContextSession"/> 传递，本引擎不持有任何会话态。
/// </summary>
public class AIContextEngine : IDisposable {
    private List<AIContextProvider> _providers;
    private AIContextHost _host;
    private AIToolSet _mainTools;
    private AISkillSet _skills;
    private bool _activated;
    private bool _disposed;

    /// <summary>
    /// 构造 Host 级组合根。初始化宿主环境（<paramref name="wiki"/> / token 预算）与
    /// 工具聚合源（<paramref name="mainTools"/> + <paramref name="skills"/> 全部工具）。
    /// 不再注册任何内置 provider——Instructions/Skill/Wiki 内置源由 Host 侧经
    /// <see cref="AddProvider"/> 作为普通 provider 注册（RFC 038 去内置源特判）。
    /// </summary>
    public AIContextEngine(AIToolSet mainTools, AISkillSet skills, AIWiki wiki, int maxContextTokens) {
        _providers = new List<AIContextProvider>();
        _host = new AIContextHost(wiki, maxContextTokens);
        _skills = skills != null ? skills : new AISkillSet();
        _mainTools = mainTools;
        _activated = false;
        _disposed = false;
    }

    /// <summary>注册上下文源（按 Name 去重：同名覆盖，保持原位置；null 忽略）。开发者扩展点。</summary>
    public void AddProvider(AIContextProvider provider) {
        if (provider == null) {
            return;
        }
        string name = provider.GetName();
        int n = _providers.Count;
        int i = 0;
        while (i < n) {
            AIContextProvider p = _providers[i];
            if (p != null && p.GetName() == name) {
                if (_activated) {
                    // 已激活：新实例接续生命周期（注入宿主环境）。
                    provider.Initialize(_host);
                }
                _providers[i] = provider;
                return;
            }
            i = i + 1;
        }
        if (_activated) {
            provider.Initialize(_host);
        }
        _providers.Add(provider);
    }

    /// <summary>上下文源数量（审计）。</summary>
    public int ProviderCount {
        get { return _providers.Count; }
    }

    /// <summary>宿主环境（供外部注入 / 审计）。</summary>
    public AIContextHost Host {
        get { return _host; }
    }

    /// <summary>
    /// 组装完整上下文（调用前方向）：激活全部源 → 逐源异步构建（容错：单源异常跳过）→
    /// 按 (Kind 固定序 → Priority 稳定序) 排序 → 预算裁剪 → 扁平化为 system 消息；并聚合
    /// 工具集。顺序确定 → 字符串稳定 → 前缀可命中 KV cache。空源跳过。
    /// <paramref name="session"/> 为本会话态载体（provider 跨会话共享实例下经其读写自身会话态）。
    /// 注：Instructions 由 Host 侧注册的 <see cref="AIInstructionContextProvider"/> 组装为
    /// Rules 层最前块（单一组装点；此前由 AISession.EnsureInstructions 旁路注入的旧双轨已消除）。
    /// </summary>
    public async Task<AIContextSource> BuildAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        this.EnsureActivated();
        _host.ResetBudget();
        // 1) 逐源异步构建（provider 注册序天然稳定；容错：单源异常跳过，不打断组装）。
        List<AIContextBlock> blocks = new List<AIContextBlock>();
        int n = _providers.Count;
        int i = 0;
        while (i < n) {
            AIContextProvider p = _providers[i];
            if (p != null) {
                try {
                    Task<List<AIContextBlock>> t = p.ProvideContextAsync(query, session, cancellationToken);
                    List<AIContextBlock> bs = await t;
                    if (bs != null) {
                        int b = 0;
                        int bn = bs.Count;
                        while (b < bn) {
                            blocks.Add(bs[b]);
                            b = b + 1;
                        }
                    }
                } catch {
                    // 单源失败 → 跳过该源（容错；审计经 result.DroppedBlocks 观察总量）。
                }
            }
            i = i + 1;
        }
        // 2) 稳定排序（层次化布局：Kind 固定序 → 同层 Priority 升序）。
        this.StableSortByLayout(blocks);
        // 3) 预算裁剪（max>0 时按重要性从尾部丢弃超限块）。
        int dropped = this.ApplyBudget(blocks);
        // 4) 扁平化为 system 消息。
        AIContextSource source = new AIContextSource();
        source.DroppedBlocks = dropped;
        int m = 0;
        int mn = blocks.Count;
        while (m < mn) {
            AIContextBlock blk = blocks[m];
            if (blk != null && blk.Enabled) {
                AIMessage msg = blk.ToMessage();
                if (msg != null) {
                    source.Messages.Add(msg);
                }
            }
            m = m + 1;
        }
        source.Tools = this.AggregateTools();
        return source;
    }

    /// <summary>
    /// 调用后方向：把模型往返后追加的消息分发给全部源（<see cref="AIContextProvider.ProcessMessageAsync"/>），
    /// 供记忆 / Wiki / Skill 落库类源抽取 / 持久化。单源异常容错跳过（与 BuildAsync 容错一致），
    /// 不打断会话回合。静态源（默认空实现）无副作用。
    /// </summary>
    public async Task ProcessMessageAsync(AIMessage message, AIContextSession session, CancellationToken cancellationToken) {
        int n = _providers.Count;
        int i = 0;
        while (i < n) {
            AIContextProvider p = _providers[i];
            if (p != null) {
                try {
                    Task t = p.ProcessMessageAsync(message, session, cancellationToken);
                    await t;
                } catch {
                    // 单源失败 → 跳过该源（容错；调用后处理失败不打断回合主流程）。
                }
            }
            i = i + 1;
        }
    }

    /// <summary>
    /// 聚合工具集：主 AIToolSet + 激活 Skill 的工具 schema。返回新的 AIToolSet（不原地
    /// 改写调用方）。无任何工具时返回空 AIToolSet。
    /// </summary>
    public AIToolSet AggregateTools() {
        AIToolSet merged = new AIToolSet();
        AIToolSet main = _mainTools;
        if (main != null) {
            this.MergeToolSet(merged, main);
        }
        AISkillSet skills = _skills;
        if (skills != null) {
            List<string> names = skills.Names();
            int n = names.Count;
            int i = 0;
            while (i < n) {
                AISkill s = skills.Find(names[i]);
                AIToolSet st = s != null ? s.Tools : null;
                if (st != null) {
                    this.MergeToolSet(merged, st);
                }
                i = i + 1;
            }
        }
        return merged;
    }

    /// <summary>释放：停用全部源（生命周期闭环）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
        int n = _providers.Count;
        int i = 0;
        while (i < n) {
            AIContextProvider p = _providers[i];
            if (p != null) {
                p.Dispose();
            }
            i = i + 1;
        }
        _providers.Clear();
    }

    // ── 私有：实现 ──

    /// <summary>首次构建前激活全部源（生命周期编排；幂等）。</summary>
    private void EnsureActivated() {
        if (_activated) {
            return;
        }
        _activated = true;
        int n = _providers.Count;
        int i = 0;
        while (i < n) {
            AIContextProvider p = _providers[i];
            if (p != null) {
                p.Initialize(_host);
            }
            i = i + 1;
        }
    }

    /// <summary>把 src 的全部工具并入 dst（按名覆盖语义，同 AIToolSet.Add）。</summary>
    private void MergeToolSet(AIToolSet dst, AIToolSet src) {
        src.ForEach((d: AIToolDescriptor, h: AIToolHandler) => {
            dst.Add(d, h);
        });
    }

    /// <summary>稳定插入排序：按 (Kind 固定序 → Priority 升序)；同键保持原顺序（前缀稳定）。</summary>
    private void StableSortByLayout(List<AIContextBlock> blocks) {
        int n = blocks.Count;
        int i = 1;
        while (i < n) {
            AIContextBlock key = blocks[i];
            int j = i - 1;
            while (j >= 0 && AIContextEngine.Less(blocks[j], key)) {
                int next = j + 1; // 避免二进制表达式作索引操作数（MIR 下探缺口）
                blocks[next] = blocks[j];
                j = j - 1;
            }
            int slot = j + 1;
            blocks[slot] = key;
            i = i + 1;
        }
    }

    /// <summary>a 应排在 b 之后（a 的布局键更大）。同键 false → 稳定保序。</summary>
    private static bool Less(AIContextBlock a, AIContextBlock b) {
        int ka = AIContextEngine.KindRank(a);
        int kb = AIContextEngine.KindRank(b);
        if (ka != kb) {
            return ka > kb;
        }
        int pa = a != null ? a.Priority : 0;
        int pb = b != null ? b.Priority : 0;
        return pa > pb;
    }

    /// <summary>层次化布局固定序（结构传达重要性）：Rules → Task → UserProfile → Knowledge → ToolOutputs；未知 Kind 按引入序靠前。</summary>
    private static int KindRank(AIContextBlock b) {
        string kind = b != null && b.Kind != null ? b.Kind : "";
        if (kind == "Rules") {
            return 0;
        }
        if (kind == "Task") {
            return 1;
        }
        if (kind == "UserProfile") {
            return 2;
        }
        if (kind == "Knowledge") {
            return 3;
        }
        if (kind == "ToolOutputs") {
            return 4;
        }
        return 0;
    }

    /// <summary>预算裁剪：max&gt;0 时按重要性（已升序）从尾部丢弃超限块；返回丢弃数。</summary>
    private int ApplyBudget(List<AIContextBlock> blocks) {
        if (_host.MaxTokens <= 0) {
            return 0;
        }
        int used = 0;
        int dropped = 0;
        int n = blocks.Count;
        int i = 0;
        while (i < n) {
            AIContextBlock b = blocks[i];
            int est = b != null ? b.TokenEstimate : 0;
            if (est < 0) {
                est = 0;
            }
            if (used + est > _host.MaxTokens) {
                if (b != null) {
                    b.Enabled = false;
                }
                dropped = dropped + 1;
            } else {
                used = used + est;
            }
            i = i + 1;
        }
        return dropped;
    }
}