// RFC 022 Sprint 2c: 表达式树求值上下文接口
//
// 解耦 Expression 类与具体数据源：Expression 的 EvalInt/EvalBool 虚方法
// 通过此接口访问字段值，不直接依赖 std/Orm/DataRow。
namespace Arc.Linq.Expressions;

/// 表达式树求值上下文——由数据源实现，供 EvalInt/EvalString/EvalBool 查找字段值。
///
/// 绑定约定：`ParameterExpression` 按 `Name`、`MemberExpression` 按 `Member`
/// 经本接口取值；`Has(name)==false` 时 `Eval*` 抛 `InvalidOperationException`
/// （禁止默默返回 0）。
///
/// 索引约定（RFC 022 §9.4.8）：`IndexExpression` 按集合名 + int 下标经
/// `HasAt`/`Get*At` 取值；未绑定同样硬错误。
public interface IEvalContext {
    /// <summary>上下文是否提供指定名（字段/成员或已绑定形参）。</summary>
    /// <param name="name">MemberExpression.Member 或 ParameterExpression.Name。</param>
    /// <returns>已绑定为 true，否则 false。</returns>
    bool Has(string name);

    /// <summary>按字段名获取整数值。</summary>
    /// <param name="name">字段名。</param>
    /// <returns>字段对应的整数值。</returns>
    int GetInt(string name);

    /// <summary>按字段名获取布尔值。</summary>
    /// <param name="name">字段名。</param>
    /// <returns>字段对应的布尔值。</returns>
    bool GetBool(string name);

    /// <summary>按字段名获取字符串值。</summary>
    /// <param name="name">字段名。</param>
    /// <returns>字段对应的字符串值。</returns>
    string GetString(string name);

    /// <summary>上下文是否提供指定集合名的下标槽位。</summary>
    bool HasAt(string name, int index);

    /// <summary>按集合名 + 下标获取整数值。</summary>
    int GetIntAt(string name, int index);

    /// <summary>按集合名 + 下标获取布尔值。</summary>
    bool GetBoolAt(string name, int index);

    /// <summary>按集合名 + 下标获取字符串值。</summary>
    string GetStringAt(string name, int index);
}
