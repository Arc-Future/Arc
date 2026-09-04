// M8 Long-Running Task 基座（RFC 038 §3.4 / §3.4.5 M8.1）：任务生命周期状态。
namespace Arc.Agent;
public enum AITaskRunStatus {
    Pending, Running, Paused, Completed, Failed, Cancelled
}