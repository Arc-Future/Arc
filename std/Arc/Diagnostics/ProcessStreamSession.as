// Arc.Diagnostics.ProcessStreamSession — 流式进程会话。

namespace Arc.Diagnostics;

using Arc.Threading;
using Arc;

/// <summary>
/// 流式进程会话——后台线程逐行推送 stdout/stderr，支持行级回调。
/// 修复：线程安全退出 + 回调异常隔离 + 取消支持。
/// </summary>
public class ProcessStreamSession {
    private Process _process;
    private Thread? _stdoutThread;
    private Thread? _stderrThread;
    private bool _stderrEnabled;
    private bool _closed;

    public Action<string>? OnOutputLine { get; set; }
    public Action<string>? OnErrorLine { get; set; }
    public Action<int>? OnExit { get; set; }

    public ProcessStreamSession(Process process) {
        _process = process;
        _stderrEnabled = (process.StandardError != null);
        _closed = false;
        _stdoutThread = new Thread(() => _readStdoutLoop());
        _stdoutThread.Start();
        if (_stderrEnabled) {
            _stderrThread = new Thread(() => _readStderrLoop());
            _stderrThread.Start();
        }
    }

    public void WriteLine(string line) {
        if (_closed) { return; }
        if (_process.StandardInput != null) {
            _process.StandardInput.WriteLine(line);
        }
    }

    public void Write(string data) {
        if (_closed) { return; }
        if (_process.StandardInput != null) {
            _process.StandardInput.WriteString(data);
        }
    }

    public int WaitForExit() {
        _process.WaitForExit();
        int code = _process.ExitCode;
        // 确保读线程完成（管道 EOF 后 ReadLine 返回 null 自然退出）
        if (_stdoutThread != null) { _stdoutThread.Join(); }
        if (_stderrThread != null) { _stderrThread.Join(); }
        if (OnExit != null) {
            try { OnExit(code); } catch { /* 回调异常隔离 */ }
        }
        return code;
    }

    public void Close() {
        if (_closed) { return; }
        _closed = true;
        // 先 join 读线程，再 Dispose process，避免线程访问已释放的 stream
        // 读线程在管道关闭后自然退出（ReadLine 返回 null）
        _process.Kill();  // 确保进程退出，管道关闭
        if (_stdoutThread != null) { _stdoutThread.Join(); }
        if (_stderrThread != null) { _stderrThread.Join(); }
        _process.Dispose();
    }

    private void _readStdoutLoop() {
        if (_process.StandardOutput == null) { return; }
        try {
            string? line = _process.StandardOutput.ReadLine();
            while (line != null && !_closed) {
                if (OnOutputLine != null) {
                    try { OnOutputLine(line); } catch { /* 回调异常隔离 */ }
                }
                line = _process.StandardOutput.ReadLine();
            }
        } catch {
            // 管道已关闭，静默退出
        }
    }

    private void _readStderrLoop() {
        if (_process.StandardError == null) { return; }
        try {
            string? line = _process.StandardError.ReadLine();
            while (line != null && !_closed) {
                if (OnErrorLine != null) {
                    try { OnErrorLine(line); } catch { /* 回调异常隔离 */ }
                }
                line = _process.StandardError.ReadLine();
            }
        } catch {
            // 管道已关闭，静默退出
        }
    }
}
