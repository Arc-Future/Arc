// RFC 042: IRoom — P2P 协作房间接口桩（无 event，解析器限制）。
namespace Arc.Net.P2P;

public interface IRoom {
    string RoomId { get; }
    List<Peer> Members { get; }
    async Task<void> JoinAsync(CancellationToken cancellationToken);
    async Task<void> LeaveAsync(CancellationToken cancellationToken);
    async Task<void> BroadcastAsync(string data, CancellationToken cancellationToken);
    async Task<void> SendToAsync(PeerId target, string data, CancellationToken cancellationToken);
}
