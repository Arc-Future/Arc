// Arc.Diagnostics.PosixSignal — POSIX 信号枚举。

namespace Arc.Diagnostics;

/// <summary>
/// POSIX 信号枚举——用于 PtyProcess.SendSignal。
/// Windows 上仅 SIGINT(2)/SIGTERM(15) 映射为 TerminateProcess。
/// </summary>
public enum PosixSignal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGKILL = 9,
    SIGTERM = 15,
    SIGSTOP = 19,
    SIGCONT = 18
}
