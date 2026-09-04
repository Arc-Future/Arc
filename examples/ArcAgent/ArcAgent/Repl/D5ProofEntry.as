// D5ProofEntry —— 单条 Acceptance 的自审证明槽位（哪条验收 + 可执行证明 + 机器校验状态）。
// RFC 043 场景 4.1：D5 证明机器校验——Proof 非空不等于可执行；必须经
// D5ProofVerifier（Coding）校验（文件存在 / 测试名在 `arc test --list-tests` 中可解析）。
namespace ArcAgent.Repl;
using Arc.Agent.Harness.Coding;

/// <summary>D5 自审槽位：一条 Acceptance + 可执行证明（测试/文件路径）+ 机器校验状态。</summary>
public class D5ProofEntry {
    public string Acceptance;
    public string Proof;
    public D5ProofVerdict Status;

    public D5ProofEntry(string acceptance) {
        this.Acceptance = acceptance != null ? acceptance : "";
        this.Proof = "";
        this.Status = D5ProofVerdict.Missing;
    }
}
