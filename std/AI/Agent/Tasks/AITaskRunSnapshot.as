// M8 Long-Running Task 基座（RFC 038 §3.4 / §3.4.5 M8.1）：任务快照（续跑事实源）。
// 承载会话快照（AISessionSnapshot，复用 AISession.Snapshot/Restore）+ 任务元数据。
namespace Arc.Agent;
public class AITaskRunSnapshot {
    public string RunId;
    public AITaskRunStatus Status;
    public int Steps;
    public int MaxSteps;
    public string Progress;
    public AISessionSnapshot SessionSnapshot;
    public AITaskRunSnapshot() {
        this.RunId = "";
        this.Status = AITaskRunStatus.Pending;
        this.Steps = 0;
        this.MaxSteps = 0;
        this.Progress = "";
        this.SessionSnapshot = null;
    }
}