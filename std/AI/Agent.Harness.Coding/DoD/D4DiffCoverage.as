// RFC 043 H-3：D4 diff 覆盖门 — 计划步骤 ↔ 工作区改动集对齐（Coding 领域）。
//
// D4 通过谓词（最小可证伪、不造假的最小契约）：
//   1. 无 AIPlan 步骤 → 数据不足，调用方必须返回 Pending（非 Passed）。
//   2. 工作区改动集主信号 = `git status --porcelain -- .`（工作目录 = 目标项目，
//      pathspec `.` 限定项目子树；含已跟踪修改 + 未跟踪新文件）。git 不可用
//      （spawn 失败 / 非 git 仓库 / 退出非 0）→ 兜底最小判定：目标项目 .as 文件清单
//      ∩ 计划文件声明（交集视为「对应改动存在」，见谓词 2'）。**K6-①**：工作区自身
//      被 `.gitignore` 遮蔽（如 target/ 下夹具）时 git status 恒 0 改动，git 信号
//      不可信 → 同样退回文件清单兜底。**K6-②**：兜底交集按 basename 对齐比较
//      （项目清单为全路径、声明为 basename）。
//   3. 计划覆盖：每个声明了文件名的步骤，其声明文件（逗号分隔，取 basename）至少
//      有一处对应改动；**K6-③** 无任何改动 → 显式 no-diff（视为覆盖满足）。
//   4. 越界检测：每个改动文件须被至少一个步骤声明；声明为空且有改动 → 全部越界。
//   5. 全部满足 → Passed；任一不满足 → Failed（带步骤/文件命中）。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.IO;

/// <summary>D4 改动证据：变更文件集 + 计划声明文件集 + 信号源。</summary>
public class D4DiffEvidence {
    public List<string> Changed;
    public List<string> Declared;
    public string Source;
    public bool FromGit;
    public bool GitOk;

    public D4DiffEvidence() {
        this.Changed = new List<string>();
        this.Declared = new List<string>();
        this.Source = "";
        this.FromGit = false;
        this.GitOk = false;
    }
}

/// <summary>D4 diff 覆盖判定器：收集改动证据 + 计划覆盖/越界判定。</summary>
public static class D4DiffCoverage {
    /// <summary>收集改动证据：git status --porcelain 主信号；不可用/被 gitignore 遮蔽 → 文件清单兜底。</summary>
    public static async Task<D4DiffEvidence> CollectAsync(string dir, AIPlan plan, CancellationToken cancellationToken) {
        D4DiffEvidence ev = new D4DiffEvidence();
        ev.Declared = D4DiffCoverage.DeclaredBasenames(plan);
        ProcessRunResult r = null;
        bool spawned = true;
        try {
            ProcessStartInfo si = new ProcessStartInfo();
            si.FileName = "git";
            si.Arguments = "status --porcelain -- .";
            si.WorkingDirectory = dir;
            r = await Process.RunCaptureAsync(si, cancellationToken);
        } catch (Exception) {
            spawned = false;
        }
        if (spawned && r != null && r.ExitCode == 0) {
            ev.GitOk = true;
            ev.FromGit = true;
            ev.Source = "git status --porcelain";
            ev.Changed = D4DiffCoverage.ParsePorcelain(r.StandardOutput);
            bool ignored = await D4DiffCoverage.IsGitIgnoredAsync(dir, cancellationToken);
            // K6-① gitignored 盲区：工作区自身被 .gitignore 遮蔽（如 target/ 下夹具）
            // 时 `git status` 恒 0 改动——git 信号不可信，退回文件系统清单兜底。
            if (ev.Changed.Count == 0 && ignored) {
                ev.FromGit = false;
                ev.GitOk = false;
                ev.Source = "project file scan (gitignored worktree fallback)";
                ev.Changed = D4DiffCoverage.IntersectProject(
                    D2ContractScanner.CollectAsFiles(dir), ev.Declared);
            }
            return ev;
        }
        // 兜底：目标项目 .as 文件清单 ∩ 计划声明（最小判定；谓词见文件头 2'）。
        // K6-②：项目清单为全路径、声明为 basename——按 basename 对齐比较（旧实现
        // 全路径 vs basename 精确相等恒假 → Changed 恒空 → 兜底不可用）。
        List<string> project = D2ContractScanner.CollectAsFiles(dir);
        ev.Source = "project .as files ∩ plan declared files";
        ev.Changed = D4DiffCoverage.IntersectProject(project, ev.Declared);
        return ev;
    }

    /// <summary>检测工作区是否被 git 忽略（`git check-ignore .` exit 0 = 忽略）。</summary>
    private static async Task<bool> IsGitIgnoredAsync(string dir, CancellationToken cancellationToken) {
        try {
            ProcessStartInfo si = new ProcessStartInfo();
            si.FileName = "git";
            si.Arguments = "check-ignore .";
            si.WorkingDirectory = dir;
            ProcessRunResult r = await Process.RunCaptureAsync(si, cancellationToken);
            return r != null && r.ExitCode == 0;
        } catch (Exception) {
            return false;
        }
    }

