use ast::ExpressionTree;
use ast::*;
use indexmap::IndexMap;
use typeck::{SpillSet, TypeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

/// RFC 017 M4-link Phase B：函数符号的 LLVM linkage 类别。
///
/// 由 typeck/MIR lower 阶段按符号来源标注（参见 RFC 017 D2 节
/// 「`linkonce_odr` 弱符号消解策略」），codegen 仅消费不判断。
///
/// - `External`：用户源码定义的函数（默认 external linkage，单一定义来源）
/// - `LinkonceOdr`：std 库代码 + 泛型单态化实例（跨 `.o` 弱符号去重，ODR 保证语义等价）
/// - `DeclareOnly`：从 `.ao` 注册的外部符号（仅声明，定义来自被链接的 lib.o；
///   实践中 MirCfgBody 不使用此变体——MirCfgBody 总有函数体；外部声明由
///   codegen 直接消费 `typeck::external_symbols` 列表发射 `declare`）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Linkage {
    #[default]
    External,
    LinkonceOdr,
    DeclareOnly,
}

#[derive(Clone, Debug)]
pub enum MirOperand {
    Local(LocalId),
    ConstInt(i64),
    ConstFloat(f64),
    ConstString(String),
    ConstBool(bool),
    /// Class field read as operand.
    Field {
        object: Box<MirOperand>,
        class: String,
        field: String,
    },
    /// Interface fat-pointer value.
    Iface {
        object: Box<MirOperand>,
        class: String,
        iface: String,
    },
    /// Interface → concrete class 转型（拆 fat-pointer 盒）。
    /// `object` 是接口胖指针（`{ ptr obj, ptr itable }` 16 字节盒，MakeIface
    /// 产生，接口返回时堆分配）；codegen 发射 `load ptr, ptr 盒` 取出底层对象
    /// 指针。与 `MirOperand::Iface`（class → iface 装箱）对称，用于
    /// `(SqliteConnection)raw`（raw: IDbConnection）。旧实现直接拷贝盒指针，
    /// 对盒做 rt_arc_inc/dec 并把它当类对象传给方法 → 0xC0000005 / 0xC0000409。
    UnboxIface {
        object: Box<MirOperand>,
        class: String,
    },
    /// RFC 045 P2：object→string 拆箱操作数（is string 收窄 / 窄化 Cast 的
    /// 叶子路径）。object 槽内的 string 是 ArcStringBox（ArcHeader + char* @
    /// offset 16）；codegen 发射 rt_string_unbox（含 vtable 校验，null 安全）。
    /// 与 MirRvalue::Unbox 的 string 分支同语义；操作数形态供无 prep 通道的
    /// 叶子下降（字段/实参接收体）使用。
    UnboxString {
        object: Box<MirOperand>,
    },
    /// 泛型 unbox cast：`(T)obj` 且 T 为泛型型参（模板 lowering 阶段未知具体
    /// 类型）时，仿 `ConstDefault` 保留型参名，单态化克隆经 `substitute_in_operand`
    /// 替换为具体类型名，codegen 按具体类型发射拆箱：
    /// 基元值类型 → rt_box_unbox + load（对齐 `MirRvalue::Unbox` 值类型路径）；
    /// string → rt_string_unbox（对齐 `MirOperand::UnboxString`）；
    /// 其余引用类型 → 类型断言直接返回 obj。
    /// 与 UnboxIface/UnboxString 的分工：静态 cast（类型已知）走那两条；泛型型参
    /// cast 走本变体——否则 `(T)boxed` 被当作引用直接透传，值类型 T 单态化后
    /// 发射 `ret ptr` 与函数结果类型（如 `i1`）不匹配。
    UnboxGeneric {
        object: Box<MirOperand>,
        type_name: String,
    },
    /// `&local` — address-of for `ref`/`out` argument passing.
    AddrOf(LocalId),
    /// `null` literal for nullable reference types.
    ConstNull,
    /// `default(T)` — 类型化默认值操作数（仿 `TypeId`/`TypeInfoPtr` 模式）。
    /// `type_name` 是降低后的类型名；泛型方法的模板体内 T 未解析时携带型参名
    /// （如 `T`），单态化克隆经 `substitute_in_operand` 替换为具体类型名，
    /// codegen 按具体类型发射默认值（基元 → 零/false，其余 → null）。
    /// 不直接折叠为 `ConstNull` 的原因：`default(bool)` 应为 `i1 false`，
    /// 折叠成 `ConstNull` 会丢失类型 → LLVM `ret ptr null` 与 `i1` 结果不匹配。
    ConstDefault {
        type_name: String,
    },
    /// Function pointer value (lifted lambda). Used for inline lambdas passed as method arguments.
    FnPtr {
        name: String,
    },
    /// Closure value (lambda with captured variables, RFC 008).
    /// `fn_name` is the lifted top-level function; `env` lists each captured
    /// variable's metadata paired with the operand that produces its value
    /// (typically `MirOperand::Local(outer_local_id)`).
    /// The function signature is `ret_t fn(void* env, params...)`.
    /// Codegen allocates a `__lambda_env_N` struct, fills it from `env` operands,
    /// and passes its address as the first argument.
    Closure {
        fn_name: String,
        env: Vec<(LambdaCapture, MirOperand)>,
    },
    /// `typeof(T)` — compile-time type token (RFC 026 / RFC 018).
    /// `type_name` is the lowered type name (e.g. `"ConsoleLogger"`).
    /// Operand variant name is historical; codegen emits a `RuntimeType`
    /// instance (`_typeInfoHandle = ptrtoint(@.typeinfo.{T})`), not a
    /// language-level TypeId struct (removed in RFC 018 M5).
    TypeId {
        type_name: String,
    },
    /// RFC 018 M1: `@.typeinfo.{Class}` 全局常量指针。
    /// 用于 `expr is T` lowering：作为 `rt_obj_isa(obj, typeinfo)` 的第二参数。
    /// codegen 发射 `ptr @.typeinfo.{type_name}`。
    TypeInfoPtr {
        type_name: String,
    },
    /// RFC 006 M3：静态字段读取——codegen 发射 `load <ty> @__static_<class>_<field>`。
    ///
    /// 由 `operand_from_expr` 在以下两种路径下生成：
    /// - 静态方法内裸访问同类静态字段（`_count`）；
    /// - 跨类静态字段访问（`Counter._count`）。
    ///
    /// `class` 与 `field` 均为非 mangled 源码标识；codegen 据此构造全局符号名。
    StaticField {
        class: String,
        field: String,
    },
}

