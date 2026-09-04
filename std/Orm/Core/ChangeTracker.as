// L3 骨架：ChangeTracker — scoped 变更追踪器（内存可证伪；≠ 写库）。
namespace Arc.Orm;

using Arc.Collections;

/// <summary>
/// 变更追踪器——DbContext 私有，scoped 生命周期。
///
/// 可证伪面：Add/Update/Remove → GetPendingChanges → AcceptAllChanges（纯内存）。
/// 禁止与 EFCore 做性能对照宣称；持久化属 Provider / SaveChanges 后置。
/// </summary>
public class ChangeTracker {
    public ChangeTracker() {
    }

    /// <summary>当前追踪的实体条目数。</summary>
    public int Count {
        get { return this.Entries.Count; }
    }

    /// <summary>追踪条目集合（直接暴露内部 List，避免拷贝）。</summary>
    public List<EntityEntry> Entries { get; } = new List<EntityEntry>();

    /// <summary>标记实体为 Added（SaveChangesAsync 时生成 INSERT）。</summary>
    public void Add(object entity, string entityTypeName) {
        this.Entries.Add(new EntityEntry(entity, EntityState.Added, entityTypeName));
    }

    /// <summary>标记实体为 Modified（SaveChangesAsync 时生成 UPDATE）。</summary>
    public void Update(object entity, string entityTypeName) {
        this.Entries.Add(new EntityEntry(entity, EntityState.Modified, entityTypeName));
    }

    /// <summary>标记实体为 Deleted（SaveChangesAsync 时生成 DELETE）。</summary>
    public void Remove(object entity, string entityTypeName) {
        this.Entries.Add(new EntityEntry(entity, EntityState.Deleted, entityTypeName));
    }

    /// <summary>清空所有追踪条目（DbContext.Dispose 调用）。</summary>
    public void Clear() {
        this.Entries.Clear();
    }

    /// <summary>
    /// 获取待提交变更分类——O(N) 单次遍历。
    /// </summary>
    public PendingChanges GetPendingChanges() {
        PendingChanges pending = new PendingChanges();
        int n = this.Entries.Count;
        int i = 0;
        while (i < n) {
            EntityEntry entry = this.Entries[i];
            EntityState s = entry.State;
            if (s == EntityState.Added) {
                pending.Added.Add(entry);
            } else if (s == EntityState.Modified) {
                pending.Modified.Add(entry);
            } else if (s == EntityState.Deleted) {
                pending.Deleted.Add(entry);
            }
            i = i + 1;
        }
        pending.TotalCount = pending.Added.Count + pending.Modified.Count + pending.Deleted.Count;
        return pending;
    }

    /// <summary>
    /// 接受所有变更——将 Added/Modified 重置为 Unchanged，移除 Deleted 条目。
    ///
    /// 双指针压缩算法（零分配，O(N) 单次遍历）：
    ///   - write 指针指向下一个保留位置
    ///   - Deleted 条目跳过（不写入），其余状态重置为 Unchanged 并写到 write 位置
    ///   - 最终截断 List 长度到 write 指针位置
    /// </summary>
    public void AcceptAllChanges() {
        int n = this.Entries.Count;
        int write = 0;
        int read = 0;
        while (read < n) {
            EntityEntry entry = this.Entries[read];
            EntityState s = entry.State;
            if (s != EntityState.Deleted) {
                entry.State = EntityState.Unchanged;
                this.Entries[write] = entry;
                write = write + 1;
            }
            read = read + 1;
        }
        // 截断尾部：从末尾移除多余条目（write..n 范围）
        // 调用 RemoveAt 从后向前，避免下标漂移
        int tail = n - 1;
        while (tail >= write) {
            this.Entries.RemoveAt(tail);
            tail = tail - 1;
        }
    }
}
