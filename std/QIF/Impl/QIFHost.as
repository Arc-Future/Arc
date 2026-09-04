namespace Arc.QIF;

using Arc;
using Arc.Collections;

/// <summary>
/// QIF 测试执行宿主。封装 QIFRunner + 报告输出。对标 XUnit TestAssemblyRunner。
/// Phase 2c: 由合成 __QIFTestHost.Main() 使用；v1.0: 由 arc-test-host.exe 替代。
/// </summary>
public class QIFHost {

    public QIFHost() { Runner = new QIFRunner(); }
    public QIFRunner Runner { get; }

    public void Report() { QIFReporting.WriteReport(Runner); }

    public void ReportFormatted() {
        string format = Runner.Options.OutputFormat;
        if (format == "json") { QIFReporting.WriteJsonReport(Runner); }
        else if (format == "junit") { QIFReporting.WriteJUnitXml(Runner); }
        else { QIFReporting.WriteReport(Runner); }
    }

    public void ReportJson() { QIFReporting.WriteJsonReport(Runner); }
    public void ReportJUnit() { QIFReporting.WriteJUnitXml(Runner); }
}
