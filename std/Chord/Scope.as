// Scope —— IScope 内部实现（RFC 045 D1/D7/D9）。
//
// 状态迁移仅由内核（ChordContext）驱动：创建 Pending → Start 置 Active →
// apply 失败置 Failed（携带 Error）→ 释放置 Disposed。
namespace Arc.Chord;

internal class Scope : IScope {
    private int _uid;
    private string _name;
    private ScopeStatus _status;
    private object? _config;
    private string? _error;

    internal Scope(int uid, string name, object? config) {
        _uid = uid;
        _name = name;
        _config = config;
        _status = ScopeStatus.Pending;
    }

    public int Uid { get { return _uid; } }

    public string Name { get { return _name; } }

    public ScopeStatus Status { get { return _status; } }

    public object? Config { get { return _config; } }

    public string? Error { get { return _error; } }

    internal void SetActive() {
        _status = ScopeStatus.Active;
        _error = null;
    }

    internal void SetFailed(string message) {
        _status = ScopeStatus.Failed;
        _error = message;
    }

    internal void SetDisposed() {
        _status = ScopeStatus.Disposed;
    }
}
