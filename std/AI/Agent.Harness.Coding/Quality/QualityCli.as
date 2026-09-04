// RFC 043 H-2b：quality.* CLI 调用 — Coding 领域；经 Process.RunCaptureAsync 调 arc。
// 只读验证面：不进 plan gate；能力键 quality.Verify。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Agent.Harness;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.Text;

/// <summary>跨平台调用 arc CLI（PATH 上的 arc 可执行文件）。</summary>
public static class QualityCli {
    /// <summary>quality.Verify 能力键（只读验证；不进 PlanGatedCapabilities）。</summary>
    public static string CapabilityName() {
        return "quality.Verify";
    }

    /// <summary>执行 `arc <args>` 并捕获输出；返回 exit/stdout/stderr 折叠文本。</summary>
    public static async Task<string> RunArcAsync(string args, CancellationToken cancellationToken) {
        // 同类静态方法须带类名前缀调用：裸调用在同文件解析路径下不归一化
        // `Task<T>` 返回类型，await 时报「expected Task<T>, found Task_XXX」。
        ProcessRunResult r = await QualityCli.RunArcResultAsync(args, cancellationToken);
        return FormatResult(r);
    }

    /// <summary>
    /// 执行 `arc <args>` 并返回结构化结果（D1 等机器判定需要 stdout 原文，不能只靠折叠文本）。
    /// 编译器路径：环境变量 <c>ARC_COMPILER</c> 优先（对齐 <c>AINativeCodeActProvider</c>，
    /// e2e 注入真实 arc 二进制），否则走 PATH 上的 <c>arc</c>。
    /// </summary>
    public static async Task<ProcessRunResult> RunArcResultAsync(string args, CancellationToken cancellationToken) {
        if (args == null || args == "") {
            throw new ArgumentException("args is empty");
        }
        ProcessStartInfo si = new ProcessStartInfo();
        string compiler = Environment.GetEnvironmentVariable("ARC_COMPILER");
        if (compiler != null && compiler != "") {
            si.FileName = compiler;
            si.Arguments = args;
        } else if (Environment.IsWindows()) {
            si.FileName = "cmd.exe";
            si.Arguments = "/c arc " + args;
        } else {
            si.FileName = "/bin/sh";
            si.Arguments = "-c arc " + args;
        }
        return await Process.RunCaptureAsync(si, cancellationToken);
    }

    /// <summary>把 ProcessRunResult 折叠为工具返回文本（模型可解析）。可空入参：null → "exit=-1 (null result)"。</summary>
    public static string FormatResult(ProcessRunResult? r) {
        if (r == null) {
            return "exit=-1\n(null result)";
        }
        string output = "exit=" + r.ExitCode;
        if (r.StandardOutput != null && r.StandardOutput != "") {
            output = output + "\nSTDOUT:\n" + r.StandardOutput;
        }
        if (r.StandardError != null && r.StandardError != "") {
            output = output + "\nSTDERR:\n" + r.StandardError;
        }
        return output;
    }

    /// <summary>exit=0 视为绿。</summary>
    public static bool IsGreen(string formatted) {
        if (formatted == null) {
            return false;
        }
        return formatted.StartsWith("exit=0");
    }

    /// <summary>
    /// 从进程 stderr 文本逐行提取告警行（trim 后判定；编译器输出格式）。
    /// 匹配两种前缀：`warning:`（clang/resx 通道）与 `warning[<code>]:`
    /// （Arc 标准诊断通道，如 `warning[arc-cycle-001]:`）。
    /// 无告警 → 空列表。结构化诊断（SR-2）落地前的诚实启发式，供 D0 门绿时回喂。
    /// </summary>
    public static List<string> ExtractWarningLines(string stderrText) {
        List<string> warnings = new List<string>();
        if (stderrText == null || stderrText == "") {
            return warnings;
        }
        string remaining = stderrText;
        while (remaining != "") {
            int nl = remaining.IndexOf("\n");
            string line = "";
            if (nl >= 0) {
                line = remaining.Substring(0, nl);
                remaining = remaining.Substring(nl + 1);
            } else {
                line = remaining;
                remaining = "";
            }
            string trimmed = line.Trim();
            if (trimmed.StartsWith("warning:") || trimmed.StartsWith("warning[")) {
                warnings.Add(trimmed);
            }
        }
        return warnings;
    }

