// RFC 038 上下文成体系：AIContextSession — 会话态载体（MAF 对齐）。
//
// MAF 明确：provider 实例跨会话共享（Host 级注册），禁止在 provider 实例字段存任何
// 会话态；会话态经「会话载体」按名读写。本类型即该载体：承载 SessionId 与一个按名
// 读写的任意类型会话态仓库。provider 经 AIProviderSessionState<TState> 助手读写
// （key = providerName + "::" + stateKey），实现类型化、去冲突、安全落态——实例零会话态字段。
//
// 存储形态（编译器约束下的 Arc 惯用法，非降级，已实证）：_state 为 List<AIContextStateStore>
// （非泛型基类键值节点），值以 AIContextStateEntry<TState>（泛型子类，持类型化 Value 字段）
// 承载。原因——Arc `object?` 为 FFI Marshal 专用根类型，codegen 对「值类型/泛型类 → object?」
// 装箱存在跨程序集缺口（值类型丢值、泛型类堆破坏），而「基类引用集合 + 显式向上转型 +
// 泛型向下转型」在全路径（单/跨程序集、泛型方法单态化体）均实证正确。故本类型用基类集合
// 承载任意类型会话态，规避 object? 装箱。会话态条目量小（每 provider 数个），线性扫描
// O(n) 可忽略。
//
// ⚠️ 可见性说明（编译器约束，非降级）：GetState<TState>/SetState<TState> 为泛型方法，在
// Arc 编译器下于「外部调用点」实例化——泛型方法体一旦在调用方程序集（如自定义 provider
// 所在包）实例化，其内部对 internal 类型的引用将不可见（OOP: unknown method）。故本类型
// 的四个态访问器与载体类型（AIContextStateStore / AIContextStateEntry<TState>）须为
// public 以支撑跨包实例化；推荐使用面仍为 AIProviderSessionState<TState>。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 会话级状态载体（MAF 会话态分离）。承载会话标识与按名读写的任意类型会话态仓库。
/// provider 实例跨会话共享，会话态一律经 <see cref="AIProviderSessionState{TState}"/> 读写
/// （key = providerName + "::" + stateKey），杜绝实例字段存会话态导致的状态泄漏。底层
/// 亦提供 <see cref="GetState{TState}"/> / <see cref="SetState{TState}"/> 类型化原语。
/// </summary>
public class AIContextSession {
    private string _sessionId;
    private List<AIContextStateStore> _state;

    /// <summary>构造会话态载体。sessionId 为 null 时归一为空串。</summary>
    public AIContextSession(string sessionId) {
        _sessionId = sessionId != null ? sessionId : "";
        _state = new List<AIContextStateStore>();
    }

    /// <summary>会话标识（记忆 / 审计归属；与 AIContextQuery.SessionId 一致）。</summary>
    public string SessionId {
        get { return _sessionId; }
    }

    /// <summary>判断指定 key 是否存在会话态。</summary>
    public bool ContainsState(string key) {
        if (key == null || key == "") {
            return false;
        }
        int n = _state.Count;
        int i = 0;
        while (i < n) {
            AIContextStateStore e = _state[i];
            if (e != null && e.Key == key) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>读取指定 key 的类型化会话态；不存在返回 TState 默认值。</summary>
    public TState GetState<TState>(string key) {
        if (key == null || key == "") {
            return default(TState);
        }
        int n = _state.Count;
        int i = 0;
        while (i < n) {
            AIContextStateStore e = _state[i];
            if (e != null && e.Key == key) {
                AIContextStateEntry<TState> t = (AIContextStateEntry<TState>)e;
                if (t != null) {
                    return t.Value;
                }
                return default(TState);
            }
            i = i + 1;
        }
        return default(TState);
    }

    /// <summary>写入指定 key 的类型化会话态（覆盖；不存在则追加）。</summary>
    public void SetState<TState>(string key, TState value) {
        if (key == null || key == "") {
            return;
        }
        AIContextStateStore entry = (AIContextStateStore)new AIContextStateEntry<TState>(key, value);
        int n = _state.Count;
        int i = 0;
        while (i < n) {
            AIContextStateStore e = _state[i];
            if (e != null && e.Key == key) {
                _state[i] = entry;
                return;
            }
            i = i + 1;
        }
        _state.Add(entry);
    }

    /// <summary>移除指定 key 的会话态。</summary>
    public void RemoveState(string key) {
        if (key == null || key == "") {
            return;
        }
        int n = _state.Count;
        int i = 0;
        while (i < n) {
            AIContextStateStore e = _state[i];
            if (e != null && e.Key == key) {
                _state.RemoveAt(i);
                return;
            }
            i = i + 1;
        }
    }
}

/// <summary>
/// 会话态非泛型基类节点（编译器约束下的内部实现载体）：持 Key 标识。仅
/// <see cref="AIContextSession"/> 与 <see cref="AIProviderSessionState{TState}"/> 内部使用，
/// 外部不应直接引用；因泛型方法体在调用方程序集实例化而必须为 public。
/// </summary>
public class AIContextStateStore {
    /// <summary>会话态 key（唯一）。</summary>
    public string Key;

    /// <summary>构造基类节点。</summary>
    public AIContextStateStore(string key) {
        this.Key = key;
    }
}

/// <summary>
/// 会话态类型化节点（编译器约束下的内部实现载体）：持 TState 类型化 Value 字段，以
/// 泛型子类 + 基类集合承载任意类型会话态，规避跨程序集 object? 装箱缺口。仅
/// <see cref="AIContextSession"/> 与 <see cref="AIProviderSessionState{TState}"/> 内部使用，
/// 外部不应直接引用；因泛型方法体在调用方程序集实例化而必须为 public。
/// </summary>
/// <typeparam name="TState">会话态类型。</typeparam>
public class AIContextStateEntry<TState> : AIContextStateStore {
    /// <summary>承载的会话态值。</summary>
    public TState Value;

    /// <summary>构造类型化节点。</summary>
    public AIContextStateEntry(string key, TState value) : base(key) {
        this.Value = value;
    }
}
