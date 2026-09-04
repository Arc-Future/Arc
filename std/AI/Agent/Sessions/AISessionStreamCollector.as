namespace Arc.Agent;
using Arc;
using Arc.Collections;
/// <summary>
/// 流式事件收集器（RFC 038 流式主惯用法）：单点消费 Provider 的
/// IAsyncEnumerable&lt;AIStreamEvent&gt; —— 文本/思维链增量直通业务端，工具事件经
/// Pump* 路由（AISessionStreamPump），终结事件（Completed/Error）收敛为最终 AIReply。
/// </summary>
internal class AISessionStreamCollector {
    private AITurnRunner _runner;
    private CancellationToken _ct;
    private AIReply _finalReply;
    private string _textAcc;

    public AISessionStreamCollector(AITurnRunner runner) {
        _runner = runner;
        _ct = CancellationToken.None;
        _finalReply = null;
        _textAcc = "";
    }

    /// <summary>
    /// 消费完整事件流并返回最终回复：序列终结（Completed/Error 之后 MoveNextAsync
    /// 返回 false）即返回；流未以终结事件收尾（异常截断）返回 StreamIncomplete 失败。
    /// </summary>
    public async Task<AIReply> CollectAsync(IAsyncEnumerable<AIStreamEvent> stream, CancellationToken cancellationToken) {
        _ct = cancellationToken;
        IAsyncEnumerator<AIStreamEvent> e = stream.GetAsyncEnumerator(cancellationToken);
        while (true) {
            bool moved = await e.MoveNextAsync();
            if (!moved) {
                break;
            }
            this.Dispatch(e.Current);
        }
        if (_finalReply == null) {
            return AIReply.Fail("StreamIncomplete", "stream did not complete");
        }
        return _finalReply;
    }

    /// <summary>按事件种类分发：增量 → 业务端/Pump*，终结 → FinalReply。</summary>
    private void Dispatch(AIStreamEvent e) {
        if (e == null) {
            return;
        }
        if (e.Kind == AIStreamEventKind.TextDelta) {
            if (e.Text != null && e.Text != "") {
                _textAcc = _textAcc + e.Text;
                // 业务端边生成边显示（session.TextDelta）。
                _runner.NotifyTextDelta(e.Text);
            }
            return;
        }
        if (e.Kind == AIStreamEventKind.ToolCallStart) {
            _runner.PumpToolCallStart(e.ToolCallStart, _ct);
            return;
        }
        if (e.Kind == AIStreamEventKind.ToolArgDelta) {
            _runner.PumpToolArgDelta(e.ToolArgDelta, _ct);
            return;
        }
        if (e.Kind == AIStreamEventKind.ToolCallEnd) {
            _runner.PumpToolCallEnd(e.ToolCallEnd, _ct);
            return;
        }
        if (e.Kind == AIStreamEventKind.Usage) {
            _runner.ReportUsage(e.Usage);
            return;
        }
        if (e.Kind == AIStreamEventKind.Completed) {
            if (e.Reply != null) {
                _finalReply = e.Reply;
            } else {
                _finalReply = AIReply.FromText(_textAcc);
            }
            if ((_finalReply.Text == null || _finalReply.Text == "") && _textAcc != "") {
                _finalReply.Text = _textAcc;
            }
            if (_finalReply.Usage != null) {
                _runner.ReportUsage(_finalReply.Usage);
            }
            return;
        }
        if (e.Kind == AIStreamEventKind.Error) {
            _finalReply = AIReply.Fail(e.ErrorKind, e.ErrorMessage);
            return;
        }
        // ReasoningDelta：思维链增量不投递业务端（与 sink 时代对外面一致），
        // 最终 ReasoningContent 由 Completed 回复承载。
    }
}
