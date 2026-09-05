// RFC 042: ITransport / IConnection / IStream 传输接口。
namespace Arc.Net.P2P;

public interface ITransport {
    async Task<IConnection> DialAsync(Multiaddr addr, CancellationToken cancellationToken);
    async Task<void> ListenAsync(Multiaddr addr, CancellationToken cancellationToken);
}
