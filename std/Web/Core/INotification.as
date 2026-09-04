// INotification —— 通知契约（RFC 040 §1.5）：事件广播，void 多 handler。
namespace Arc.Web;

/// <summary>
/// 通知契约：事件广播。经 IMediator.PublishAsync 分发到多个通知处理器（void）。
/// 与请求（IRequest，单 handler 强类型返回）区分。
/// </summary>
public interface INotification {
}
