// InMemoryPeerStore —— 拆分自 IPeerStore.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class InMemoryPeerStore : IPeerStore {
    private List<PeerRecord> _records;

    public InMemoryPeerStore() {
        _records = new List<PeerRecord>();
    }

    public bool Put(PeerRecord peerRecord) {
        if (peerRecord == null || peerRecord.PeerId == null) {
            return false;
        }
        string key = peerRecord.PeerId.PublicKey;
        for (int i = 0; i < _records.Count; i++) {
            PeerRecord cur = _records[i];
            if (cur != null && cur.PeerId != null && cur.PeerId.PublicKey == key) {
                _records[i] = peerRecord;
                return true;
            }
        }
        _records.Add(peerRecord);
        return true;
    }

    public PeerRecord Get(PeerId peerId) {
        if (peerId == null) {
            return null;
        }
        string key = peerId.PublicKey;
        for (int i = 0; i < _records.Count; i++) {
            PeerRecord cur = _records[i];
            if (cur != null && cur.PeerId != null && cur.PeerId.PublicKey == key) {
                return cur;
            }
        }
        return null;
    }

    public List<PeerId> GetConnectedPeers() {
        List<PeerId> ids = new List<PeerId>();
        for (int i = 0; i < _records.Count; i++) {
            PeerRecord cur = _records[i];
            if (cur != null && cur.PeerId != null) {
                ids.Add(cur.PeerId);
            }
        }
        return ids;
    }

    public void Remove(PeerId peerId) {
        if (peerId == null) {
            return;
        }
        string key = peerId.PublicKey;
        // NLL（RFC 005 无条件启用）：遍历期禁止修改容器——快照重建替代 RemoveAt。
        List<PeerRecord> kept = new List<PeerRecord>();
        for (int i = 0; i < _records.Count; i = i + 1) {
            PeerRecord cur = _records[i];
            if (cur == null || cur.PeerId == null || cur.PeerId.PublicKey != key) {
                kept.Add(cur);
            }
        }
        _records = kept;
    }
}
