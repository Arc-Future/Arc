// Phase A: IQueryProvider — 查询执行接口
//
// Arc 异步一体原则 + C# 命名规范：
//   - I/O 方法 Async 后缀 + CancellationToken
//   - 无参重载收敛：同名重载会导致 itable 符号碰撞（仅保留 CT 重载）
//
// 注意：IQueryable<T> 泛型接口暂不可用（Arc typeck 不支持泛型类实现泛型接口）。
namespace Arc.Linq;

using Arc;
using Arc.Collections;
using Arc.Linq.Expressions;

/// <summary>查询提供者接口——ORM Provider 实现此接口。</summary>
public interface IQueryProvider {
    /// <summary>翻译并执行表达式树，返回结果列表（可取消）。</summary>
    Task<List<T>> ExecuteAsync<T>(Expression expression, CancellationToken cancellationToken);

    /// <summary>翻译并执行标量查询（可取消）。</summary>
    Task<R> ExecuteScalarAsync<R>(Expression expression, CancellationToken cancellationToken);
}
