// RFC 038 §3.3（M7）+ §13（冲突织物）：宿主级跨会话冲突协调——AIRfc / AIPlan / AITool 共用
// 统一租约登记表。三 Kind（ToolPath / Plan / RfcSpec）一表，冲突策略锁定为**后到拒绝**
// （不排队、不自旋、可审计）。现有路径写协调（AcquireWrite / CommitAsync / 覆写审计）
// 保留为 ToolPath 的薄兼容包装，语义不变。
namespace Arc.Agent;
using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Security;
using Arc.Text;

/// <summary>
/// 写意图授权（ToolPath 薄兼容视图）。由 <see cref="AICoordinator.AcquireWrite"/> 签发：
/// 冲突（其它会话已登记同一路径租约 / 已登记覆写）→ <see cref="Acquired"/> = false。
/// 提交经 <see cref="AICoordinator.CommitAsync"/> 原子落盘；写权持续持有至会话显式
/// <see cref="AICoordinator.Release"/> / <see cref="AICoordinator.ReleaseSession"/> 释放。
/// </summary>
public class AIWriteGrant {
    public string SessionId;
    public string Path;
    public bool Acquired;
    public bool IsOverwrite;

    public AIWriteGrant(string sessionId, string path, bool acquired, bool isOverwrite) {
        this.SessionId = sessionId != null ? sessionId : "";
        this.Path = path != null ? path : "";
        this.Acquired = acquired;
        this.IsOverwrite = isOverwrite;
    }
}

/// <summary>
/// 覆写审计记录。由 <see cref="AICoordinator.CommitAsync"/> 在**显式覆写已存在文件**
/// 时生成（§3.3 第 4 点「不静默覆写」）：记录 谁（<see cref="SessionId"/>）/ 何时
/// （<see cref="Timestamp"/>）/ 覆盖前内容 hash（<see cref="PreHash"/>）+ 新内容 hash
/// （<see cref="PostHash"/>），供宿主追溯覆写来源与内容变更。
/// </summary>
public class AIWriteAuditEntry {
    public string SessionId;
    public string Path;
    public DateTime Timestamp;
    public string PreHash;
    public string PostHash;

    public AIWriteAuditEntry(string sessionId, string path, DateTime timestamp, string preHash, string postHash) {
        this.SessionId = sessionId != null ? sessionId : "";
        this.Path = path != null ? path : "";
        this.Timestamp = timestamp;
        this.PreHash = preHash != null ? preHash : "";
        this.PostHash = postHash != null ? postHash : "";
    }
}

/// <summary>
/// 宿主级冲突织物协调器（单例挂 <see cref="AIHost"/> 门下）：统一租约登记 → 跨会话冲突
/// 检测（后到拒绝，不排队、不自旋）→ ToolPath 原子提交 + 覆写审计。三 Kind 键空间
/// （ToolPath / Plan / RfcSpec）走**同一**登记表——AIRfc / AIPlan / AITool 只消费本协调器，
/// 禁止平行实现第二套锁。
///
/// 并发模型：Arc 会话在主线程串行；本协调器以登记表（List）在单线程内做判定，无需锁。
/// ToolPath 路径经 <see cref="AIWorkspace.ResolvePath"/> 规范化为冲突键——逃逸路径本身即被
/// 工作区拒绝，此处仅做跨会话仲裁。
/// </summary>
public class AICoordinator {
    // 统一租约登记表：注册键（"Kind|资源id"）/ 持有者 两条平行列表（单线程判定，无需锁）。
    // 注：不用 List<AILeaseKind> 承载种类——实测 `List<枚举>.Add(枚举)` 触发 codegen 运行时
    // 崩溃（见 docs/plan.md CD-7），故以字符串前缀入注册键（与既有 List<string> 风格一致）。
    private List<string> _leaseKeys;
    private List<string> _leaseSessions;
    // 覆写审计日志：显式覆写已存在文件时追加（谁/何时/覆盖前内容 hash）。
    private List<AIWriteAuditEntry> _auditLog;

    public AICoordinator() {
        _leaseKeys = new List<string>();
        _leaseSessions = new List<string>();
        _auditLog = new List<AIWriteAuditEntry>();
    }

    /// <summary>
    /// 通用租约获取（Plan / RfcSpec 键空间）。冲突（其它会话已持有同 Kind 同资源租约）
    /// → <see cref="AIResourceGrant.Acquired"/> = false：后到拒绝、不排队，先到者不受阻。
    /// 同会话重复 Acquire 幂等返回 true（不重复登记）。
    /// </summary>
    public AIResourceGrant Acquire(string holderId, AILeaseKey key) {
        return this.AcquireCore(holderId, key, null, false, null);
    }

    /// <summary>带任务运行 id（审计元数据，非第二锁）的租约获取。</summary>
    public AIResourceGrant Acquire(string holderId, AILeaseKey key, string taskRunId) {
        return this.AcquireCore(holderId, key, null, false, taskRunId);
    }

