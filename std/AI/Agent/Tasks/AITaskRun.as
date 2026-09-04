// M8 Long-Running Task 基座（RFC 038 §3.4 / §3.4.5 M8.1）：
// 长时回合任务的状态机 + 快照续跑 + 心跳。承载一个 AISession 驱动的回合循环——
// 提供原子回合迭代（RunStepAsync）与至完结的有界循环（RunAsync），
// 快照（Checkpoint/Resume）使任务可中断续跑，心跳/进度经回调可观测。
//
// 实现纪律：`Steps` 为外部可观测任务进度（Steps 属性），须以字段承载（非 compiler
// workaround）；回调以自动属性承载（编译器生成 private 后备字段 + 访问器，规避 public 委托字段缺陷 C6）。
namespace Arc.Agent;
using Arc;

public class AITaskRun {
    private long _maxDurationMs;
    private long _startedAtTicks;

    public AITaskRun(AISession session, int maxSteps) {
        Session = session;
        MaxSteps = maxSteps > 0 ? maxSteps : 1;
        Status = AITaskRunStatus.Pending;
        Steps = 0;
        _maxDurationMs = 0;
        HeartbeatCount = 0;
        LastHeartbeatTicks = 0;
        _startedAtTicks = 0;
        Progress = "";
        RunId = this.GenerateRunId();
        PlanProvider = null;
        OnProgress = null;
        OnStateChanged = null;
        OnHeartbeat = null;
    }

    public string RunId { get; set; }
    public AITaskRunStatus Status { get; set; }
    public AISession Session { get; }
    public int Steps { get; set; }
    public int MaxSteps { get; set; }
    /// <summary>任务总时长熔断上限（毫秒；0 = 不限）。Start 起计，RunAsync 每回合前检查，超时 → Failed("TaskTimeout")。</summary>
    public long MaxDurationMs { get { return _maxDurationMs; } set { _maxDurationMs = value > 0 ? value : 0; } }
    public long LastHeartbeatTicks { get; set; }
    public int HeartbeatCount { get; set; }
    public string Progress { get; set; }
    /// <summary>计划感知（可选挂载）：设置后每回合自动同步计划完成/拒绝状态。</summary>
    public AIPlanContextProvider PlanProvider { get; set; }
    public Action<string> OnProgress { get; set; }
    public Action<string> OnStateChanged { get; set; }
    public Action OnHeartbeat { get; set; }

    /// <summary>起始：Pending → Running。</summary>
    public bool Start() {
        if (Status != AITaskRunStatus.Pending) { return false; }
        Status = AITaskRunStatus.Running;
        _startedAtTicks = this.NowTicks();
        this.FireState();
        return true;
    }

    /// <summary>心跳：更新计数/时间戳并触发回调（供宿主判定任务存活）。</summary>
    public void Beat() {
        HeartbeatCount = HeartbeatCount + 1;
        LastHeartbeatTicks = this.NowTicks();
        if (OnHeartbeat != null) { OnHeartbeat(); }
    }

    /// <summary>心跳是否已超时（timeoutMs 阈值；0 = 从不超时；未心跳过 = 视为超时）。</summary>
    public bool IsStale(long timeoutMs) {
        if (timeoutMs <= 0) { return false; }
        if (LastHeartbeatTicks == 0) { return true; }
        long now = this.NowTicks();
        long elapsedMs = (now - LastHeartbeatTicks) / 10000;
        return elapsedMs > timeoutMs;
    }

    /// <summary>原子回合迭代：Running 下执行一次会话回合并推进步骤/进度/心跳。</summary>
    public async Task<AIReply> RunStepAsync(string userText, CancellationToken cancellationToken) {
        if (Status != AITaskRunStatus.Running || Session == null) {
            return AIReply.Fail("NotRunning", "AITaskRun.RunStepAsync: task not Running");
        }
        Steps = Steps + 1;
        string prompt = userText != null ? userText : "";
        Progress = "step " + Steps + ": " + prompt;
        if (OnProgress != null) { OnProgress(Progress); }
        AIReply reply = await Session.RunAsync(prompt, cancellationToken);
        this.Beat();
        // 计划感知：回合后同步计划状态（全部步骤完成 → 任务完结；被拒 → 任务失败）。
        this.SyncPlan();
        return reply;
    }

    /// <summary>任务是否已超过总时长熔断上限（MaxDurationMs > 0 才判定）。</summary>
    public bool IsDurationExceeded() {
        if (_maxDurationMs <= 0) { return false; }
        if (_startedAtTicks == 0) { return false; }
        long now = this.NowTicks();
        long elapsedMs = (now - _startedAtTicks) / 10000;
        return elapsedMs > _maxDurationMs;
    }

