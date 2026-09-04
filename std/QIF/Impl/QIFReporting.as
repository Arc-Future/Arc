namespace Arc.QIF;

using Arc;
using Arc.IO;
using Arc.Text;
using Arc.Threading;

/// <summary>
/// QIF 测试结果输出器。Human / JSON（RFC 032 M1）/ JUnit XML 三种格式。
/// </summary>
public static class QIFReporting {

    // TEMP-TRACE (root-cause): crash-at-exit 定位用。写完即删。
    private static void TraceW(string line) {
        File.WriteAllText("d:/GitCode/RF/dlang/examples/UnitTest/obj/qif/trace.txt", line);
    }

    private static string StatusLabel(QIFTestStatus s) {
        if (s == QIFTestStatus.Pass) { return "[PASS]"; }
        if (s == QIFTestStatus.Fail) { return "[FAIL]"; }
        if (s == QIFTestStatus.Skip) { return "[SKIP]"; }
        if (s == QIFTestStatus.Error) { return "[ERROR]"; }
        return "[UNKNOWN]";
    }

    private static string KindLabel(QIFTestKind k) {
        if (k == QIFTestKind.Fact) { return "Fact"; }
        if (k == QIFTestKind.Theory) { return "Theory"; }
        if (k == QIFTestKind.Integration) { return "Integration"; }
        if (k == QIFTestKind.E2e) { return "E2E"; }
        if (k == QIFTestKind.Benchmark) { return "Benchmark"; }
        if (k == QIFTestKind.Property) { return "Property"; }
        if (k == QIFTestKind.Snapshot) { return "Snapshot"; }
        if (k == QIFTestKind.Contract) { return "Contract"; }
        return "Unknown";
    }

    private static string StatusJson(QIFTestStatus s) {
        if (s == QIFTestStatus.Pass) { return "passed"; }
        if (s == QIFTestStatus.Fail) { return "failed"; }
        if (s == QIFTestStatus.Skip) { return "skipped"; }
        if (s == QIFTestStatus.Error) { return "error"; }
        return "unknown";
    }

    /// <summary>JSON 字符串转义（`"`、`\`、换行、回车、制表）。保证 report.json 合法可被 CI 解析。</summary>
    private static string EscapeJson(string s) {
        if (s == "") { return ""; }
        StringBuilder sb = new StringBuilder(s);
        sb.Replace("\\", "\\\\");
        sb.Replace("\"", "\\\"");
        sb.Replace("\n", "\\n");
        sb.Replace("\r", "\\r");
        sb.Replace("\t", "\\t");
        return sb.ToString();
    }

    /// <summary>XML 文本/属性转义（`&` 优先，`&lt;`/`&gt;`/`&quot;`/`&apos;`）。保证 JUnit XML 合法。</summary>
    private static string EscapeXml(string s) {
        if (s == "") { return ""; }
        StringBuilder sb = new StringBuilder(s);
        sb.Replace("&", "&amp;");
        sb.Replace("\"", "&quot;");
        sb.Replace("'", "&apos;");
        sb.Replace("<", "&lt;");
        sb.Replace(">", "&gt;");
        return sb.ToString();
    }

    public static void WriteSummary(int total, int passed, int failed, int skipped) {
        Console.WriteLine("");
        Console.WriteLine("Tests completed.");
        Console.WriteLine("Total: " + total.ToString());
        Console.WriteLine("Passed: " + passed.ToString());
        Console.WriteLine("Failed: " + failed.ToString());
        Console.WriteLine("Skipped: " + skipped.ToString());
        if (failed == 0 && skipped == 0 && total > 0) {
            Console.WriteLine("All tests passed");
        }
    }