#[derive(Clone, Debug)]
pub enum MirRvalue {
    Use(MirOperand),
    Binary {
        op: BinOp,
        left: MirOperand,
        right: MirOperand,
    },
    Call {
        func: String,
        args: Vec<MirOperand>,
    },
    New {
        class: String,
        args: Vec<MirOperand>,
        /// 已解析 ctor 的形参类型名（CD-10/D1，与 MethodCall.params 同机制）。
        ///
        /// 供 codegen 按签名 mangle ctor 符号——`__ctor::C_1` 仅按参数个数，
        /// 同参数量不同类型参数的 ctor 重载（`C(int)` / `C(string)`）会符号碰撞
        /// （后者覆盖前者 → 调用方按错误签名执行 → AV）。非空即按
        /// `__ctor::Class_<arity>_<p0>_<p1>...` 发射，空则保持旧 arity 形式。
        ctor_params: Vec<String>,
    },
    FieldGet {
        object: MirOperand,
        class: String,
        field: String,
    },
    MethodCall {
        receiver: MirOperand,
        method: String,
        args: Vec<MirOperand>,
        receiver_type: String,
        impl_class: Option<String>,
        /// Resolved link name (`Class::M` or `Class::M_int`) when overload applies.
        target_fn: Option<String>,
        /// Whether to dispatch via class vtable (virtual/override/abstract).
        is_virtual: bool,
        /// CD-10/D1：已解析重载的形参类型名。vtable/itable 槽位按「名+形参」定位，
        /// 重载各有其槽；缺失时调用点回退按名单槽兜底。
        params: Vec<String>,
    },
    MakeIface {
        class: String,
        iface: String,
        object: MirOperand,
    },
    /// Class → interface when static type may be a base that does not declare
    /// the interface（`Widget w = new Impl(); if (w is IChild c)`）。
    /// codegen 按对象 runtime type_id 在已知实现类中选择 `@.itable.{Class}_{Iface}`。
    MakeIfaceDyn {
        iface: String,
        object: MirOperand,
    },
    /// Interface → variance-compatible interface（`IGetter_Dog` → `IGetter_IAnimal`）。
    /// codegen 比较源 fat pointer 的 itable 指针，重绑定到适配器 itable（无需 type_id）。
    AdaptIface {
        from_iface: String,
        to_iface: String,
        object: MirOperand,
    },
    StructLit {
        struct_name: String,
        fields: Vec<(String, MirOperand)>,
    },
    ArrayLit {
        elem_type: TypeId,
        elements: Vec<ArrayLitElement>,
    },
    /// `new T[n]` — 运行时长度、零初始化的堆数组分配。
    /// codegen 发射 `rt_array_create(length, elem_size)`（带 RtArrayHeader，`Length` 可读）。
    /// `elem_type` 为元素类型（不含数组后缀）；`length` 为长度操作数（int）。
    NewArray {
        elem_type: TypeId,
        length: MirOperand,
    },
    IndexGet {
        array: MirOperand,
        index: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 005：`arr.AsSpan()` / `arr.AsSpan(start, len)` / `AsReadOnlySpan`。
    /// codegen 构造栈上 `{ ptr, i32 }` 胖指针并返回其地址（用户面无裸指针）。
    SpanFromArray {
        array: MirOperand,
        start: Option<MirOperand>,
        length: Option<MirOperand>,
        mutable: bool,
    },
    /// RFC 005 params@Span / `[…]`→Span：栈缓冲脱糖（`alloca [N x T]`，非堆数组）。
    SpanFromStack {
        elements: Vec<MirOperand>,
        elem_type: TypeId,
        mutable: bool,
    },
    /// RFC 005：`span.Slice(start, length)`。
    SpanSlice {
        span: MirOperand,
        start: MirOperand,
        length: Option<MirOperand>,
        mutable: bool,
    },
    /// RFC 005：`span.Fill(value)`（可变 Span；逐元素写入）。   
    SpanFill {
        span: MirOperand,
        value: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 005：`span.Clear()`（等价 Fill(0) 于标量零值）。   
    SpanClear {
        span: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 005 std 面：`src.CopyTo(dest)`（元素按序复制；dest 过短 panic）。
    SpanCopyTo {
        src: MirOperand,
        dest: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 005：`src.TryCopyTo(dest)` → bool（dest 过短则 false，不 panic）。
    SpanTryCopyTo {
        src: MirOperand,
        dest: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 005：`span.ToArray()` → 新 `T[]`（堆分配 + 元素拷贝）。
    SpanToArray {
        span: MirOperand,
        elem_type: TypeId,
    },
    /// RFC 009 D3：SoA struct 数组字段访问融合。
    ///
    /// 当源码为 `soaArr[i].field` 且 `soaArr` 元素类型为 `[SoA]` struct 时，
    /// lower 阶段直接生成此 rvalue，避免 `arr[i]` 物化为临时 local 后
    /// codegen 无法回溯 IndexGet 的 AoS 回退路径。
    ///
    /// codegen 发射：
    ///   1. `%field_arr = call ptr @rt_soa_field_ptr(ptr %arr, i32 %field_idx)`
    ///   2. `%elem_ptr = getelementptr inbounds %field_ty, ptr %field_arr, i32 %i`
    ///   3. `%val = load %field_ty, ptr %elem_ptr`
    SoaFieldGet {
        array: MirOperand,
        index: MirOperand,
        /// SoA struct 类名（用于查询 field_idx 与 field_ty）
        class: String,
        /// 字段名
        field: String,
    },
    LinqChain(LinqChain),
    ExpressionTreeConst {
        name: String,
        tree: ExpressionTree,
    }, // codegen → rodata; not runtime IR
    FnPtr {
        name: String,
    },
    IndirectCall {
        func: MirOperand,
        args: Vec<MirOperand>,
    },
    /// `left ?? right` — null coalescing.
    Coalesce {
        left: MirOperand,
        right: MirOperand,
    },
    /// `cond ? then_val : else_val` — ternary conditional.
    Ternary {
        cond: MirOperand,
        then_val: MirOperand,
        else_val: MirOperand,
    },
    /// `receiver?.field` — null-conditional field access.
    NullCondField {
        receiver: MirOperand,
        class: String,
        field: String,
        default: MirOperand,
    },
    /// `receiver?.method(args)` — null-conditional method call.
    NullCondMethod {
        receiver: MirOperand,
        method: String,
        args: Vec<MirOperand>,
        receiver_type: String,
        impl_class: Option<String>,
        target_fn: Option<String>,
        is_virtual: bool,
        default: MirOperand,
        /// CD-10/D1：已解析重载的形参类型名（见 MethodCall.params）。
        params: Vec<String>,
    },
    /// `receiver!.field` — force-deref field access (runtime panic if null).
    ForceDerefField {
        receiver: MirOperand,
        class: String,
        field: String,
        span: Span,
    },
    /// `receiver!.method(args)` — force-deref method call.
    ForceDerefMethod {
        receiver: MirOperand,
        method: String,
        args: Vec<MirOperand>,
        receiver_type: String,
        impl_class: Option<String>,
        target_fn: Option<String>,
        is_virtual: bool,
        span: Span,
        /// CD-10/D1：已解析重载的形参类型名（见 MethodCall.params）。
        params: Vec<String>,
    },
    /// FFI Marshal 装箱（RFC 016 v2 M2 / RFC 016 M3）。
    /// 值类型 `src` → `object` 引用类型（堆分配 + ARC）。
    /// codegen 发射 `@rt_box_create(size, align)` + `@llvm.memcpy` + `@rt_arc_inc`。
    Box {
        src: MirOperand,
        /// 装箱前的值类型（用于推导 size/align）
        src_ty: TypeId,
    },
    /// FFI Marshal 拆箱（RFC 016 v2 M2 / RFC 016 M3）。
    /// `object` 引用类型 `src` → 值类型（size 校验 + memcpy）。
    /// codegen 发射 `@rt_box_unbox(ptr, expected_size, out_ptr, out_size)`；不匹配 panic。
    Unbox {
        src: MirOperand,
        /// 拆箱后的值类型（用于推导 expected_size/out_size）
        target_ty: TypeId,
    },
    /// RFC 004 M1：variant case 构造。
    /// `Value.Int(42)` → `VariantConstruct { variant_name: "Value", case_name: "Int", payload: Some(operand) }`
    /// `Value.Null`     → `VariantConstruct { variant_name: "Value", case_name: "Null", payload: None }`
    ///
    /// codegen 发射：
    /// 1. alloca `%variant.Value`，零初始化
    /// 2. store tag = discriminant(case_name)
    /// 3. 若 payload 为 Some：store payload 到 union 对应字段；
    ///    若 payload 类型为 class/string，发射 `rt_arc_inc` 维护引用计数
    VariantConstruct {
        variant_name: String,
        case_name: String,
        payload: Option<MirOperand>,
    },
    /// RFC 004 M1：variant tag 读取（用于 switch 分派）。
    /// 从 variant 值中读取 tag 字段（u8），返回 i32 用于比较。
    /// codegen 发射 `getelementptr %variant.Value, ptr, 0, 0` + `load u8` + `zext i32`。
    VariantTag {
        scrutinee: MirOperand,
        variant_name: String,
    },
    /// RFC 004 M1：variant payload 提取（用于 case binding）。
    /// 在 switch case 块内，从 variant 中提取 case_name 对应的 payload，
    /// 绑定到 local variable。borrow 语义——不 inc 引用计数。
    /// codegen 发射 `getelementptr %variant.Value, ptr, 0, 1`（union）+
    /// `getelementptr %union.payload, ptr, 0, case_index` + `load payload_ty`。
    VariantExtract {
        scrutinee: MirOperand,
        variant_name: String,
        case_name: String,
        /// 提取后的 payload 类型（用于 codegen 选择 load 指令）
        payload_ty: TypeId,
    },
}

/// RFC 017 #8：集合表达式 spread 的 MIR 元素。
#[derive(Clone, Debug)]
pub enum ArrayLitElement {
    Value(MirRvalue),
    Spread(MirOperand),
}

#[derive(Clone, Debug)]
pub enum MirStatement {
    Assign {
        place: LocalId,
        rvalue: MirRvalue,
    },
    Drop(LocalId),
    Return(Option<MirRvalue>),
    If {
        cond: MirOperand,
        then_body: Vec<MirStatement>,
        else_body: Vec<MirStatement>,
    },
    While {
        cond: MirRvalue,
        body: Vec<MirStatement>,
        /// 迭代溯源：`Some(source)` 表示本循环由 `foreach`/LINQ 枚举 lowering 合成，
        /// `source` 为被枚举容器 operand；`None` 为用户手写循环。迭代器失效检测
        /// （E_ITERATOR_INVALIDATION）只信任该溯源——索引读（`get_Item`）不持有
        /// 枚举器，用户循环内「索引读 + mutator」是合法写法，不得凭启发式误报。
        foreach_source: Option<MirOperand>,
    },
    FieldSet {
        object: MirOperand,
        class: String,
        field: String,
        value: MirRvalue,
    },
    /// RFC 006 M3：静态字段写入——codegen 发射 `store <ty> <value>, ptr @__static_<class>_<field>`。
    ///
    /// 与 `FieldSet` 对偶：后者通过 `this` 指针 GEP store 到实例字段，
    /// 本变体直接 store 到模块级全局变量。
    StaticFieldSet {
        class: String,
        field: String,
        value: MirRvalue,
    },
    /// `arr[i] = v` — 原生 `T[]` 元素写入（非 C# 索引器）。
    ///
    /// 与 `IndexGet` 对偶：codegen 发射 GEP + store。此前 Assign 仅覆盖
    /// `set_Item` 索引器路径，数组元素赋值被静默丢弃（如 `md[1]=29`）。
    /// RFC 005：当 `array` 局部类型为 `Span` 时，codegen 走胖指针索引写。
    IndexSet {
        array: MirOperand,
        index: MirOperand,
        elem_type: TypeId,
        value: MirRvalue,
    },
    /// `foreach (var x in query) { ... }` — Enumerable path.
    LinqForeach {
        var: Ident,
        chain: LinqChain,
        body: Vec<MirStatement>,
    },
    /// `await task` — suspend point; `place` receives the unwrapped `T` from `Task<T>`.
    Await {
        place: LocalId,
        task: MirRvalue,
    },
    /// `throw expr` — unwinds to innermost `try`.
    Throw {
        value: MirRvalue,
    },
    /// `try { } catch (T id) { }` — lowered by codegen to invoke/catchswitch
    /// (zero-cost EH, RFC 010)。
    /// P1-B2：`when` 在 MIR lower 脱糖为 catch 内 `if`+rethrow；同条 `finally`
    /// 脱糖为外层 `TryFinally { body: [TryCatch], finally }`。
    TryCatch {
        try_body: Vec<MirStatement>,
        catch_var: LocalId,
        catch_ty: TypeId,
        catch_body: Vec<MirStatement>,
    },
    /// `try { body } finally { cleanup }` — body 完成后（正常路径）执行 cleanup。
    /// A1：return/throw 路径经 codegen `finally_stack` 内联执行 cleanup。
    TryFinally {
        body: Vec<MirStatement>,
        finally: Vec<MirStatement>,
    },
    /// `break;` — 仅作 lower 内部 scratch；`to_cfg` 展平为 `Goto(最近循环 exit)`。
    /// 语义：跳出最近一层 `while`/`for`/`foreach`（C# 对齐）；与 switch case 语法
    /// `break`（parse 时消费、不入 AST）无关。
    Break,
    /// `continue;` — 仅作 lower 内部 scratch；`to_cfg` 展平为 `Goto(最近循环 header)`。
    Continue,
}

#[derive(Clone, Debug)]
pub(crate) struct MirBasicBlock {
    pub statements: Vec<MirStatement>,
}

#[derive(Clone, Debug)]
pub(crate) struct MirBody {
    pub params: Vec<(Ident, TypeId)>,
    pub ret: TypeId,
    pub param_count: usize,
    pub locals: IndexMap<LocalId, (Ident, TypeId)>,
    pub blocks: Vec<MirBasicBlock>,
    pub is_async: bool,
    pub owner: Option<Ident>,
    pub class_fields: Vec<Ident>,
    pub is_ctor: bool,
    /// RFC 006 M2（M3 预留）：是否为静态方法。
    ///
    /// M3 将据 `is_static == true` 把 `class_fields` 中的字段访问降级为
    /// `MirOperand::StaticField`（加载 `@__static_<Class>_<field>` 全局变量），
    /// 而非走 `MirOperand::Field`（通过 `this` 指针 GEP）。
    pub is_static: bool,
    /// Captured variables for lifted lambdas (RFC 008).
    /// Non-empty when this body belongs to a `__lambda_rt_N` function that has
    /// an `__env__` first parameter. Each entry records (local_id, field_index, capture).
    /// Codegen emits GEP+load from `%__env__` into the local alloca for each capture.
    pub captures: Vec<(LocalId, usize, LambdaCapture)>,
    /// RFC 017 M4-link Phase B：函数符号的 LLVM linkage。
    /// 由 typeck/MIR lower 按符号来源标注（参见 `Linkage` 文档）。
    /// `to_cfg()` 透传到 `MirCfgBody.linkage`，codegen 据此发射
    /// `define` / `define linkonce_odr`。
    pub linkage: Linkage,
    /// RFC 009 M3：`[Parallelize]` attribute 标记，透传自 `TypedFn.parallelize`。
    /// `to_cfg()` 透传到 `MirCfgBody.parallelize`，codegen 据此在 while 循环
    /// backedge 附加 `!llvm.loop.vectorize.enable` metadata。
    ///
    /// **平台无关提示**：标记本身不绑定具体指令集；实际向量化由 LLVM 据目标
    /// CPU 特征选择（x86 SSE2/AVX2/AVX-512、ARM NEON、其他标量退化）。
    pub parallelize: bool,
    /// RFC 009 M3：按需 spill 集合——env struct 中转为 ptr 的 large local
    /// （`typeck::analyze_spill_candidates` 分析结果，key = `LocalId.0`）。
    /// `to_cfg()` 透传到 `MirCfgBody.spill_set`，codegen 据此将 spilled local
    /// 的 env 字段由值类型替换为 ptr（堆槽指针），并在 ctor/save/dtor 维护
    /// 堆槽生命周期。
    pub spill_set: SpillSet,
}

#[derive(Clone, Debug)]
pub struct LinqChain {
    pub source: MirOperand,
    pub source_len: Option<usize>,
    pub operators: Vec<LinqOp>,
}

#[derive(Clone, Debug)]
pub enum LinqOp {
    Where(LambdaExpr),
    Select(LambdaExpr),
    OrderBy {
        key: LambdaExpr,
        descending: bool,
    },
    /// `let <ident> = <value>`：value 以 from range var 为参的 lambda 求值，
    /// 结果绑定为局部，供后续子句引用（元素自身继续前流）。
    Let {
        ident: Ident,
        value: LambdaExpr,
    },
    /// `join <inner> in <source> on <outer.key> == <inner.key>`：inner join。
    /// outer range var 绑定当前元素；inner 绑定源元素；命中则后续子句
    /// 在 (outer, inner) 双变量作用域内继续。
    Join {
        outer: Ident,
        inner: Ident,
        source: Spanned<Expr>,
        on_left: LambdaExpr,
        on_right: LambdaExpr,
    },
    /// `group <element> by <key>`：物化为 `List_<Grouping_<K, T>>`，
    /// 后续子句的 range var 重绑为分组（见 `lower_query` 的 `group_ident`）。
    GroupBy {
        key: LambdaExpr,
        element: Option<LambdaExpr>,
    },
}

// ---- CFG MIR（对外契约；`lower_module` 仅暴露此形）----
//
// MirBlock = 线性/region 语句 + terminator。
// - If/While/Break/Continue：仅作 lower 内部 scratch；`to_cfg` 展平为
//   CondBr/Goto，不得出现在 MirBlock.statements 顶层（debug_assert 守护）。
// - TryCatch / LinqForeach / TryFinally / Await：有意保留的 **region 语句**
//   （Phase A 终态，非债务）；codegen 以嵌套区域发射，非第二套消费 API。
//
// 内部仍经 `MirBody`（pub(crate) scratch）→ `to_cfg()`；下游（codegen /
// pipeline / integration）只见 `MirCfgBody`。

/// Basic block identifier (function-local).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// Terminator — the last instruction of a CFG basic block, determining control flow.
#[derive(Clone, Debug)]
pub enum MirTerminator {
    /// Unconditional jump.
    Goto(BlockId),
    /// Conditional branch: `cond` true → `then_bb`, false → `else_bb`.
    CondBr {
        cond: MirOperand,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    /// Function return.
    Return(Option<MirOperand>),
    /// Throw exception (non-local control flow).
    Throw(MirOperand),
    /// Unreachable (e.g. after a Return or infinite loop with no exit).
    Unreachable,
}

/// CFG basic block: statements + terminator.
///
/// Top-level statements are linear ops or **region** forms (`TryCatch`,
/// `LinqForeach`, `TryFinally`, `Await`). `If`/`While`/`Break`/`Continue`
/// must not appear here after `to_cfg` (they may still appear *inside*
/// region bodies; codegen nested emitters handle Break/Continue there).
#[derive(Clone, Debug)]
pub struct MirBlock {
    pub id: BlockId,
    pub statements: Vec<MirStatement>,
    pub terminator: MirTerminator,
}

/// Canonical MIR function body — sole consumer-facing form (`lower_module`).
#[derive(Clone, Debug)]
pub struct MirCfgBody {
    pub params: Vec<(Ident, TypeId)>,
    pub ret: TypeId,
    pub param_count: usize,
    pub locals: IndexMap<LocalId, (Ident, TypeId)>,
    pub entry: BlockId,
    pub blocks: IndexMap<BlockId, MirBlock>,
    pub is_async: bool,
    pub owner: Option<Ident>,
    pub class_fields: Vec<Ident>,
    pub is_ctor: bool,
    /// RFC 006 M2（M3 预留）：是否为静态方法。透传自 `MirBody.is_static`。
    pub is_static: bool,
    /// Captured variables for lifted lambdas (RFC 008). See `MirBody::captures`.
    pub captures: Vec<(LocalId, usize, LambdaCapture)>,
    /// RFC 017 M4-link Phase B：函数符号的 LLVM linkage。
    /// 透传自 `MirBody.linkage`，由 typeck/MIR lower 按符号来源标注。
    /// codegen 据此发射 `define` / `define linkonce_odr`；`declare` 由
    /// codegen 直接消费 `typeck::external_symbols` 列表发射（不经 MirCfgBody）。
    pub linkage: Linkage,
    /// RFC 009 M3：`[Parallelize]` attribute 标记，透传自 `MirBody.parallelize`。
    /// codegen 据此在 while 循环 backedge 附加 `!llvm.loop.vectorize.enable`
    /// metadata，强制 LLVM loop-vectorize pass 向量化。
    ///
    /// **平台无关提示**：标记本身不绑定具体指令集；实际向量化由 LLVM 据目标
    /// CPU 特征选择（x86 SSE2/AVX2/AVX-512、ARM NEON、其他标量退化）。
    pub parallelize: bool,
    /// RFC 009 M3：while 循环 backedge 源块集合。`to_cfg()` 在展平 while 时
    /// 记录每个 backedge 的源块 ID（即循环体末尾跳回 header 的块）。
    /// codegen 在 `parallelize=true` 时，对这些块的 `Goto` terminator
    /// 附加 `!llvm.loop !N` metadata。
    ///
    /// **平台无关**：backedge 标记本身平台无关；实际向量化由 LLVM 据目标
    /// CPU 特征选择指令集（x86 SSE2/AVX2/AVX-512、ARM NEON、其他退化）。
    pub loop_backedges: std::collections::HashSet<BlockId>,
    /// 迭代溯源：`(循环 header, 被枚举容器)` 对。`to_cfg()` 展平携带
    /// `foreach_source` 的 While 时记录，供迭代器失效检测（borrow check）
    /// 精确定位「源级枚举循环」及其容器——取代按 `get_Item` 启发式猜测
    ///（普通索引读不持有枚举器，启发式会把用户循环内合法的索引读+修改
    /// 误判为迭代失效）。
    pub foreach_loops: Vec<(BlockId, MirOperand)>,
    /// RFC 009 M3：按需 spill 集合（透传自 `MirBody.spill_set`）。
    /// codegen 据此将 spilled local 的 env 字段替换为 ptr（堆槽指针），
    /// 并在 env 构造/save/dtor 中维护堆槽生命周期。
    pub spill_set: SpillSet,
}

impl MirCfgBody {
    /// 构建「仅入口空块」的骨架 body——供管线为 builtin stub 类的属性 getter
    ///（如 `TextBuffer::get_LineCount`）合成发射条目。
    ///
    /// 这些 getter 是 get-only `[Builtin]` custom-accessor 属性：typeck 不为其
    /// 合成 MirCfgBody（方法级 stub 有显式体，属性 getter 无源体），但 codegen
    /// 在属性访问点按 `mangle_method(Class, get_X)` 直调 `@<Class>_get_X` 符号，
    /// MIR 调用图**无 Call 边**。若缺失则 tree-shake 后 stub 无从发射（见
    /// `arc-prune-001`）。本骨架的真实 IR 由 codegen `emit_stubs::try_emit_stub`
    /// 按**符号名**生成，body 内容不被消费（`emit_function` 在 `try_emit_stub`
    /// 返回后即短路）；linkonce_odr + comdat 去重，未引用时被链接器丢弃，无冗余。
    pub fn stub_skeleton(_link_name: &str, owner: &str) -> Self {
        Self {
            params: Vec::new(),
            ret: TypeId::Void,
            param_count: 0,
            locals: IndexMap::new(),
            entry: BlockId(0),
            blocks: IndexMap::new(),
            is_async: false,
            owner: Some(owner.into()),
            class_fields: Vec::new(),
            is_ctor: false,
            is_static: false,
            captures: Vec::new(),
            linkage: Linkage::LinkonceOdr,
            parallelize: false,
            loop_backedges: std::collections::HashSet::new(),
            foreach_loops: Vec::new(),
            spill_set: SpillSet::empty(),
        }
    }
}
