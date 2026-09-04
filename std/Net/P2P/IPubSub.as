// RFC 042: IPubSub + GossipSubRouter 桩（无 event/?.，解析器限制）。
namespace Arc.Net.P2P;

public interface IPubSub {
    void Publish(string topic, string data);
    ISubscription Subscribe(string topic);
    void RegisterTopicValidator(string topic, Func<string, PeerId, bool> validator);
}

public interface ISubscription {
    string Topic { get; }
    void Unsubscribe();
}

public class GossipSubRouter : IPubSub {
    private int _meshDegree;
    private int _meshDegreeLow;
    private int _meshDegreeHigh;
    private Dictionary<string, string> _topics;

    public GossipSubRouter() {
        _meshDegree = 6;
        _meshDegreeLow = 4;
        _meshDegreeHigh = 12;
        _topics = new Dictionary<string, string>();
    }

    public GossipSubRouter(int d, int dl, int dh) {
        _meshDegree = d;
        _meshDegreeLow = dl;
        _meshDegreeHigh = dh;
        _topics = new Dictionary<string, string>();
    }

    public void Publish(string topic, string data) { }
    public ISubscription Subscribe(string topic) { return new GossipSubSubscription(topic); }
    public void RegisterTopicValidator(string topic, Func<string, PeerId, bool> validator) { }
}

internal class GossipSubSubscription : ISubscription {
    public string Topic { get; }
    public GossipSubSubscription(string topic) { Topic = topic; }
    public void Unsubscribe() { }
}