// ContributeOptions —— 贡献注册元数据（RFC 045 D11 修订）。
//
// 描述贡献项的组织方式：分组、排序与父级归属，供插件容器在
// Register 时对扩展项做层级与顺序编排。值类型语义（结构体）。
namespace Arc.Chord;

/// <summary>
/// 贡献注册元数据：分组 / 排序 / 父级标识。
/// </summary>
public struct ContributeOptions {

    /// <summary>默认组织构造（零分组、零顺序、无父级）。</summary>
    public ContributeOptions() {
        this.GroupId = 0;
        this.Order = 0;
        this.ParentId = null;
    }

    /// <summary>完整组织构造。</summary>
    public ContributeOptions(int groupId, int order, string? parentId) {
        this.GroupId = groupId;
        this.Order = order;
        this.ParentId = parentId;
    }

    /// <summary>分组（同组贡献归入同一功能区）。</summary>
    public int GroupId { get; }

    /// <summary>排序（同组内执行 / 展示顺序）。</summary>
    public int Order { get; }

    /// <summary>父级贡献标识（层级归属；null 为顶层）。</summary>
    public string? ParentId { get; }
}
