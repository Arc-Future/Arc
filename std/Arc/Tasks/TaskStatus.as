// TaskStatus 枚举 (RFC 009)
namespace Arc {

/// <summary>异步任务状态枚举。对标 C# System.Threading.Tasks.TaskStatus。</summary>
public enum TaskStatus {
    /// <summary>已完成（成功）。</summary>
    Ready,
    /// <summary>等待中（未完成）。</summary>
    Pending,
    /// <summary>异常终止。</summary>
    Faulted,
    /// <summary>已取消。</summary>
    Canceled,
}

}