    /// <summary>
    /// 从进程 stderr 文本提取结构化错误条目（SR-2 前的诚实启发式）。
    /// 匹配三种来源：`error[<code>]:`（Arc 标准诊断通道）、`error:`（顶层/typeck 折叠头）、
    /// clang `path:line:col: error:`（带位置，Windows 盘符路径从右往左切 line/col）。
    /// 无错误 → 空列表。
    /// </summary>
    public static List<AIDoDErrorItem> ExtractErrorItems(string stderrText) {
        List<AIDoDErrorItem> errors = new List<AIDoDErrorItem>();
        if (stderrText == null || stderrText == "") {
            return errors;
        }
        string[] lines = stderrText.Split("\n");
        int i = 0;
        while (i < lines.Length) {
            string trimmed = lines[i].Trim();
            AIDoDErrorItem item = new AIDoDErrorItem();
            if (trimmed.StartsWith("error[")) {
                int close = trimmed.IndexOf("]");
                if (close > 6) {
                    item.Code = trimmed.Substring(6, close - 6);
                    item.Message = QualityCli.TrimLeadingColon(trimmed.Substring(close + 1));
                } else {
                    item.Message = trimmed;
                }
            } else if (trimmed.StartsWith("error:")) {
                item.Message = QualityCli.TrimLeadingColon(trimmed.Substring(6));
            } else {
                QualityCli.TryParseClangError(trimmed, item);
            }
            if (item.Message != "") {
                errors.Add(item);
            }
            i = i + 1;
        }
        return errors;
    }

    /// <summary>告警折叠摘要：数量 + 前 N 条（自适应折叠：绿门 Detail 只带摘要，明细留在信号面）。</summary>
    public static string FoldWarnings(List<string> warnings, int maxLines) {
        if (warnings == null || warnings.Count == 0) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        sb.Append(warnings.Count + " warnings:");
        int shown = warnings.Count < maxLines ? warnings.Count : maxLines;
        int i = 0;
        while (i < shown) {
            sb.Append("\n  " + warnings[i]);
            i = i + 1;
        }
        if (warnings.Count > shown) {
            sb.Append("\n  (+" + (warnings.Count - shown) + " more)");
        }
        return sb.ToString();
    }

    /// <summary>错误折叠摘要：数量 + 前 N 条 Format（自适应折叠：失败门 Detail 只带摘要，明细在 ErrorItems）。</summary>
    public static string FoldErrors(List<AIDoDErrorItem> errors, int maxLines) {
        if (errors == null || errors.Count == 0) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        sb.Append(errors.Count + " errors:");
        int shown = errors.Count < maxLines ? errors.Count : maxLines;
        int i = 0;
        while (i < shown) {
            sb.Append("\n  - " + errors[i].Format());
            i = i + 1;
        }
        if (errors.Count > shown) {
            sb.Append("\n  (+" + (errors.Count - shown) + " more)");
        }
        return sb.ToString();
    }

    /// <summary>解析 clang 风格 `path:line:col: error: message`；不匹配则不填充（返回空消息）。</summary>
    private static void TryParseClangError(string line, AIDoDErrorItem item) {
        int marker = line.IndexOf(": error");
        if (marker <= 0) {
            return;
        }
        string head = line.Substring(0, marker);
        int p1 = head.LastIndexOf(":");
        if (p1 <= 0) {
            return;
        }
        string colPart = head.Substring(p1 + 1);
        string rest = head.Substring(0, p1);
        int p2 = rest.LastIndexOf(":");
        if (p2 <= 0) {
            return;
        }
        string linePart = rest.Substring(p2 + 1);
        if (!QualityCli.IsAllDigits(colPart) || !QualityCli.IsAllDigits(linePart)) {
            return;
        }
        item.File = rest.Substring(0, p2);
        item.Line = Convert.ToInt32(linePart);
        item.Col = Convert.ToInt32(colPart);
        item.Message = QualityCli.TrimLeadingColon(line.Substring(marker + 7));
    }

    /// <summary>去除前导 `:` 与空白（`error[code]: msg` / `: msg` 归一化）。</summary>
    private static string TrimLeadingColon(string s) {
        string t = s != null ? s.Trim() : "";
        while (t.StartsWith(":")) {
            t = t.Substring(1).Trim();
        }
        return t;
    }

    private static bool IsAllDigits(string s) {
        if (s == null || s == "") {
            return false;
        }
        int i = 0;
        while (i < s.Length) {
            char c = s[i];
            if (c < '0' || c > '9') {
                return false;
            }
            i = i + 1;
        }
        return true;
    }
}
