// RFC 038 上下文成体系：AIProviderSessionState — 类型化会话态助手（MAF 对齐）。
//
// MAF 提供 AIProviderSessionState<TState>：把类型化会话态安全落进会话载体，key 用
// provider 名 + stateKey 拼接避免跨 provider 冲突。本类型同构：
//   - _key = (providerName + "::" + stateKey)，null 段归一为空串。
//   - GetOrInitialize：不存在则用 initializer 惰性初始化并落态，存在则取回（类型强转）。
//   - Set：覆盖写入。
//
// 存储安全：值经 AIContextSession 的类型化访问器（GetState<TState>/SetState<TState>）
// 以「泛型子类 + 基类集合」承载（见 AIContextSession 注释），规避跨程序集 object? 装箱
// 缺口，故任意 TState（含 string/值类型/自定义类）均可安全往返。
namespace Arc.Agent;

/// <summary>
/// 类型化会话态助手：把 <typeparamref name="TState"/> 型会话态安全落进
/// <see cref="AIContextSession"/>，key 以 (providerName + "::" + stateKey) 避免跨
/// provider 冲突。provider 实例跨会话共享时经本助手读写自身会话态（实例零会话态字段）。
/// </summary>
/// <typeparam name="TState">会话态类型。</typeparam>
public class AIProviderSessionState<TState> {
    private string _key;

    /// <summary>构造类型化会话态助手。providerName / stateKey 为 null 时归一为空串。</summary>
    public AIProviderSessionState(string providerName, string stateKey) {
        string pn = providerName != null ? providerName : "";
        string sk = stateKey != null ? stateKey : "";
        _key = pn + "::" + sk;
    }

    /// <summary>
    /// 获取会话态；不存在则用 <paramref name="initializer"/> 惰性初始化并落态后返回。
    /// session 为 null 抛 <see cref="ArgumentNullException"/>。
    /// </summary>
    public TState GetOrInitialize(AIContextSession session, Func<TState> initializer) {
        if (session == null) {
            throw new ArgumentNullException("session");
        }
        if (session.ContainsState(_key)) {
            return session.GetState<TState>(_key);
        }
        TState v = initializer();
        session.SetState<TState>(_key, v);
        return v;
    }

    /// <summary>覆盖写入会话态。session 为 null 抛 <see cref="ArgumentNullException"/>。</summary>
    public void Set(AIContextSession session, TState value) {
        if (session == null) {
            throw new ArgumentNullException("session");
        }
        session.SetState<TState>(_key, value);
    }
}
