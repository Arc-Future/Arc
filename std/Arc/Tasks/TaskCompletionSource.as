// TaskCompletionSource<T> — 外部事件驱动的 Task 生产端（RFC 008 AsyncStream）。
namespace Arc {

/// <summary>
/// 任务完成源——把外部事件（回调/IO 完成/定时器）桥接为 Task&lt;T&gt;。
/// 对象指针即底层 RtTask*（PENDING 态句柄）：new 拦截为
/// rt_task_create_pending，.Task 直接返回自身（共享指针，零拷贝）。
///
/// 此声明为 stub，方法体不执行；codegen 在 emit_call.rs 的
/// try_emit_tcs_method 中直接发射 rt_task_* ABI。TCS 与其 Task
/// 共享同一 RtTask 生命周期（rt_task_release 统一收口）。
/// </summary>
/// <typeparam name="T">结果类型。</typeparam>
public class TaskCompletionSource<T> {
    /// <summary>由此完成源控制的 Task（与 TCS 共享同一底层句柄）。</summary>
    // get_Task 由 codegen try_emit_tcs_method 拦截（follower task 扇出），须以
    // `[Builtin]` auto-property 书写——否则注册为 backing field，`tcs.Task`
    // 降级 FieldGet 读 RtTask 偏移 16（ptr_result）→ null（task_tcs 回归根因）。
    [Builtin(ABI = "rt_task_add_follower")]
    public Task<T> Task { get; }

    /// <summary>以成功结果完成 Task。</summary>
    /// <param name="value">任务结果值。</param>
    public void SetResult(T value) {}

    /// <summary>以异常完成 Task（Status = Faulted）。</summary>
    /// <param name="exception">失败原因。</param>
    public void SetException(Exception exception) {}

    /// <summary>以取消完成 Task（Status = Canceled）。</summary>
    public void SetCanceled() {}
}

} // namespace Arc