    /// <summary>
    /// ToolPath 特化租约获取：path 经 workspace 解析为规范键；逃逸 → 拒绝（不登记）。
    /// isOverwrite=true 且目标已存在 → 显式覆写确权（不静默覆写：CommitAsync 记录覆写审计）。
    /// </summary>
    public AIResourceGrant Acquire(string holderId, AILeaseKey key, AIWorkspace workspace, bool isOverwrite) {
        return this.AcquireCore(holderId, key, workspace, isOverwrite, null);
    }

    private AIResourceGrant AcquireCore(string holderId, AILeaseKey key, AIWorkspace workspace, bool isOverwrite, string taskRunId) {
        AILeaseKind kind = key != null ? key.Kind : AILeaseKind.ToolPath;
        string resId = key != null ? key.ResourceId : "";
        if (holderId == null || holderId == "" || key == null || resId == "") {
            return new AIResourceGrant(kind, holderId, taskRunId, resId, false, isOverwrite);
        }
        // ToolPath 键空间：路径规范化 + 不静默覆写（目标已存在而未显式确权 → 拒绝，
        // 确保每次真实覆写都走 isOverwrite=true，从而被 CommitAsync 记录审计日志）。
        if (kind == AILeaseKind.ToolPath) {
            if (workspace == null) {
                return new AIResourceGrant(kind, holderId, taskRunId, resId, false, isOverwrite);
            }
            string rp = workspace.ResolvePath(resId);
            if (rp == null) {
                // 逃逸路径：工作区已拒绝，协调器不登记。
                return new AIResourceGrant(kind, holderId, taskRunId, resId, false, isOverwrite);
            }
            resId = rp;
            if (!isOverwrite && workspace.FileExists(resId)) {
                return new AIResourceGrant(kind, holderId, taskRunId, resId, false, false);
            }
        }
        // 冲突检测：其它会话已持有同 Kind 同资源租约 → 后到拒绝（可审计；不排队）。
        string regKey = this.RegistryKey(kind, resId);
        string holder = this.FindHolder(regKey);
        if (holder != null && holder != holderId) {
            return new AIResourceGrant(kind, holderId, taskRunId, resId, false, isOverwrite);
        }
        if (holder == holderId) {
            // 同会话重复获取：幂等（不重复登记；先到者不受阻）。
            return new AIResourceGrant(kind, holderId, taskRunId, resId, true, isOverwrite);
        }
        _leaseKeys.Add(regKey);
        _leaseSessions.Add(holderId);
        return new AIResourceGrant(kind, holderId, taskRunId, resId, true, isOverwrite);
    }

    /// <summary>
    /// 登记写意图（ToolPath 薄兼容包装）：等价
    /// <see cref="Acquire(string, AILeaseKey, AIWorkspace, bool)"/>，返回既有
    /// <see cref="AIWriteGrant"/> 视图。冲突（其它会话已登记同路径租约）→ Acquired=false，
    /// 后到者报告冲突、不阻塞先到者。
    /// </summary>
    public AIWriteGrant AcquireWrite(string sessionId, AIWorkspace workspace, string path, bool isOverwrite) {
        AILeaseKey key = new AILeaseKey(AILeaseKind.ToolPath, path);
        AIResourceGrant grant = this.Acquire(sessionId, key, workspace, isOverwrite);
        return new AIWriteGrant(grant.SessionId, grant.Key, grant.Acquired, grant.IsOverwrite);
    }

