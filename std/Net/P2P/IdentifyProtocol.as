// RFC 042 Web3.0: IdentifyProtocol — 自动能力交换桩。
namespace Arc.Net.P2P;

internal class IdentifyInfo {
    public string ProtocolVersion { get; set; }
    public string AgentVersion { get; set; }
    public List<string> ListenAddresses { get; set; }
    public string ObservedAddress { get; set; }
    public List<string> Protocols { get; set; }
}

internal class IdentifyProtocol {
    public IdentifyInfo LocalInfo { get; }

    public IdentifyProtocol() {
        LocalInfo = new IdentifyInfo();
        LocalInfo.AgentVersion = "arc-p2p/1.0.0";
        LocalInfo.ProtocolVersion = "ipfs/0.1.0";
        LocalInfo.Protocols = new List<string>();
    }

    public void RegisterProtocol(string protocolId) { LocalInfo.Protocols.Add(protocolId); }
}