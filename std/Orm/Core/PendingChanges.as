// L3 骨架：PendingChanges — 待提交变更分类。
namespace Arc.Orm;

using Arc.Collections;

/// <summary>
/// 待提交变更分类——SaveChangesAsync 的输入。
///
/// 三个 List<EntityEntry> 字段分别持有 Added/Modified/Deleted 条目。
/// 由 ChangeTracker.GetPendingChanges() 单次遍历填充。
/// </summary>
public class PendingChanges {
    /// <summary>Added 条目（待 INSERT）。</summary>
    public List<EntityEntry> Added;

    /// <summary>Modified 条目（待 UPDATE）。</summary>
    public List<EntityEntry> Modified;

    /// <summary>Deleted 条目（待 DELETE）。</summary>
    public List<EntityEntry> Deleted;

    /// <summary>总待处理条目数（Added + Modified + Deleted）。</summary>
    public int TotalCount;

    public PendingChanges() {
        this.Added = new List<EntityEntry>();
        this.Modified = new List<EntityEntry>();
        this.Deleted = new List<EntityEntry>();
        this.TotalCount = 0;
    }
}
