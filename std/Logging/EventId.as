// EventId —— 日志事件标识（对齐 .NET Microsoft.Extensions.Logging.EventId）。
namespace Arc.Logging;

/// <summary>
/// 日志事件标识——用于区分同一类别下的不同日志事件，便于检索与过滤。
/// <c>Id</c> 为整数编码，<c>Name</c> 为可读名称（可为空）。
/// </summary>
public struct EventId {

    /// <summary>仅整数编码构造。</summary>
    public EventId(int id) {
        this.Id = id;
        this.Name = "";
    }

    /// <summary>整数编码 + 可读名称构造。</summary>
    public EventId(int id, string name) {
        this.Id = id;
        this.Name = name;
    }

    /// <summary>事件整数编码。</summary>
    public int Id { get; }

    /// <summary>事件可读名称（可为空串）。</summary>
    public string Name { get; }

    /// <summary>事件为零（默认事件，未显式指定）。</summary>
    public bool IsDefault {
        get { return this.Id == 0 && (this.Name == null || this.Name == ""); }
    }

    /// <summary>字符串表示：有名称用 <c>Name(Id)</c>，否则仅 <c>Id</c>。</summary>
    public string ToString() {
        if (this.Name == null || this.Name == "") {
            return "" + this.Id;
        }
        return this.Name + "(" + this.Id + ")";
    }
}
