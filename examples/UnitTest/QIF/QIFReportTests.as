namespace UnitTest.QIF;

using Arc;
using Arc.QIF;

/// <summary>
/// QIF 报告生产特性自测：JSON 转义、长时长不溢出、summary 元数据、.arcqif schema。
/// 通过公开面 QIFReporting.BuildJsonReport / BuildArcqif 直接断言报告文本，
/// 保证 CI 可解析（report.json / run.arcqif）。
/// </summary>
public class QIFReportTests
{
    // JSON 转义：错误信息含真实换行必须输出为 \n（2 字符），否则报告非法。
    [Fact]
    public void JsonReport_EscapesNewlineInError()
    {
        QIFRunner runner = new QIFRunner();
        runner.RecordFail("fail_name", QIFTestKind.Fact, 1000, "boom\nline2");
        string json = QIFReporting.BuildJsonReport(runner);
        Assert.True(json.Contains("boom\\nline2"), "newline must be escaped to backslash-n");
    }

    // 长时长不溢出：3_000_000_000 ns（3s）超过 int.MaxValue，须以 long 原值输出。
    [Fact]
    public void JsonReport_LongDuration_NoOverflow()
    {
        QIFRunner runner = new QIFRunner();
        long big = 3000000000;
        runner.RecordPass("slow_test", QIFTestKind.Fact, big);
        string json = QIFReporting.BuildJsonReport(runner);
        Assert.True(json.Contains("3000000000"), "duration must not truncate to int");
    }

    // summary 含全量 wall-clock 耗时字段（性能可见性数据源）。
    [Fact]
    public void JsonReport_Summary_HasWallDurationMs()
    {
        QIFRunner runner = new QIFRunner();
        runner.RecordSkip("skipped_test", QIFTestKind.Fact, "not on windows");
        string json = QIFReporting.BuildJsonReport(runner);
        Assert.True(json.Contains("\"duration_ms\""), "summary must include duration_ms");
    }

    // .arcqif 持久化格式声明 schema 元数据。
    [Fact]
    public void Arcqif_Report_HasSchema()
    {
        QIFRunner runner = new QIFRunner();
        runner.RecordPass("ok", QIFTestKind.Fact, 50);
        string arcqif = QIFReporting.BuildArcqif(runner);
        Assert.True(arcqif.Contains("\"schema\": \"arc.qif/run\""), "arcqif must declare schema");
    }
}