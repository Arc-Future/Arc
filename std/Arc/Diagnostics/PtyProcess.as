// Arc.Diagnostics.PtyProcess — PTY 终端子进程。

namespace Arc.Diagnostics;

using Arc.IO;

/// <summary>
/// PTY 终端子进程——在 Process 基础上增加伪终端控制能力。
/// Windows 使用 ConPTY（Win10 1809+）；POSIX 使用 openpty。
/// </summary>
public class PtyProcess : Process {
    private int _masterFd;
    private ProcessStream? _rawStream;

    public int MasterFd {
        get { return _masterFd; }
    }

    public ProcessStream RawStream {
        get { return _rawStream; }
    }

    public PtyProcess() : base() {
        _masterFd = -1;
        _rawStream = null;
    }

    public void ResizeTerminal(int cols, int rows) {
        rt_process.rt_pty_resize(_handle, cols, rows);
    }

    public int WriteRaw(string data) {
        if (_masterFd < 0) { return -1; }
        return rt_process.rt_pty_write_string(_masterFd, data);
    }

    public string? ReadRaw() {
        if (_masterFd < 0) { return null; }
        return rt_process.rt_pty_read_line(_masterFd);
    }

    public int SendSignal(PosixSignal signal) {
        return rt_process.rt_pty_send_signal(_handle, (int)signal);
    }

    public override void Dispose() {
        if (_disposed) { return; }
        _disposed = true;
        if (_rawStream != null) {
            _rawStream.Dispose();
            _rawStream = null;
        }
        if (_masterFd >= 0) {
            rt_process.rt_pty_close(_masterFd);
            _masterFd = -1;
        }
        if (_stdinStream != null) { _stdinStream.Dispose(); }
        if (_stdoutStream != null) { _stdoutStream.Dispose(); }
        if (_stderrStream != null) { _stderrStream.Dispose(); }
        if (_started) {
            rt_process.rt_proc_close(_handle);
        }
    }

    public static PtyProcess Start(PtyProcessStartInfo startInfo) {
        PtyProcess pty = new PtyProcess();
        pty.StartInfo = startInfo;
        int masterFd = -1;
        NativePtr h = rt_process.rt_pty_spawn(
            startInfo.FileName,
            startInfo.Arguments,
            startInfo.WorkingDirectory,
            startInfo.TerminalWidth,
            startInfo.TerminalHeight,
            out masterFd
        );
        if (h == null) {
            throw new InvalidOperationException("Failed to start PTY process: " + startInfo.FileName);
        }
        pty._handle = h;
        pty._pid = rt_process.rt_proc_get_pid(h);
        if (pty._pid == 0) {
            throw new InvalidOperationException("Failed to start PTY process: " + startInfo.FileName);
        }
        pty._started = true;
        pty._masterFd = masterFd;
        pty._rawStream = new ProcessStream(masterFd, true, true);
        return pty;
    }
}
