// RFC 038 —— HITL 门闩（Session 内嵌；无第二 HITL 包）。
// M1 锁定惯用法（单线程宿主 / QIF）：
//   EnterAwaiting —— ApproveAsync|RejectAsync|ProvideInputAsync（无第二入口）
// 门闩关闭后经 LastHumanOutcome 取结果（先关闭、后取结果；无等待原语）。
// 并发 park（线程池信号量）后置；禁 NeedsHuman 第二入口。
namespace Arc.Agent;

using Arc;

/// <summary>人机协同门闩。由 <see cref="AISession"/> 持有。</summary>
public class AIHumanGate {
    private AIHumanRequest _pending;
    private AIHumanOutcome _outcome;
    private bool _awaiting;
    private bool _completed;

    public AIHumanGate() {
        _pending = null;
        _outcome = null;
        _awaiting = false;
        _completed = false;
    }

    public AIHumanRequest PendingHuman { get { return _pending; } }
    public bool IsAwaiting { get { return _awaiting && !_completed; } }
    public AIHumanOutcome LastOutcome { get { return _outcome; } }

    public void EnterAwaiting(AIHumanRequest request) {
        if (request == null) { throw new ArgumentNullException("request"); }
        if (_awaiting && !_completed) {
            throw new InvalidOperationException("Already awaiting human.");
        }
        _pending = request;
        _outcome = null;
        _awaiting = true;
        _completed = false;
    }

    public Task ApproveAsync(string editedToolName, string editedToolArguments, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (!_awaiting || _completed) {
            throw new InvalidOperationException("No pending human gate.");
        }
        string name = editedToolName != null ? editedToolName : "";
        string args = editedToolArguments != null ? editedToolArguments : "";
        if (name.Length == 0) { name = _pending.ToolName; }
        if (args.Length == 0) { args = _pending.ToolArguments; }
        this.Complete(AIHumanOutcome.Approved(name, args));
        return Task.CompletedTask;
    }

    public Task RejectAsync(string reason, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (!_awaiting || _completed) {
            throw new InvalidOperationException("No pending human gate.");
        }
        this.Complete(AIHumanOutcome.Rejected(reason));
        return Task.CompletedTask;
    }

    public Task ProvideInputAsync(string text, CancellationToken ct) {
        ct.ThrowIfCancellationRequested();
        if (text == null) { throw new ArgumentNullException("text"); }
        if (!_awaiting || _completed) {
            throw new InvalidOperationException("No pending human gate.");
        }
        this.Complete(AIHumanOutcome.Input(text));
        return Task.CompletedTask;
    }

    public void CompleteCancelled() {
        if (_awaiting && !_completed) {
            this.Complete(AIHumanOutcome.Cancelled());
        }
    }

    public void Reset() {
        _pending = null;
        _outcome = null;
        _awaiting = false;
        _completed = false;
    }

    private void Complete(AIHumanOutcome outcome) {
        _outcome = outcome;
        _completed = true;
        _awaiting = false;
        _pending = null;
    }
}
