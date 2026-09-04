// D5SelfReview —— D5 自审槽位表：对照当前 AIRfc Acceptance 逐条附可执行证明（测试/文件）。
// RFC 043 场景 4.1 修复：D5 证明机器校验——SetProof 后证明须经引用存在性校验
// （文件存在 / 测试名在 `arc test --list-tests` 中可解析，委托 Coding D5ProofVerifier），
// 无机器校验/证明无效标红，非「字符串非空即 Passed」。骨架在基座（D5 门 = Human）。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent.Harness;
using Arc.Agent.Harness.Coding;
using Arc.Text;

/// <summary>D5 自审槽位：Acceptance 逐条 + 机器校验证明；无有效证明项即红项（D5 未过 → 禁 Completed）。</summary>
public class D5SelfReview {
    private List<D5ProofEntry> _entries;
    private D5ProofVerifier _verifier;

    public D5SelfReview() {
        _entries = new List<D5ProofEntry>();
        _verifier = new D5ProofVerifier("");
    }

    public List<D5ProofEntry> Entries {
        get { return _entries; }
    }

    /// <summary>设置项目根（证明文件引用与 `arc test --list-tests` 的解析基）。</summary>
    public void SetProjectRoot(string root) {
        _verifier = new D5ProofVerifier(root);
    }

    /// <summary>
    /// 对照当前 AIRfc 重建槽位：结构化 Acceptance 条目优先（每项一条，
    /// 含场景/断言 + [test:] 测试名 + [verify:] 命令），纯文本兼容期退化为逐行拆条。
    /// </summary>
    public void Reset(AIRfc rfc) {
        _entries.Clear();
        if (rfc == null || rfc.Acceptance == null) {
            return;
        }
        AIAcceptanceSpec acc = rfc.Acceptance;
        if (acc.HasStructuredItems) {
            int i = 0;
            int n = acc.Items.Count;
            while (i < n) {
                _entries.Add(new D5ProofEntry(acc.Items[i].ToLine()));
                i = i + 1;
            }
            return;
        }
        this.AddLines(acc.Assertions);
        this.AddLines(acc.Scenarios);
    }

    private void AddLines(string text) {
        if (text == null || text.Trim() == "") {
            return;
        }
        string[] lines = text.Split("\n");
        int i = 0;
        int n = lines.Length;
        while (i < n) {
            string line = lines[i].Trim();
            if (line != "") {
                _entries.Add(new D5ProofEntry(line));
            }
            i = i + 1;
        }
    }

    /// <summary>
    /// 填证明（1-based 槽位）+ 同步文件引用校验（证明含真实存在的文件路径 → Valid；
    /// 否则置 Unchecked 待 <see cref="ValidateProofsAsync"/> 用 `--list-tests` 解析测试名）。
    /// 返回是否命中槽位。
    /// </summary>
    public bool SetProof(int index, string proof) {
        if (index < 1 || index > _entries.Count) {
            return false;
        }
        // 先物化元素引用再改字段：List 索引作字段赋值接收体在 MIR lower 未处理
        // （Expr::Index → operand_from_expr panic）；D5ProofEntry 为引用类型，改本地即改同对象。
        D5ProofEntry entry = _entries[index - 1];
        entry.Proof = proof != null ? proof.Trim() : "";
        if (entry.Proof == "") {
            entry.Status = D5ProofVerdict.Missing;
        } else if (_verifier.ResolvesToFile(entry.Proof)) {
            entry.Status = D5ProofVerdict.Valid;
        } else {
            entry.Status = D5ProofVerdict.Unchecked;
        }
        return true;
    }

    /// <summary>
    /// 机器校验全部证明（委托 <see cref="D5ProofVerifier"/>：文件命中已 Valid 的槽位跳过；
    /// 其余经 `arc test --list-tests` 测试名交叉解析 → Valid / Invalid）。项目根不可解析 →
    /// 保持 Unchecked（诚实标注「未校验」而非假绿）。
    /// </summary>
    public async Task<bool> ValidateProofsAsync(CancellationToken cancellationToken) {
        bool needList = false;
        int i = 0;
        int n = _entries.Count;
        while (i < n) {
            D5ProofEntry e = _entries[i];
            if (e.Proof != "" && e.Status == D5ProofVerdict.Unchecked) {
                needList = true;
            }
            i = i + 1;
        }
        if (!needList) {
            return true;
        }
        int j = 0;
        int m = _entries.Count;
        while (j < m) {
            D5ProofEntry e = _entries[j];
            if (e.Proof != "" && e.Status == D5ProofVerdict.Unchecked) {
                e.Status = await _verifier.VerifyAsync(e.Proof, cancellationToken);
            }
            j = j + 1;
        }
        return true;
    }

    public bool HasEntries {
        get { return _entries.Count > 0; }
    }

    /// <summary>有效证明计数（仅 Valid 计为已证明；Unchecked/Invalid 不算）。</summary>
    public int ProvenCount {
        get {
            int c = 0;
            int i = 0;
            int n = _entries.Count;
            while (i < n) {
                if (_entries[i].Status == D5ProofVerdict.Valid) {
                    c = c + 1;
                }
                i = i + 1;
            }
            return c;
        }
    }

    /// <summary>全部槽位均有有效机器校验证明（禁「字符串非空即 Passed」）。</summary>
    public bool AllProven {
        get { return _entries.Count > 0 && this.ProvenCount == _entries.Count; }
    }

    /// <summary>渲染 D5 槽位表（[✓] 有效证明 / [~] 未校验 / [✗] 无证明或证明无效——后两类标红）。</summary>
    public string Render() {
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = _entries.Count;
        while (i < n) {
            int idx = i + 1;
            D5ProofEntry e = _entries[i];
            if (e.Status == D5ProofVerdict.Valid) {
                sb.Append("  [✓] " + idx + ". " + e.Acceptance + " → 证明: " + e.Proof + "\n");
            } else if (e.Status == D5ProofVerdict.Unchecked) {
                sb.Append("  [~] " + idx + ". " + e.Acceptance + " → 证明: " + e.Proof + "（未机器校验）  ← 红\n");
            } else if (e.Proof != "") {
                sb.Append("  [✗] " + idx + ". " + e.Acceptance + " → 证明: " + e.Proof + "（引用不存在）  ← 红\n");
            } else {
                sb.Append("  [✗] " + idx + ". " + e.Acceptance + " → 证明: (无)  ← 红\n");
            }
            i = i + 1;
        }
        return sb.ToString();
    }
}
