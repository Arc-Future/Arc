// Phase A: IQueryable<T> — 查询接口契约（暂不可用，保留作为架构契约文档）
//
// Arc 异步一体原则 + C# 命名规范：
//   - 查询构建方法（Where/Select/OrderBy/Skip/Take/Distinct）保持同步——纯表达式树组合
//   - 执行方法（ToListAsync/FirstAsync/CountAsync 等）均为 async，Async 后缀 + CancellationToken 重载
//   - 对标 C# System.Linq.IQueryable<T>
//
// 注意：Arc typeck 当前不支持泛型类实现泛型接口。此接口作为架构契约保留，
// 链式方法在 EntityQueryable<T> 具体基类上承载，待 typeck 支持后恢复接口实现。
namespace Arc.Linq;

using Arc;
using Arc.Collections;
using Arc.Linq.Expressions;

public interface IQueryable<T> {
    IQueryProvider Provider { get; }
    Expression Expression { get; }

    // ── 查询构建（同步，纯表达式树组合）──

    IQueryable<T> Where(Expression<Func<T, bool>> predicate);
    IQueryable<U> Select<U>(Expression<Func<T, U>> selector);
    IQueryable<T> OrderBy(Expression<Func<T, object>> keySelector);
    IQueryable<T> OrderByDescending(Expression<Func<T, object>> keySelector);
    IQueryable<T> Skip(int count);
    IQueryable<T> Take(int count);
    IQueryable<T> Distinct();

    // ── 执行方法（异步 I/O）──

    Task<List<T>> ToListAsync();
    Task<List<T>> ToListAsync(CancellationToken cancellationToken);

    Task<T> FirstAsync();
    Task<T> FirstAsync(CancellationToken cancellationToken);

    Task<T> FirstOrDefaultAsync();
    Task<T> FirstOrDefaultAsync(CancellationToken cancellationToken);

    Task<T> SingleAsync();
    Task<T> SingleAsync(CancellationToken cancellationToken);

    Task<int> CountAsync();
    Task<int> CountAsync(CancellationToken cancellationToken);

    Task<bool> AnyAsync(Expression<Func<T, bool>> predicate);
    Task<bool> AnyAsync(Expression<Func<T, bool>> predicate, CancellationToken cancellationToken);

    Task<bool> AllAsync(Expression<Func<T, bool>> predicate);
    Task<bool> AllAsync(Expression<Func<T, bool>> predicate, CancellationToken cancellationToken);

    Task<int> SumAsync(Expression<Func<T, int>> selector);
    Task<int> SumAsync(Expression<Func<T, int>> selector, CancellationToken cancellationToken);

    Task<int> MaxAsync(Expression<Func<T, int>> selector);
    Task<int> MaxAsync(Expression<Func<T, int>> selector, CancellationToken cancellationToken);

    Task<int> MinAsync(Expression<Func<T, int>> selector);
    Task<int> MinAsync(Expression<Func<T, int>> selector, CancellationToken cancellationToken);
}
