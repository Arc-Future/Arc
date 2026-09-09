namespace Arc.QIF;

using Arc;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.Threading;

/// <summary>
/// QIF 测试执行编排器。对标 XUnit TestRunner。
/// 支持串行与并行（Parallel.For）执行；并行时以 Collection 为边界
/// （集间并行、集内串行；无 [Collection] 时每类自成隐式集合）。
/// 所有对共享状态（_results、计数器）的访问均通过内部 Lock 保护。
/// </summary>
public class QIFRunner {
    private List<QIFResult> _results;
    // 增量计数：WriteReport 路径避免 foreach 遍历 _results（H1：满套件
    // WriteResults/Passed 交界偶发 0xC0000005）。
    private int _passed;
    private int _failed;
    private int _skipped;
    private int _errors;
    // [Fact(Skip)] / [Theory(Skip)] 属性跳过计数（RFC 032 §6 硬门禁）。
    private int _factSkipped;
    // QIF-7：保护 _results 与计数器的 Lock（并行执行安全）。
    private Lock _sync;
    // 全量 wall-clock 计时（QIF 报告 summary 的 duration_ms 数据源）。
    private Stopwatch _wall;

    public QIFRunner() {
        _results = new List<QIFResult>();
        _sync = new Lock();
        Options = new QIFOptions();
        _passed = 0;
        _failed = 0;
        _skipped = 0;
        _errors = 0;
        _factSkipped = 0;
        _wall = Stopwatch.StartNew();
    }

    /// <summary>运行器配置（框架内部；用户经 `arc.toml [qif]` 配置）。</summary>
    internal QIFOptions Options { get; set; }

    /// <summary>并行度（生成代码读取；`QIFOptions` 内部化后的标量访问器）。</summary>
    public int MaxParallel { get { return Options.MaxParallel; } set { Options.MaxParallel = value; } }

    /// <summary>默认单测试超时毫秒（0 = 不限制；生成代码/宿主据此强制超时）。</summary>
    public int DefaultTimeoutMs { get { return Options.DefaultTimeoutMs; } set { Options.DefaultTimeoutMs = value; } }

    /// <summary>
    /// Assert.Skip（运行时 QIF_SKIP）是否导致非零退出。
    /// 默认 false——契约自测可计 Skipped；CI 可经 `--fail-on-skip` / `[qif].fail_on_skip` 开启。
    /// 属性 Fact-Skip 始终硬失败（见 <see cref="ShouldFailExit"/>）。
    /// </summary>
    public bool FailOnSkip { get { return Options.FailOnSkip; } set { Options.FailOnSkip = value; } }

    public int Total { get { lock (_sync) { return _results.Count; } } }

    public int Passed { get { lock (_sync) { return _passed; } } }

    public int Failed { get { lock (_sync) { return _failed; } } }

    public int Skipped { get { lock (_sync) { return _skipped; } } }

    public int Errors { get { lock (_sync) { return _errors; } } }

    /// <summary>属性级 Fact-Skip 计数（RFC 032 §6；≠ Assert.Skip）。</summary>
    public int FactSkipped { get { lock (_sync) { return _factSkipped; } } }

    public bool HasFailures { get { lock (_sync) { return _failed > 0 || _errors > 0; } } }
    public bool AllPassed { get { lock (_sync) { return _failed == 0 && _errors == 0 && _skipped == 0; } } }

    /// <summary>
    /// 宿主退出判据：失败/错误、属性 Fact-Skip，或 FailOnSkip 开启时的任意 Skip。
    /// </summary>
    public bool ShouldFailExit {
        get {
            lock (_sync) {
                if (_failed > 0 || _errors > 0 || _factSkipped > 0) {
                    return true;
                }
                if (Options.FailOnSkip && _skipped > 0) {
                    return true;
                }
                return false;
            }
        }
    }

    /// <summary>全量 wall-clock 耗时（毫秒；从 Runner 创建起计）。</summary>
    public long TotalDurationMs { get { return _wall.ElapsedMilliseconds; } }

    internal void Record(QIFResult result) {
        lock (_sync) {
            _results.Add(result);
            if (result.Status == QIFTestStatus.Pass) { _passed = _passed + 1; }
            else if (result.Status == QIFTestStatus.Fail) { _failed = _failed + 1; }
            else if (result.Status == QIFTestStatus.Skip) { _skipped = _skipped + 1; }
            else if (result.Status == QIFTestStatus.Error) { _errors = _errors + 1; }
        }
    }

    public void RecordPass(string name, QIFTestKind kind, long durationNs) {
        QIFResult r = new QIFResult(name, kind, QIFTestStatus.Pass, durationNs);
        lock (_sync) {
            _results.Add(r);
            _passed = _passed + 1;
        }
    }

