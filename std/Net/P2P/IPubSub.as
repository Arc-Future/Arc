// RFC 042: IPubSub + GossipSubRouter 桩（无 event/?.，解析器限制）。
namespace Arc.Net.P2P;

public interface IPubSub {
    void Publish(string topic, string data);
    ISubscription Subscribe(string topic);
    void RegisterTopicValidator(string topic, Func<string, PeerId, bool> validator);
}