    /// <summary>
    /// 有界循环：Running 下逐回合执行至 MaxSteps 或出现错误。返回任务完结状态枚举
    /// （Completed → 正常完结；Failed → 出错/超时；Cancelled → 取消；Pending → 未在 Running）。
    /// 每回合前检查总时长熔断（MaxDurationMs），超时 → Failed("TaskTimeout")，防长时任务挂死。
    /// 取消令牌已请求 → Cancelled（对齐 .NET Task 取消语义，不再被 Fail 吞成 Failed）。
    /// </summary>
    public async Task<AITaskRunStatus> RunAsync(string initialPrompt, CancellationToken cancellationToken) {
        if (Status != AITaskRunStatus.Running || Session == null) {
            return Status;
        }
        string prompt = initialPrompt != null ? initialPrompt : "";
        while (Status == AITaskRunStatus.Running && Steps < MaxSteps) {
            if (this.IsDurationExceeded()) {
                Status = AITaskRunStatus.Failed;
                Progress = "TaskTimeout";
                this.FireState();
                return AITaskRunStatus.Failed;
            }
            if (cancellationToken.IsCancellationRequested) {
                Status = AITaskRunStatus.Cancelled;
                Progress = "Cancelled";
                this.FireState();
                return AITaskRunStatus.Cancelled;
            }
            AIReply reply = await this.RunStepAsync(prompt, cancellationToken);
            if (cancellationToken.IsCancellationRequested) {
                Status = AITaskRunStatus.Cancelled;
                Progress = "Cancelled";
                this.FireState();
                return AITaskRunStatus.Cancelled;
            }
            if (reply == null || reply.IsError) {
                Status = AITaskRunStatus.Failed;
                this.FireState();
                return AITaskRunStatus.Failed;
            }
            prompt = reply.Text != null ? reply.Text : "";
        }
        if (Status == AITaskRunStatus.Running) {
            Status = AITaskRunStatus.Completed;
            this.FireState();
        }
        return Status;
    }

    /// <summary>落快照：Running → Paused，捕获会话快照 + 任务元数据（续跑事实源）。</summary>
    public AITaskRunSnapshot Checkpoint() {
        if (Status != AITaskRunStatus.Running) { return null; }
        Status = AITaskRunStatus.Paused;
        Progress = "paused";
        this.FireState();
        AITaskRunSnapshot snap = new AITaskRunSnapshot();
        snap.RunId = RunId;
        snap.Status = Status;
        snap.Steps = Steps;
        snap.MaxSteps = MaxSteps;
        snap.Progress = Progress;
        snap.SessionSnapshot = Session != null ? Session.Snapshot() : null;
        return snap;
    }

    /// <summary>续跑：载入快照 → Running，恢复会话上下文。</summary>
    public bool Resume(AITaskRunSnapshot snapshot) {
        if (snapshot == null) { return false; }
        RunId = snapshot.RunId;
        Steps = snapshot.Steps;
        MaxSteps = snapshot.MaxSteps;
        Progress = "resumed";
        if (Session != null && snapshot.SessionSnapshot != null) {
            Session.Restore(snapshot.SessionSnapshot);
        }
        Status = AITaskRunStatus.Running;
        this.FireState();
        return true;
    }

    public void Complete() {
        Status = AITaskRunStatus.Completed;
        this.FireState();
    }

    public void Fail(string reason) {
        Status = AITaskRunStatus.Failed;
        Progress = reason != null ? reason : "failed";
        this.FireState();
    }

    public void Cancel() {
        Status = AITaskRunStatus.Cancelled;
        Progress = "Cancelled";
        this.FireState();
    }

    private void FireState() {
        if (OnStateChanged != null) {
            OnStateChanged(this.StatusName(Status));
        }
    }

    /// <summary>
    /// 计划感知同步（可选挂载）：计划全部步骤完成（Verifying，执行已收口）或 DoD 判定通过
    /// （Completed）→ 任务 Completed；计划被拒绝 → 任务 Failed("PlanRejected")。未挂载或无计划
    /// → 无副作用。M8：满额即 Verifying，任务执行收口与 DoD 完成判定解耦。
    /// </summary>
    private void SyncPlan() {
        if (PlanProvider == null || Status != AITaskRunStatus.Running) {
            return;
        }
        AIPlan plan = PlanProvider.GetPlan();
        if (plan == null) {
            return;
        }
        if (plan.Status == AIPlanStatus.Verifying || plan.Status == AIPlanStatus.Completed) {
            this.Complete();
        } else if (plan.Status == AIPlanStatus.Rejected) {
            this.Fail("PlanRejected");
        }
    }

    private string StatusName(AITaskRunStatus s) {
        if (s == AITaskRunStatus.Running) { return "Running"; }
        if (s == AITaskRunStatus.Paused) { return "Paused"; }
        if (s == AITaskRunStatus.Completed) { return "Completed"; }
        if (s == AITaskRunStatus.Failed) { return "Failed"; }
        if (s == AITaskRunStatus.Cancelled) { return "Cancelled"; }
        return "Pending";
    }

    private string GenerateRunId() {
        return "run-" + ("" + this.NowTicks());
    }

    /// <summary>当前时间戳（100ns ticks）。先取 DateTime 到局部再读 Ticks——规避结构体临时值
    /// 属性访问在 codegen 的 ptr/i32 类型错位。</summary>
    private long NowTicks() {
        DateTime now = DateTime.Now;
        return now.Ticks;
    }
}