    /// <summary>
    /// 原子提交（ToolPath）：经 workspace 异步写根内暂存文件，再 move 到目标
    /// （staging → 原子替换；同一目录内 Move 为原子改名）。提交后**不**释放写权
    /// ——写权须持续持有至会话显式 <see cref="Release"/> / <see cref="ReleaseSession"/>，
    /// 否则单次提交即放锁会让其它会话趁编辑间隙取得写权，破坏冲突保护。
    /// 异步优先：内容落盘走 async API，不阻塞调用线程。
    /// </summary>
    public async Task<bool> CommitAsync(AIWriteGrant grant, AIWorkspace workspace, string content) {
        if (grant == null || !grant.Acquired || workspace == null) {
            return false;
        }
        string rp = grant.Path;
        if (rp == null || rp == "") {
            return false;
        }
        // 覆写审计（§3.3 第 4 点「不静默覆写」）：显式覆写已存在文件时，在替换目标
        // 之前读取覆盖前内容 hash + 新内容 hash，供宿主追溯。覆盖前内容异步读取。
        string prev = null;
        if (grant.IsOverwrite && File.Exists(rp)) {
            prev = await File.ReadAllTextAsync(rp);
        }
        // 原子替换统一走 workspace.WriteAllTextAsync（staging → 同目录 Move 覆盖），
        // 不预删目标——任何失败路径都不丢原文件；审计仅在替换成功后登记，避免记录虚假覆写。
        bool ok = await workspace.WriteAllTextAsync(rp, content);
        if (ok && prev != null) {
            AIWriteAuditEntry entry = new AIWriteAuditEntry(
                grant.SessionId, rp, DateTime.Now,
                SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes(prev))),
                SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes(content))));
            _auditLog.Add(entry);
        }
        return ok;
    }

    /// <summary>
    /// RfcSpec 提交门（RFC 038 §13 / 043 conflict-fabric §3）：校验调用方**仍持有**该 RfcSpec
    /// 租约后才允许执行 Spec 突变。Commit 不自动放锁（编辑间隙保护）；后到者/非持有者 → false。
    /// 突变本身由调用方（<see cref="AIRfcRuntime"/>）执行，本方法只做「仅持有者可提交」的确权。
    /// </summary>
    public bool CommitRfcSpec(string holderId, AILeaseKey key) {
        if (key == null || key.Kind != AILeaseKind.RfcSpec) {
            return false;
        }
        string holder = this.FindHolder(this.RegistryKey(key.Kind, key.ResourceId));
        return holder != null && holder == holderId;
    }

    /// <summary>
    /// 释放某会话对指定租约的持有（通用键：Plan = "plan:"+PlanId；RfcSpec = "airfc:"+RfcId；
    /// ToolPath 为规范路径）。
    /// </summary>
    public void Release(string holderId, AILeaseKey key) {
        if (key == null) {
            return;
        }
        this.RemoveByHolder(key.Kind, key.ResourceId, holderId);
    }

    /// <summary>
    /// 释放某会话对该规范路径的 ToolPath 租约（ToolPath 兼容重载；path 须为规范路径，
    /// 即 <see cref="AIWriteGrant.Path"/>，与 AcquireWrite 内部登记键一致）。
    /// </summary>
    public void Release(string sessionId, string path) {
        this.RemoveByHolder(AILeaseKind.ToolPath, path, sessionId);
    }

    /// <summary>会话结束释放其全部租约登记。</summary>
    public void ReleaseSession(string sessionId) {
        this.RemoveAllBySession(sessionId);
    }

    /// <summary>某资源当前租约持有者；无则空串（冲突审计：谁持有 / 谁被拒可追溯）。</summary>
    public string HolderOf(AILeaseKey key) {
        if (key == null) {
            return "";
        }
        string holder = this.FindHolder(this.RegistryKey(key.Kind, key.ResourceId));
        return holder != null ? holder : "";
    }

    /// <summary>某路径当前是否已被其它会话登记写（读写互斥提示用；ToolPath 兼容视图）。</summary>
    public string WriterOf(string path) {
        string holder = this.FindHolder(this.RegistryKey(AILeaseKind.ToolPath, path));
        return holder != null ? holder : "";
    }

    /// <summary>覆写审计日志条数。</summary>
    public int AuditCount {
        get { return _auditLog.Count; }
    }

    /// <summary>取第 index 条覆写审计记录；越界返回 null。可空返回：null = 越界。</summary>
    public AIWriteAuditEntry? GetAuditEntry(int index) {
        if (index < 0 || index >= _auditLog.Count) {
            return null;
        }
        return _auditLog[index];
    }

    // ── 私有登记表操作 ──

    /// <summary>注册键 = "Kind|资源id"（种类以字符串前缀入键，规避 List<枚举> codegen 缺陷 CD-7）。</summary>
    private string RegistryKey(AILeaseKind kind, string resourceId) {
        string name = "ToolPath";
        if (kind == AILeaseKind.Plan) {
            name = "Plan";
        } else if (kind == AILeaseKind.RfcSpec) {
            name = "RfcSpec";
        }
        string id = resourceId != null ? resourceId : "";
        return name + "|" + id;
    }

    private string? FindHolder(string regKey) {
        int n = _leaseKeys.Count;
        int i = 0;
        while (i < n) {
            if (_leaseKeys[i] == regKey) {
                return _leaseSessions[i];
            }
            i = i + 1;
        }
        return null;
    }

    private void RemoveByHolder(AILeaseKind kind, string resourceId, string holderId) {
        // 先快照待删索引（只读），再逆序按索引删——规避 NLL「迭代期间修改容器」检查。
        string regKey = this.RegistryKey(kind, resourceId);
        List<int> indices = new List<int>();
        int n = _leaseKeys.Count;
        int i = 0;
        while (i < n) {
            if (_leaseKeys[i] == regKey && _leaseSessions[i] == holderId) {
                indices.Add(i);
            }
            i = i + 1;
        }
        int k = indices.Count - 1;
        while (k >= 0) {
            int idx = indices[k];
            _leaseKeys.RemoveAt(idx);
            _leaseSessions.RemoveAt(idx);
            k = k - 1;
        }
    }

    private void RemoveAllBySession(string holderId) {
        List<int> indices = new List<int>();
        int n = _leaseKeys.Count;
        int i = 0;
        while (i < n) {
            if (_leaseSessions[i] == holderId) {
                indices.Add(i);
            }
            i = i + 1;
        }
        int k = indices.Count - 1;
        while (k >= 0) {
            int idx = indices[k];
            _leaseKeys.RemoveAt(idx);
            _leaseSessions.RemoveAt(idx);
            k = k - 1;
        }
    }
}
