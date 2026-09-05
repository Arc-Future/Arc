// ISubscription —— 拆分自 IPubSub.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public interface ISubscription {
    string Topic { get; }
    void Unsubscribe();
}
