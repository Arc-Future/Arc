// RFC 042 D5/D7: IPeerConnection — 对等连接抽象接口桩（无 event，解析器限制）。
namespace Arc.Net.P2P;

public interface IPeerConnection {
    PeerId RemotePeerId { get; }
    bool IsConnected { get; }
    async Task<IStream> OpenStreamAsync(CancellationToken cancellationToken);
    async Task<IStream> AcceptStreamAsync(CancellationToken cancellationToken);
    async Task<void> SendDatagramAsync(string data, CancellationToken cancellationToken);
    async Task<void> CloseAsync(CancellationToken cancellationToken);
    void Write(string data);
    string Read();
}