    /// <summary>
    /// 分段 Write。先完成主行再读可选字段，缩短末条截断窗口。
    /// Duration：恒打 &lt;1ms（避免 ms.ToString 堆分配踩已损堆）；
    /// Pass 不读 Output（减少对可能已损字段指针的 str_equals）。
    /// </summary>
    private static void WriteOneResult(QIFRunner runner, int index) {
        QIFReporting.TraceW("i=" + index.ToString() + " getresult\n");
        QIFResult result = runner.GetResult(index);
        QIFReporting.TraceW("i=" + index.ToString() + " name=" + result.Name + "\n");
        Console.Write(QIFReporting.StatusLabel(result.Status));
        Console.Write(" ");
        Console.Write(result.Name);
        Console.Write(" (");
        Console.Write(QIFReporting.KindLabel(result.Kind));
        Console.Write(") ");
        // 先结束主行，再碰 Fail/Skip 字段（H1 抗震）。
        Console.Write(result.DurationMs);
        Console.WriteLine("");
        if (result.Status == QIFTestStatus.Fail || result.Status == QIFTestStatus.Error) {
            Console.Write("    Error: ");
            Console.WriteLine(result.ErrorMessage);
            if (result.StackTrace != "") {
                Console.Write("    StackTrace: ");
                Console.WriteLine(result.StackTrace);
            }
            if (result.Output != "") {
                Console.Write("    Output: ");
                Console.WriteLine(result.Output);
            }
        }
        if (result.Status == QIFTestStatus.Skip) {
            Console.Write("    Skip Reason: ");
            Console.WriteLine(result.SkipReason);
        }
    }

    public static void WriteResults(QIFRunner runner) {
        Console.WriteLine("");
        Console.WriteLine("Test Results:");
        Console.WriteLine("-------------");
        int i = 0; int total = runner.Total;
        while (i < total) {
            QIFReporting.WriteOneResult(runner, i);
            i = i + 1;
        }
    }

    public static void WriteReport(QIFRunner runner) {
        // H1: 先 ShutdownDefaultPool（join 默认池 + join_live Thread），
        // 再 WriteResults——禁跳过逐条输出粉饰堆损伤。
        ThreadPoolScheduler.ShutdownDefaultPool();
        QIFReporting.TraceW("after shutdown total=" + runner.Total.ToString() + "\n");
        QIFReporting.WriteResults(runner);
        QIFReporting.TraceW("after results\n");
        QIFReporting.WriteSummary(runner.Total, runner.Passed, runner.Failed, runner.Skipped);
        QIFReporting.TraceW("after summary\n");
    }

    /// <summary>构建 JSON 报告串（RFC 032 §7：console / `report.json` 单源）。</summary>
    public static string BuildJsonReport(QIFRunner runner) {
        StringBuilder sb = new StringBuilder();
        sb.Append("{\n");
        sb.Append("  \"summary\": {\n");
        sb.Append("    \"total\": " + runner.Total.ToString() + ",\n");
        sb.Append("    \"passed\": " + runner.Passed.ToString() + ",\n");
        sb.Append("    \"failed\": " + runner.Failed.ToString() + ",\n");
        sb.Append("    \"skipped\": " + runner.Skipped.ToString() + ",\n");
        sb.Append("    \"errors\": " + runner.Errors.ToString() + ",\n");
        sb.Append("    \"duration_ms\": " + runner.TotalDurationMs.ToString() + "\n");
        sb.Append("  },\n");
        QIFReporting.AppendJsonResults(sb, runner);
        sb.Append("}\n");
        return sb.ToString();
    }

    public static void WriteJsonReport(QIFRunner runner) {
        Console.Write(QIFReporting.BuildJsonReport(runner));
    }

    /// <summary>结果数组 JSON（两报告共享，单源；不含堆损伤期的可选字段读取风险）。</summary>
    private static void AppendJsonResults(StringBuilder sb, QIFRunner runner) {
        sb.Append("  \"results\": [\n");
        int i = 0; int total = runner.Total;
        while (i < total) {
            QIFResult result = runner.GetResult(i);
            bool isLast = (i == total - 1);
            sb.Append("    {\n");
            sb.Append("      \"name\": \"" + QIFReporting.EscapeJson(result.Name) + "\",\n");
            sb.Append("      \"kind\": \"" + QIFReporting.KindLabel(result.Kind) + "\",\n");
            sb.Append("      \"status\": \"" + QIFReporting.StatusJson(result.Status) + "\",\n");
            if (result.ErrorMessage != "") {
                sb.Append("      \"error\": \"" + QIFReporting.EscapeJson(result.ErrorMessage) + "\",\n");
            }
            if (result.Output != "") {
                sb.Append("      \"output\": \"" + QIFReporting.EscapeJson(result.Output) + "\",\n");
            }
            if (result.SkipReason != "") {
                sb.Append("      \"skip_reason\": \"" + QIFReporting.EscapeJson(result.SkipReason) + "\",\n");
            }
            sb.Append("      \"duration_ns\": " + result.DurationNs.ToString());
            if (isLast) { sb.Append("\n    }\n"); }
            else { sb.Append("\n    },\n"); }
            i = i + 1;
        }
        sb.Append("  ]\n");
    }

