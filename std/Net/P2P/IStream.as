// IStream —— 拆分自 ITransport.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public interface IStream {
    int StreamId { get; }
    async Task<void> WriteAsync(string data, CancellationToken cancellationToken);
    async Task<string> ReadAsync(CancellationToken cancellationToken);
    async Task<void> CloseWriteAsync(CancellationToken cancellationToken);
    async Task<void> CloseAsync(CancellationToken cancellationToken);
}