    /// <summary>
    /// D4 判定：计划覆盖（每步骤声明至少一处改动）+ 越界检测（改动无对应步骤声明）。
    /// 返回 Passed / Failed；数据不足（无步骤/无法定位项目）由调用方返回 Pending。
    /// 注：同步方法返回 AIDoDGateResult 触 typeck「expected Task<T>, found bool」怪癖
    /// （已实证），故本判定走 async Task<AIDoDGateResult>（与 D1/D6 同式）。
    /// </summary>
    public static async Task<AIDoDGateResult> VerdictAsync(AIDoDGateKind gate, AIPlan plan, D4DiffEvidence ev) {
        if (ev.Changed.Count == 0) {
            // K6-③：无任何改动 → 显式 no-diff（视为覆盖满足），与文档「无改动 →
            // no-diff 满足」一致——不再要求 Declared 同时为空才 Pass（改动集为空
            // 即无可覆盖项，声明文件是计划面承诺而非改动证据）。
            return AIDoDGateResult.Pass(gate, "diff-coverage no changed files (explicit no-diff)");
        }
        if (ev.Declared.Count == 0) {
            return AIDoDGateResult.Fail(gate, "diff-coverage",
                "out-of-bounds: " + ev.Changed.Count + " changed file(s) match no plan step file declaration (source=" + ev.Source + ")");
        }
        int i = 0;
        while (i < plan.Steps.Count) {
            AIPlanNode step = plan.Steps[i];
            List<string> stepFiles = D4DiffCoverage.DeclaredForStep(step);
            if (stepFiles.Count > 0 && !D4DiffCoverage.AnyCovered(stepFiles, ev.Changed)) {
                return AIDoDGateResult.Fail(gate, "diff-coverage",
                    "uncovered step " + (i + 1) + " '" + step.Title + "' declared ["
                        + D4DiffCoverage.Join(stepFiles, ", ") + "] but no matching change (source=" + ev.Source + ")");
            }
            i = i + 1;
        }
        int j = 0;
        while (j < ev.Changed.Count) {
            if (!D4DiffCoverage.ListContains(ev.Declared, ev.Changed[j])) {
                return AIDoDGateResult.Fail(gate, "diff-coverage",
                    "out-of-bounds change '" + ev.Changed[j] + "' not declared by any plan step (source=" + ev.Source + ")");
            }
            j = j + 1;
        }
        string signal = "diff-coverage changed=" + ev.Changed.Count
            + ", declared=" + ev.Declared.Count + ", source=" + ev.Source;
        return AIDoDGateResult.Pass(gate, signal);
    }

    /// <summary>计划全部步骤声明文件（去重 basename）。</summary>
    public static List<string> DeclaredBasenames(AIPlan plan) {
        List<string> declared = new List<string>();
        if (plan == null || plan.Steps == null) {
            return declared;
        }
        int i = 0;
        while (i < plan.Steps.Count) {
            List<string> files = D4DiffCoverage.DeclaredForStep(plan.Steps[i]);
            int j = 0;
            while (j < files.Count) {
                if (!D4DiffCoverage.ListContains(declared, files[j])) {
                    declared.Add(files[j]);
                }
                j = j + 1;
            }
            i = i + 1;
        }
        return declared;
    }

    /// <summary>解析 git status --porcelain 输出：`XY path` → path（basename）；rename 取新路径。</summary>
    public static List<string> ParsePorcelain(string stdout) {
        List<string> result = new List<string>();
        if (stdout == null || stdout == "") {
            return result;
        }
        string[] lines = stdout.Split("\n");
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i] : "";
            if (line.Length >= 4) {
                string path = line.Substring(3, line.Length - 3).Trim();
                int arrow = path.IndexOf(" -> ");
                if (arrow >= 0) {
                    path = path.Substring(arrow + 4, path.Length - arrow - 4);
                }
                if (path != "") {
                    result.Add(D4DiffCoverage.Basename(path));
                }
            }
            i = i + 1;
        }
        return result;
    }

    private static List<string> DeclaredForStep(AIPlanNode step) {
        List<string> result = new List<string>();
        string files = step != null && step.Files != null ? step.Files : "";
        string[] parts = files.Split(",");
        int i = 0;
        while (i < parts.Length) {
            string f = parts[i].Trim();
            if (f != "") {
                result.Add(D4DiffCoverage.Basename(f));
            }
            i = i + 1;
        }
        return result;
    }

    private static bool AnyCovered(List<string> stepFiles, List<string> changed) {
        int i = 0;
        while (i < stepFiles.Count) {
            if (D4DiffCoverage.ListContains(changed, stepFiles[i])) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private static List<string> IntersectProject(List<string> projectFiles, List<string> declared) {
        List<string> result = new List<string>();
        int i = 0;
        while (i < declared.Count) {
            // K6-②：项目文件为全路径、声明为 basename——按 basename 对齐命中。
            if (D4DiffCoverage.ProjectHasFile(projectFiles, declared[i])
                && !D4DiffCoverage.ListContains(result, declared[i])) {
                result.Add(declared[i]);
            }
            i = i + 1;
        }
        return result;
    }

    /// <summary>项目文件中是否存在与指定 basename 同名的源文件（K6-② 对齐比较）。</summary>
    private static bool ProjectHasFile(List<string> projectFiles, string basename) {
        int i = 0;
        while (i < projectFiles.Count) {
            if (D4DiffCoverage.Basename(projectFiles[i]) == basename) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private static bool ListContains(List<string> list, string value) {
        int i = 0;
        while (i < list.Count) {
            if (list[i] == value) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private static string Basename(string path) {
        string p = path != null ? path : "";
        p = p.Replace("\\", "/");
        int last = D4DiffCoverage.LastIndexOf(p, "/");
        if (last < 0) {
            return p;
        }
        return p.Substring(last + 1, p.Length - last - 1);
    }

    private static int LastIndexOf(string s, string sub) {
        int n = s.Length;
        int i = n - sub.Length;
        while (i >= 0) {
            if (s.Substring(i, sub.Length) == sub) {
                return i;
            }
            i = i - 1;
        }
        return -1;
    }

    private static string Join(List<string> items, string sep) {
        string result = "";
        int i = 0;
        while (i < items.Count) {
            if (i > 0) {
                result = result + sep;
            }
            result = result + items[i];
            i = i + 1;
        }
        return result;
    }
}
