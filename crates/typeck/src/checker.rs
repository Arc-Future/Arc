/// RFC 009 M3.1：Async spill liveness analysis —— 分析跨 await 存活的大类型 local。
/// 供 codegen emit_async_sm.rs 决定 env 字段是否转为 ptr。
pub(crate) mod check_async_spill;
pub(crate) mod check_builtin;
pub(crate) mod check_class;
pub(crate) mod check_generics;
pub(crate) mod check_native;
pub(crate) mod check_struct;
pub(crate) mod check_type;
/// RFC 037：Partial class 合并模块。在 `check_module` 入口预合并 HIR 中
/// 的 partial class 声明，下游 `TypeRegistry::from_module` 与
/// `check_module_items` 仅看到合并后的 ClassDef。
pub(crate) mod partial;
/// 影子阶段：栈式 in-progress 环检测（与深度哨兵并存，验证零回归后再删哨兵）。
pub(crate) mod recursion_guard;
mod resolve_attr;
/// RFC 009 M3.0：Type size table pass —— 编译期计算所有类型的 size_of / align_of。
/// 供 M3 按需 spill、RFC 018 Type.SizeOf、RFC 004 sizeof(T) 共享。
pub(crate) mod type_size_table;

use crate::{
    extension_mangle_base, method_link_name, method_link_name_static_abi, AccessContext,
    AttributeTable, BuiltinMeta, ConstValue, CtorSig, ExtensionScope, FieldInfo, NominalType,
    OopMethodSig, ParamSig, TypeKind, TypeRegistry,
};
use ast::*;
use hir::{DefId, HirBuilder, HirItem, HirModule};
use indexmap::{IndexMap, IndexSet};

use crate::checker::recursion_guard::RecursionGuard;
use crate::error::{TypeError, TypeWarning};
use crate::generics::{
    mangle_generic, substitute_class_def, substitute_fn_def, substitute_type, substitute_type_ast,
    substitution_map, type_id_to_field_name,
};
use crate::match_pat::MatchPat;
use crate::null_flow::NullFlowState;
use crate::out_flow::OutParamState;
use crate::type_id::TypeId;
use crate::typed::{FnLinkage, TypedBlock, TypedFn};

/// RFC 009 M4-7: typeck Pass 模式（D12.2 编译 Pass 顺序）。
///
/// - [`MacroPassMode::Skeleton`]：Pass 2 骨架 typeck。宏容器类跳过方法体
///   检查（仅校验类/方法签名 + 属性解析）。被赋能类与非容器类照常完成
///   完整 typeck。Pass 2 末尾构建 `macro_catalog`。
/// - [`MacroPassMode::Full`]：Pass 4 完整 typeck。Pass 3 将展开代码 splice
///   到宏容器类方法体后，对宏容器类重新运行 typeck，校验注入代码的类型
///   合法性。属性解析在 Pass 2 已完成，Pass 4 跳过以避免重复注册。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacroPassMode {
    /// Pass 2: 骨架 typeck —— 宏容器类跳过方法体检查
    Skeleton,
    /// Pass 4: 完整 typeck —— 对 splice 后的宏容器类方法体做完整检查
    Full,
}

