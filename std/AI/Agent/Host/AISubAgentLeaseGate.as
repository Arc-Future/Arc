// A1（subagent-management）：惰性 ToolPath 租约门 —— 子代理首次真实写前取租约。
// 取代「派发即预取整波 Scope 租约」：同波工作项不再因预取而互相误伤（假冲突）；
// 后到拒绝语义保留——首次写时若路径已被其它会话持有 → GuardWrite 返回 false，
// 由宿主把该工作项标记 Failed + 必答小结 + 升级人（不静默降级、不自旋等待）。
//
// 写工具识别约定：capability 含 "Write"（对齐既有 `fs.Write` 能力命名，见
// AISessionOptions.PlanGatedCapabilities 注释）。只读工具（如 review.Run）即使
// 携带 path 参数也不参与写面仲裁——读取不阻塞写入，避免反向假冲突。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 惰性 ToolPath 租约门（A1）：子代理首次真实写前经它按路径逐个
/// <see cref="AICoordinator.Acquire(string, AILeaseKey, AIWorkspace, bool)"/>。
/// 本会话已持有（幂等）/ 路径不在声明写面 / 非写能力 → 放行；被其它会话持有 →
/// 记录冲突并拒绝（后到拒绝，可审计）。宿主据此把工作项标记 Failed + 必答小结。
/// </summary>
public class AISubAgentLeaseGate {
    private AICoordinator _coordinator;
    private AIWorkspace _workspace;
    private string _sessionId;
    // 声明写面（规范化键；经 AIWorkspace.ResolvePath 与写路径同键比较）。
    private List<string> _resolvedScope;
    // 本会话已获租约的规范键（幂等放行依据）。
    private List<string> _acquired;
    private string _blockedPath;
    private string _blockedHolder;

    public AISubAgentLeaseGate(
        AICoordinator coordinator,
        AIWorkspace workspace,
        string sessionId,
        List<string> scope) {
        _coordinator = coordinator;
        _workspace = workspace;
        _sessionId = sessionId != null ? sessionId : "";
        _resolvedScope = new List<string>();
        _acquired = new List<string>();
        _blockedPath = "";
        _blockedHolder = "";
        if (scope != null) {
            int i = 0;
            while (i < scope.Count) {
                string p = scope[i];
                if (p != null && p != "" && _workspace != null) {
                    string rp = _workspace.ResolvePath(p);
                    if (rp != null) {
                        _resolvedScope.Add(rp);
                    }
                }
                i = i + 1;
            }
        }
    }

    /// <summary>是否已发生写面冲突（Gate 已拒绝过一次；后续写一律拒绝）。</summary>
    public bool IsBlocked {
        get { return _blockedPath != ""; }
    }

    /// <summary>被拒路径的规范键（未冲突 = 空串）。</summary>
    public string BlockedPath {
        get { return _blockedPath; }
    }

    /// <summary>冲突持有者会话 id（审计：谁持有被拒路径；空 = 未知）。</summary>
    public string BlockedHolder {
        get { return _blockedHolder; }
    }

    /// <summary>
    /// 首次真实写前取租约：工具调用携带路径参数、capability 为写能力、且路径命中
    /// 本工作项声明写面 → 尝试 <see cref="AICoordinator.Acquire(string, AILeaseKey, AIWorkspace, bool)"/>。
    /// 已被本会话持有（幂等）/ 不满足上述条件 → 放行（true）；被其它会话持有 →
    /// 记录冲突并返回 false（后到拒绝，不排队、不自旋）。
    /// </summary>
    public bool GuardWrite(string capability, string argsJson) {
        if (_blockedPath != "") {
            return false;
        }
        if (capability == null || capability.IndexOf("Write") < 0) {
            return true;
        }
        string path = this.ExtractPath(argsJson);
        if (path == null || path == "") {
            return true;
        }
        string rp = _workspace != null ? _workspace.ResolvePath(path) : null;
        if (rp == null || !this.InScope(rp)) {
            return true;
        }
        if (this.HasAcquired(rp)) {
            return true;
        }
        if (_coordinator == null) {
            return true;
        }
        AILeaseKey key = AILeaseKey.ToolPath(rp);
        AIResourceGrant grant = _coordinator.Acquire(_sessionId, key, _workspace, true);
        if (grant == null || !grant.Acquired) {
            // ToolPath 登记键为规范路径（grant.Key）；HolderOf 用同一键做冲突审计。
            string holder = _coordinator.HolderOf(AILeaseKey.ToolPath(grant != null ? grant.Key : rp));
            _blockedPath = rp;
            _blockedHolder = holder != null ? holder : "";
            return false;
        }
        _acquired.Add(rp);
        return true;
    }

    /// <summary>
    /// A3 决策重对齐租约重验：把声明写面更新为新 Scope，并对新增写面路径逐个取租约
    /// （幂等：已在写面 / 本会话已持有放行）。新增路径被其它会话持有 → 记录冲突并返回
    /// false（后到拒绝 → 宿主把工作项标记 Failed + 必答小结）。Scope 不变（全部已在
    /// 写面）→ 平凡通过。
    /// </summary>
    public bool Revalidate(List<string> newScope) {
        if (_blockedPath != "") {
            return false;
        }
        if (newScope == null || newScope.Count == 0) {
            return true;
        }
        int i = 0;
        while (i < newScope.Count) {
            string p = newScope[i];
            if (p == null || p == "") {
                i = i + 1;
                continue;
            }
            string rp = _workspace != null ? _workspace.ResolvePath(p) : null;
            if (rp == null) {
                i = i + 1;
                continue;
            }
            if (this.InScope(rp) || this.HasAcquired(rp)) {
                i = i + 1;
                continue;
            }
            if (_coordinator == null) {
                _resolvedScope.Add(rp);
                i = i + 1;
                continue;
            }
            AILeaseKey key = AILeaseKey.ToolPath(rp);
            AIResourceGrant grant = _coordinator.Acquire(_sessionId, key, _workspace, true);
            if (grant == null || !grant.Acquired) {
                // ToolPath 登记键为规范路径（grant.Key）；HolderOf 用同一键做冲突审计。
                string holder = _coordinator.HolderOf(AILeaseKey.ToolPath(grant != null ? grant.Key : rp));
                _blockedPath = rp;
                _blockedHolder = holder != null ? holder : "";
                return false;
            }
            _acquired.Add(rp);
            _resolvedScope.Add(rp);
            i = i + 1;
        }
        return true;
    }

    /// <summary>工具参数中提取写面路径（顶层 "path" 参数；缺失 → 空串）。</summary>
    private string ExtractPath(string argsJson) {
        if (argsJson == null || argsJson == "") {
            return "";
        }
        AIToolArgsReader reader = new AIToolArgsReader(argsJson);
        return reader.GetString("path");
    }

    private bool InScope(string rp) {
        int i = 0;
        while (i < _resolvedScope.Count) {
            if (_resolvedScope[i] == rp) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private bool HasAcquired(string rp) {
        int i = 0;
        while (i < _acquired.Count) {
            if (_acquired[i] == rp) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }
}
