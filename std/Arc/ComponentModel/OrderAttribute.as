// RFC 009 M3: 标准库属性 — 执行顺序控制 [Order]。
//
// 通用顺序控制元数据，不绑定特定领域。
// 典型用法之一：QIF 测试方法执行顺序（与 [Fact]/[Theory] 配合，
//   相同 Order 值的测试可并行执行，不同 Order 组间按升序串行）。

namespace Arc.ComponentModel;

using Arc;

/// <summary>
/// 指定元素在同类元素中的执行/显示顺序。
///
/// 用法：`[Order(0)]`、`[Order(1)]`（值越小越靠前，可为负数）。
/// 合法附加目标：All（具体语义由消费方解释，如测试框架按方法 Order 分组并行）。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class OrderAttribute : Attribute {
    /// 顺序值（默认语义：升序排列，相同值的元素由消费方自定义策略处理）。
    public int Order { get; }

    public OrderAttribute(int order) { Order = order; }
}
