# RFC 012 编译期元编程

## 背景

Arc 是**编译型、强类型、C# 表面**的 AOT 系统语言。编译期能力（属性、代码生成、表达式树、LINQ）优先于运行时解释。目标是**编译期发现、无反射**——程序结构是可分析、可变换、可传递的数据（信条④）。

## 设计决策

### 编译型定位

- 源码经 `crates/` 管线 **AOT** 为原生机器码，**非**以 CLR/JIT 或解释器为主执行模型。
- 用户可见表面以 **C# 惯用法**为约束基准；语义与性能契约由编译期决定。
- `const eval` 与过程机制在编译期完成；泛型单态化无运行时擦除。

### 属性（attribute）

属性是编译期的**声明式元数据**，经编译期收集（AttributeTable），**无运行时反射**：

```as
[Fact]
public void Test_Addition() {
    Assert.Equal(3, 1 + 2);
}
```

- 编译期 AttributeTable 收集 + 代码生成 registry。
- QIF 测试发现：`[Fact]` / `[Theory]` / `[InlineData]` 在编译期收集，合成 `__QifTestHost.Main` → 静态可执行。
- UI 框架的属性（如 `[Observable]`）经 codegen 合成路径处理。

### 代码生成（GenerateTo / Source Generator）

- 编译期生成代码：`[GenerateTo]` 等驱动生成 registry / 绑定 / 样板，替代运行时反射。
- `partial class` 跨文件合并（`(namespace, name, generic_arity)` 合并键）服务 code-behind 与 Source Generator（见 [对象模型](006-object-model.md)）。
- 生成发生在编译期，产物进入静态可执行，无运行时动态生成。

### comptime 子集

统一泛型/常量/元编程的**编译期常量求值**（compile-time function evaluation），避免「const generics + macros + traits」三套机制迭加（Zig comptime 启发：泛型即函数、编译期一等公民）：

- **`comptime` 关键字为局部前缀**（仅函数/表达式前缀，非全局求值上下文）：`comptime int BufferSize = 4096;`；求值发生在编译期，结果以常量形式进入产物，无运行时开销。
- **能力面**：整型 / bool / string 字面量运算；调用其他 comptime 函数（编译期函数图）；以值为类型参数构造类型（`Vector<T, N>` 的 N 由 comptime int 提供）。
- **`Type::ConstInt(i64)` 为 `comptime int` 的语法糖**：旧 ConstInt 解析为 comptime int，不破坏既有内置 facade。
- **表面取舍**：不采纳 C# `const`（混入编译期常量与 readonly 语义，无法表达编译期计算）；`Expression<T>` / LINQ 树化在编译期完成，是第一条宏式编译期管线的范例。
- 编译期变换器（用户自定义编译期插件）为方向性能力，需要沙箱与 ABI 约定专门设计，**不授权立即实现通用宏系统**。

### 与表达式树 / LINQ 的统一管线

- `Expression<T>` 与 LINQ 的编译期树化是本能力集的先行面（见 [表达式树与查询语言](011-expression-trees-query.md)）。
- 编译期展开叙事统一，避免双轨元编程。

### 泛型模板不可发射语义（历史编号 S6 A1）

泛型模板（泛型类 / 泛型方法 / 泛型扩展方法）是**编译期蓝图**：其方法体引用未单态化的类型参数符号（如 `Weak_T_GetWeakSlot`），无独立可发射的运行期 body——仅单态化实例才有合法 body。实现上 typeck 维护模板表（`fn_templates` / `extension_fn_templates` / 类模板）并经 `generic_template_names` 汇出符号名集合；pipeline 在 tree-shake 前据此把模板从 MIR 发射集剔除。`arc build --dynamic`（库无 Main/Entry，tree-shake 全量保留）若不剔除模板会误将其纳入发射集导致 LLVM undefined symbol；可执行构建中模板因不可达被 tree-shake 剪除，两侧语义一致。

> **历史编号说明**：「S6 A1」为已归档计划文档的行动项编号（动态库构建工作包），其实现横跨多个编译器阶段，代码注释以「RFC 012 S6 A1」标注并统一登记于此。同源行动项中主题归属他文档的子项：全语言用户 struct 统一 ptr 表示（见 [内存模型与资源安全](005-memory-model.md) / [运行时 ABI](014-runtime-abi.md)）。插件声明式贡献载体（`@__arc_contributions`）已随「跨包装配改为源码打包下显式静态注册」裁决整体移除，编译器零残留。

## 边界

- 表达式树与 LINQ 双路径见 [表达式树与查询语言](011-expression-trees-query.md)。
- 反射元数据（只读 Type 面）见 [类型体系与反射元数据](018-type-reflection-metadata.md)、[类型反射面](028-type-reflection.md)。
- `partial class` 合并见 [对象模型](006-object-model.md)。

## 禁止项

- **禁止运行时反射调用**（`obj.GetType()` 反射写；元数据仅只读）。
- **禁止 comptime 反射**（编译期动态类型运算，见 [类型反射面](028-type-reflection.md)）。
- **禁止 comptime 代码生成**（token 替换 / 宏解释器——代码生成本节 GenerateTo 已管，comptime 只管值运算）。
- **禁止全局 comptime 上下文**（`comptime` 仅局部前缀）。
- **拒绝 C# `const` 语义**（语义模糊，不采纳）。
- **不授权通用 proc-macro 式插件**（任意 token 替换 / 运行时宏解释器；需专门 RFC）。
- **拒绝** `expression` 关键字（见 [语法表面与编码标准](002-surface-contract.md)）。

---

上一节：[011 表达式树与查询语言](011-expression-trees-query.md) · 下一节：[013 编译管线架构](013-compiler-pipeline.md)