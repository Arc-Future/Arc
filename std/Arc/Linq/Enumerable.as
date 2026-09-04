namespace Arc.Linq;

using Arc.Collections;

/// <summary>
/// LINQ to Objects 扩展方法面——对标 C# <c>System.Linq.Enumerable</c> 约定。
///
/// <b>诚实子集（Stable）</b>：由 MIR 编译期展开，不依赖本类方法体：
/// - query comprehension：<c>from … where … select …</c>（数组 / <c>List&lt;T&gt;</c>）
/// - 方法链：<c>.Where(…).</c><c>Select(…)</c>（同 MIR <c>try_lower_linq_chain</c>）
/// - <c>orderby</c> / <c>OrderBy</c> / <c>OrderByDescending</c>：真排序（无捕获 key；
///   缓冲 <c>List&lt;T&gt;</c> + <c>rt_list_sort</c> comparator；数组 / <c>List</c> 源；
///   数值 / <c>bool</c> / <c>char</c> / <c>string</c> / 可 <c>CompareTo</c> key；
///   <c>descending</c> 取反）。捕获 key 或不可比较类型诚实跳过（同
///   <c>List.Sort(cmp)</c> 限制）
/// - 多键排序：连续 <c>orderby k1, k2</c>（或 <c>OrderBy</c> 链）折叠为单 comparator，
///   先 k1 后 k2 依次生效——对标 C# <c>OrderBy(…).ThenBy(…)</c>，不依赖 qsort 稳定性
/// - <c>let</c> / <c>join</c> / <c>groupby</c>：查询子句多变量流由 MIR 特化物化
///   （RFC 003 编译期展开红线不变）；<c>join</c> 为 inner join（等值条件
///   <c>on outer.key == inner.key</c>，内层源为 <c>List&lt;T&gt;</c>）；<c>group … by …
///   [into g]</c> 产物为 <see cref="Grouping{K, T}"/>（首次出现序，等值判定走
///   key 的 <c>Compare == 0</c>，与 orderby 同支持面）
/// - 终端：<c>Any</c> / <c>Count</c> / <c>First</c> / <c>FirstOrDefault</c>
///   （0 参或单谓词；数组 / <c>List&lt;T&gt;</c>；可接 Where/Select/OrderBy 前缀；同 MIR
///   <c>try_parse_linq_terminal</c>）。
///   空序列 <c>First</c> → <c>rt_panic</c>（非静默零值；异常对象 Throw 在表达式
///   实参上下文仍有已知债，不以假异常面冒充 C# <c>InvalidOperationException</c>）；
///   空序列 / 无匹配 <c>FirstOrDefault</c> → <c>default(T)</c>（标量 0 / 引用 null）
/// - 泛型物化终端：<c>ToList</c> / <c>ToArray</c>（RFC 007；0 值实参；任意可枚举源
///   ——数组 / <c>List&lt;T&gt;</c> / 查询链；MIR <c>lower_linq_terminal</c> 整链物化，
///   <c>ToList</c> → <c>List_&lt;T&gt;</c>、<c>ToArray</c> → <c>T[]</c>（经 List facade
///   <c>rt_list_to_array</c>）。裸 <c>List&lt;T&gt;.ToArray()</c> 走 OOP facade，非 LINQ 路径）
/// - foreach 流式展开（<c>lower_linq_foreach</c>）
/// 证据：<c>UnitTest/Arc/LinqTests</c>（含方法链、终端、orderby 真排序与多键用例）；
/// 原 <c>linq_let_join_groupby_e2e</c> / <c>linq_to_list_e2e</c> 已随 arc-integration
/// 退场（a2627a0f）。
///
/// <b>未落地（禁止冒充 Stable）</b>：
/// - 赋值物化 <c>List&lt;T&gt; xs = from …</c>：MIR <c>materialize_linq_chain_to_list</c> 已有路径；
///   赋值目标 typeck 仍后置——UnitTest 以 foreach 计数证明过滤/投影
/// - <c>join … into</c>（group join）、相关（correlated）join 内层源、分组值
///   默认相等（对象哈希）——诚实后置
/// - Queryable / Orm Provider = L3 后置（本面非 Queryable）
/// </summary>
public static class Enumerable {
}
