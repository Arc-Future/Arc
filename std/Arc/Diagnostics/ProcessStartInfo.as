// Arc.Diagnostics.ProcessStartInfo — 子进程启动配置。

namespace Arc.Diagnostics;

using Arc.Collections;

/// <summary>
/// 子进程启动配置——文件名、参数、工作目录、重定向选项。
/// </summary>
public class ProcessStartInfo {
    public string FileName { get; set; }
    public string Arguments { get; set; }
    public string? WorkingDirectory { get; set; }
    public bool RedirectStandardInput { get; set; }
    public bool RedirectStandardOutput { get; set; }
    public bool RedirectStandardError { get; set; }
    public bool CreateNoWindow { get; set; }
    public Dictionary<string, string?> Environment { get; set; }
    public List<string> ArgumentList { get; set; }
    public bool UseShellExecute { get; set; }

    public ProcessStartInfo() {
        FileName = "";
        Arguments = "";
        WorkingDirectory = null;
        RedirectStandardInput = false;
        RedirectStandardOutput = false;
        RedirectStandardError = false;
        CreateNoWindow = false;
        Environment = new Dictionary<string, string?>();
        ArgumentList = new List<string>();
        UseShellExecute = false;
    }

    public ProcessStartInfo(string fileName) {
        FileName = fileName;
        Arguments = "";
        WorkingDirectory = null;
        RedirectStandardInput = false;
        RedirectStandardOutput = false;
        RedirectStandardError = false;
        CreateNoWindow = false;
        Environment = new Dictionary<string, string?>();
        ArgumentList = new List<string>();
        UseShellExecute = false;
    }

    public ProcessStartInfo(string fileName, string arguments) {
        FileName = fileName;
        Arguments = arguments;
        WorkingDirectory = null;
        RedirectStandardInput = false;
        RedirectStandardOutput = false;
        RedirectStandardError = false;
        CreateNoWindow = false;
        Environment = new Dictionary<string, string?>();
        ArgumentList = new List<string>();
        UseShellExecute = false;
    }
}
