// GossipSubRouter —— 拆分自 IPubSub.as（一文件一公开类型）。
namespace Arc.Net.P2P;

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
