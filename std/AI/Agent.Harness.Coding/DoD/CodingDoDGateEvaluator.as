// RFC 043 H-3：Coding 门判定 — 实现 IAIDoDGateEvaluator；D0/D1/D3 经 quality CLI / .arcgr；
// D2/D4 真实自动判定（契约扫描 / diff 覆盖）；未接线门诚实 Pending。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Agent;
using Arc.Agent.Harness;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.IO;
using Arc.Text;

/// <summary>用 quality.* / arc build / arc test / .arcgr 等产生 D0–D7 信号；未接线门返回 Pending（禁假绿）。</summary>
public class CodingDoDGateEvaluator : IAIDoDGateEvaluator {
    // D3 防降级基线：同一项目会话内记录已见过的最大用例数——「测试被改弱」（用例数骤降）判红。
    private string _d3BaselineProject;
    private int _d3BaselineTotal;

    public CodingDoDGateEvaluator() {
        _d3BaselineProject = "";
        _d3BaselineTotal = 0;
    }

    public async Task<AIDoDGateResult> EvaluateAsync(
        AIDoDGateKind gate,
        string project,
        AIRfc rfc,
        CancellationToken cancellationToken) {
        string target = project != null && project != "" ? project : ".";
        if (gate == AIDoDGateKind.D0Compile) {
            return await this.EvaluateD0Async(target, cancellationToken);
        }
        if (gate == AIDoDGateKind.D1Semantics) {
            return await this.EvaluateD1Async(target, cancellationToken);
        }
        if (gate == AIDoDGateKind.D3Behavior) {
            // 信号源 = `arc test --logger json` 用例级明细（非退出码）。防降级基线在会话内保留。
            return await this.EvaluateD3Async(target, rfc, cancellationToken);
        }
        if (gate == AIDoDGateKind.D5SelfReview) {
            return AIDoDGateResult.Human(AIDoDGateKind.D5SelfReview, "self-review proof");
        }
        if (gate == AIDoDGateKind.D7HumanAccept) {
            return AIDoDGateResult.Human(AIDoDGateKind.D7HumanAccept, "collaboration checkpoint");
        }
        if (gate == AIDoDGateKind.D2Contract) {
            return await this.EvaluateD2Async(target, cancellationToken);
        }
        if (gate == AIDoDGateKind.D4DiffCoverage) {
            return await this.EvaluateD4Async(target, rfc, cancellationToken);
        }
        if (gate == AIDoDGateKind.D6AntiPattern) {
            return await this.EvaluateD6Async(target, cancellationToken);
        }
        if (gate == AIDoDGateKind.D9Perf) {
            // P3 D9 性能门：arc build 基线 diff 回归阈值（版本化基线随绿点落盘）。
            return await this.EvaluateD9Async(target, cancellationToken);
        }
        return AIDoDGateResult.Pending(gate, "unknown gate");
    }

