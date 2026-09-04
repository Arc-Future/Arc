# 10 表达式树

表达式树使 Lambda **在编译期**成为可遍历的常量数据结构，支撑 Queryable 路径与「代码即数据」信条。

**设计约束**：`Expression<T>` 类型的 Lambda 在编译期降为**静态 rodata / constexpr IR**；用户程序热路径中**不存在**运行时 AST walker。树结构在链接期完全已知，供编译期 Provider 插件消费，而非运行时解释。

## C# 表面语法

对齐 C#：使用类型 `Expression<Func<...>>`（或 `Expression<Func<T, R>>`）与**普通 Lambda**，**无** `expression` 关键字。

```as
Expression<Func<User, bool>> pred = u => u.Age >= 18 && u.Active;

var q = db.Users
    .Where(u => u.Age >= 18)
    .OrderBy(u => u.Name)
    .Select(u => u.Name);
```

（`IQueryable<T>` 链上的 Lambda 由 typeck 按 Queryable 路径树化；`expression u =>` 已废弃并报错。）

规则：

1. 声明为 `Expression<Func<...>>` 的 Lambda，或传给 `IQueryable<T>` 方法的 Lambda，编译为表达式树
2. `body` 限于可树化的表达式子集（成员访问、比较、逻辑运算、常量等）
3. 不得包含语句块、局部声明或任意副作用调用

## 类型 `Expression<T>`

Queryable 方法接受的谓词类型为 `Expression<Func<Arg, Ret>>` 等形式：

```as
// 概念性签名
IQueryable<U> Where<T, U>(this IQueryable<T> source, Expression<Func<T, bool>> pred);
```

`crates/typeck` 中对应 `TypeId::Expression { inner }`（编译器内部名）。

## IR：`crates/expr`

表达式树 IR（`ExpressionTree`）节点种类包括：

| 节点 | 含义 |
|------|------|
| Parameter | Lambda 参数 |
| MemberAccess | 字段/属性访问 |
| Binary / Unary | 运算 |
| Constant | 字面量 |
| Call | 可树化的调用子集 |
| Lambda | 嵌套 Lambda（受限） |

typeck 在检查 `Expression<T>` Lambda 时同步构建 `ExpressionTree`，供 linq 与 codegen 消费。

**边界**：`crates/expr` 仅定义树 IR 与**编译期**序列化辅助（如过渡 demo 用的 `tree_summary`——在 **codegen 阶段**将树格式化为 C 字符串字面量写入 rodata，而非运行时遍历）。**禁止**在编译器 crate 内实现 SQL 或其他查询方言翻译；此类逻辑属于 `std/Linq` 与 Provider 实现，且须在**构建时**完成。

## AOT 序列化

Queryable 链在编译期折叠为静态树，序列化至二进制 **只读段**（`.rodata`）——节点表、偏移表或编译期物化的 Provider 产物（如 SQL 字符串）。

```
.rodata:
  expression_tree_users_where_0:
    node_kind: Binary
    op: Gte
    left: MemberAccess(Age)
    right: Constant(18)
    ...
```

特性：

- **无运行时解释器**遍历源码字符串或动态 AST
- 相同 Query 在相同版本中树结构稳定
- Provider 通过符号或偏移读取树；翻译在**构建时**完成，产物链入二进制；`tree_summary` 摘要由 codegen 静态写入 rodata（`rt_expr_tree_summary` 仅返回静态指针），无运行时动态解析

## Provider 消费

```as
interface IQueryProvider {
    string Translate<T>(Expression<T> expression);
}
```

Provider 实现（位于 **`std/Linq`**，非编译器核心）：

1. **构建时**读取静态 `ExpressionTree`（自 rodata 或编译期常量折叠结果）
2. 验证节点组合合法
3. 翻译为目标查询语言或执行计划（SQL 仅为众多 Provider 之一），将结果物化为 rodata 或特化桩代码

运行时用户程序**不**执行步骤 1–3；至多调用已物化计划的入口（如执行静态 SQL 字符串）。

标准库入口：`std/Arc/Linq/`（`IQueryProvider` / `IQueryable<T>`）、`std/Arc/Linq/Expressions/`（节点类型）。通用 SQL 翻译在 `std/Orm/SqlTranslator.as`（`Arc.Orm`）；具体数据库提供程序（如 `SqliteProvider`）在平级方言子库 `std/Orm.SQLite/`（`Arc.Orm.SQLite`）。

## 与 Enumerable 的对比