    /// <summary>RecordPass 带 traits 字符串（分号分隔的 "k:v" 对）。</summary>
    public void RecordPassT(string name, QIFTestKind kind, long durationNs, string traits) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Pass, durationNs);
        // 内联 trait 解析——Arc 不支持同 class 跨方法调用
        if (traits != "") {
            int start = 0; int len = traits.Length;
            while (start < len) {
                int end = start;
                while (end < len && traits.Substring(end, 1) != ";") { end = end + 1; }
                string pair = traits.Substring(start, end - start);
                if (pair != "") { result.Traits.Add(pair); }
                start = end + 1;
            }
        }
        lock (_sync) {
            _results.Add(result);
            _passed = _passed + 1;
        }
    }

    public void RecordFail(string name, QIFTestKind kind, long durationNs, string errorMessage) {
        QIFResult r = new QIFResult(name, kind, QIFTestStatus.Fail, durationNs, errorMessage);
        lock (_sync) {
            _results.Add(r);
            _failed = _failed + 1;
        }
    }

    public void RecordError(string name, QIFTestKind kind, long durationNs, string errorMessage) {
        QIFResult r = new QIFResult(name, kind, QIFTestStatus.Error, durationNs, errorMessage);
        lock (_sync) {
            _results.Add(r);
            _errors = _errors + 1;
        }
    }

    /// <summary>RecordFail 带 traits 字符串（分号分隔的 "k:v" 对）。</summary>
    public void RecordFailT(string name, QIFTestKind kind, long durationNs, string errorMessage, string traits) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Fail, durationNs, errorMessage);
        // 内联 trait 解析
        if (traits != "") {
            int start = 0; int len = traits.Length;
            while (start < len) {
                int end = start;
                while (end < len && traits.Substring(end, 1) != ";") { end = end + 1; }
                string pair = traits.Substring(start, end - start);
                if (pair != "") { result.Traits.Add(pair); }
                start = end + 1;
            }
        }
        lock (_sync) {
            _results.Add(result);
            _failed = _failed + 1;
        }
    }

    public void RecordSkip(string name, QIFTestKind kind, string skipReason) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Skip, 0);
        result.SkipReason = skipReason;
        lock (_sync) {
            _results.Add(result);
            _skipped = _skipped + 1;
        }
    }

    /// <summary>RecordSkip 带 traits 字符串（分号分隔的 "k:v" 对）。</summary>
    public void RecordSkipT(string name, QIFTestKind kind, string skipReason, string traits) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Skip, 0);
        result.SkipReason = skipReason;
        // 内联 trait 解析
        if (traits != "") {
            int start = 0; int len = traits.Length;
            while (start < len) {
                int end = start;
                while (end < len && traits.Substring(end, 1) != ";") { end = end + 1; }
                string pair = traits.Substring(start, end - start);
                if (pair != "") { result.Traits.Add(pair); }
                start = end + 1;
            }
        }
        lock (_sync) {
            _results.Add(result);
            _skipped = _skipped + 1;
        }
    }

    /// <summary>
    /// 记录属性级 Fact-Skip（`[Fact(Skip=…)]` / `[Theory(Skip=…)]`）。
    /// 计入 Skipped 与 FactSkipped；宿主经 ShouldFailExit 恒非零退出（RFC 032 §6）。
    /// </summary>
    public void RecordFactSkip(string name, QIFTestKind kind, string skipReason) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Skip, 0);
        result.SkipReason = skipReason;
        lock (_sync) {
            _results.Add(result);
            _skipped = _skipped + 1;
            _factSkipped = _factSkipped + 1;
        }
    }

    /// <summary>RecordFactSkip 带 traits。</summary>
    public void RecordFactSkipT(string name, QIFTestKind kind, string skipReason, string traits) {
        QIFResult result = new QIFResult(name, kind, QIFTestStatus.Skip, 0);
        result.SkipReason = skipReason;
        // 内联 trait 解析
        if (traits != "") {
            int start = 0; int len = traits.Length;
            while (start < len) {
                int end = start;
                while (end < len && traits.Substring(end, 1) != ";") { end = end + 1; }
                string pair = traits.Substring(start, end - start);
                if (pair != "") { result.Traits.Add(pair); }
                start = end + 1;
            }
        }
        lock (_sync) {
            _results.Add(result);
            _skipped = _skipped + 1;
            _factSkipped = _factSkipped + 1;
        }
    }

    /// <summary>按索引取结果（框架内部：QIFReporting 消费）。调用方须已持有锁或单线程语境。</summary>
    internal QIFResult GetResult(int index) { return _results[index]; }

    /// <summary>设置最近记录的测试结果输出（仅在单线程或串行语境调用）。</summary>
    public void SetLastOutput(string output) {
        if (_results.Count > 0) {
            QIFResult last = _results[_results.Count - 1];
            last.Output = output;
        }
    }

    /// <summary>设置最近记录的测试结果 traits。</summary>
    public void SetLastTraits(List<string> traits) {
        if (_results.Count > 0) {
            QIFResult last = _results[_results.Count - 1];
            last.Traits = traits;
        }
    }

    /// <summary>追加单个 trait 到最近记录的测试结果。</summary>
    public void AddLastTrait(string trait) {
        if (_results.Count > 0) {
            QIFResult last = _results[_results.Count - 1];
            last.Traits.Add(trait);
        }
    }

    /// <summary>全部结果快照（框架内部；已加锁）。</summary>
    internal List<QIFResult> GetResults() {
        lock (_sync) {
            List<QIFResult> copy = new List<QIFResult>();
            foreach (var r in _results) { copy.Add(r); }
            return copy;
        }
    }

    /// <summary>失败结果列表（框架内部；已加锁）。</summary>
    internal List<QIFResult> GetFailed() {
        lock (_sync) {
            List<QIFResult> failed = new List<QIFResult>();
            foreach (var r in _results) {
                if (r.Status == QIFTestStatus.Fail || r.Status == QIFTestStatus.Error) { failed.Add(r); }
            }
            return failed;
        }
    }
}
