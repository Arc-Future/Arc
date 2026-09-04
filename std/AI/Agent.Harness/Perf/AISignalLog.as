// Arc.Agent.Harness.AISignalLog — 性能/运行信号日志（RFC 043 P1）。
//
// 落盘 `<project>/target/scratch/arc-logs/<tool>-<seq>.log`（对齐 AICheckpointStore
// 先例：仅写 scratch，禁写源码树）。seq 从既有 `<tool>-*.log` 递增，多轮运行互不覆盖。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Text;

/// <summary>
/// 信号日志：<see cref="Add"/> 累积（级别/来源/类别/描述行/键信号），
/// <see cref="WriteAsync"/> 一次性落盘并返回路径（失败 → null）。项目根不可解析 → 不建目录、不污染。
/// </summary>
public class AISignalLog {
    private const string StoreRelDir = "target/scratch/arc-logs";

    private string _root;
    private string _logDir;
    private List<AIPerfSignal> _signals;
    private bool _ready;

    public AISignalLog(string project) {
        string target = project != null && project != "" ? project : ".";
        string root = target;
        if (File.Exists(target)) {
            string parent = Path.GetDirectoryName(target);
            root = parent != null && parent != "" ? parent : ".";
        }
        _root = root != null ? root : "";
        _ready = Directory.Exists(_root);
        _logDir = _ready ? Path.Combine(_root, AISignalLog.StoreRelDir) : "";
        _signals = new List<AIPerfSignal>();
    }

    public string Root {
        get { return _root; }
    }

    public bool IsReady {
        get { return _ready; }
    }

    public List<AIPerfSignal> Signals {
        get { return _signals; }
    }

    public int Count {
        get { return _signals.Count; }
    }

    /// <summary>追加一条信号。null 字段归一为空串，保持日志行定长可解析。</summary>
    public void Add(AISignalLevel level, string source, string category, string line, string keySignal) {
        AIPerfSignal s = new AIPerfSignal();
        s.Level = level;
        s.Source = source != null ? source : "";
        s.Category = category != null ? category : "";
        s.Line = line != null ? line : "";
        s.KeySignal = keySignal != null ? keySignal : "";
        _signals.Add(s);
    }

    /// <summary>
    /// 落盘 <c>&lt;project&gt;/target/scratch/arc-logs/&lt;name&gt;-&lt;seq&gt;.log</c>。
    /// 返回日志绝对路径；无信号 / 项目根不可用 / 写失败 → 空串（不建目录、不污染）。seq 从既有同名文件递增。
    /// </summary>
    public async Task<string> WriteAsync(string name, CancellationToken cancellationToken) {
        if (!_ready || _signals == null || _signals.Count == 0) {
            return "";
        }
        string tool = name != null && name != "" ? name : "run";
        string text = AISignalLog.Format(_signals);
        cancellationToken.ThrowIfCancellationRequested();
        if (!await this.EnsureDirectoryAsync(_logDir)) {
            return "";
        }
        int seq = AISignalLog.NextSeq(_logDir, tool);
        string path = Path.Combine(_logDir, tool + "-" + seq + ".log");
        bool wrote = await File.WriteAllTextAsync(path, text);
        if (!wrote) {
            return "";
        }
        return path;
    }

    private static string Format(List<AIPerfSignal> signals) {
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = signals.Count;
        while (i < n) {
            AIPerfSignal s = signals[i];
            if (s != null) {
                if (sb.Length > 0) {
                    sb.Append("\n");
                }
                sb.Append(s.Format());
            }
            i = i + 1;
        }
        return sb.ToString();
    }

    private static int NextSeq(string dir, string tool) {
        string prefix = tool + "-";
        int max = 0;
        string[] files = Directory.GetFiles(dir);
        int i = 0;
        while (i < files.Length) {
            string baseName = Path.GetFileName(files[i]);
            if (baseName.StartsWith(prefix) && baseName.EndsWith(".log")) {
                int v = AISignalLog.ParseSeq(baseName.Substring(prefix.Length, baseName.Length - prefix.Length - 4));
                if (v > max) {
                    max = v;
                }
            }
            i = i + 1;
        }
        return max + 1;
    }

    private static int ParseSeq(string s) {
        if (s == null || s == "") {
            return 0;
        }
        int v = 0;
        int i = 0;
        while (i < s.Length) {
            char c = s[i];
            if (c < '0' || c > '9') {
                return 0;
            }
            v = v * 10 + (c - '0');
            i = i + 1;
        }
        return v;
    }

    /// <summary>逐层创建目录（rt_dir_create 仅建单层，父目录缺失时需递归创建）。</summary>
    private async Task<bool> EnsureDirectoryAsync(string dir) {
        bool exists = await Directory.ExistsAsync(dir);
        if (exists) {
            return true;
        }
        string parent = Path.GetDirectoryName(dir);
        if (parent != null && parent != "" && parent != dir) {
            bool okParent = await this.EnsureDirectoryAsync(parent);
            if (!okParent) {
                return false;
            }
        }
        return await Directory.CreateDirectoryAsync(dir);
    }
}
