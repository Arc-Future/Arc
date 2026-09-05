// IConnection —— 拆分自 ITransport.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public interface IConnection {
    PeerId RemotePeerId { get; }
    bool IsConnected { get; }
    async Task<IStream> OpenStreamAsync(CancellationToken cancellationToken);
    async Task<IStream> AcceptStreamAsync(CancellationToken cancellationToken);
    async Task<void> SendDatagramAsync(string data, CancellationToken cancellationToken);
    async Task<void> CloseAsync(CancellationToken cancellationToken);
}
