namespace Arc.QIF;

/// <summary>
/// QIF 执行配置。对标 XUnit TestAssemblyRunnerContext / XunitFilters。
/// 配置来源：arc.toml [qif] 段 / 命令行参数 / 环境变量。
/// </summary>
internal class QIFOptions {
    public QIFOptions() { }

    public int MaxParallel { get; set; } = 1;
    public int DefaultTimeoutMs { get; set; } = 30000;
    public bool StopOnFail { get; set; }
    public bool FailOnSkip { get; set; }
    public string OutputFormat { get; set; } = "human";
    public string Filter { get; set; } = "";
    public bool Diagnostics { get; set; }
    public bool IsParallel { get { return MaxParallel > 1; } }
    public bool IsJsonOutput { get { return OutputFormat == "json"; } }
    public bool IsJUnitOutput { get { return OutputFormat == "junit"; } }
}