    /// <summary>构建 `.arcqif` 运行持久化串（RFC 032 §7）：schema 元数据 + summary + 结果。</summary>
    public static string BuildArcqif(QIFRunner runner) {
        StringBuilder sb = new StringBuilder();
        sb.Append("{\n");
        sb.Append("  \"schema\": \"arc.qif/run\",\n");
        sb.Append("  \"version\": 1,\n");
        sb.Append("  \"summary\": {\n");
        sb.Append("    \"total\": " + runner.Total.ToString() + ",\n");
        sb.Append("    \"passed\": " + runner.Passed.ToString() + ",\n");
        sb.Append("    \"failed\": " + runner.Failed.ToString() + ",\n");
        sb.Append("    \"skipped\": " + runner.Skipped.ToString() + ",\n");
        sb.Append("    \"errors\": " + runner.Errors.ToString() + ",\n");
        sb.Append("    \"duration_ms\": " + runner.TotalDurationMs.ToString() + "\n");
        sb.Append("  },\n");
        QIFReporting.AppendJsonResults(sb, runner);
        sb.Append("}\n");
        return sb.ToString();
    }

    /// <summary>RFC 032 §7：报告产物落盘。`persist` 写 `run.arcqif`，`emitJson` 写 `report.json`。
    /// 目录由 CLI `arc test` 预建（`File.WriteAllText` 不建父目录）；落盘在 host
    /// `Environment.Exit` 前执行，与控制台输出互不抢占。</summary>
    public static void PersistArtifacts(QIFRunner runner, string outputDir, bool emitJson, bool persist) {
        if (persist) {
            File.WriteAllText(outputDir + "/run.arcqif", QIFReporting.BuildArcqif(runner));
        }
        if (emitJson) {
            File.WriteAllText(outputDir + "/report.json", QIFReporting.BuildJsonReport(runner));
        }
    }

    public static void WriteJUnitXml(QIFRunner runner) {
        Console.WriteLine("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        Console.WriteLine("<testsuites>");
        Console.WriteLine("  <testsuite name=\"QIF Tests\" tests=\"" + runner.Total.ToString() + "\" passed=\"" + runner.Passed.ToString() + "\" failures=\"" + runner.Failed.ToString() + "\" errors=\"" + runner.Errors.ToString() + "\" skipped=\"" + runner.Skipped.ToString() + "\" time=\"0\">");
        int i = 0; int total = runner.Total;
        while (i < total) {
            QIFResult result = runner.GetResult(i);
            string xmlStatus = "";
            if (result.Status == QIFTestStatus.Fail) { xmlStatus = "    <failure message=\"" + QIFReporting.EscapeXml(result.ErrorMessage) + "\" />"; }
            else if (result.Status == QIFTestStatus.Error) { xmlStatus = "    <error message=\"" + QIFReporting.EscapeXml(result.ErrorMessage) + "\" />"; }
            else if (result.Status == QIFTestStatus.Skip) { xmlStatus = "    <skipped message=\"" + QIFReporting.EscapeXml(result.SkipReason) + "\" />"; }
            Console.WriteLine("    <testcase name=\"" + QIFReporting.EscapeXml(result.Name) + "\" classname=\"QIF\" time=\"" + (result.DurationNs / 1000000000.0).ToString() + "\">");
            if (xmlStatus != "") { Console.WriteLine(xmlStatus); }
            Console.WriteLine("    </testcase>");
            i = i + 1;
        }
        Console.WriteLine("  </testsuite>");
        Console.WriteLine("</testsuites>");
    }
}
