// GossipSubSubscription —— 拆分自 IPubSub.as（一文件一公开类型）。
namespace Arc.Net.P2P;

internal class GossipSubSubscription : ISubscription {
    public string Topic { get; }
    public GossipSubSubscription(string topic) { Topic = topic; }
    public void Unsubscribe() { }
}
