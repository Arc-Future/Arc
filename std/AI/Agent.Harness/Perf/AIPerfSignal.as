// Arc.Agent.Harness.AIPerfSignal — 单条信号日志条目（RFC 043 P1）。
namespace Arc.Agent.Harness;

/// <summary>
/// 单条性能/运行信号：级别 + 来源 + 类别 + 描述行 + 机器可读键。
/// 由 <see cref="AISignalLog"/> 累积并落盘 <c>target/scratch/arc-logs/</c>。
/// </summary>
public class AIPerfSignal {
    public AISignalLevel Level;
    public string Source;
    public string Category;
    public string Line;
    public string KeySignal;

    public AIPerfSignal() {
        this.Level = AISignalLevel.Info;
        this.Source = "";
        this.Category = "";
        this.Line = "";
        this.KeySignal = "";
    }

    /// <summary>折叠为日志行文本。</summary>
    public string Format() {
        return "[" + AIPerfSignal.LevelName(this.Level) + "] "
            + (this.Source != "" ? this.Source + "/" : "")
            + (this.Category != "" ? this.Category + ": " : "")
            + this.Line
            + (this.KeySignal != "" ? " (" + this.KeySignal + ")" : "");
    }

    private static string LevelName(AISignalLevel level) {
        if (level == AISignalLevel.Warn) { return "warn"; }
        if (level == AISignalLevel.Error) { return "error"; }
        return "info";
    }
}