    /// <summary>
    /// D6 反模式门：源码级确定性扫描（可机器查、不造假）。
    /// 判红项：占位壳（NotImplemented / NotImplementedException / todo!()）与
    /// TODO/FIXME 注释标记；unreachable 死符号不判红（属正常输入面，见 D6AntiPatternScan）。
    /// B4「声明=行为」追加：疑似死代码（public 符号零引用）+ 宣称待证（宣称符号反向 grep
    /// 无实现）为咨询信号，不改变通过/失败判定，仅经 Detail 回喂人/模型复核。
    /// 数据不足（无 `.as` 源文件 / 路径不可解析）→ Pending，禁空扫 Passed。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD6Async(string projectOrFile, CancellationToken cancellationToken) {
        D6AntiPatternScan scan = await D6AntiPatternScan.ScanAsync(projectOrFile, cancellationToken);
        if (scan.FileCount == 0) {
            return AIDoDGateResult.Pending(
                AIDoDGateKind.D6AntiPattern,
                "antipattern scan: no source files for '" + projectOrFile + "'");
        }
        if (scan.Hits.Count > 0) {
            return AIDoDGateResult.Fail(
                AIDoDGateKind.D6AntiPattern,
                "antipattern scan",
                scan.Describe() + "\n" + scan.Detail());
        }
        AIDoDGateResult result = AIDoDGateResult.Pass(AIDoDGateKind.D6AntiPattern, scan.Describe());
        if (scan.DeadCodeSignals.Count > 0 || scan.ClaimSignals.Count > 0) {
            result.Detail = scan.Advisories();
        }
        return result;
    }

    /// <summary>
    /// D2 契约硬规则门：源码级真实扫描（<see cref="D2ContractScanner"/>），逐项给出
    /// 通过/失败 + 命中样例（file:line）。通过谓词见 <see cref="D2ContractScanner"/>；
    /// 空文件集 → Pending（数据不足，非 Passed）。
    /// </summary>
    public Task<AIDoDGateResult> EvaluateD2Async(string projectOrFile, CancellationToken cancellationToken) {
        List<string> files = D2ContractScanner.CollectAsFiles(projectOrFile);
        if (files.Count == 0) {
            return Task.FromResult(AIDoDGateResult.Pending(AIDoDGateKind.D2Contract,
                "no .as files found for target — contract scan needs source files"));
        }
        D2ScanResult report = D2ContractScanner.ScanFiles(files);
        if (!report.Passed) {
            return Task.FromResult(AIDoDGateResult.Fail(AIDoDGateKind.D2Contract, "contract-scan", report.Describe()));
        }
        return Task.FromResult(AIDoDGateResult.Pass(AIDoDGateKind.D2Contract, "contract-scan " + report.Describe()));
    }

    /// <summary>
    /// D4 diff 覆盖门：比对 AIPlan 步骤文件声明与工作区改动集（git status --porcelain；
    /// git 不可用 → 项目文件清单 ∩ 声明兜底）。通过谓词见 <see cref="D4DiffCoverage"/>；
    /// 无 AIPlan 步骤 → Pending（数据不足，非 Passed）。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD4Async(string project, AIRfc rfc, CancellationToken cancellationToken) {
        AIPlan plan = rfc != null ? rfc.Plan : null;
        if (plan == null || plan.Steps == null || plan.Steps.Count == 0) {
            return AIDoDGateResult.Pending(AIDoDGateKind.D4DiffCoverage,
                "no AIPlan steps attached to AIRfc — diff coverage needs plan step file declarations");
        }
        string dir = CodingDoDGateEvaluator.ResolveProjectDir(project);
        if (dir == "") {
            return AIDoDGateResult.Pending(AIDoDGateKind.D4DiffCoverage,
                "target project directory not found — diff coverage needs a project dir");
        }
        D4DiffEvidence evidence = await D4DiffCoverage.CollectAsync(dir, plan, cancellationToken);
        return await D4DiffCoverage.VerdictAsync(AIDoDGateKind.D4DiffCoverage, plan, evidence);
    }

    /// <summary>
    /// D0 编译门：经 AIPerfMonitor 捕获运行 `arc build <target>`。
    /// 信号源 = 退出码 + stderr 文本（<c>--message-format json</c> 未实现，勿依赖）。
    /// 红时把 stderr 中 `error:`/`error[code]:` 行提取为结构化 <see cref="AIDoDErrorItem"/>
    /// 进 ErrorItems（修复回喂直接消费），Detail 只带折叠摘要（前 N 条 + log 指针）；
    /// 绿时提取告警为 Warn 咨询信号，Detail 只带告警折叠摘要（仅可见不判红）。
    /// 自适应折叠：wall/峰值内存等性能指标留在 PerfSignals，不进 Detail 防噪声。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD0Async(string target, CancellationToken cancellationToken) {
        AIPerfRun perf = await AIPerfMonitor.RunAsync("build " + target, target, cancellationToken);
        string outText = QualityCli.FormatResult(perf.Result);
        AIDoDGateResult result = new AIDoDGateResult();
        if (QualityCli.IsGreen(outText)) {
            result = AIDoDGateResult.Pass(AIDoDGateKind.D0Compile, "arc build");
            // 绿：自适应折叠——Detail 只带告警摘要（数量 + 前 3 条），明细进 Warn 信号面。
            if (perf.Result != null && perf.Result.StandardError != null && perf.Result.StandardError != "") {
                List<string> warnings = QualityCli.ExtractWarningLines(perf.Result.StandardError);
                if (warnings.Count > 0) {
                    int wi = 0;
                    while (wi < warnings.Count) {
                        AIPerfSignal sig = new AIPerfSignal();
                        sig.Level = AISignalLevel.Warn;
                        sig.Source = "CodingDoD";
                        sig.Category = "d0-compile";
                        sig.Line = warnings[wi];
                        sig.KeySignal = "d0-warning";
                        result.PerfSignals.Add(sig);
                        wi = wi + 1;
                    }
                    result.Detail = QualityCli.FoldWarnings(warnings, 3);
                }
            }
        } else {
            // 红：结构化错误提取 → ErrorItems（完整明细供修复）；Detail 折叠摘要（前 5 条）。
            // 提取不到错误行（如链接阶段失败）时诚实回退原始折叠文本，不吞信息。
            List<AIDoDErrorItem> errors = QualityCli.ExtractErrorItems(
                perf.Result != null ? perf.Result.StandardError : "");
            if (errors.Count == 0) {
                errors = QualityCli.ExtractErrorItems(outText);
            }
            string folded = QualityCli.FoldErrors(errors, 5);
            if (folded == "") {
                folded = outText;
            }
            result = AIDoDGateResult.Fail(AIDoDGateKind.D0Compile, "arc build", folded);
            int ei = 0;
            while (ei < errors.Count) {
                result.ErrorItems.Add(errors[ei]);
                ei = ei + 1;
            }
        }
        // 汇集 AIPerfMonitor 信号（wall/peak/log/exit）到门结果（信号面保持，供日志消费）。
        if (perf.Signals != null) {
            int pn = 0;
            while (pn < perf.Signals.Count) {
                result.PerfSignals.Add(perf.Signals[pn]);
                pn = pn + 1;
            }
        }
        // 自适应折叠：性能指标不进 Detail，仅保留日志路径指针（明细在 PerfSignals/日志文件）。
        result.Detail = AIPerfMonitor.AttachLogPointer(result.Detail, perf);
        return result;
    }

    /// <summary>
    /// D3 行为验证门：解析 `arc test --logger json` 用例级明细（非退出码）。
    /// 通过谓词见 <see cref="D3TestReport.PassedPredicate"/>（passed > 0 且 failed/errors == 0）；
    /// 结构化 Acceptance 条目声明的 TestName（验收对照）须在结果中真实 passed；
    /// 用例数骤降（相对会话内基线）标疑判红——防「测试被改弱」（防降级）。
    /// 无 JSON 报告（无测试用例）→ Pending（数据不足，禁空跑 Passed）。
    /// 经 AIPerfMonitor 捕获运行；判定逻辑不变；Detail 附 wall/峰值内存 + 日志路径。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD3Async(string target, AIRfc rfc, CancellationToken cancellationToken) {
        AIPerfRun perf = await AIPerfMonitor.RunAsync(
            "test " + target + " --logger json", target, cancellationToken);
        if (perf.SpawnFailed || perf.Result == null) {
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Fail(
                    AIDoDGateKind.D3Behavior,
                    "arc test",
                    "arc test failed to start: " + perf.SpawnError),
                perf);
        }
        ProcessRunResult pr = perf.Result;
        D3TestReport report = D3TestReport.Parse(pr.ExitCode, pr.StandardOutput);
        if (!report.JsonOk) {
            if (pr.ExitCode != 0) {
                return CodingDoDGateEvaluator.WithPerf(
                    AIDoDGateResult.Fail(
                        AIDoDGateKind.D3Behavior,
                        "arc test",
                        report.Describe() + "\n" + QualityCli.FormatResult(pr)),
                    perf);
            }
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Pending(
                    AIDoDGateKind.D3Behavior,
                    "arc test: no test-case JSON report for '" + target + "' — behavior needs test cases"),
                perf);
        }
        // 防降级：用例数相对会话内基线骤降（如从 N 降到 0）→ 标疑判红（改测须 AIRfc 纠偏）。
        if (_d3BaselineProject == target && _d3BaselineTotal > 0 && report.Total < _d3BaselineTotal) {
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Fail(
                    AIDoDGateKind.D3Behavior,
                    "arc test",
                    "test count reduced from " + _d3BaselineTotal + " to " + report.Total
                        + " (suspect weakening — 防降级；改测 = AIRfc 纠偏)\n" + report.Describe()),
                perf);
        }
        if (!report.PassedPredicate) {
            string detail = report.Describe() + "\n" + report.FailedDetail();
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Fail(AIDoDGateKind.D3Behavior, "arc test", detail),
                perf);
        }
        // 验收对照：结构化 Acceptance 条目声明的测试名须真实 passed。
        if (rfc != null && rfc.Acceptance != null && rfc.Acceptance.HasStructuredItems) {
            int i = 0;
            int n = rfc.Acceptance.Items.Count;
            while (i < n) {
                AIAcceptanceItem item = rfc.Acceptance.Items[i];
                if (item.TestName != null && item.TestName != "" && !report.ContainsPassedTest(item.TestName)) {
                    return CodingDoDGateEvaluator.WithPerf(
                        AIDoDGateResult.Fail(
                            AIDoDGateKind.D3Behavior,
                            "arc test",
                            "acceptance item references test '" + item.TestName + "' but it did not pass in "
                                + report.Describe()),
                        perf);
                }
                i = i + 1;
            }
        }
        // 更新防降级基线（仅记录已见过的最大用例数）。
        _d3BaselineProject = target;
        if (report.Total > _d3BaselineTotal) {
            _d3BaselineTotal = report.Total;
        }
        return CodingDoDGateEvaluator.WithPerf(
            AIDoDGateResult.Pass(
                AIDoDGateKind.D3Behavior,
                "arc test " + report.Describe()),
            perf);
    }

    /// <summary>
    /// D9 性能门（P3）：跑 `arc build <target>` 采集墙钟/峰值内存 → 版本化基线 diff 回归
    /// 阈值（<see cref="D9PerfEvaluator.Compare"/>）。无基线 → 建立首编译基线（Passed）；
    /// 超硬阈值 → Failed；超软阈值 → Passed 附 warning（软回归不判红）。基线随绿点落盘
    /// （<see cref="AIPerfBaselineStore"/>）。判定逻辑经 <see cref="AIPerfMonitor"/> 采集。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD9Async(string target, CancellationToken cancellationToken) {
        AIPerfRun perf = await AIPerfMonitor.RunAsync("build " + target, target, cancellationToken);
        if (perf.SpawnFailed || perf.Result == null) {
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Fail(
                    AIDoDGateKind.D9Perf,
                    "arc bench",
                    "arc build failed to start: " + perf.SpawnError),
                perf);
        }
        long wallMs = perf.ElapsedMs;
        long peakMem = 0;
        ProcessRunStats? stats = perf.Result.Stats;
        if (stats != null) {
            peakMem = stats.PeakMemoryBytes;
        }
        AIPerfBaselineStore store = new AIPerfBaselineStore();
        await store.LoadAsync(target, cancellationToken);
        AIPerfBaseline? incremental = store.Find("D9-compile", AIPerfBaselineKind.Incremental);
        AIPerfBaseline? first = store.Find("D9-compile", AIPerfBaselineKind.FirstCompile);
        AIPerfBaseline? baseline = incremental != null ? incremental : first;
        if (baseline == null) {
            store.Record("D9-compile", AIPerfBaselineKind.FirstCompile, wallMs, peakMem);
            await store.SaveAsync(target, cancellationToken);
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Pass(
                    AIDoDGateKind.D9Perf,
                    "arc bench: baseline established (first compile) wall=" + wallMs + "ms mem=" + peakMem + "B"),
                perf);
        }
        D9PerfComparison cmp = D9PerfEvaluator.Compare(
            baseline.WallMs, baseline.PeakMemoryBytes, wallMs, peakMem, D9PerfThresholds.Default);
        store.Record("D9-compile", AIPerfBaselineKind.Incremental, wallMs, peakMem);
        await store.SaveAsync(target, cancellationToken);
        if (cmp.Verdict == D9PerfVerdict.Failed) {
            return CodingDoDGateEvaluator.WithPerf(
                AIDoDGateResult.Fail(AIDoDGateKind.D9Perf, "arc bench", cmp.Detail),
                perf);
        }
        string signal = "arc bench " + cmp.Detail;
        if (cmp.Verdict == D9PerfVerdict.Warning) {
            signal = "arc bench (warning) " + cmp.Detail;
        }
        return CodingDoDGateEvaluator.WithPerf(
            AIDoDGateResult.Pass(AIDoDGateKind.D9Perf, signal),
            perf);
    }

    /// <summary>把 AIPerfRun 的 PerfSignals 挂到门结果并附加日志路径指针（自适应折叠：指标留信号面）。</summary>
    private static AIDoDGateResult WithPerf(AIDoDGateResult result, AIPerfRun perf) {
        result.PerfSignals = perf.Signals;
        result.Detail = AIPerfMonitor.AttachLogPointer(result.Detail, perf);
        return result;
    }

    /// <summary>解析目标目录：目录直用；文件用其所在目录；均不存在 → ""。</summary>
    private static string ResolveProjectDir(string projectOrFile) {
        string target = projectOrFile != null && projectOrFile != "" ? projectOrFile : ".";
        if (Directory.Exists(target)) {
            return target;
        }
        if (File.Exists(target)) {
            string dir = Path.GetDirectoryName(target);
            if (dir != null && dir != "") {
                return dir;
            }
        }
        return "";
    }

    /// <summary>
    /// D1 语义完整性门：基于 `.arcgr` 的引用图完整性与可达性判定。
    /// 通过谓词见 <see cref="D1ArcgrInspect"/>（exit 0 + symbol 表 + 引用图可导出 +
    /// 无引用断裂 + 无不可达入口 + 入口 explain 可达性一致）。
    /// </summary>
    public async Task<AIDoDGateResult> EvaluateD1Async(string projectOrFile, CancellationToken cancellationToken) {
        string entry = CodingDoDGateEvaluator.ResolveEntryFile(projectOrFile);
        if (entry == "") {
            return AIDoDGateResult.Fail(
                AIDoDGateKind.D1Semantics,
                "arcgr",
                "no .as entry file found for '" + (projectOrFile != null ? projectOrFile : "") + "'");
        }
        string arcgrPath = Path.Combine(
            Path.GetTempPath(),
            "arc-d1-" + Process.GetCurrentProcessId() + "-" + Stopwatch.GetTimestamp() + ".arcgr");
        // 1) 源码模式 inspect：一次调用同时产出 JSON 判定输入 + `--emit` 落盘 .arcgr 供 explain。
        ProcessRunResult inspect = await QualityCli.RunArcResultAsync(
            "inspect " + CodingDoDGateEvaluator.Quote(entry)
                + " --format json --emit " + CodingDoDGateEvaluator.Quote(arcgrPath),
            cancellationToken);
        D1ArcgrInspect report = D1ArcgrInspect.Parse(inspect.ExitCode, inspect.StandardOutput);
        if (!report.Passed) {
            File.Delete(arcgrPath);
            return AIDoDGateResult.Fail(AIDoDGateKind.D1Semantics, "arcgr", report.Describe());
        }
        // 2) 入口符号 explain 可达性探测（≤3 个入口；is_reachable 必须为真——入口不可达 = 可达性崩溃）。
        List<string> probes = report.EntryPointNames(3);
        string evidence = report.Describe();
        int pi = 0;
        while (pi < probes.Count) {
            ProcessRunResult exp = await QualityCli.RunArcResultAsync(
                "explain " + CodingDoDGateEvaluator.Quote(arcgrPath)
                    + " " + CodingDoDGateEvaluator.Quote(probes[pi]) + " --format json",
                cancellationToken);
            if (exp.ExitCode != 0 || !D1ArcgrInspect.ExplainIsReachable(exp.StandardOutput)) {
                File.Delete(arcgrPath);
                return AIDoDGateResult.Fail(
                    AIDoDGateKind.D1Semantics,
                    "arcgr",
                    "entry-point reachability probe failed for '" + probes[pi]
                        + "' (exit=" + exp.ExitCode + "): " + report.Describe());
            }
            evidence = evidence + " | " + probes[pi] + " reachable";
            pi = pi + 1;
        }
        File.Delete(arcgrPath);
        return AIDoDGateResult.Pass(AIDoDGateKind.D1Semantics, "arcgr " + evidence);
    }

    /// <summary>解析入口 `.as` 文件：文件直用；目录优先 `Program.as`（既有单文件入口约定），兜底取目录内首个 `.as`。</summary>
    private static string ResolveEntryFile(string projectOrFile) {
        string target = projectOrFile != null && projectOrFile != "" ? projectOrFile : ".";
        if (File.Exists(target)) {
            return target;
        }
        if (Directory.Exists(target)) {
            string program = Path.Combine(target, "Program.as");
            if (File.Exists(program)) {
                return program;
            }
            string[] files = Directory.GetFiles(target, "*.as");
            if (files != null && files.Length > 0) {
                return files[0];
            }
        }
        return "";
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
