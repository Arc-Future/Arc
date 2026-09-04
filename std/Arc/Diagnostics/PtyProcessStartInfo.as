// Arc.Diagnostics.PtyProcessStartInfo — PTY 进程启动配置。

namespace Arc.Diagnostics;

/// <summary>
/// PTY 进程启动配置——在 ProcessStartInfo 基础上增加终端尺寸。
/// </summary>
public class PtyProcessStartInfo : ProcessStartInfo {
    public int TerminalWidth { get; set; }
    public int TerminalHeight { get; set; }

    public PtyProcessStartInfo() : base() {
        TerminalWidth = 80;
        TerminalHeight = 24;
    }

    public PtyProcessStartInfo(string fileName) : base(fileName) {
        TerminalWidth = 80;
        TerminalHeight = 24;
    }

    public PtyProcessStartInfo(string fileName, string arguments) : base(fileName, arguments) {
        TerminalWidth = 80;
        TerminalHeight = 24;
    }
}