pub struct TypeChecker {
    pub(crate) scopes: Vec<IndexMap<Ident, TypeId>>,
    pub(crate) typed_fns: Vec<TypedFn>,
    pub(crate) errors: Vec<TypeError>,
    /// RFC 005 里程碑④：编译期 warning 列表（与 `errors` 语义分离，不阻断编译）。
    /// 当前仅承载 `arc-cycle-001` 声明级字段环 warning。
    pub(crate) warnings: Vec<TypeWarning>,
    pub(crate) registry: TypeRegistry,
    pub(crate) current_class: Option<Ident>,
    /// RFC 006 M2：当前正在检查的方法/构造函数是否为 `static`。
    ///
    /// 进入静态方法体前置 `true`，离开时复位 `false`。`check_expr_inner`
    /// 据此拦截「静态方法内访问实例字段」违规。构造函数与实例方法恒为 `false`。
    /// 自由函数（非方法）也保持 `false`——它无 `current_class` 上下文。
    pub(crate) current_fn_is_static: bool,
    /// Expected type for `return` in the current function/method body.
    pub(crate) return_slot: Vec<TypeId>,
    pub(crate) in_async: bool,
    /// Nesting depth of `while` / `for` / `foreach` bodies; `break`/`continue` require > 0.
    pub(crate) loop_depth: u32,
    pub(crate) in_ctor: bool,
    /// `true` when checking a non-constructor method of a `readonly struct`.
    /// When set, assignments to `this.field` are rejected.
    pub(crate) current_readonly_context: bool,
    /// RFC 019 M-B：泛型类单态化嵌套深度。>0 时 `ensure_type_accessible` 跳过
    /// 类型可见性门禁——单态化由编译器驱动（模板在归属包内已校验），模板签名
    /// 引用的 internal 类型（如 `DispatchContext`）不应对消费端报不可访问。
    pub(crate) mono_depth: u32,
    pub(crate) next_def_id: u32,
    /// 集合表达式 → `List<T>` 脱糖的临时名序号（按编译单元隔离，保证并行确定性）。
    pub(crate) list_target_seq: std::cell::Cell<u32>,
    /// 位置模式 switch 前奏的临时名序号（按编译单元隔离，保证并行确定性）。
    pub(crate) pos_scrut_seq: u32,
    /// Generic class templates keyed by definition name (`Box` for `class Box<T>`).
    pub(crate) class_templates: IndexMap<Ident, ClassDef>,
    /// Generic function templates keyed by definition name.
    pub(crate) fn_templates: IndexMap<Ident, FnDef>,
    /// 泛型函数模板声明侧上下文：模板名 → (声明 span, 声明命名空间)。
    ///
    /// `instantiate_generic_fn` 单态化时据 span 切换回声明包的包上下文
    /// （`enter_package_for_span`），使方法体内访问库内 `internal` 类型成员
    /// 不被消费端包的 `can_access_type` 拒掉（与 `instantiate_generic_class`
    /// 经 `registry.types[def].span` 恢复同源，但 FnDef 本身不携带 span，
    /// 故在 `check_module_items` 收集时记录）。
    pub(crate) fn_template_origins: IndexMap<Ident, (ast::Span, Vec<Ident>)>,
    /// RFC 007：非泛型自由函数定义（供可选/命名实参绑定）。
    pub(crate) fn_defs: IndexMap<Ident, FnDef>,
    /// 泛型扩展方法模板（决策 #7，RFC 010），键为 `Container::Method`。
    /// 调用点 `instantiate_generic_extension_fn` 据此生成单态化方法体。
    pub(crate) extension_fn_templates: IndexMap<Ident, FnDef>,
    /// Generic interface templates keyed by definition name (`IComparable` for `interface IComparable<T>`).
    pub(crate) interface_templates: IndexMap<Ident, InterfaceDef>,
    /// GAP #5 扩展：泛型委托模板（`delegate R Map<T, R>(T x);`），键为委托名。
    /// 引用点经 `instantiate_generic_delegate` 按实参单态化为 `TypeId::Func`。
    pub(crate) delegate_templates: IndexMap<Ident, DelegateDef>,
    /// RFC 009 P1-C2：单态化名 → (模板名, 实参列表)，供 variance 赋值兼容判定。
    pub(crate) mono_origins: IndexMap<String, (Ident, Vec<TypeId>)>,
    /// Monomorphized symbol names already emitted (`Box_int`, `Identity_int`, …).
    pub(crate) instantiated: IndexSet<String>,
    /// 单态化负缓存：已确认约束违约的实例化（mangled 名 → 首次违约哨兵）。
    ///
    /// 与 [`TypeChecker::instantiated`]（正缓存）对称：正缓存登记「已成功
    /// 单态化」的实例，负缓存登记「约束违约被中止」的实例化点。
    /// `check_constraints` 是确定性纯检查，违约事实与触达次数无关——
    /// 负缓存命中即短路返回缓存哨兵，不重跑检查（违约明细零重复入池）。
    /// 哨兵仍沿 `?` 冒泡保持「违约实参不得参与单态化」契约，其重复由
    /// 错误池收敛出口的内容级去重（[`TypeChecker::take_errors_deduped`]）
    /// 消除。
    pub(crate) violated: IndexMap<String, TypeError>,
    /// 影子阶段：泛型类单态化 in-progress 环检测栈（区别于 `instantiated` memoize）。
    pub(crate) recursion_class: RecursionGuard<String>,
    /// 影子阶段：泛型接口单态化 in-progress 环检测栈。
    pub(crate) recursion_iface: RecursionGuard<String>,
    /// Stack of type-parameter bindings while checking generic templates.
    pub(crate) type_param_scope: Vec<IndexMap<Ident, TypeId>>,
    /// RFC 004 M1：泛型参数的 `where` 约束作用域栈。
    ///
    /// 进入泛型方法/类/接口体时 push 当前模板的 `where_clause`，退出时 pop。
    /// `check_static_abstract_call` 据此查询 `T` 的 `where T : IFace<T>` 约束，
    /// 验证 `T.Method()` 调用走 static abstract 单态化分派。
    pub(crate) where_clause_scope: Vec<Vec<ast::TypeConstraint>>,
    /// Namespace paths imported for extension method lookup (from root `using` directives).
    pub(crate) extension_imports: Vec<Vec<Ident>>,
    /// Namespace of the function/method body currently being checked.
    pub(crate) enclosing_namespace: Vec<Ident>,
    /// RFC 016 M3 §3.4 能力 gating Phase 1+（[4.4 能力系统]）：
    /// namespace 能力栈。每层 `HirModule.capabilities` 在 `check_module_items`
    /// 进入时推入、离开时弹出。栈底为根层空 Vec。
    /// `current_namespace_caps` 取栈中所有层级的并集——子 namespace 继承
    /// 父 namespace 的 capabilities（声明更多能力不破坏安全性）。
    pub(crate) namespace_caps_stack: Vec<Vec<Ident>>,
    /// RFC 016 M3 §3.4 能力 gating Phase 1+：native 模块名 → capability 标签。
    /// 在 `check_and_register_native_module` 中填充，供 `check_native_method`
    /// 校验调用方 namespace 是否声明了对应能力。
    pub(crate) native_caps: IndexMap<Ident, Option<Ident>>,
    /// `out` parameter definite-assignment state for the current function/method body.
    pub(crate) out_flow: Option<OutParamState>,
    /// Nullable variable narrowing state for the current function/method body.
    pub(crate) null_flow: Option<NullFlowState>,
    /// RFC 009 L2 链式 `?.`/`!.` 守卫深度：NullCond/ForceDeref 检查其 access
    /// 时递增，Field/MethodCall 的 nullable receiver 分支据此豁免——access 的
    /// receiver 可空性由外层 `?.`/`!.` 的空测试兜底，不构成裸 `.` 误用。
    pub(crate) null_guard_depth: usize,
    /// RFC 016 M1: native 契约模块缓存。
    ///
    /// `check_module` 会用 `TypeRegistry::from_module` 重建 registry，覆盖
    /// 之前注册的 native 模块。此处缓存 native 契约，使 `check_module` 在
    /// 重建 registry 后能自动重注册，保证 native 方法分派可用。
    pub(crate) native_modules: Vec<NativeModule>,
    /// RFC 016 M1：native callback 类型注册表（类型名 → NativeCallback 定义）。
    ///
    /// 在 `check_and_register_native_module` 中从 `module.callbacks` 填充，
    /// 供 `check_native_fn` 识别 callback 参数类型、`check_native_method`
    /// 在 lambda 实参匹配 callback 形参时执行有无捕获检查。
    pub(crate) native_callbacks: IndexMap<Ident, NativeCallback>,
    /// RFC 017 M4-link Phase B: 跨 `.ao` 包外部符号缓存。
    ///
    /// `register_external_symbols` 缓存条目并立即注册到 registry；
    /// `check_module` 重建 registry 后 `reregister_external_symbols` 用此缓存
    /// 重注册，保证跨包类型引用在 `check_module` 期间仍能命中 registry。
    pub(crate) external_symbols: Vec<crate::external_symbols::ExternalSymbolEntry>,
    /// RFC 012 M1: 符号属性表，typeck 产物之一。
    ///
    /// `check_class` / `check_struct` / `check_interface` 在声明处理阶段
    /// 将解析后的 `ResolvedAttribute` 注册到此表；typeck 完成后通过
    /// [`attribute_table`](TypeChecker::attribute_table) 暴露给外部消费者
    /// （如 `arc-orm` 子库构建 EntityMap、codegen 诊断）。
    pub(crate) attribute_table: AttributeTable,
    /// [Builtin] 方法注册表：DefId → BuiltinMeta。
    ///
    /// check_class 在 resolve_attributes 之后，检查 method 是否有 [Builtin] 属性，
    /// 有则提取 ABI 参数存入此表。typeck 据此决定 skip_body（跳过方法体检查），
    /// MIR lower 据此路由 func 名格式（`.` vs `::`），codegen 据此分发 ABI。
    pub(crate) builtin_registry: IndexMap<DefId, BuiltinMeta>,
    /// RFC 012 M1: 类型符号名 → DefId 反查表。
    ///
    /// 键为类型名（class / struct / interface / enum 名），值为 typeck
    /// 内部分配的 `DefId`，与 `attribute_table` 的键对应。供外部消费者
    /// （如 `arc-orm`）按类型名查询属性：先反查 `DefId`，再用
    /// `attribute_table.find_attr(def_id, "Table")` 取属性。
    pub(crate) class_def_ids: IndexMap<Ident, DefId>,
    /// RFC 012 M4-1: `(类名, generic_arity) → DefId` 反查表（仅 arity > 0）。
    ///
    /// 支持同名类按泛型 arity 重载（C# 风格 arity overloading）：
    /// 如 `GenerateToAttribute`（arity 0）与 `GenerateToAttribute<T>`（arity 1）
    /// 共享同名但分配独立 DefId，避免 attribute_table 中 AllowMultiple 校验
    /// 误把两个不同类的 `[AttributeUsage]` 当作同符号重复附加。
    ///
    /// arity 0 的类仍走 `class_def_ids`（保留 name-only 外部 API）。
    pub(crate) generic_class_def_ids: IndexMap<(Ident, usize), DefId>,
    /// RFC 012 M1: `(类型名, 成员名) → DefId` 反查表。
    ///
    /// 键为 `(owner_type, member_name)`，覆盖 field / property / method。
    /// 供外部消费者按 `User.age` 形式查询成员上的 `[Column]` 等属性。
    pub(crate) member_def_ids: IndexMap<(Ident, Ident), DefId>,
    /// RFC 012 B2: `DefId → (类型名, 成员名)` 反向反查表。
    ///
    /// 与 `member_def_ids` 对称——键值互换，供 `method_signature(DefId)`
    /// API 反查 DefId 对应的类型名 + 成员名，再从 `registry.types` 查
    /// 方法签名。仅在 `member_def_ids` 插入时同步填充，保持一致。
    pub(crate) def_id_members: IndexMap<DefId, (Ident, Ident)>,
    /// RFC 009 M4-3: 所有 class 的原始 ClassDef 缓存，供 M4-3 在
    /// `collect_macros` 中扫描派生宏特性类的构造函数体识别
    /// `this.<slot>(Func<string>)` 调用。
    ///
    /// 与 `class_templates`（仅泛型）不同，此处缓存**所有** class 的 AST
    /// （含非泛型）。即便派生宏特性类在 `check_class_inner` 因 `this.<slot>`
    /// 方法查找失败而提前 return Err，未把 ctor body 推入 `typed_fns`，
    /// 仍可通过此表直接访问原始 AST Block。
    pub(crate) class_defs: IndexMap<Ident, ast::ClassDef>,
    /// RFC 038：枚举 AST 缓存（名 → EnumDef），由 `collect_enum_attributes`
    /// 在 `check_module_items` pre-pass 填充。供 `Enum.GetOptions<E>()` 的
    /// 泛型特化（`specialize_enum_options_body`）按枚举名遍历各成员变体，
    /// 生成编译期烘焙的 `EnumOptions<E>` 构造体（零反射）。
    pub(crate) enum_defs: IndexMap<Ident, ast::EnumDef>,
    /// RFC 009 M4-2: 宏目录（容器 + 特性），由 `collect_macros` 在
    /// `check_module` 末尾构建。供 M4-3 ~ M4-9 各阶段查询：
    /// - M4-3 扫描 feature 构造函数体识别 `this.<method>(Func<string>)` 调用
    /// - M4-4 受限求值器求值委托
    /// - M4-6 splice 展开代码到 container 对应 slot 方法体
    pub macro_catalog: crate::macro_eval::MacroCatalog,
    /// RFC 009 M5-3: Source Generator 在 Pass 3 生成的源代码 AST 列表。
    ///
    /// `expand_source_generators` 把每个 `[SourceGenerator]` 类的 `Generate`
    /// 方法体求值为 `Vec<String>`，每个字符串再用 `Parser::parse_program`
    /// 解析为 `Program` 追加到此列表。Pass 4 完整 typeck 后，codegen 层
    /// 读取此列表将这些生成代码与原代码同等待遇输出。
    pub generated_programs: Vec<ast::Program>,
    /// RFC 009 M4-7: 当前 typeck Pass 模式。
    ///
    /// `check_module` 入口置为 [`MacroPassMode::Skeleton`]；Pass 3 splice
    /// 完成后由外部管线调用 [`check_macro_containers_pass4`] 置为
    /// [`MacroPassMode::Full`]。`check_class_inner` 据此决定是否跳过
    /// 宏容器类方法体检查（Pass 2）与是否跳过属性解析（Pass 4）。
    pub(crate) macro_pass_mode: MacroPassMode,
    /// RFC 032 v0.11: 反向推断的宏容器类名集合。
    ///
    /// 由 [`compute_macro_container_names`](Self::compute_macro_container_names)
    /// 在 `check_module` 入口（`from_module` 之后、`check_module_items`
    /// 之前）预计算，供 Pass 2 骨架模式 `check_class` 决定是否跳过宏容器
    /// 方法体检查。与 `collect_macros` Pass 1b 使用相同反向推断规则——
    /// 避免不同入口使用不同识别逻辑（v0.10 旧 `class_def_is_macro_container`
    /// 检查 `[GenerateTo]` 属性的设计已被 v0.11 修订废弃）。
    pub(crate) macro_container_names: std::collections::HashSet<Ident>,
    /// RFC 017 M4-link Phase B：std 库中定义的所有类/struct/interface/enum 名称。
    ///
    /// `fn_linkage_for_class` 按此判定类是否来自 std 库（不论具体路径），
    /// 来自 std 库的类统一标记 `FnLinkage::Monomorphized` → MIR
    /// `LinkonceOdr`，使 std 库代码在跨 `.o` 场景（main.o + lib.o
    /// 均 `using Arc;`）中被链接器自动去重。
    ///
    /// 由管线层（`crates/arc/src/pipeline.rs`）在 `TypeChecker::new()` 后
    /// 调用 `set_std_class_names` 注入。管线扫描 `CompileUnit.program.items`
    /// 中 file_id 位于 `std/` 目录下的所有类定义，收集其名称填入此集合。
    /// `is_builtin_facade` 列表中的 stub facade 类也自动归属此集合。
    pub(crate) std_class_names: std::collections::HashSet<String>,
    /// RFC 026 M2：FileId → 包名（与 `TypeRegistry.file_packages` 同步）。
    pub(crate) file_packages: std::collections::HashMap<ast::FileId, String>,
    /// RFC 026 M2+：包名 → InternalsVisibleTo 列表（与 `TypeRegistry.internals_visible_to` 同步）。
    pub(crate) internals_visible_to: std::collections::HashMap<String, Vec<String>>,
    /// RFC 026 M2：当前检查站点所属包。
    pub(crate) current_package: Option<String>,
    /// RFC 026 M2：入口项目默认包（无更具体 file 上下文时回退）。
    pub(crate) default_package: Option<String>,
    /// P0 双引擎收敛：span 键表达式类型表（`check_expr_at` 出口记录）。
    /// 管线在 MIR lower 前经 `take_expr_type_table` 导出传入 lower_module，
    /// MIR `infer_type_from_expr` 命中即采用 typeck 结论，未命中回落旧推断。
    pub(crate) expr_type_table: crate::typed::ExprTypeTable,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            scopes: vec![Self::builtin_scope()],
            typed_fns: vec![],
            errors: vec![],
            warnings: vec![],
            registry: TypeRegistry {
                types: IndexMap::new(),
                extensions: IndexMap::new(),
                init_only_props: Default::default(),
                declared_properties: Default::default(),
                file_packages: Default::default(),
                internals_visible_to: Default::default(),
                shadowed_types: Default::default(),
                synth_hosts: Default::default(),
                builtin_static_props: Default::default(),
                entry_package: None,
                delegate_aliases: std::collections::HashMap::new(),
            },
            current_class: None,
            current_fn_is_static: false,
            return_slot: vec![],
            in_async: false,
            loop_depth: 0,
            in_ctor: false,
            current_readonly_context: false,
            mono_depth: 0,
            next_def_id: 0,
            list_target_seq: std::cell::Cell::new(0),
            pos_scrut_seq: 0,
            class_templates: IndexMap::new(),
            fn_templates: IndexMap::new(),
            fn_template_origins: IndexMap::new(),
            fn_defs: IndexMap::new(),
            extension_fn_templates: IndexMap::new(),
            interface_templates: IndexMap::new(),
            delegate_templates: IndexMap::new(),
            mono_origins: IndexMap::new(),
            instantiated: IndexSet::new(),
            violated: IndexMap::new(),
            recursion_class: RecursionGuard::new(),
            recursion_iface: RecursionGuard::new(),
            type_param_scope: vec![],
            where_clause_scope: vec![],
            extension_imports: vec![],
            enclosing_namespace: vec![],
            namespace_caps_stack: vec![vec![]],
            native_caps: IndexMap::new(),
            out_flow: None,
            null_flow: Some(NullFlowState::new()),
            null_guard_depth: 0,
            native_modules: Vec::new(),
            native_callbacks: IndexMap::new(),
            external_symbols: Vec::new(),
            attribute_table: AttributeTable::new(),
            builtin_registry: IndexMap::new(),
            class_def_ids: IndexMap::new(),
            generic_class_def_ids: IndexMap::new(),
            member_def_ids: IndexMap::new(),
            def_id_members: IndexMap::new(),
            class_defs: IndexMap::new(),
            enum_defs: IndexMap::new(),
            macro_catalog: crate::macro_eval::MacroCatalog::new(),
            generated_programs: Vec::new(),
            macro_pass_mode: MacroPassMode::Skeleton,
            macro_container_names: std::collections::HashSet::new(),
            std_class_names: std::collections::HashSet::new(),
            file_packages: std::collections::HashMap::new(),
            internals_visible_to: std::collections::HashMap::new(),
            current_package: None,
            default_package: None,
            expr_type_table: crate::typed::ExprTypeTable::default(),
        }
    }

    /// P0 双引擎收敛：导出 span 键表达式类型表（取出后 checker 内表清空）。
    /// 调用时机：`check_module()` 之后、`mir::lower_module()` 之前。
    pub fn take_expr_type_table(&mut self) -> crate::typed::ExprTypeTable {
        std::mem::take(&mut self.expr_type_table)
    }

    /// RFC 017 M4-link Phase B：注入 std 库中定义的类名集合。
    /// 调用时机：`TypeChecker::new()` 之后、`check_module()` 之前。
    pub fn set_std_class_names(&mut self, names: std::collections::HashSet<String>) {
        self.std_class_names = names;
    }

    /// RFC 026 M2：注入 FileId → 包名映射，并可选设置入口默认包。
    /// 调用时机：`TypeChecker::new()` 之后、`check_module()` 之前。
    pub fn set_file_packages(
        &mut self,
        packages: std::collections::HashMap<ast::FileId, String>,
        entry_package: Option<String>,
    ) {
        self.file_packages = packages;
        self.default_package = entry_package;
        self.current_package = self.default_package.clone();
    }

    /// RFC 025 M2+：注入 包名 → InternalsVisibleTo 列表（对标 C# `[assembly: InternalsVisibleTo]`）。
    /// 调用时机：`TypeChecker::new()` 之后、`check_module()` 之前。
    pub fn set_internals_visible_to(
        &mut self,
        map: std::collections::HashMap<String, Vec<String>>,
    ) {
        self.internals_visible_to = map;
    }

    pub fn registry(&self) -> &TypeRegistry {
        &self.registry
    }

    /// RFC 009 M1: 获取 typeck 产出的符号属性表。
    ///
    /// 调用方（如 `arc-orm` 子库构建 EntityMap、codegen 诊断）通过此方法
    /// 查询符号上的属性；属性注册由 typeck 内部 `check_*` 模块完成，外部
    /// 只读访问。
    pub fn attribute_table(&self) -> &AttributeTable {
        &self.attribute_table
    }

    /// RFC 017 M4-link Phase B：获取从 `.ao` 包加载的外部符号缓存。
    ///
    /// 调用方（codegen）通过此方法取出 `ExternalSymbolKind::Function` 条目，
    /// 按签名发射 `declare <ret> @<name>(<params>)`（DeclareOnly linkage）。
    /// 定义来自被链接的 lib.o，跨 `.o` 不重复（external linkage 单一定义来源）。
    pub fn external_symbols(&self) -> &[crate::external_symbols::ExternalSymbolEntry] {
        &self.external_symbols
    }

    /// RFC 009 M4-2: 获取 typeck 产出的宏目录。
    ///
    /// 调用方（如 M4-3 构造函数分析、M4-4 求值器、M4-6 splice 注入器）
    /// 通过此方法查询宏容器与宏特性派生类信息。目录在 `check_module`
    /// 末尾构建，外部只读访问。
    pub fn macro_catalog(&self) -> &crate::macro_eval::MacroCatalog {
        &self.macro_catalog
    }

    /// RFC 012 M1: 按类型名查询其 `DefId`，用于反查 `attribute_table`。
    ///
    /// 类型涵盖 class / struct / interface / enum。返回的 `DefId` 可传给
    /// [`attribute_table`](Self::attribute_table) 的查询方法。
    pub fn class_def_id(&self, type_name: &str) -> Option<DefId> {
        self.class_def_ids.get(&Ident::from(type_name)).copied()
    }

    /// RFC 009 M1: 按类型名 + 成员名查询 `DefId`，用于反查 `attribute_table`。
    ///
    /// 成员涵盖 field / property / method。如 `User.age` 上的 `[Column]`
    /// 属性可通过 `member_def_id("User", "age")` 获取 `DefId` 后查询。
    pub fn member_def_id(&self, type_name: &str, member: &str) -> Option<DefId> {
        self.member_def_ids
            .get(&(Ident::from(type_name), Ident::from(member)))
            .copied()
    }

    /// RFC 032 B2: 按 `DefId` 查询方法签名（通用机制 API）。
    ///
    /// typeck 仅提供通用查询能力——不感知「测试」「断言」等 QIF 语义。
    /// 调用方（如 arc CLI 的 QIF 收集器）通过 `attribute_table` 获取标记了
    /// 某 attribute 的方法 DefId 后，调用本 API 查询方法签名做校验。
    ///
    /// 返回首个匹配的 `OopMethodSig`（MVP 不处理同名重载——QIF 测试方法
    /// 不重载，单签名足够）。DefId 不存在或对应类/方法未注册返回 `None`。
    pub fn method_signature(&self, def_id: DefId) -> Option<&crate::oop_types::OopMethodSig> {
        let (class_name, method_name) = self.def_id_members.get(&def_id)?;
        let nom = self.registry.types.get(class_name)?;
        let overloads = nom.methods.get(method_name)?;
        overloads.first()
    }

    /// RFC 012 B2: 按 `DefId` 反查类型名 + 成员名（通用机制 API）。
    ///
    /// 与 `method_signature` 共享 `def_id_members` 反查表。供调用方在
    /// 获取 DefId 后查询归属类型与成员名（如 codegen 序列化方法元数据
    /// 到 rodata 全局表时需类名 + 方法名）。
    pub fn def_id_member(&self, def_id: DefId) -> Option<(&Ident, &Ident)> {
        let (class_name, member_name) = self.def_id_members.get(&def_id)?;
        Some((class_name, member_name))
    }

    /// RFC 037 M-D0：收集 `[Observable]` 属性集合（(类名, 属性名) 对）。
    ///
    /// 经 `member_def_ids`（成员 → DefId 反查表）逐一查询
    /// `attribute_table.has_attr(def_id, "Observable")`；覆盖两类属性：
    /// - **auto-property**（backing field 以实例字段注册在 registry）——
    ///   codegen 在 FieldSet 发射点合成「相等性短路 + 隐藏通知通道」（setter
    ///   合成路径，`emit_observable_property_set`）；
    /// - **custom-accessor 属性**（注册为 `get_X`/`set_X` 方法、无同名 backing
    ///   field，RFC 037 §5.3 场景 6）——同样分配隐藏通道槽，但**不合成
    ///   setter**；显式 `NotifyPropertyChanged("Name")` 为其唯一通知路径。
    ///
    /// 静态字段排除（通道为每实例存储，无静态语义）。
    ///
    /// codegen 消费 `ProgramLayouts.observable_properties`（管线层合并本
    /// 结果）在 FieldSet 发射点合成「相等性短路 + 隐藏通知通道」，并为
    /// `ObserveProperty` / `NotifyPropertyChanged` 定位通道槽。
    pub fn observable_properties(&self) -> IndexSet<(Ident, Ident)> {
        let mut out = IndexSet::new();
        for ((owner, member), def_id) in &self.member_def_ids {
            if !self.attribute_table.has_attr(*def_id, "Observable") {
                continue;
            }
            // auto-property：backing field 以实例字段注册在 registry。
            let is_auto_backing_field = self
                .registry
                .types
                .get(owner)
                .and_then(|nom| nom.fields.get(member))
                .is_some_and(|f| !f.is_static);
            if is_auto_backing_field {
                out.insert((owner.clone(), member.clone()));
                continue;
            }
            // custom-accessor 属性：类上存在 `get_<Name>` 实例访问器且无同名
            // backing field（与 MIR `is_custom_accessor_property` 判定一致；
            // auto-property 不入 methods）。static 属性排除（通道为每实例）。
            let getter: Ident = format!("get_{member}").into();
            let is_custom_accessor_prop = self
                .registry
                .resolve_method(owner, &getter, &self.access_ctx())
                .map(|sig| !matches!(sig.modifier, ast::MethodModifier::Static))
                .unwrap_or(false)
                && self
                    .registry
                    .types
                    .get(owner)
                    .map(|nom| !nom.fields.contains_key(member))
                    .unwrap_or(false);
            if is_custom_accessor_prop {
                out.insert((owner.clone(), member.clone()));
            }
        }
        out
    }

    /// RFC 016 M1: 设置 native 契约模块。
    ///
    /// 应在 `check_module` 之前调用。`check_module` 会在重建 registry 后
    /// 从此字段重新注册 native 模块，避免 `from_module` 丢弃注册。
    pub fn set_native_modules(&mut self, modules: Vec<NativeModule>) {
        self.native_modules = modules;
    }

    fn alloc_def_id(&mut self) -> DefId {
        let id = DefId(self.next_def_id);
        self.next_def_id += 1;
        id
    }

    /// RFC 012 M3-5: 复用或分配类 DefId，用于处理前向引用。
    ///
    /// 当属性解析（如 `[AttributeUsage(AttributeTargets.Class)]`）在类被
    /// `check_class_inner` 处理之前需要引用类（如 `AttributeUsageAttribute`
    /// 引用 `AttributeTargets`）时，预先分配 `DefId` 并填入 `class_def_ids`。
    /// 后续 `check_class_inner` 调用时复用同一 `DefId`，保证属性表反查链一致。
    ///
    /// RFC 012 M4-1: 支持 arity overloading。`arity > 0` 的类走
    /// `generic_class_def_ids`（按 `(name, arity)` 键），避免与同名 arity 0
    /// 类共享 DefId 导致 attribute_table AllowMultiple 校验误判。
    pub(crate) fn ensure_class_def_id(&mut self, name: &Ident, arity: usize) -> DefId {
        if arity == 0 {
            if let Some(&existing) = self.class_def_ids.get(name) {
                return existing;
            }
            let id = self.alloc_def_id();
            self.class_def_ids.insert(name.clone(), id);
            id
        } else {
            let key = (name.clone(), arity);
            if let Some(&existing) = self.generic_class_def_ids.get(&key) {
                return existing;
            }
            let id = self.alloc_def_id();
            self.generic_class_def_ids.insert(key, id);
            id
        }
    }

    fn class_field_names(&self, class: &Ident) -> Vec<Ident> {
        let mut names = Vec::new();
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            let Some(nom) = self.registry.types.get(&cn) else {
                break;
            };
            for (f, finfo) in nom.fields.iter() {
                if finfo.is_const {
                    continue;
                }
                if !names.contains(f) {
                    names.push(f.clone());
                }
            }
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        names
    }

    /// RFC 006 M2：返回 `class` 及其基类链中所有**静态字段**名（含 `const` 字段，
    /// 因为 `const` 隐含 `static`）。
    ///
    /// 用于静态方法的 `TypedFn.class_fields` 过滤——静态方法仅能访问静态字段。
    /// MIR lower（M3）据此将字段访问降级为 `MirOperand::StaticField`。
    fn static_field_names(&self, class: &Ident) -> Vec<Ident> {
        let mut names = Vec::new();
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            let Some(nom) = self.registry.types.get(&cn) else {
                break;
            };
            for (f, finfo) in nom.fields.iter() {
                // const 隐含 static，一并纳入静态字段集合
                if (finfo.is_static || finfo.is_const) && !names.contains(f) {
                    names.push(f.clone());
                }
            }
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        names
    }

    /// RFC 006 M2：判断 `name` 是否为 `class`（或其基类链）的**实例字段**。
    ///
    /// 实例字段定义：`!is_static && !is_const`。用于 `check_expr_inner` 拦截
    /// 静态方法内访问实例字段的违规。
    pub(crate) fn is_instance_field_of(&self, class: &Ident, name: &Ident) -> bool {
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            let Some(nom) = self.registry.types.get(&cn) else {
                break;
            };
            if let Some(finfo) = nom.fields.get(name) {
                if !finfo.is_static && !finfo.is_const {
                    return true;
                }
            }
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        false
    }

    /// Rewrite a bare identifier that names an instance field in the current
    /// class/struct context into `this.<field>` for type-checking. Returns
    /// `None` when there is no current class context, the current function is
    /// static, or the name does not match any instance field.
    pub(crate) fn rewrite_bare_instance_field(&self, name: &Ident) -> Option<ast::Expr> {
        let class_name = self.current_class.as_ref()?;
        if self.current_fn_is_static {
            return None;
        }
        if self.is_instance_field_of(class_name, name) {
            Some(ast::Expr::Field {
                receiver: Box::new(Spanned::new(ast::Expr::This, Span::DUMMY)),
                field: name.clone(),
            })
        } else {
            None
        }
    }

    /// 判断 `name` 是否为当前类（含基类链）的实例方法，且存在参数个数匹配的
    /// 重载。供裸实例方法调用 `_bump()` → `this._bump()` 重写使用。
    pub(crate) fn has_instance_method(&self, class: &Ident, name: &Ident, argc: usize) -> bool {
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            let Some(nom) = self.registry.types.get(&cn) else {
                break;
            };
            if let Some(sigs) = nom.methods.get(name) {
                if sigs.iter().any(|s| {
                    s.modifier != MethodModifier::Static
                        && !s.is_static_abstract
                        && s.params.len() == argc
                }) {
                    return true;
                }
            }
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        false
    }

    fn push_typed_fn(
        &mut self,
        name: Ident,
        owner: Option<Ident>,
        is_ctor: bool,
        params: Vec<(Ident, TypeId)>,
        ret: TypeId,
        body: Option<Block>,
        typed_body: Option<TypedBlock>,
        is_async: bool,
        linkage: FnLinkage,
        // RFC 006 M2：当前函数是否为 `static` 方法。true 时 `class_fields`
        // 仅含静态字段（含 const），供 MIR lower 走 `StaticField` 路径（M3）。
        is_static: bool,
        // RFC 009 M3：`[Parallelize]` attribute 标记。true 时 codegen 在函数内
        // 所有 while 循环 backedge 附加 `!llvm.loop.vectorize.enable` metadata。
        parallelize: bool,
    ) {
        let class_fields = owner
            .as_ref()
            .map(|c| {
                // RFC 006 M2：静态方法仅可见静态字段；实例方法/构造函数可见所有非 const 字段。
                if is_static {
                    self.static_field_names(c)
                } else {
                    self.class_field_names(c)
                }
            })
            .unwrap_or_default();
        let def_id = self.alloc_def_id();
        self.typed_fns.push(TypedFn {
            def_id,
            name,
            params,
            ret,
            body,
            typed_body,
            is_async,
            owner,
            is_ctor,
            class_fields,
            is_static,
            linkage,
            parallelize,
        });
    }

    /// RFC 009 M3：检测属性列表中是否含 `[Parallelize]`。
    ///
    /// 该属性为编译提示（hint），无参数。出现即标记函数为向量化候选，
    /// codegen 据此在 while 循环 backedge 附加 `!llvm.loop.vectorize.enable`
    /// metadata。属性平台无关——实际向量化效果由 LLVM 据目标 CPU 特征决定：
    /// - x86-64：SSE2 强制，AVX2/AVX-512 运行时检测
    /// - AArch64：NEON 恒定
    /// - 其他：标量退化（无错误，仅无性能收益）
    ///
    /// 遵循 C# 规范：`[Parallelize]` 与 `[ParallelizeAttribute]` 等价。
    pub(crate) fn has_parallelize_attr(attrs: &[ast::Attribute]) -> bool {
        attrs.iter().any(|a| {
            // 属性路径取最后一段作为属性名（支持 `Arc.Parallel.Parallelize` 等限定形式）
            a.path.last().is_some_and(|name| {
                let s = name.as_str();
                s == "Parallelize" || s == "ParallelizeAttribute"
            })
        })
    }

    pub fn with_registry(registry: TypeRegistry) -> Self {
        let mut tc = Self::new();
        tc.registry = registry;
        tc
    }

    fn builtin_scope() -> IndexMap<Ident, TypeId> {
        let mut scope = IndexMap::new();
        scope.insert("int".into(), TypeId::Int);
        scope.insert("bool".into(), TypeId::Bool);
        scope.insert("string".into(), TypeId::String);
        scope.insert("uint".into(), TypeId::UInt);
        scope.insert("ulong".into(), TypeId::ULong);
        scope.insert("ushort".into(), TypeId::UShort);
        scope.insert("sbyte".into(), TypeId::SByte);
        // RFC 006 M1: object 根类型作为预定义标识符
        scope.insert("object".into(), TypeId::Object);
        scope.insert("void".into(), TypeId::Void);
        scope.insert("Task".into(), TypeId::Named("Task".into()));
        scope.insert(
            "rt_expr_tree_summary".into(),
            TypeId::Func {
                params: vec![],
                ret: Box::new(TypeId::String),
            },
        );
        scope
    }

    /// Pass 2 / Pass 4 累积的已类型检查函数（含宏容器与 Source Generator 产物）。
    pub fn typed_fns(&self) -> &[TypedFn] {
        &self.typed_fns
    }

    /// RFC 005 里程碑④：编译期 warning 列表（`arc-cycle-001` 字段环等）。
    /// 与 `check_module` 的 `Err` 路径分离——warning 永不当 error。
    pub fn warnings(&self) -> &[TypeWarning] {
        &self.warnings
    }

    /// 取出并清空编译期 warning 列表（供 pipeline 打印到 stderr）。
    pub fn take_warnings(&mut self) -> Vec<TypeWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// 强制实例化泛型函数（供 pipeline 在静态字段初始化器中引用泛型函数时使用）。
    ///
    /// 静态字段初始化器不经过 `check_expr` 类型检查路径（其 init 表达式以原始 AST
    /// 形式存储），因此泛型调用（如 `RegisterProperty<string>(...)`）不会自动触发
    /// `instantiate_generic_fn`。此方法允许 pipeline 在收集 typed_fns 之前显式
    /// 请求单态化。
    pub fn force_instantiate_generic_fn(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<Ident, TypeError> {
        self.instantiate_generic_fn(def, args)
    }

    /// 强制实例化泛型类（供 pipeline 在合成 `[Observable]` 隐藏通知通道时使用）。
    ///
    /// `[Observable]` auto-property 的 setter 由 codegen 合成，发射
    /// `@Signal_<T>_Set` / `@__ctor::Signal_<T>` 调用。若用户源码未显式引用
    /// `Signal<T>`（无 `ObserveProperty` 调用），typeck 不会自动单态化该泛型类，
    /// 导致 tree-shake 后 `Signal_<T>` 方法缺失（LLVM undefined value）。此方法
    /// 允许 pipeline 在收集 typed_fns 之前显式请求单态化。
    pub fn force_instantiate_generic_class(
        &mut self,
        def: &Ident,
        args: &[TypeId],
    ) -> Result<TypeId, TypeError> {
        self.instantiate_generic_class(def, args)
    }

    /// 判断 `name`（如 `AssemblyLoadContext::RegisterWeakReference`）是否为
    /// 泛型函数/方法**模板**（非单态化实例）。
    ///
    /// RFC 012 S6 A1：泛型模板是编译期蓝图，其方法体引用未解析的类型参数符号
    /// （如 `weak.GetWeakSlot()` → `Weak_T_GetWeakSlot`），无独立可发射的
    /// 运行期 body——仅单态化实例才有合法 body。`arc build --dynamic`（库无
    /// Main/Entry）在 tree-shake 时「全量保留」，会误把模板纳入发射集 → LLVM
    /// undefined symbol。pipeline 据此把模板从发射集剔除（与可执行构建中
    /// 模板不可达被 tree-shake 剪除的既有语义一致）。
    pub fn is_generic_template_fn(&self, name: &str) -> bool {
        self.fn_templates.contains_key(name) || self.extension_fn_templates.contains_key(name)
    }

    /// RFC 012 S6 A1：收集所有泛型模板（`fn_templates` + `extension_fn_templates`）
    /// 的符号名集合。pipeline 在 tree-shake 前据此把模板从 MIR 发射集剔除——
    /// 模板是编译期蓝图，其方法体引用未单态化的类型参数符号（如
    /// `Weak_T_GetWeakSlot`），仅单态化实例才有合法 body。对 `--dynamic` 库
    /// 构建（无 Main/Entry，tree-shake 全量保留）尤为关键。
    pub fn generic_template_names(&self) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        for k in self.fn_templates.keys() {
            out.insert(k.as_str().to_string());
        }
        for k in self.extension_fn_templates.keys() {
            out.insert(k.as_str().to_string());
        }
        out
    }

    /// 错误池收敛出口：内容级去重（保留首次出现序）。
    ///
    /// 诊断无位置信息（[`TypeError`] 不携带 span），同一条违约因同一实例
    /// 化点多次触达（错误恢复语义下重跑约束检查）、同一声明点多次 lower
    /// 而重复入池时不携带任何增量信息，纯噪声。在池的收敛出口统一去重
    /// ——而非在写入点各自防重（写入点散布全库，散点治理不可维护）；
    /// 对齐 tests.rs「诊断去重属管道层议题」的官方落点。诊断顺序有意义，
    /// 不排序，仅去除全等重复（变体 + 字段值完全一致的后续项）。
    pub(crate) fn take_errors_deduped(&mut self) -> Vec<TypeError> {
        let errors = std::mem::take(&mut self.errors);
        let mut seen: IndexSet<TypeError> = IndexSet::new();
        errors
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .collect()
    }

    pub fn check_module(&mut self, module: &HirModule) -> Result<Vec<TypedFn>, Vec<TypeError>> {
        // RFC 009 M4-7: Pass 2 骨架 typeck —— 宏容器类跳过方法体检查。
        // 模式在 `check_class_inner` 中决定是否跳过 body、是否解析属性。
        self.macro_pass_mode = MacroPassMode::Skeleton;

        // RFC 037: 预合并 partial class 声明。若 HIR 含 partial class，克隆
        // HIR 并合并同组的所有 partial 声明为单一 ClassDef，下游
        // `from_module` 与 `check_module_items` 仅看到合并后的 ClassDef。
        // 多数项目无 partial class，无需克隆 HIR（避免无谓开销）。
        let owned_module;
        let module: &HirModule = if self.has_partial_classes(module) {
            owned_module = self.merge_partials_in_hir(module.clone());
            &owned_module
        } else {
            module
        };

        // CD-30：包感知注册——入口包同名类型优先于依赖包（顶层类遮蔽依赖包
        // internal 类）。file_packages 已在 from_module 内部注入（取代下方
        // 重建后再补的旧路径），default_package 即入口包名。
        self.registry = TypeRegistry::from_module_with_entry(
            module,
            &self.file_packages,
            self.default_package.as_deref(),
        );
        // RFC 025 M2+：重建 registry 后恢复 InternalsVisibleTo 映射。
        self.registry.internals_visible_to = self.internals_visible_to.clone();
        // RFC 016 M1: `from_module` 重建 registry 会覆盖之前注册的 native 契约，
        // 此处用缓存的 native_modules 重注册，保证 `libc.puts(...)` 等分派可用。
        self.reregister_native_modules();
        // RFC 017 M4-link Phase B: 同样重注册缓存的外部符号（来自 `.ao` 包），
        // 保证 `using Lib;` 后跨包类型引用能命中 registry。
        self.reregister_external_symbols();
        // 外部符号重注册后再次确保包图仍在（reregister 不碰 file_packages）。
        self.registry.file_packages = self.file_packages.clone();
        if self.current_package.is_none() {
            self.current_package = self.default_package.clone();
        }
        if let Err(oop_errs) = self.registry.validate_all() {
            for e in oop_errs {
                self.errors.push(TypeError::Oop(e.to_string()));
            }
        }

        self.extension_imports = self.registry.resolve_extension_imports(&module.imports);

        // Register type names in global scope (skip generic templates — need type args).
        for (name, nom) in &self.registry.types {
            if nom.generic_params.is_empty() {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(name.clone(), TypeId::Named(name.clone()));
            }
        }
        for import in &module.imports {
            match import.kind {
                hir::ImportKind::Alias => {
                    let target = import
                        .path
                        .last()
                        .cloned()
                        .unwrap_or_else(|| import.alias.clone());
                    if self.registry.types.contains_key(&target) {
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(import.alias.clone(), TypeId::Named(target));
                    }
                }
                hir::ImportKind::Type | hir::ImportKind::Namespace => {
                    if self.registry.types.contains_key(&import.alias) {
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(import.alias.clone(), TypeId::Named(import.alias.clone()));
                    }
                }
            }
        }

        // RFC 012 v0.11: 预计算宏容器类名集合——通过反向推断（扫描所有
        // `GenerateToAttribute<T>` 派生类的 T 参数）。`from_module` 已基于
        // AST 填充 `nom.base_types`，故可在 `check_module_items` 之前完成。
        // 供 Pass 2 骨架模式 `check_class` 决定是否跳过宏容器方法体检查，
        // 与 `collect_macros` Pass 1b 使用相同识别规则。
        self.macro_container_names = self.compute_macro_container_names();

        // 全局预扫描：递归遍历整个模块树，预注册所有泛型类模板到
        // `class_templates`。解决跨模块前向引用问题——当子模块 A 的 Pre-pass
        // 0b 需要解析 `List<QIFResult>` 但 `List<T>` 模板在兄弟子模块 B 中时，
        // 若仅依赖 per-module Pre-pass 0，B 的模板在进入 A 的 Pre-pass 0b 时
        // 尚未注册，导致 `lower_type` 丢弃泛型实参返回裸名 `List`。
        self.pre_register_all_generic_templates(module);
        self.pre_register_delegate_aliases(module);
        self.pre_instantiate_field_generic_types(module);

        self.check_module_items(module, &[]);

        // RFC 009 M4-2: 所有类 typeck 完成后构建宏目录。
        // 容器与特性可能相互引用，须在全 registry 就绪后扫描。
        // RFC 009 M4-7: collect_macros 同时返回 Pass 3 累积的诊断
        // （如 Expression 实参/形参数量不匹配），合并到 self.errors。
        let (catalog, macro_errors) = self.collect_macros();
        self.macro_catalog = catalog;
        self.errors.extend(macro_errors);

        // RFC 012 M4-8 D12.4: 循环依赖检测——禁止宏容器类自身标注
        // GenerateToAttribute<T>（T 自我引用或循环引用）。
        self.check_cyclic_macro_dependencies();

        // RFC 005 里程碑④：编译期声明级字段环 warning 通道（arc-cycle-001）。
        // 检测时机 = 注册表完全填充后（`check_module_items` 之后）、最终错误检查前
        //（RFC 005 §2.6）。warning 与 TypeError 语义分离，不阻断编译。
        let detector = crate::field_cycle::FieldCycleDetector::new(
            &self.registry,
            &self.mono_origins,
            &self.class_defs,
        );
        self.warnings.extend(detector.detect());

        if self.errors.is_empty() {
            Ok(self.typed_fns.clone())
        } else {
            Err(self.take_errors_deduped())
        }
    }

    /// RFC 009 M4-7 Pass 4: 对宏容器类进行完整 typeck。
    ///
    /// 在 Pass 3（外部管线 splice 展开代码到宏容器类方法体）完成后调用。
    /// 切换到 [`MacroPassMode::Full`] 模式，对 `macro_catalog` 中所有容器
    /// 类重新运行 `check_class`——此时方法体已包含 splice 注入的展开代码，
    /// typeck 校验其类型合法性、调用白名单等。
    ///
    /// Pass 2 已完成属性解析与签名校验；Pass 4 跳过属性解析（避免重复注册）
    /// 但重新检查方法体并 push `typed_fns`（Pass 2 中宏容器类 `emit_fns=false`
    /// 未 push 任何 typed_fn）。
    ///
    /// 展开代码错误（如类型不匹配）的 span 指向委托位置（RFC 009 D10.4），
    /// 由 Pass 3 的 `parse_expansion` span 映射保证。
    ///
    /// 返回 `Err(Vec<TypeError>)` 当且仅当 Pass 4 发现类型错误。Pass 2 的
    /// 错误不在此方法报告（Pass 2 错误在 `check_module` 返回值中）。
    pub fn check_macro_containers_pass4(&mut self) -> Result<(), Vec<TypeError>> {
        self.macro_pass_mode = MacroPassMode::Full;

        // 快照容器类名列表（避免借用冲突）
        let container_names: Vec<Ident> = self.macro_catalog.containers.keys().cloned().collect();

        for container_name in container_names {
            // 从 class_defs 获取 splice 后的 ClassDef（Pass 3 可能已 mutate 方法体）
            let Some(class) = self.class_defs.get(&container_name).cloned() else {
                // 容器类未在 class_defs 中（不应发生）——跳过
                continue;
            };
            // 静态类走 check_static_class 路径（当前不支持静态宏容器，
            // 留待后续扩展；此处保守调用 check_class）
            let result = if class.is_static {
                self.check_static_class(&class)
            } else {
                self.check_class(&class)
            };
            if let Err(e) = result {
                self.errors.push(e);
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.take_errors_deduped())
        }
    }

    /// RFC 009 M4-7: 获取 `class_defs` 的可变引用，供 Pass 3 splice
    /// 展开代码到宏容器类方法体。
    ///
    /// 外部管线（M4-9）在 Pass 2 完成后、Pass 4 调用前，通过此方法
    /// 获取容器类 ClassDef 的可变访问，将 `parse_expansion` 产出的
    /// AST 语句列表追加到对应 slot 方法体末尾。
    pub fn class_defs_mut(&mut self) -> &mut IndexMap<Ident, ast::ClassDef> {
        &mut self.class_defs
    }

    /// RFC 009 M4-9: 获取 `class_defs` 的只读引用，供外部消费者
    /// （如测试与 codegen）查询 splice 后的容器类 AST。
    pub fn class_defs(&self) -> &IndexMap<Ident, ast::ClassDef> {
        &self.class_defs
    }

    /// RFC 009 M4-9 D12.2 Pass 3: 宏展开 Pass。
    ///
    /// 在 Pass 2（`check_module`）完成后、Pass 4（`check_macro_containers_pass4`）
    /// 之前调用。遍历 `macro_catalog` 中所有宏特性派生类的每个
    /// `this.<slot>(Func<string>)` 委托注册，依次执行：
    ///
    /// 1. 调用受限求值器（M4-4/M4-5）求值委托 → 展开字符串
    /// 2. 调用 `parse_expansion`（M4-6）将字符串解析为 AST 语句片段
    ///    （span 重写为委托位置，遵循 D10.4 诊断锚点）
    /// 3. 将 AST 片段 splice 到关联容器类的对应 slot 方法体末尾
    ///
    /// 求值失败（白名单越界 / 禁用构造）或解析失败（语法错误）时，
    /// 生成 `arc-macro-002` / `arc-macro-003` 错误并加入错误流，
    /// 跳过该委托后续处理（不 splice），继续处理其他注册。
    ///
    /// **错误码**（RFC 009 D12.3）：
    /// - `arc-macro-002`：委托求值失败（白名单越界 / 禁用构造 / 类型不匹配）
    /// - `arc-macro-003`：展开字符串解析失败（语法错误）
    ///
    /// 调用方完成 Pass 3 后应调用 `check_macro_containers_pass4` 触发 Pass 4
    /// 完整 typeck，校验 splice 后的宏容器类方法体类型合法性。
    pub fn expand_macros(&mut self) -> Result<(), Vec<TypeError>> {
        // 快照所有待处理委托：(container, slot_name, lambda, delegate_span, expression_locals)
        // 先克隆出工作列表，避免后续 `self.class_defs.get_mut` 与
        // `self.macro_catalog` 的不可变借用冲突。
        let work_items: Vec<(
            Ident,
            Ident,
            ast::LambdaExpr,
            Span,
            Vec<(Ident, ast::ExpressionTree)>,
        )> = self
            .macro_catalog
            .features
            .values()
            .flat_map(|feature| {
                feature
                    .registrations
                    .iter()
                    .map(|r| {
                        (
                            feature.container.clone(),
                            r.slot_name.clone(),
                            r.expansion.clone(),
                            r.span,
                            r.expression_locals.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let whitelist = crate::macro_eval::whitelist::Whitelist::new();

        for (container_name, slot_name, lambda, delegate_span, expression_locals) in work_items {
            // 1. 求值委托 → 展开字符串
            let mut evaluator = crate::macro_eval::evaluator::Evaluator::new(&whitelist);
            // RFC 009 M4-7: 注入 Expression 形参到求值器环境。
            // 委托体内可引用形参名访问 Expression 对象的属性与子节点。
            if !expression_locals.is_empty() {
                evaluator.inject_expression_locals(&expression_locals);
            }
            let expansion = match evaluator.eval_lambda(&lambda) {
                Ok(s) => s,
                Err(e) => {
                    self.errors.push(TypeError::Macro {
                        code: "arc-macro-002",
                        message: format!(
                            "Func<string> 委托求值失败 (委托位置 {:?}): {:?}",
                            delegate_span, e
                        ),
                    });
                    continue;
                }
            };

            // 2. 解析展开字符串 → AST 语句（span 重写为委托位置）
            let stmts =
                match crate::macro_eval::splice::parse_expansion(&expansion, delegate_span, 0) {
                    Ok(s) => s,
                    Err(e) => {
                        self.errors.push(e.to_type_error());
                        continue;
                    }
                };

            // 3. splice 到 container 的 slot 方法体末尾
            //    匹配首个同名 slot（同名重载取首个），追加语句后 break。
            if let Some(class_def) = self.class_defs.get_mut(&container_name) {
                for method in &mut class_def.methods {
                    if method.node.sig.name == slot_name {
                        if let Some(body) = &mut method.node.body {
                            body.stmts.extend(stmts);
                            break;
                        }
                    }
                }
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.take_errors_deduped())
        }
    }

    /// Phase 2 序列化体系：从 TypeRegistry 构建 TypeTable 快照。
    fn build_type_table(
        registry: &crate::TypeRegistry,
        class_def_ids: &indexmap::IndexMap<ast::Ident, hir::DefId>,
    ) -> indexmap::IndexMap<hir::DefId, crate::macro_eval::evaluator::TypeTableEntry> {
        let mut table = indexmap::IndexMap::new();
        for (type_name, nominal) in &registry.types {
            if let Some(&def_id) = class_def_ids.get(type_name) {
                let kind = match nominal.kind {
                    crate::TypeKind::Class => "class",
                    crate::TypeKind::StaticClass => "class",
                    crate::TypeKind::Struct => "struct",
                    crate::TypeKind::Interface => "interface",
                    crate::TypeKind::Enum => "enum",
                    crate::TypeKind::Variant => "variant",
                };
                let mut field_names = Vec::new();
                let mut field_types = Vec::new();
                for (fname, finfo) in &nominal.fields {
                    field_names.push(fname.as_str().to_string());
                    field_types.push(finfo.ty.as_str().to_string());
                }
                // RFC 038：枚举成员的编译期元数据（供 EnumItemsSourceGenerator
                // 生成 EnumOptions<T>.Create()，无需运行时反射）。
                let enum_member_names: Vec<String> = if nominal.kind == crate::TypeKind::Enum {
                    nominal
                        .variants
                        .iter()
                        .map(|v| v.name.as_str().to_string())
                        .collect()
                } else {
                    Vec::new()
                };
                let base_type = nominal
                    .base_types
                    .first()
                    .map(|t| match t {
                        ast::Type::Named { path, .. } => path
                            .last()
                            .map(|i| i.as_str().to_string())
                            .unwrap_or_default(),
                        _ => format!("{:?}", t),
                    })
                    .unwrap_or_default();
                table.insert(
                    def_id,
                    crate::macro_eval::evaluator::TypeTableEntry {
                        type_name: type_name.as_str().to_string(),
                        kind: kind.to_string(),
                        field_names,
                        field_types,
                        enum_member_names,
                        base_type,
                    },
                );
            }
        }
        table
    }

    /// RFC 009 M5-3 D13.5 Pass 3 M5 分支：执行 Source Generator
    /// `Generate(GeneratorContext)` 方法，将返回的字符串列表解析为
    /// 新的 `Program` AST 追加到当前编译单元。
    ///
    /// # 流程
    ///
    /// 1. 遍历 `macro_catalog.source_generators` 中每个 Source Generator
    /// 2. 取出 `generate_method_body`（`Option<Block>`）；None 则报
    ///    `arc-macro-020` 错误并跳过
    /// 3. 用受限求值器（M5-3 扩展，支持 `List<string>` 构造与 `Add` 方法）
    ///    求值方法体得到 `Vec<String>`——每个字符串是一个完整的 Arc 源文件
    /// 4. 对每个字符串调用 `Parser::parse_program` 解析为 `Program`
    /// 5. 把解析成功的 `Program` 追加到 `self.generated_programs`，
    ///    供 Pass 4 完整 typeck 与 codegen 层消费
    ///
    /// # 错误码（RFC 009 D12.3 + M5-3 扩展）
    ///
    /// - `arc-macro-002`：求值失败（白名单越界 / 禁用构造 / 类型不匹配）
    /// - `arc-macro-003`：生成字符串解析失败（语法错误）
    /// - `arc-macro-020`：Source Generator 缺失 `Generate` 方法
    ///
    /// **错误隔离**：单个生成器的失败不阻塞其他生成器；单个字符串解析
    /// 失败不阻塞同列表中其他字符串。
    ///
    /// # Pass 4 协同
    ///
    /// 调用方完成本方法后，应对 `generated_programs` 中每个 `Program`
    /// 调用 [`check_module`](Self::check_module) 触发完整 typeck
    /// （D13.5：M5 生成的新源文件与原代码同等待遇）。
    pub fn expand_source_generators(&mut self) -> Result<(), Vec<TypeError>> {
        // 快照工作列表：避免在迭代中跨借用 self.macro_catalog / self.errors
        let work_items: Vec<(Ident, Option<ast::Block>, Option<Ident>, Span)> = self
            .macro_catalog
            .source_generators
            .values()
            .map(|sg| {
                (
                    sg.class_name.clone(),
                    sg.generate_method_body.clone(),
                    sg.context_param_name.clone(),
                    sg.span,
                )
            })
            .collect();

        let whitelist = crate::macro_eval::whitelist::Whitelist::new();

        // RFC 012 M5-2b: 构造 GeneratorContext 值。
        //
        // `attributes` 共享 typeck 产物 `self.attribute_table`（Rc 共享，不可变）。
        // `symbols` 是 `DefId → (类型名, 成员名)` 映射，合并两个数据源：
        //   1. `class_def_ids`：类自身的 DefId，成员名为空串
        //      （`GetMemberName` 对类返回空串）
        //   2. `def_id_members`：方法成员的 DefId → (类名, 方法名)
        //      供 Generate 方法内 `context.Symbols.GetTypeName(defId)` 与
        //      `context.Symbols.GetMemberName(defId)` 反查。
        // `source_files` 暂为空 Vec——typeck 当前不持有源文件路径列表；
        //   后续如需按文件过滤可由外部管线注入（M5-2b 范围内非必需）。
        //
        // 注意：构造时机在迭代外，使所有 Source Generator 共享同一份 context
        //   快照（typeck 产物在 Pass 2 末尾已稳定，Pass 3 不再变化）。
        let attr_table_rc = std::rc::Rc::new(self.attribute_table.clone());
        let mut symbols: indexmap::IndexMap<hir::DefId, (String, String)> = self
            .class_def_ids
            .iter()
            .map(|(name, def_id)| (*def_id, (name.as_str().to_string(), String::new())))
            .collect();
        for (def_id, (class_name, member_name)) in &self.def_id_members {
            symbols.insert(
                *def_id,
                (
                    class_name.as_str().to_string(),
                    member_name.as_str().to_string(),
                ),
            );
        }
        let context_value = crate::macro_eval::evaluator::make_generator_context(
            attr_table_rc,
            symbols,
            Vec::new(),
            Self::build_type_table(&self.registry, &self.class_def_ids),
        );

        for (class_name, body_opt, context_param_name, span) in work_items {
            // 1. Generate 方法缺失 → arc-macro-020
            let body = match body_opt {
                Some(b) => b,
                None => {
                    self.errors.push(TypeError::Macro {
                        code: "arc-macro-020",
                        message: format!(
                            "Source Generator `{}` 缺失 Generate 方法 (类位置 {:?})",
                            class_name, span
                        ),
                    });
                    continue;
                }
            };

            // 2. 求值 Generate 方法体 → Vec<String>
            //    RFC 009 M5-2b: 若 SourceGenerator 提取到 context_param_name，
            //    把 GeneratorContext 值通过 eval_generate_method_with_context
            //    注入求值器 locals；否则退化为 M5-3 旧路径（不传 context）。
            let mut evaluator = crate::macro_eval::evaluator::Evaluator::new(&whitelist);
            let context_clone = context_value.clone();
            let source_strings = match evaluator.eval_generate_method_with_context(
                &body,
                context_param_name.clone(),
                if context_param_name.is_some() {
                    Some(context_clone)
                } else {
                    None
                },
            ) {
                Ok(v) => v,
                Err(e) => {
                    self.errors.push(TypeError::Macro {
                        code: "arc-macro-002",
                        message: format!(
                            "Source Generator `{}` Generate 方法求值失败 (类位置 {:?}): {:?}",
                            class_name, span, e
                        ),
                    });
                    continue;
                }
            };

            // 3. 每个字符串解析为 Program 并追加
            for source in source_strings {
                // 空字符串跳过（生成器允许产出空文件占位）
                if source.trim().is_empty() {
                    continue;
                }
                match parse::Parser::parse_program(&source) {
                    Ok(program) => {
                        // span 重写：所有节点 span 替换为 Generate 方法声明位置
                        // （RFC 009 D10.4 诊断锚点——生成代码错误指向 Generate 方法）
                        let rewritten =
                            crate::macro_eval::splice::rewrite_program_span(program, span);
                        self.generated_programs.push(rewritten);
                    }
                    Err(parse_err) => {
                        self.errors.push(TypeError::Macro {
                            code: "arc-macro-003",
                            message: format!(
                                "Source Generator `{}` 生成字符串解析失败 (Generate 位置 {:?}): {}",
                                class_name, span, parse_err
                            ),
                        });
                        // 错误隔离：继续处理下一个字符串
                    }
                }
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.take_errors_deduped())
        }
    }

    /// RFC 009 M5-3: 获取 Source Generator 在 Pass 3 生成的 `Program` 列表。
    ///
    /// 由 `expand_source_generators` 在 Pass 3 填充。codegen 层与 Pass 4
    /// typeck 通过此方法读取生成代码——每个 `Program` 与原代码同等待遇。
    pub fn generated_programs(&self) -> &[ast::Program] {
        &self.generated_programs
    }

    /// RFC 009 M5-4 D13.5 Pass 4 M5 分支：对 Source Generator 生成的
    /// `Program` 列表执行完整 typeck。
    ///
    /// 在 Pass 3 `expand_source_generators` 完成后调用。每个生成的
    /// `Program` 与原代码同等待遇（D13.5）：
    ///
    /// 1. 通过 `HirBuilder::lower_program` 降级为 `HirModule`
    /// 2. 通过 `TypeRegistry::register_module` 增量注册新类型到当前
    ///    registry（不替换，保证生成代码可引用原模块已注册的类型）
    /// 3. 切换到 [`MacroPassMode::Skeleton`] 模式——生成代码是首次进入
    ///    typeck，需要解析属性（resolve_attrs=true）；非宏容器类不受
    ///    emit_fns=false 影响，照常 push typed_fns
    /// 4. 调用 `check_module_items` 走完整 item typeck 流程
    ///
    /// **错误隔离**：单个 `Program` 的 lowering 失败不阻塞其他 Program。
    /// 单个 item 的 typeck 错误不阻塞同 Program 中其他 item。
    ///
    /// **span 锚点**：生成代码所有节点的 span 已在 Pass 3 由
    /// `rewrite_program_span` 重写为 Generate 方法声明位置（D10.4），
    /// Pass 4 报告的类型错误会指向 Generate 方法，便于用户定位。
    ///
    /// 返回 `Err(Vec<TypeError>)` 当且仅当 Pass 4 M5 分支发现类型错误。
    pub fn check_generated_programs_pass4(&mut self) -> Result<(), Vec<TypeError>> {
        // 取出所有权避免借用冲突；最后写回（即使中途出错也保留生成内容供查询）
        let programs = std::mem::take(&mut self.generated_programs);

        for program in &programs {
            // 1. Lower Program → HirModule
            let mut hir = HirBuilder::new();
            let module = match hir.lower_program(program) {
                Ok(m) => m,
                Err(e) => {
                    self.errors.push(TypeError::Generic(format!(
                        "Source Generator 生成代码 HIR 降级失败: {:?}",
                        e
                    )));
                    continue;
                }
            };

            // 2. 增量注册新类型到现有 registry（合并，不替换）
            self.registry.register_module(&module, &[]);

            // 3. Skeleton 模式：首次 typeck 生成代码，需要解析属性
            //    （resolve_attrs=true）；生成代码通常非宏容器，emit_fns=true。
            let prev_mode = self.macro_pass_mode;
            self.macro_pass_mode = MacroPassMode::Skeleton;

            // 4. typeck 所有 items（class/struct/fn/interface/enum）
            self.check_module_items(&module, &[]);

            self.macro_pass_mode = prev_mode;
        }

        // 写回 generated_programs（供 codegen 层与后续查询使用）
        self.generated_programs = programs;

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.take_errors_deduped())
        }
    }

    /// RFC 009 M5-4 D13.5: 统一 Pass 3 入口——协同执行 M4 与 M5 两个分支。
    ///
    /// Pass 3 同时承载 M4（宏特性代码注入）与 M5（Source Generator 代码生成）
    /// 两个分支（D13.5），共享受限求值器与白名单（D13.6）。本方法按顺序调用：
    ///
    /// 1. [`expand_macros`](Self::expand_macros)：M4 分支，求值委托 → splice
    ///    到宏容器类方法体
    /// 2. [`expand_source_generators`](Self::expand_source_generators)：M5 分支，
    ///    求值 Generate 方法 → 解析为 `Program` 追加到 `generated_programs`
    ///
    /// 任一分支的错误不阻塞另一分支（错误隔离）；最终汇总的错误列表同时
    /// 包含两个分支的所有错误。调用方完成 Pass 3 后应调用
    /// [`run_pass4`](Self::run_pass4) 触发完整 Pass 4 typeck。
    pub fn run_pass3(&mut self) -> Result<(), Vec<TypeError>> {
        // M4 分支错误先收集，不立刻返回
        let m4_errs = match self.expand_macros() {
            Ok(()) => Vec::new(),
            Err(e) => e,
        };
        // M5 分支错误
        let m5_errs = match self.expand_source_generators() {
            Ok(()) => Vec::new(),
            Err(e) => e,
        };

        let mut all = m4_errs;
        all.extend(m5_errs);
        if all.is_empty() {
            Ok(())
        } else {
            Err(all)
        }
    }

    /// RFC 009 M5-4 D13.5: 统一 Pass 4 入口——协同执行 M4 与 M5 两个分支。
    ///
    /// Pass 4 同时承载 M4（注入后宏容器类完整 typeck）与 M5（生成的新源文件
    /// 完整 typeck）两个分支（D13.5）。本方法按顺序调用：
    ///
    /// 1. [`check_macro_containers_pass4`](Self::check_macro_containers_pass4)：
    ///    M4 分支，Pass 4 完整 typeck 容器类（含 splice 注入的展开代码）
    /// 2. [`check_generated_programs_pass4`](Self::check_generated_programs_pass4)：
    ///    M5 分支，Pass 4 完整 typeck 生成的新源文件（与原代码同等待遇）
    ///
    /// 任一分支的错误不阻塞另一分支；最终汇总的错误列表同时包含两个分支的
    /// 所有错误。
    pub fn run_pass4(&mut self) -> Result<(), Vec<TypeError>> {
        let m4_errs = match self.check_macro_containers_pass4() {
            Ok(()) => Vec::new(),
            Err(e) => e,
        };
        let m5_errs = match self.check_generated_programs_pass4() {
            Ok(()) => Vec::new(),
            Err(e) => e,
        };

        let mut all = m4_errs;
        all.extend(m5_errs);
        if all.is_empty() {
            Ok(())
        } else {
            Err(all)
        }
    }

    /// 全局预扫描：递归遍历整个 HirModule 树，将所有泛型类模板
    /// 和泛型接口模板预注册到 `class_templates` / `interface_templates`。
    /// 确保任何模块的 Pre-pass 0b 运行时，所有模块的模板均已就绪，
    /// 解决跨模块前向引用问题。
    fn pre_register_all_generic_templates(&mut self, module: &HirModule) {
        for item in &module.items {
            if let HirItem::Class { def_ast, .. } = item {
                if !def_ast.generics.is_empty() && !self.class_templates.contains_key(&def_ast.name)
                {
                    self.class_templates
                        .insert(def_ast.name.clone(), def_ast.clone());
                }
            }
            if let HirItem::Interface { def_ast, .. } = item {
                if !def_ast.generics.is_empty()
                    && !self.interface_templates.contains_key(&def_ast.name)
                {
                    self.interface_templates
                        .insert(def_ast.name.clone(), def_ast.clone());
                }
            }
        }
        for child in &module.children {
            self.pre_register_all_generic_templates(child);
        }
    }

    fn pre_register_delegate_aliases(&mut self, module: &HirModule) {
        for item in &module.items {
            if let HirItem::Delegate { def_ast, .. } = item {
                if !def_ast.generics.is_empty() {
                    // GAP #5 扩展：泛型委托按模板收集（不注册裸名别名），
                    // 引用点 `Name<Args>` 经 `instantiate_generic_delegate`
                    // 单态化为 `TypeId::Func`。
                    self.delegate_templates
                        .insert(def_ast.name.clone(), def_ast.clone());
                    continue;
                }
                let ret = match def_ast.ret.as_ref() {
                    Some(ty) => match self.lower_type(&ty.node) {
                        Ok(r) => r,
                        Err(e) => {
                            self.errors.push(e);
                            continue;
                        }
                    },
                    None => TypeId::Void,
                };
                let params: Vec<_> = match def_ast
                    .params
                    .iter()
                    .map(|p| self.lower_type(&p.ty.node))
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(p) => p,
                    Err(e) => {
                        self.errors.push(e);
                        continue;
                    }
                };
                let type_id = TypeId::Func {
                    params,
                    ret: Box::new(ret),
                };
                self.registry
                    .delegate_aliases
                    .insert(def_ast.name.as_str().to_string(), type_id.clone());
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(def_ast.name.clone(), type_id);
            }
        }
        for child in &module.children {
            self.pre_register_delegate_aliases(child);
        }
    }

    fn pre_instantiate_field_generic_types(&mut self, module: &HirModule) {
        for item in &module.items {
            if let HirItem::Class { def_ast, .. } = item {
                if !def_ast.generics.is_empty() {
                    continue;
                }
                for f in &def_ast.fields {
                    let _ = self.lower_type(&f.ty.node);
                }
                for p in &def_ast.properties {
                    if p.is_indexer() {
                        continue;
                    }
                    let _ = self.lower_type(&p.ty.node);
                }
            }
        }
        for child in &module.children {
            self.pre_instantiate_field_generic_types(child);
        }
    }

    fn check_module_items(&mut self, module: &HirModule, namespace: &[Ident]) {
        let mut path = namespace.to_vec();
        if let Some(name) = &module.name {
            path.push(name.clone());
        }

        // 追加本 namespace 模块的 `using` 导入到扩展方法可见命名空间集。
        //
        // 对标 C#：命名空间块内的 `using`（如 `namespace Foo; using Arc.DI;`）
        // 对该块内所有扩展方法调用生效。`check_module` 入口仅解析顶层模块的
        // imports（`self.extension_imports`），此处沿模块树向下累积各嵌套
        // namespace 的 imports，使 `using` 位于 namespace 块内的消费代码也能
        // 解析跨包扩展方法（否则报 `OOP: unknown method ...`）。
        for resolved in self.registry.resolve_extension_imports(&module.imports) {
            if !self.extension_imports.contains(&resolved) {
                self.extension_imports.push(resolved);
            }
        }

        // RFC 016 M3 §3.4 能力 gating Phase 1+：推入当前 namespace 层的 capabilities。
        // 栈中所有层级的并集构成当前有效能力集；离开时弹出。
        self.namespace_caps_stack.push(module.capabilities.clone());

        // RFC 037 M1: Pre-pass 0 — 预注册所有泛型类模板，解决前向引用问题。
        //
        // 同一 namespace 内可能多个类相互引用泛型类（如 `Element` 类方法签名
        // 引用 `DependencyProperty<T>`，但 `Element.as` 在文件加载顺序上可能
        // 早于 `DependencyProperty.as`）。若不预注册，`check_class(Element)`
        // 处理方法签名时 `lower_type(DependencyProperty<T>)` 会因
        // `class_templates` 为空而 fallback 到 `Named("DependencyProperty")`
        // （丢失泛型实参），后续 `prop.Id` 等字段访问因 `registry.types`
        // 中无 `DependencyProperty_double`（RFC 009 M4-1：泛型类模板不注册
        // 到 registry.types）而失败。
        //
        // 预注册仅填充 `class_templates`（模板元数据），不做类型检查——
        // 类体检查仍由后续 `check_class` 完成。重复 insert 是幂等的（同 key
        // 覆盖同值），但仅在尚未注册时插入以保留先注册者的优先级（防止
        // 派生类覆盖基类模板）。
        for item in &module.items {
            if let HirItem::Class { def_ast, .. } = item {
                if !def_ast.generics.is_empty() && !self.class_templates.contains_key(&def_ast.name)
                {
                    self.class_templates
                        .insert(def_ast.name.clone(), def_ast.clone());
                }
            }
        }

        // Pre-pass 0b — 预实例化所有类的字段/属性泛型类型，解决跨文件前向引用。
        //
        // `from_module` 仅用 `type_path_name` 做 AST 级 mangle（如 `List<EntityEntry>`
        // → `List_EntityEntry` 字符串），不调用 `instantiate_generic_class`，故
        // `registry.types` 中无 `List_EntityEntry` 条目。当类 A（文件序在前）
        // 方法体访问 `List<B>.Count`，而 B 在文件序在后时，A 先于 B 被
        // `check_class` 处理，此时 `List_B` 未实例化，字段/方法查找失败
        // （`no field or property 'Count' on 'List_B'`）。
        //
        // 此预遍历对所有非泛型类的字段与属性调用 `lower_type`，触发泛型实例化
        // 与 `register_monomorphized_class`，确保后续 `check_class` 的方法体
        // 检查能命中已注册的 mangled 类型。错误延迟到 `check_class` 报告，
        // 此处仅吞掉以避免重复诊断。
        for item in &module.items {
            if let HirItem::Class { def_ast, .. } = item {
                if !def_ast.generics.is_empty() {
                    continue;
                }
                for f in &def_ast.fields {
                    let _ = self.lower_type(&f.ty.node);
                }
                for p in &def_ast.properties {
                    if p.is_indexer() {
                        continue;
                    }
                    let _ = self.lower_type(&p.ty.node);
                }
            }
        }

        // First pass: register function signatures for forward references
        for item in &module.items {
            if let HirItem::Fn { span, def_ast, .. } = item {
                if !def_ast.generics.is_empty() {
                    continue;
                }
                // RFC 025 M2：与下方 item 主循环一致，签名 lower 亦须按声明文件
                // 切换包上下文——否则 file 级 fn 形参里的 internal 类型（同包
                // variant/类）会被按当前默认包判定「不可访问」而误报（实测
                // `ContentLikeConsume(ContentLike c)` 报 `type ContentLike is not
                // accessible from this context`）。
                self.enter_package_for_span(*span);
                let ret = match self.fn_return_type(def_ast.ret.as_ref(), def_ast.is_async) {
                    Ok(r) => r,
                    Err(e) => {
                        self.errors.push(e);
                        continue;
                    }
                };
                let params: Vec<_> = match def_ast
                    .params
                    .iter()
                    .map(|p| self.lower_type(&p.ty.node))
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(p) => p,
                    Err(e) => {
                        self.errors.push(e);
                        continue;
                    }
                };
                self.scopes.last_mut().unwrap().insert(
                    def_ast.name.clone(),
                    TypeId::Func {
                        params,
                        ret: Box::new(ret),
                    },
                );
            }
        }
        for child in &module.children {
            self.check_module_items(child, &path);
        }
        // RFC 038 pre-pass：先收集本模块**所有**枚举的成员属性，再检查方法体。
        // 保证任意声明顺序下（含枚举声明于消费方之后），`Enum.GetOptions<E>()`
        // 的泛型特化在方法体 typeck 时都能读到枚举成员属性。
        for item in &module.items {
            if let HirItem::Enum { def_ast, .. } = item {
                self.collect_enum_attributes(def_ast);
            }
        }
        for item in &module.items {
            let prev_ns = self.enclosing_namespace.clone();
            self.enclosing_namespace = path.clone();
            // RFC 025 M2：按 item 声明文件切换包上下文，供跨包 internal 判定。
            let item_span = match item {
                HirItem::Fn { span, .. }
                | HirItem::Class { span, .. }
                | HirItem::Struct { span, .. }
                | HirItem::Interface { span, .. }
                | HirItem::Enum { span, .. }
                | HirItem::Variant { span, .. }
                | HirItem::Delegate { span, .. } => *span,
            };
            self.enter_package_for_span(item_span);
            match item {
                HirItem::Fn { def, def_ast, .. } => {
                    // 记录泛型函数模板声明侧上下文（span + 命名空间），供
                    // `instantiate_generic_fn` 单态化时恢复声明包（见字段注释）。
                    self.fn_template_origins
                        .insert(def_ast.name.clone(), (item_span, path.clone()));
                    if let Err(e) = self.check_fn(*def, def_ast) {
                        self.errors.push(e);
                    }
                }
                HirItem::Class { def_ast, .. } => {
                    // RFC 009 M4-3: 缓存所有 class 的 ClassDef 供 macro_eval
                    // 扫描派生宏特性类构造函数体（即便 typeck 失败也可访问）。
                    self.class_defs
                        .insert(def_ast.name.clone(), def_ast.clone());
                    let result = if def_ast.is_static {
                        self.check_static_class(def_ast)
                    } else {
                        self.check_class(def_ast)
                    };
                    if let Err(e) = result {
                        self.errors.push(e);
                    }
                }
                HirItem::Struct { def_ast, .. } => {
                    // RFC 012 M1: struct 在 registry 注册阶段不处理属性，
                    // 在此显式收集 struct 自身与各 field 的属性。
                    self.collect_struct_attributes(def_ast);
                    // 类型检查 struct 方法/构造函数/属性（M1 新增）
                    if let Err(e) = self.check_struct(def_ast) {
                        self.errors.push(e);
                    }
                }
                HirItem::Interface { def_ast, .. } => {
                    if let Err(e) =
                        self.validate_where_clause(&def_ast.generics, &def_ast.where_clause)
                    {
                        self.errors.push(e);
                    }
                    if let Err(e) = self.validate_interface_variance(def_ast) {
                        self.errors.push(e);
                    }
                    if !def_ast.generics.is_empty() {
                        self.interface_templates
                            .insert(def_ast.name.clone(), def_ast.clone());
                    }
                    // RFC 012 M1: 收集 interface 自身、properties 与 methods 属性。
                    self.collect_interface_attributes(def_ast);
                }
                HirItem::Delegate { def_ast, .. } => {
                    // 声明期 where 子句校验（对齐 interface 分支）：约束的
                    // param 须在泛型参数表中声明——非泛型委托携带 where 即报
                    // UndefinedTypeParameter（C# CS0081 语义）。
                    if let Err(e) =
                        self.validate_where_clause(&def_ast.generics, &def_ast.where_clause)
                    {
                        self.errors.push(e);
                    }
                }
                _ => {}
            }
            self.enclosing_namespace = prev_ns;
        }

        // RFC 016 M3 §3.4 能力 gating Phase 1+：离开当前 namespace 层，弹出 capabilities。
        self.namespace_caps_stack.pop();
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+：当前 namespace 有效能力集。
    /// 取 `namespace_caps_stack` 中所有层级的并集——子 namespace 继承父 namespace
    /// 的 capabilities（声明更多能力不破坏安全性，最终判定在调用点完成）。
    pub(crate) fn current_namespace_caps(&self) -> Vec<Ident> {
        let mut out: Vec<Ident> = Vec::new();
        for layer in &self.namespace_caps_stack {
            for cap in layer {
                if !out.contains(cap) {
                    out.push(cap.clone());
                }
            }
        }
        out
    }

    fn check_fn(&mut self, def_id: DefId, f: &FnDef) -> Result<(), TypeError> {
        if !f.generics.is_empty() {
            self.fn_templates.insert(f.name.clone(), f.clone());
            self.push_type_params(&f.generics);
            // RFC 004 M1：泛型方法体进入时同步 push where_clause，供
            // `check_static_abstract_call` 查询 `T` 的接口约束。
            self.where_clause_scope.push(f.where_clause.clone());
            // 泛型模板：仅注册到 fn_templates，不发射 typed_fn（emit_fns=false）。
            // linkage 标记对 emit_fns=false 无影响，传 User 占位即可。
            let result = self.check_fn_inner(def_id, f, false, FnLinkage::User);
            self.where_clause_scope.pop();
            self.pop_type_params();
            return result;
        }
        // RFC 017 M4-link Phase B：用户源码非泛型函数 → User linkage
        // （codegen 发射为 external，单一权威定义来源）。
        self.check_fn_inner(def_id, f, true, FnLinkage::User)
    }

    pub(crate) fn check_fn_inner(
        &mut self,
        _def_id: DefId,
        f: &FnDef,
        emit_fns: bool,
        linkage: FnLinkage,
    ) -> Result<(), TypeError> {
        if f.is_async && f.params.iter().any(|p| p.is_ref || p.is_out || p.is_in) {
            return Err(TypeError::Oop(
                "ref/out/in parameters are not allowed in async methods".into(),
            ));
        }
        self.scopes.push(IndexMap::new());
        if f.generics.is_empty() {
            self.fn_defs.insert(f.name.clone(), f.clone());
        }
        self.validate_params_m2b(&f.params)?;
        let mut params = Vec::new();
        for p in &f.params {
            let ty = self.lower_type(&p.ty.node)?;
            // RFC 009 P1-F #8：`in` 参数为 `readonly ref`——`mutable: false`
            // 使赋值检查（`check_assignment_to`）拒绝写入。
            let final_ty = if p.is_in {
                TypeId::Ref {
                    inner: Box::new(ty),
                    mutable: false,
                    kind: ast::RefKind::Var,
                }
            } else if p.is_ref || p.is_out {
                TypeId::Ref {
                    inner: Box::new(ty),
                    mutable: true,
                    kind: ast::RefKind::Var,
                }
            } else {
                ty
            };
            self.scopes
                .last_mut()
                .unwrap()
                .insert(p.name.clone(), final_ty.clone());
            params.push((p.name.clone(), final_ty));
        }
        let ret = self.fn_return_type(f.ret.as_ref(), f.is_async)?;

        if f.is_async && !ret.is_task() {
            return Err(TypeError::AsyncReturn(ret.display()));
        }

        let body_expected = self.body_return_slot(&ret, f.is_async);
        let prev_async = self.in_async;
        self.in_async = f.is_async;
        self.return_slot.push(body_expected.clone());

        let out_params: IndexSet<Ident> = f
            .params
            .iter()
            .filter(|p| p.is_out)
            .map(|p| p.name.clone())
            .collect();
        let prev_flow = self.out_flow.take();
        self.out_flow = if out_params.is_empty() {
            None
        } else {
            Some(OutParamState::new(out_params))
        };
        // null narrowing 状态同样须随函数边界重置（对齐 out_flow 的 save/restore）：
        // 否则前一函数 `if (x is T n)` 的窄化结论泄漏进后续函数，
        // `resolve_value_name` 会给同名局部返回错误类型，`is` 静态关系被误折叠。
        let prev_null_flow = self.null_flow.replace(NullFlowState::new());

        let typed_body = if let Some(body) = &f.body {
            Some(self.check_block(body, &body_expected)?)
        } else {
            None
        };

        if let Some(flow) = &self.out_flow {
            let missing = flow.unassigned();
            if !missing.is_empty() {
                self.out_flow = prev_flow;
                self.null_flow = prev_null_flow;
                self.return_slot.pop();
                self.in_async = prev_async;
                self.scopes.pop();
                return Err(TypeError::Oop(format!(
                    "out parameter `{}` must be assigned before control leaves the current method",
                    missing[0]
                )));
            }
        }
        self.out_flow = prev_flow;
        self.null_flow = prev_null_flow;

        self.return_slot.pop();
        self.in_async = prev_async;
        self.scopes.pop();
        if emit_fns {
            self.push_typed_fn(
                f.name.clone(),
                None,
                false,
                params,
                ret,
                f.body.clone(),
                typed_body,
                f.is_async,
                linkage,
                false,
                // RFC 009 M3：检测 `[Parallelize]` 属性，标记向量化候选。
                Self::has_parallelize_attr(&f.attributes),
            );
        }
        Ok(())
    }

    /// Declared return type of a function (defaults: sync -> void, async -> Task<void>).
    pub(crate) fn fn_return_type(
        &mut self,
        ret: Option<&Spanned<Type>>,
        is_async: bool,
    ) -> Result<TypeId, TypeError> {
        match ret {
            Some(t) => self.lower_type(&t.node),
            None if is_async => Ok(TypeId::Task {
                inner: Box::new(TypeId::Void),
            }),
            None => Ok(TypeId::Void),
        }
    }

    /// Type checked against `return` expressions inside the body.
    pub(crate) fn body_return_slot(&self, declared: &TypeId, is_async: bool) -> TypeId {
        if is_async {
            declared.task_inner().cloned().unwrap_or(TypeId::Void)
        } else {
            declared.clone()
        }
    }

    pub(crate) fn check_method_return(
        &mut self,
        ret: Option<&Spanned<Type>>,
        is_async: bool,
    ) -> Result<TypeId, TypeError> {
        self.fn_return_type(ret, is_async)
    }

    pub(crate) fn inherited_field_types(&self, class: &Ident) -> IndexMap<Ident, TypeId> {
        let mut out = IndexMap::new();
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current = Some(class.clone());
        while let Some(cn) = current {
            // RFC 009 M4-1: 循环检测防御。
            // 同名 arity 重载场景下若 registry 误存自引用 bases，避免无限递归。
            if !visited.insert(cn.clone()) {
                break;
            }
            let Some(nom) = self.registry.types.get(&cn) else {
                break;
            };
            chain.push(cn.clone());
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        chain.reverse();
        for cn in chain {
            let Some(nom) = self.registry.types.get(&cn) else {
                continue;
            };
            for (fname, finfo) in &nom.fields {
                if !out.contains_key(fname) {
                    out.insert(fname.clone(), TypeId::Named(finfo.ty.clone()));
                }
            }
        }
        out
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
