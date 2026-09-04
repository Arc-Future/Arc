// L3 骨架 + execute-MVP：DbContext — ORM 会话基类。
//
// 诚实边界：
//   - ChangeTracker / ModelCache 可内存演练（Sprint0）
//   - SaveChanges：有挂起 → 返回 -1 且不 Accept（禁 return 1 假成功）；无挂起 → 0
//   - INSERT/UPDATE/DELETE SQL 构造 = 后置（不扩 IDbProvider）
namespace Arc.Orm;

using Arc;
using Arc.Data;
using Arc.Linq.Expressions;

public class DbContext : IDisposable {
    private string _contextTypeName;
    private bool _disposed;

    public DbContext() {
        this.ChangeTracker = new ChangeTracker();
        _contextTypeName = "";
        _disposed = false;
    }

    protected IDbProvider Provider { get; set; }

    protected void SetProvider(IDbProvider provider) {
        this.Provider = provider;
    }

    protected void SetContextTypeName(string contextTypeName) {
        _contextTypeName = contextTypeName;
        this.Model = ModelCache.Get(_contextTypeName);
        if (this.Model == null) {
            this.Model = this.OnModelCreating();
            ModelCache.Set(_contextTypeName, this.Model);
        }
    }

    protected FrozenModel Model { get; set; }

    protected ChangeTracker ChangeTracker { get; }

    /// <summary>证伪入口：暴露 ChangeTracker（写库仍后置）。</summary>
    public ChangeTracker Tracker {
        get { return this.ChangeTracker; }
    }

    protected virtual FrozenModel OnModelCreating() {
        return new FrozenModel(_contextTypeName);
    }

    protected Expression TableExpression(string tableName) {
        ConstantExpression tableExpr = new ConstantExpression();
        tableExpr.StringValue = tableName;
        MethodCallExpression tableCall = new MethodCallExpression();
        tableCall.MethodName = "Table";
        tableCall.Arg0 = tableExpr;
        return tableCall;
    }

    /// <summary>异步提交变更。有挂起 → -1（禁假成功 1）；无挂起 → 0；不 AcceptAllChanges。</summary>
    public async Task<int> SaveChangesAsync() {
        return await this.SaveChangesAsync(new CancellationToken());
    }

    /// <summary>同步提交（委托给异步版本）。遗留兼容入口；新产品代码应使用 SaveChangesAsync。</summary>
    public int SaveChanges() {
        // 注意：勿把 PendingChanges 赋给局部再读 TotalCount（struct 拷贝丢字段 codegen 债）
        int n = this.Tracker.GetPendingChanges().TotalCount;
        if (n > 0) {
            return -1;
        }
        return 0;
    }

    /// <summary>骨架/证伪：向 ChangeTracker 添加实体（写库仍后置）。</summary>
    public void TrackAdd(object entity, string entityTypeName) {
        this.Tracker.Add(entity, entityTypeName);
    }

    /// <summary>当前挂起变更数（证伪 SaveChanges 不 Accept）。</summary>
    public int PendingCount() {
        return this.Tracker.GetPendingChanges().TotalCount;
    }

    /// <summary>异步提交（带取消令牌）。有挂起 → -1（禁假成功 1）；无挂起 → 0；不 AcceptAllChanges。</summary>
    public async Task<int> SaveChangesAsync(CancellationToken cancellationToken) {
        if (_disposed) {
            return 0;
        }
        cancellationToken.ThrowIfCancellationRequested();
        int n = this.Tracker.GetPendingChanges().TotalCount;
        if (n > 0) {
            return -1;
        }
        return 0;
    }

    public void Dispose() {
        if (!_disposed) {
            _disposed = true;
            if (this.ChangeTracker != null) {
                this.ChangeTracker.Clear();
            }
        }
    }
}
