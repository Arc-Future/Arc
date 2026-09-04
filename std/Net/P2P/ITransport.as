// RFC 042: ITransport / IConnection / IStream 传输接口。
namespace Arc.Net.P2P;

public interface ITransport {
    async Task<IConnection> DialAsync(Multiaddr addr, CancellationToken cancellationToken);
    async Task<void> ListenAsync(Multiaddr addr, CancellationToken cancellationToken);
}

public interface IConnection {
    PeerId RemotePeerId { get; }
    bool IsConnected { get; }
    async Task<IStream> OpenStreamAsync(CancellationToken cancellationToken);
    async Task<IStream> AcceptStreamAsync(CancellationToken cancellationToken);
    async Task<void> SendDatagramAsync(string data, CancellationToken cancellationToken);
    async Task<void> CloseAsync(CancellationToken cancellationToken);
}

public interface IStream {
    int StreamId { get; }
    async Task<void> WriteAsync(string data, CancellationToken cancellationToken);
    async Task<string> ReadAsync(CancellationToken cancellationToken);
    async Task<void> CloseWriteAsync(CancellationToken cancellationToken);
    async Task<void> CloseAsync(CancellationToken cancellationToken);
}
