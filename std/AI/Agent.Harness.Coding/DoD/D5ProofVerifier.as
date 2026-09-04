// RFC 043 场景 4.1：D5 证明机器校验——证明必须指向真实存在的测试/文件，
// 非「字符串非空即 Passed」。务实契约：至少校验「证明引用存在」（文件存在 /
// 测试名在 `arc test --list-tests` 中可解析）；深度校验（真实运行通过）为增强面。
// 判定信号在 Coding（经 quality CLI 调 arc）；D5SelfReview（REPL 交互面）委托本类。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.IO;

/// <summary>D5 证明机器校验判定（场景 4.1 验收对照回路）。</summary>
public enum D5ProofVerdict {
    /// <summary>无证明。</summary>
    Missing,
    /// <summary>有证明文本，尚未机器校验（测试名待 `--list-tests` 解析）。</summary>
    Unchecked,
    /// <summary>证明引用真实存在（文件存在或测试名可解析）。</summary>
    Valid,
    /// <summary>证明引用不存在（文件不存在 / 测试名未解析）——标红，D5 不放过。</summary>
    Invalid
}

/// <summary>
/// D5 证明引用存在性校验：文件路径（相对项目根 / 绝对）或测试名
/// （`arc test <项目> --list-tests` 输出交叉匹配）。校验以项目根为解析基。
/// </summary>
public class D5ProofVerifier {
    private string _projectRoot;
    private string _testListCache;

    public D5ProofVerifier(string projectRoot) {
        _projectRoot = projectRoot != null ? projectRoot : "";
        _testListCache = "";
    }

    /// <summary>证明文本是否引用真实存在的文件（绝对路径或相对项目根的路径）。</summary>
    public bool ResolvesToFile(string proof) {
        if (proof == null || proof == "") {
            return false;
        }
        string[] tokens = proof.Split(" ");
        int i = 0;
        int n = tokens.Length;
        while (i < n) {
            string token = tokens[i].Trim();
            if (token != "") {
                if (File.Exists(token)) {
                    return true;
                }
                if (_projectRoot != "") {
                    string candidate = Path.Combine(_projectRoot, token);
                    if (File.Exists(candidate)) {
                        return true;
                    }
                }
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>证明文本中的测试名是否在 `arc test <项目> --list-tests` 输出中可解析（缓存一次）。</summary>
    public async Task<bool> ResolvesToTestAsync(string proof, CancellationToken cancellationToken) {
        if (proof == null || proof == "") {
            return false;
        }
        if (_projectRoot == "") {
            return false;
        }
        if (_testListCache == "") {
            ProcessRunResult pr = await QualityCli.RunArcResultAsync(
                "test " + D5ProofVerifier.Quote(_projectRoot) + " --list-tests", cancellationToken);
            _testListCache = pr.StandardOutput != null ? pr.StandardOutput : "";
        }
        return this.MatchesTestList(proof, _testListCache);
    }

    /// <summary>
    /// 完整校验：空证明 → Missing；文件命中 → Valid；否则经 `--list-tests` 测试名
    /// 交叉解析 → Valid / Invalid。测试列表不可解析（项目无测试）→ Invalid（诚实标红）。
    /// </summary>
    public async Task<D5ProofVerdict> VerifyAsync(string proof, CancellationToken cancellationToken) {
        if (proof == null || proof == "") {
            return D5ProofVerdict.Missing;
        }
        if (this.ResolvesToFile(proof)) {
            return D5ProofVerdict.Valid;
        }
        bool test = await this.ResolvesToTestAsync(proof, cancellationToken);
        return test ? D5ProofVerdict.Valid : D5ProofVerdict.Invalid;
    }

    /// <summary>证明文本与 `--list-tests` 输出是否可交叉解析（测试名 ↔ 证明文本任一包含）。</summary>
    private bool MatchesTestList(string proof, string testListOutput) {
        if (testListOutput == "" || proof == "") {
            return false;
        }
        string[] lines = testListOutput.Split("\n");
        int i = 0;
        int n = lines.Length;
        while (i < n) {
            string name = D5ProofVerifier.ExtractTestName(lines[i]);
            if (name != "" && (proof.IndexOf(name) >= 0 || name.IndexOf(proof) >= 0)) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>从 `--list-tests` 输出行提取测试名（`[Fact] K4Tests.TestAdd (Order=0)` → `K4Tests.TestAdd`）。</summary>
    public static string ExtractTestName(string line) {
        if (line == null) {
            return "";
        }
        int close = line.IndexOf("]");
        if (close < 0) {
            return "";
        }
        string rest = line.Substring(close + 1).Trim();
        int paren = rest.IndexOf(" (");
        if (paren > 0) {
            rest = rest.Substring(0, paren).Trim();
        }
        return rest;
    }

    private static string Quote(string path) {
        if (path == null) {
            return "\"\"";
        }
        if (path.IndexOf(" ") >= 0) {
            return "\"" + path + "\"";
        }
        return path;
    }
}
