// RFC 049 M-C: Arc.Net.Grpc — gRPC 状态码（对齐 gRPC 规范 status codes）。
//
// 公开契约类型：开发者可直接触碰。数字取值对齐 gRPC 官方 status codes，
// 与 trailers `grpc-status` 头字段数值一一对应（0=OK · 1=CANCELLED · …）。
//
// 访问权限：public（用户面契约）。内部实现不依赖本类型以外的任何细节。

namespace Arc.Net.Grpc;

/// <summary>gRPC 状态码（对齐 gRPC 规范 · 本系列用集）。</summary>
public enum GrpcStatus {
    /// <summary>成功。</summary>
    Ok = 0,

    /// <summary>调用被取消。</summary>
    Cancelled = 1,

    /// <summary>未知错误。</summary>
    Unknown = 2,

    /// <summary>参数非法。</summary>
    InvalidArgument = 3,

    /// <summary>超时。</summary>
    DeadlineExceeded = 4,

    /// <summary>未找到。</summary>
    NotFound = 5,

    /// <summary>已存在。</summary>
    AlreadyExists = 6,

    /// <summary>权限不足。</summary>
    PermissionDenied = 7,

    /// <summary>资源耗尽。</summary>
    ResourceExhausted = 8,

    /// <summary>前置条件不满足。</summary>
    FailedPrecondition = 9,

    /// <summary>操作被中止。</summary>
    Aborted = 10,

    /// <summary>越界。</summary>
    OutOfRange = 11,

    /// <summary>未实现。</summary>
    Unimplemented = 12,

    /// <summary>内部错误。</summary>
    Internal = 13,

    /// <summary>服务不可用。</summary>
    Unavailable = 14,

    /// <summary>数据损坏。</summary>
    DataLoss = 15,

    /// <summary>未认证。</summary>
    Unauthenticated = 16,
}
