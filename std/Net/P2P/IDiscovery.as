// RFC 042 D3: IDiscovery — 节点发现抽象接口桩（无 event，解析器限制）。
namespace Arc.Net.P2P;

public interface IDiscovery {
    async Task<void> StartAsync(CancellationToken cancellationToken);
    async Task<void> StopAsync(CancellationToken cancellationToken);
}
