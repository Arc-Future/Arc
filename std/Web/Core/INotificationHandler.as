// INotificationHandler —— 通知处理器契约（RFC 040 §1.5）：void 异步。
namespace Arc.Web;
using Arc;

/// <summary>
/// 通知处理器契约：同一通知可注册多个处理器，PublishAsync 全部触发，返回值 void。
/// 注：接口级 where 约束（TNotification : INotification）为 Arc 泛型接口声明
/// 暂不支持（语言缺口），强类型仍由泛型参数保证。
public interface INotificationHandler<TNotification> {
    Task HandleAsync(TNotification notification, CancellationToken cancellationToken);
}
