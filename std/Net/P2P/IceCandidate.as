// IceCandidate —— 拆分自 IceAgent.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public class IceCandidate {
    public string Type { get; set; }
    public string Address { get; set; }
    public int Priority { get; set; }
    public string Transport { get; set; }
}
