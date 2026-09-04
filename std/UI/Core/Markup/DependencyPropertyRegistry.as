// RFC 037 D1: Arc.UI — DependencyPropertyRegistry 全局分配器 + 类型作用域索引。
//
// 三职责：
//   1. NextId()：分配全局唯一 Id（既有，供 RegisterProperty<T> 使用）。
//   2. 类型作用域索引：按 OwnerType.TypeId 登记「属性名 → DependencyProperty」，
//      供目标元素沿类型链按名解析 DP（StyleEvaluator 动态分派）。
//   3. 环境属性登记（RFC 037 §4）：注册期元数据声明 Inherits 的 DP 按 Id 登记，
//      供 AddChild 挂接时重算继承槽（RefreshInheritanceFromAncestors）枚举——
//      继承属性集合完全由注册驱动，零硬编码。
//
// 单一惯用：本类只做类型导出，无业务代码——业务分派在 Element.ResolveProperty
// 与 StyleEvaluator 中。
//
// 擦除视图说明：value 存 `object`。Arc 泛型模板（DependencyProperty<T>）按
// RFC 018 M4-1 不参与类型注册，无法以非泛型类型作方法实参传递；存 object
// 上转型恒成立，StyleEvaluator 再经 `is`/cast 分派值种类
//（与 ItemsControl 的 `src is List<string>` 同构）。
//
// **避免嵌套泛型 Dictionary**：Arc codegen 不支持 `Dictionary<int, Dictionary<...>>`
// 这种「值为另一泛型」的嵌套字典——其静态字段初始化器不执行、字段恒为 null。
// 故用非泛型包装类 OwnerPropertyScope 持有内层 `Dictionary<string, object>`，
// 使 `_byOwner` 退化为单层泛型 `Dictionary<int, OwnerPropertyScope>`（与
// CultureInfo._cache 等同构，可用）。

namespace Arc.UI;

using Arc.Collections;

/// <summary>
/// 单个所有者类型下的「属性名 → DependencyProperty」作用域（非泛型包装，
/// 规避嵌套泛型 Dictionary 的 codegen 限制）。
/// </summary>
internal class OwnerPropertyScope {
    /// <summary>属性名 → DP（object 擦除视图）映射。</summary>
    public Dictionary<string, object> Entries;

    /// <summary>构造空作用域。</summary>
    public OwnerPropertyScope() {
        Entries = new Dictionary<string, object>();
    }
}

/// <summary>
/// 依赖属性全局分配器 + 类型作用域索引。
/// </summary>
/// <remarks>
/// 普通类 + private 构造函数模拟静态类——Arc 当前限制为 static class 不允许
/// 任何字段（包括 static 字段）。改用普通 class 承载 static 字段。
/// </remarks>
public class DependencyPropertyRegistry {
    /// <summary>下一个可用 Id（单调递增）。</summary>
    private static long _nextId;

    /// <summary>
    /// 类型作用域索引：OwnerType.TypeId → OwnerPropertyScope（属性名 → DP，object 擦除视图）。
    /// 急切初始化安全：codegen 静态初始化依赖分析**穿透被调函数体**（`RegisterProperty`
    /// → `NextId`/`Register` 读写本类静态字段），`__sinit_DependencyPropertyRegistry` 恒排在
    /// 所有 `RegisterProperty` 调用之前执行（RFC 006 M4 方案 B，CD-5 架构级修复）。
    /// TypeId 为 <see cref="Type.TypeId"/>（int，FNV-1a 哈希），故 key 用 int。
    /// </summary>
    private static Dictionary<int, OwnerPropertyScope> _byOwner =
        new Dictionary<int, OwnerPropertyScope>();

    /// <summary>
    /// 所有者 TypeId 的**注册顺序**（append-only）。`_byOwner` 的迭代序非契约，
    /// `FindGlobal` 需要确定性遍历（同名 DP 按声明序稳定裁决，如 Control 先于
    /// Panel 注册 Background、TextBlock 先于 TextBox 注册 Text）——注册序即类型静态字段
    /// 初始化拓扑序（codegen 确定性输出）。
    /// </summary>
    private static List<int> _ownerOrder = new List<int>();

