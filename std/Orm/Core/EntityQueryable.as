// Phase A: EntityQueryable<T> — 链式查询基类
//
// C# 命名规范：
//   - 查询构建方法（Where/Select/OrderBy 等）同步，纯表达式树组合
//   - 执行方法（ToListAsync/FirstAsync 等）异步，Async 后缀 + CancellationToken 重载
//
// 并发安全：
//   - 此类不保证线程安全（随 DbContext scoped 使用）
//   - 表达式树组合为纯内存操作，无共享状态竞争
//   - 实际 I/O 由 IDbProvider 执行，Provider 内部管理连接池/线程安全
//
// 注意：Arc typeck 暂不支持泛型类实现泛型接口，本类暂不声明 `: IQueryable<T>`。
// 异步执行方法（ToListAsync/FirstAsync 等 + CancellationToken 重载）契约
// 已在 IQueryable<T> 接口中完整定义；待 Phase B 接入 Provider.ExecuteAsync
// 后在此类落地具体实现。当前仅提供同步 ToList 用于 Phase A 翻译链路验证。
namespace Arc.Orm;

using Arc;
using Arc.Collections;
using Arc.Data;
using Arc.Linq;
using Arc.Linq.Expressions;

public class EntityQueryable<T> {
    private IDbProvider _provider;
    private Expression _expression;

    public EntityQueryable(IDbProvider provider, Expression expression) {
        _provider = provider;
        _expression = expression;
    }

    // ── 查询构建（同步，纯表达式树组合，无 I/O）──

    public EntityQueryable<T> Where(Expression<Func<T, bool>> predicate) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "Where";
        call.Target = _expression;
        call.Arg0 = predicate;
        return new EntityQueryable<T>(_provider, call);
    }

    public EntityQueryable<U> Select<U>(Expression<Func<T, U>> selector) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "Select";
        call.Target = _expression;
        call.Arg0 = selector;
        return new EntityQueryable<U>(_provider, call);
    }

    public EntityQueryable<T> OrderBy(Expression<Func<T, object>> keySelector) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "OrderBy";
        call.Target = _expression;
        call.Arg0 = keySelector;
        return new EntityQueryable<T>(_provider, call);
    }

    public EntityQueryable<T> OrderByDescending(Expression<Func<T, object>> keySelector) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "OrderByDescending";
        call.Target = _expression;
        call.Arg0 = keySelector;
        return new EntityQueryable<T>(_provider, call);
    }

    public EntityQueryable<T> Skip(int count) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "Skip";
        call.Target = _expression;
        ConstantExpression arg = new ConstantExpression();
        arg.IntValue = count;
        call.Arg0 = arg;
        return new EntityQueryable<T>(_provider, call);
    }

    public EntityQueryable<T> Take(int count) {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "Take";
        call.Target = _expression;
        ConstantExpression arg = new ConstantExpression();
        arg.IntValue = count;
        call.Arg0 = arg;
        return new EntityQueryable<T>(_provider, call);
    }

    public EntityQueryable<T> Distinct() {
        MethodCallExpression call = new MethodCallExpression();
        call.MethodName = "Distinct";
        call.Target = _expression;
        return new EntityQueryable<T>(_provider, call);
    }

    // ── 执行方法 ──
    //
    // 异步契约（ToListAsync/FirstAsync 等 + CancellationToken 重载）
    // 已在 IQueryable<T> 接口中完整定义。
    // 此处提供同步 ToList 占位实现（Phase B 接入 Provider.ExecuteAsync）。

    /// <summary>同步物化查询结果（Phase A 占位实现）。</summary>
    public List<T> ToList() {
        return new List<T>();
    }

    /// <summary>异步执行查询，物化为列表。</summary>
    public async Task<List<T>> ToListAsync() {
        return await this.ToListAsync(new CancellationToken());
    }

    /// <summary>异步执行查询（带取消令牌），物化为列表。</summary>
    /// <param name="cancellationToken">取消令牌。</param>
    public async Task<List<T>> ToListAsync(CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        // Phase A: 委托给同步 ToList（Provider.ExecuteAsync 待 Phase B 接入）
        return this.ToList();
    }
}
