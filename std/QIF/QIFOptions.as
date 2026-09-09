namespace Arc.QIF;

/// <summary>
/// QIF 执行配置。对标 XUnit TestAssemblyRunnerContext / XunitFilters。
/// 配置来源：arc.toml [qif] 段 / 命令行参数 / 环境变量。
/// </summary>
internal class QIFOptions {
    public QIFOptions() { }

    public int MaxParallel { get; set; } = 1;
    /// <summary>默认单测超时毫秒；0 = 不限制（与 RFC 032 / CLI <c>--timeout</c> 默认对齐）。</summary>
    public int DefaultTimeoutMs { get; set; } = 0;
    public bool StopOnFail { get; set; }
    /// <summary>
    /// Assert.Skip（QIF_SKIP）计 Skipped 时是否非零退出。默认 false。
    /// 属性 Fact-Skip 不经此开关——宿主恒硬失败（RFC 032 §6）。
    /// </summary>
    public bool FailOnSkip { get; set; }
    public string OutputFormat { get; set; } = "human";
    public string Filter { get; set; } = "";
    public bool Diagnostics { get; set; }
    public bool IsParallel { get { return MaxParallel > 1; } }
    public bool IsJsonOutput { get { return OutputFormat == "json"; } }
    public bool IsJUnitOutput { get { return OutputFormat == "junit"; } }
}