| 维度 | Enumerable | Queryable + Expression |
|------|------------|------------------------|
| Lambda | 普通 `=>` | `Expression<Func<...>>` 或 IQueryable 链上的 `=>` |
| 展开时机 | 编译期脱糖为特化循环 | 编译期树化 + Provider 构建时翻译 |
| 执行时机 | 编译后运行时迭代（无委托） | 执行已物化计划；**无**运行时树遍历 |
| 堆分配 | 最小化 | 树在 rodata，无 per-call 解析 |
| 可分析性 | 编译期已知循环体 | 全静态结构 |

## 编译期展开 vs 运行时解释

| 机制 | 运行时解释 | Arc 目标（编译期展开） |
|------|------------|------------------------|
| `Expression<T>` 赋值 | 分配节点、动态 dispatch | `ExpressionTree::from_lambda`（根为 `Lambda`）→ MIR `ExpressionTreeConst` → codegen 运行时构造 `LambdaExpression` 对象树；`Eval*` 经虚方法 + `IEvalContext` 内存求值（非热路径 SQL 解释器） |
| Provider | `CreateQuery` 后 walk 树 | 构建时插件读树/rodata，产出 SQL/等 |
| 调试/demo | `rt_expr_tree_summary()` 动态格式化 | 允许作过渡；最终仅返回编译期物化的静态字符串指针 |

`Expression<T>` 在 MIR/codegen 中按运行时类名 `Expression` 解析方法（含虚 `EvalBool`/`EvalInt`/`EvalString`）；`receiver_type == "unknown"` 时 codegen **硬错误**，禁止生成 `@unknown_*`。

内存求值：`ParameterExpression` 按 `Name`、`MemberExpression` 按 `Member` 经 `IEvalContext.Has`/`GetInt`/`GetBool`/`GetString` 取值；`Has` 为 false 时抛 `InvalidOperationException`（禁止默默 0）。`MemberExpression.EvalBool` 走 `GetBool(Member)`；`CaptureExpression` 按捕获类型写入独立快照字段 `IntValue`/`BoolValue`/`StringValue`。`BinaryExpression` 的 `==`/`!=`：操作数为 bool（`TypeName=="bool"` 或关系/逻辑子表达式）时走 `EvalBool`，为 string（`TypeName=="string"`）时走 `EvalString`，勿一律 `EvalInt`。`MemberAccess` IR 携带成员类型（`ty` → codegen `TypeName`），使 `Member==Member` 两侧均可分派。`ConditionalExpression` 按 `Cond` 分派 `Then`/`Else` 的 `Eval*`；codegen 写入结果 `TypeName` 供嵌套比较。MIR 对 `from_lambda` 失败硬错误，禁止静默 `Constant(true)`。

详见 [09 查询语言](09-query-language.md) 性能契约。

## 扩展节点

新增树节点种类须：

1. 更新 `std/Arc/Linq/Expressions/` 下对应节点声明
2. 更新 `crates/expr` 与 typeck 构建逻辑
3. 提交 RFC 或规范补丁

## 表达式节点类型

引入完整 Type 体系后，本节描述的 Expression 节点字段类型从 `string` 修订为 `Type` / `MemberInfo` / `MethodInfo` 等强类型。迁移路径：

| 节点 | 字段（前） | 字段（后） | 说明 |
|------|----------|----------|------|
| `Expression`（基类） | `Type: string` | `Type: Type` | 表达式运行时类型由字符串改为 `Type` 强类型 |
| `MemberExpression` | `MemberName: string` | `Member: MemberInfo` | 字段/属性元数据由字符串改为 `MemberInfo` |
| `MethodCallExpression` | `MethodName: string` | `Method: MethodInfo` | 方法签名由字符串改为 `MethodInfo` |
| `CastExpression` | `TargetType: string` | `TargetType: Type` | 目标类型由字符串改为 `Type` |
| `NewExpression` | `TypeName: string` | `Type: Type` | 构造类型由字符串改为 `Type` |
| `LambdaExpression` | `ReturnType: string` | `ReturnType: Type` | 返回类型由字符串改为 `Type` |
| `ParameterExpression` | 仅 `Name: string` | 新增 `ParameterType: Type` | 参数类型信息 |
| `TypeOfExpression` | `TypeName: string` | 新增 `Type: Type` | typeof(T) 表达式树节点 |

**迁移分阶段**：

- **第一阶段**：新增 `Type` / `MemberInfo` 字段，**保留**旧 `string` 字段作为别名（getter 调用 `Type.FullName` / `MemberInfo.Name`）
- **第二阶段**：移除旧 `string` 字段，全面切换到强类型

---

上一节：[09 查询语言](09-query-language.md) · 下一节：[11 编译模型](11-compilation-model.md)