    /// <summary>
    /// 环境属性 Id 集（注册期元数据 MarkInherited 登记；集合极小，线性扫描即可）。
    /// 急切初始化安全：同 _byOwner（CD-5，__sinit 恒先于所有 RegisterProperty 执行）。
    /// </summary>
    private static List<long> _inheritedIds = new List<long>();

    private DependencyPropertyRegistry() {
        // 防止实例化——所有成员均为 static
    }

    /// <summary>
    /// 登记环境属性（注册期元数据 Inherits 声明时由 RegisterProperty 工厂调用）。
    /// </summary>
    /// <param name="id">依赖属性 Id。</param>
    public static void MarkInherited(long id) {
        _inheritedIds.Add(id);
    }

    /// <summary>
    /// 已登记的环境属性 Id 集（挂接重算继承槽时枚举；只读视图语义，调用方不得修改）。
    /// </summary>
    /// <returns>环境属性 Id 列表。</returns>
    public static List<long> InheritedIds() {
        return _inheritedIds;
    }

    /// <summary>
    /// 分配下一个全局唯一 Id。
    /// </summary>
    /// <returns>新分配的 Id。</returns>
    public static long NextId() {
        long id = _nextId;
        _nextId = _nextId + 1;
        return id;
    }

    /// <summary>
    /// 按所有者类型登记依赖属性（属性名 → DP，object 擦除视图）。
    /// </summary>
    /// <param name="ownerTypeId">所有者类型 TypeId。</param>
    /// <param name="name">属性名。</param>
    /// <param name="dp">依赖属性（以 object 擦除视图存储）。</param>
    public static void Register(int ownerTypeId, string name, object dp) {
        OwnerPropertyScope scope = null;
        if (_byOwner.ContainsKey(ownerTypeId)) {
            scope = _byOwner[ownerTypeId];
        } else {
            scope = new OwnerPropertyScope();
            _byOwner[ownerTypeId] = scope;
            _ownerOrder.Add(ownerTypeId);
        }
        scope.Entries[name] = dp;
    }

    /// <summary>
    /// 在指定所有者类型下按名查找依赖属性（返回 object 擦除视图）。
    /// </summary>
    /// <param name="ownerTypeId">所有者类型 TypeId。</param>
    /// <param name="name">属性名。</param>
    /// <returns>命中的依赖属性（object）；未登记返回 null。</returns>
    public static object Find(int ownerTypeId, string name) {
        if (_byOwner.ContainsKey(ownerTypeId)) {
            OwnerPropertyScope scope = _byOwner[ownerTypeId];
            if (scope.Entries.ContainsKey(name)) {
                return scope.Entries[name];
            }
        }
        return null;
    }

    /// <summary>
    /// 全局按名查找：沿所有者**注册顺序**遍历全部作用域，返回首个登记该名的 DP。
    /// 供 <see cref="Element.ResolveProperty"/> 类型链未命中时回退——mock / TypeName
    /// 标识元素（运行时 Type 链不含 DP 所有者，如 Element + TypeName="Button" 命中
    /// Control 作用域）也能按名解析。注册序即类型静态字段初始化拓扑序（确定性），
    /// 同名 DP（Control/Panel 均注册 Background、TextBlock/TextBox 均注册 Text）按声明序
    /// 稳定裁决。
    /// </summary>
    /// <param name="name">属性名。</param>
    /// <returns>首个登记该名的依赖属性（object）；全局未登记返回 null。</returns>
    public static object FindGlobal(string name) {
        int count = _ownerOrder.Count;
        for (int i = 0; i < count; i++) {
            object dp = Find(_ownerOrder[i], name);
            if (dp != null) {
                return dp;
            }
        }
        return null;
    }
}
