use ast::ExpressionTree;
use ast::*;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use typeck::{
    mangle_generic, type_id_to_field_name, AccessContext, ConstValue, ExtensionScope,
    ProgramLayouts, TypeRegistry, VirtualSlot,
};
use typeck::{FnLinkage, SpillSet, TypeId, TypedBlock, TypedFn, TypedStmt};

use crate::types::*;

/// RFC 017 M4-link Phase B：`typeck::FnLinkage` → `mir::Linkage` 转换。
///
/// - `User` → `External`：用户源码定义的函数（单一定义来源）
/// - `Monomorphized` → `LinkonceOdr`：泛型单态化实例（跨 `.o` 弱符号去重）
fn mir_linkage_from_fn(linkage: FnLinkage) -> Linkage {
    match linkage {
        FnLinkage::User => Linkage::External,
        FnLinkage::Monomorphized => Linkage::LinkonceOdr,
    }
}

/// RFC 对齐 C# 语义：`for (init; cond; inc) BODY` 被脱糖为
/// `while (cond) { BODY; inc; }`。`continue` 在 CFG 中跳回循环头（header），
/// 会跳过原循环体末尾的 `inc`，导致 `i` 永不递增而陷入死循环。
///
/// 此函数在 for-body 中每个「属于本 for 循环」的 `continue` 之前就地注入
/// `inc` 语句，使 `continue` 先执行 increment 再跳回循环头，语义与 C# 一致。
/// 嵌套循环（`While`/`LinqForeach`）内的 `continue` 属于内层循环，不注入——
/// 通过 `in_nested_loop` 标记排除。
fn inject_for_increment(
    stmts: Vec<MirStatement>,
    inc: &[MirStatement],
    in_nested_loop: bool,
) -> Vec<MirStatement> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            MirStatement::Continue if !in_nested_loop => {
                out.extend(inc.iter().cloned());
                out.push(MirStatement::Continue);
            }
            MirStatement::While {
                cond,
                body,
                foreach_source,
            } => {
                out.push(MirStatement::While {
                    cond,
                    body: inject_for_increment(body, inc, true),
                    foreach_source,
                });
            }
            MirStatement::If {
                cond,
                then_body,
                else_body,
            } => {
                out.push(MirStatement::If {
                    cond,
                    then_body: inject_for_increment(then_body, inc, in_nested_loop),
                    else_body: inject_for_increment(else_body, inc, in_nested_loop),
                });
            }
            MirStatement::TryCatch {
                try_body,
                catch_var,
                catch_ty,
                catch_body,
            } => {
                out.push(MirStatement::TryCatch {
                    try_body: inject_for_increment(try_body, inc, in_nested_loop),
                    catch_var,
                    catch_ty,
                    catch_body: inject_for_increment(catch_body, inc, in_nested_loop),
                });
            }
            MirStatement::TryFinally { body, finally } => {
                out.push(MirStatement::TryFinally {
                    body: inject_for_increment(body, inc, in_nested_loop),
                    finally: inject_for_increment(finally, inc, in_nested_loop),
                });
            }
            MirStatement::LinqForeach { var, chain, body } => {
                out.push(MirStatement::LinqForeach {
                    var,
                    chain,
                    body: inject_for_increment(body, inc, true),
                });
            }
            other => out.push(other),
        }
    }
    out
}

mod lower_call;
mod lower_expr;
mod lower_linq;
mod lower_match;
mod lower_span;
mod lower_type;

pub(super) struct LowerCtx<'a> {
    pub(super) scopes: Vec<IndexMap<Ident, LocalId>>,
    /// Loop-carried local bindings. Each active loop body contributes one
    /// scope; names bound while a loop scope is active are treated as
    /// per-iteration variables. A lambda created inside the loop captures
    /// them `ByValue` (snapshot at closure creation) so a spawned thread
    /// sees its own iteration's value instead of the shared slot (C# 循环
    /// 变量捕获语义；RFC 002 surface 对齐）。
    pub(super) loop_scopes: Vec<IndexMap<Ident, LocalId>>,
    pub(super) locals: &'a mut IndexMap<LocalId, (Ident, TypeId)>,
    pub(super) array_lengths: IndexMap<LocalId, usize>,
    pub(super) owner: Option<Ident>,
    pub(super) class_fields: &'a [Ident],
    pub(super) fn_sigs: &'a HashMap<String, (Vec<TypeId>, TypeId)>,
    pub(super) registry: &'a TypeRegistry,
    pub(super) layouts: &'a ProgramLayouts,
    /// RFC 017 M4-link Phase B：宿主函数的 linkage，用于让 lifted lambda
    /// 跟随宿主来源（单态化方法体内的 lambda 也应是 linkonce_odr）。
    pub(super) host_linkage: Linkage,
    /// RFC 009 M3：类型大小表（async spill 判定用），构建一次传给所有 lowering。
    pub(super) type_sizes: &'a typeck::TypeSizeTable,
    /// 当前函数/委托的声明返回类型。Return 降低时据此把 class 值包裹为
    /// 接口胖指针（`MirRvalue::MakeIface`）——接口类型返回缺失物化是既有缺口
    /// （covariance_e2e 明示"方法返回接口的 MakeIface 为既有缺口"）。
    pub(super) fn_ret: TypeId,
    /// P0 双引擎收敛：typeck 下达的 span 键表达式类型表。
    /// `infer_type_from_spanned` 查表命中即采用 typeck 结论（消除
    /// `infer_type_from_expr` 与 typeck 的双引擎漂移）；未命中 / Ambiguous
    /// 回落旧推断。表由管线 `take_expr_type_table()` 导出后 move 进 lower_module。
    pub(super) expr_types: &'a typeck::ExprTypeTable,
}

impl LowerCtx<'_> {
    pub(super) fn lookup(&self, name: &Ident) -> Option<LocalId> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(IndexMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn bind(&mut self, name: &Ident, id: LocalId) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.clone(), id);
        }
        if let Some(loop_scope) = self.loop_scopes.last_mut() {
            loop_scope.insert(name.clone(), id);
        }
    }

    /// Enter a loop body scope: names bound from here on are loop-carried.
    pub(super) fn enter_loop_body(&mut self) {
        self.loop_scopes.push(IndexMap::new());
    }

    /// Leave the innermost loop body scope.
    pub(super) fn exit_loop_body(&mut self) {
        self.loop_scopes.pop();
    }

    /// True if `name` was declared inside an active loop body (its slot is
    /// reused across iterations, so closures must snapshot it at creation).
    pub(super) fn is_loop_captured_local(&self, name: &Ident) -> bool {
        self.loop_scopes.iter().any(|s| s.contains_key(name))
    }

    pub(super) fn is_class_field(&self, name: &Ident) -> bool {
        self.class_fields.iter().any(|f| f == name)
    }

    /// RFC 006 M3：判断 `name` 是否为 owner 类（或其基类链）的**静态字段**。
    ///
    /// 通过 `TypeRegistry::field_info` 查询 `FieldInfo.is_static`。
    /// 静态字段在 typeck M2 已从 `class_fields` 过滤掉（实例方法仅见实例字段），
    /// 但本方法独立判定以覆盖以下两种场景：
    /// - 实例方法内裸访问同类静态字段（实例 + 静态字段均可见，但访问路径不同）
    /// - 跨类 `ClassName._staticField` 访问（在 `Expr::Field` 路径使用）
    pub(super) fn is_static_field_of(&self, class: &Ident, name: &Ident) -> bool {
        self.registry
            .field_info(class, name)
            .is_some_and(|f| f.is_static || f.is_const)
    }
}

/// True if `field` is a custom-accessor property on `class`: a `get_{field}` method
/// exists and `{field}` is NOT a backing field. Auto-properties (registered as fields)
/// return false, so they keep the direct field-read path.
///
/// 仅查类自身与其**类基类链**上的 custom accessor（接口 getter 不入列）：字段实现
/// 接口只读属性（`IShape.Name { get; }` ← `public string Name;`）时，`get_Name` 只
/// 存在于接口方法表——若接口 getter 也算 custom accessor，字段写会误走 `set_Name`
/// 调用（该函数从未被合成 → LLVM undefined value）。
/// Task facade（RFC 009 M1）**实例**属性清单——单一事实源。
///
/// `lower_expr.rs`（rvalue 位置）与 `lower_call.rs`（实参位置）两处拦截共用；
/// 新增/调整 Task 实例属性（`Result`/`Status`/`IsCanceled`/`IsCompleted`/
/// `IsFaulted`/`Exception`）只改此处，禁止两处各自维护清单（历史教训：
/// 清单漂移曾致 lower_expr 缺 `IsFaulted`/`Exception`——任何绕过 expected
/// 的 rvalue 路径都会静默 FieldGet 读 RtTask 偏移）。
pub(super) fn task_facade_instance_property(field: &str) -> bool {
    matches!(
        field,
        "Result" | "Status" | "IsCanceled" | "IsCompleted" | "IsFaulted" | "Exception"
    )
}

/// CancellationTokenSource facade（RFC 009 M4）实例属性清单——单一事实源。
///
/// 与 `task_facade_instance_property` 同理：lower_expr / lower_call 共用，
/// 禁止两处各自维护清单。
pub(super) fn cts_facade_instance_property(field: &str) -> bool {
    matches!(field, "Token" | "IsCancellationRequested")
}

pub(super) fn is_custom_accessor_property(
    registry: &TypeRegistry,
    class: &str,
    field: &str,
) -> bool {
    let class_ident: Ident = class.into();
    if registry.is_interface(&class_ident) {
        return false;
    }
    let getter: Ident = format!("get_{field}").into();
    // 以「类自身或其类基类声明 getter 方法」为准：auto 属性（`{ get; }`）不注册
    // `get_X` 方法（registry 仅 custom 访问器注册 getter），走 backing 字段访问；
    // custom 属性（`{ get { … } }`）必注册 `get_X` → 走 MethodCall。
    // **不能**叠加 `!has_field` 判定——泛型模板（如 `Arc.Math.Tensor<T>`）
    // 的 auto 属性 backing 会被注册到短名键（与同名非泛型类如
    // `Arc.AI.Tensor` 的字段表合并），使 custom 属性的 has_field 误为 true →
    // 误走 FieldGet → codegen 以错误的字段偏移读取（core_tensor_e2e
    // `Rank`/`Total` 读 offset 76 而非 `_shape@16` 的根因）。
    let mut cur = class.to_string();
    loop {
        let Some(nom) = registry.types.get(cur.as_str()) else {
            return false;
        };
        if nom.methods.contains_key(&getter) {
            return true;
        }
        let Some(next) = nom.bases.iter().find(|b| registry.is_class(b)).cloned() else {
            return false;
        };
        cur = next.to_string();
    }
}

pub(super) fn is_virtual_member(
    layouts: &ProgramLayouts,
    class: &str,
    method: &str,
    params: &[String],
) -> bool {
    // CD-10/D1：签名键匹配（名+形参）；同名槽唯一时按名兜底。
    let Some(c) = layouts.classes.get(class) else {
        return false;
    };
    if c.virtual_slots.iter().any(|s| {
        s.name.as_str() == method
            && s.params.len() == params.len()
            && s.params
                .iter()
                .zip(params.iter())
                .all(|(a, b)| a.as_str() == b.as_str())
    }) {
        return true;
    }
    // CD-29 防御：签名不匹配（重载）时**不得**按名兜底——codegen 槽位解析同样
    // 按名会错位到其他重载（`WriteString(string,string)` 经槽 3 误调单参
    // `WriteString(string)`，参数错位致 JSON 值缺失）。按名兜底仅保留给调用点
    // 参数信息缺失（空参）的兼容场景（历史 MIR 简化路径）。
    if !params.is_empty() {
        return false;
    }
    let name_matches: Vec<&VirtualSlot> = c
        .virtual_slots
        .iter()
        .filter(|s| s.name.as_str() == method)
        .collect();
    name_matches.len() == 1
}

/// Resolve `(impl_class, target_fn)` for a method on a class/interface receiver.
/// Mirrors the dispatch logic in `method_call_rvalue` for the no-arg/overload case.
///
/// When the strict overload lookup fails (e.g., method has args but we look up
/// with empty `arg_types`), fall back to `resolve_method_with_declaring`, which
/// walks up the inheritance hierarchy via `collect_method_overloads` and returns
/// the **declaring class**. This ensures inherited methods (like `Window.GetValue<T>`
/// declared on `Element`) are mangled with the base class symbol
/// (`@Element_GetValue`) instead of the receiver class (`@Window_GetValue`).
pub(super) fn resolve_method_target(
    registry: &TypeRegistry,
    recv_class: &Ident,
    method: &Ident,
    current_type: Option<Ident>,
) -> (Option<String>, Option<String>) {
    let ctx = AccessContext {
        current_type,
        extension_scope: ExtensionScope {
            imported: registry.extension_namespace_paths(),
            enclosing: vec![],
        },
        enclosing_namespace: vec![],
        current_package: None,
        skip_type_visibility: false,
    };
    if let Ok((declaring, sig)) = registry.resolve_method_with_declaring(recv_class, method, &ctx) {
        let target = registry.method_link_name_for(&declaring, &sig);
        // Interface receivers already hold fat pointers (`MakeIface` /
        // `MirOperand::Iface`). Do not attach a concrete impl_class — codegen
        // must dispatch via the stored itable, not rebuild a fat pointer.
        let impl_cls = if registry.is_interface(recv_class) {
            None
        } else {
            Some(declaring.to_string())
        };
        (impl_cls, Some(target))
    } else {
        (None, None)
    }
}

/// Class / struct → interface assignment metadata for `MirRvalue::MakeIface`.
///
/// Returns `(object_ty, itable_provider_class, itable_iface_name)`.
/// `itable_provider_class` walks inheritance so `Derived : Base : IFoo`
/// still references `@.itable.Base_IFoo` (itables are emitted only for
/// directly declared interfaces).
///
/// RFC 004 P0 Phase 2：struct 实现接口 → `itable_provider = "{Struct}_Box"`，
/// codegen `emit_make_iface` 据此先装箱再包裹 fat pointer。
pub(super) fn class_to_iface_make(
    registry: &TypeRegistry,
    value_ty: &TypeId,
    iface_name: &Ident,
) -> Option<(TypeId, String, String)> {
    let TypeId::Named(type_name) = value_ty else {
        return None;
    };
    if registry.is_class(type_name) {
        let impl_cls = registry.interface_impl_class(type_name, iface_name)?;
        let itable_iface = registry.interface_itable_name(&impl_cls, iface_name);
        return Some((
            value_ty.clone(),
            impl_cls.to_string(),
            itable_iface.to_string(),
        ));
    }
    // struct 无父链、无 vtable，仅自身显式声明接口可静态装箱分派。
    if registry.is_struct(type_name) && registry.implements_interface(type_name, iface_name) {
        return Some((
            value_ty.clone(),
            format!("{type_name}_Box"),
            iface_name.to_string(),
        ));
    }
    None
}

/// RHS type for MakeIface：剥开 `Cast`，取源 class 类型。
///
/// `if (x is IFace n)` 脱糖为 `IFace n = (IFace)x`；`infer_type_from_expr(Cast)`
/// 得到接口类型，导致 `class_to_iface_make` 失败、裸对象指针写入接口 local →
/// 后续接口分派 ACCESS_VIOLATION。
pub(super) fn class_ty_for_iface_wrap(expr: &Expr, ctx: &LowerCtx<'_>) -> TypeId {
    match expr {
        Expr::Cast { expr: inner, .. } => lower_type::infer_type_from_spanned(inner, ctx),
        _ => lower_type::infer_type_from_expr(expr, ctx),
    }
}

/// 将 class / 接口值包装为目标接口 fat pointer 的 MIR。
///
/// - 静态类型直接实现接口 → `MakeIface`（固定 itable 符号；variance 目标用适配器 itable）
/// - 静态类型是 class 但未声明该接口（基类引用）→ `MakeIfaceDyn`
/// - 源已是接口且为 variance 子类型 → `AdaptIface`（重绑定 itable）
pub(super) fn iface_wrap_rvalue(
    registry: &TypeRegistry,
    src_ty: &TypeId,
    iface_name: &Ident,
    object: MirOperand,
) -> Option<MirRvalue> {
    let src_ty = normalize_iface_type_id(src_ty);
    if let Some((_, class_name, itable_iface)) = class_to_iface_make(registry, &src_ty, iface_name)
    {
        return Some(MirRvalue::MakeIface {
            class: class_name,
            iface: itable_iface,
            object,
        });
    }
    // `object` 根类型：静态类型无接口信息，须按对象 runtime type_id 动态选 itable。
    // 与基类引用（TypeId::Named 但未声明接口）同走 MakeIfaceDyn。
    if src_ty == TypeId::Object {
        return Some(MirRvalue::MakeIfaceDyn {
            iface: iface_name.to_string(),
            object,
        });
    }
    let TypeId::Named(cn) = &src_ty else {
        return None;
    };
    if registry.is_interface(cn) {
        if cn == iface_name {
            return None;
        }
        // CD-14/D3：接口→接口重绑定（含父接口→子接口 downcast，如
        // `if (ib2 is IChild cc)` 的 `cc = (IChild)ib2` 绑定）。AdaptIface
        // 按源 itable 指针匹配实现类并重绑定到目标接口视图；is-check 已在
        // then 分支前通过，对象必实现目标接口，候选对必然存在。无匹配时
        // 保持源 itable（安全回退，与旧 None→Use 行为等价）。
        return Some(MirRvalue::AdaptIface {
            from_iface: cn.to_string(),
            to_iface: iface_name.to_string(),
            object,
        });
    }
    if !registry.is_class(cn) {
        return None;
    }
    Some(MirRvalue::MakeIfaceDyn {
        iface: iface_name.to_string(),
        object,
    })
}

/// Builtin `IEnumerable<T>` / `IQueryable<T>` → 单态接口名（供 MakeIface / AdaptIface）。
fn iface_dest_name(ty: &TypeId, registry: &TypeRegistry) -> Option<Ident> {
    match ty {
        TypeId::Named(n) if registry.is_interface(n) => Some(n.clone()),
        TypeId::IEnumerable { inner } => {
            Some(mangle_generic("IEnumerable", &[inner.as_ref().clone()]).into())
        }
        TypeId::IQueryable { inner } => {
            Some(mangle_generic("IQueryable", &[inner.as_ref().clone()]).into())
        }
        // 可空接口局部（`IFoo? h = (IFoo?)...`）：剥开 Nullable 后按内层接口名
        // 走 MakeIface / MakeIfaceDyn 包裹。否则 fat pointer 槽位直接拿到裸对象
        // 指针，后续接口分派解引用垃圾 itable → ACCESS_VIOLATION
        //（MediatorIsolateTests `IRequestHandler<PingRequest, PingResponse>?` 实测）。
        TypeId::Nullable { inner } => iface_dest_name(inner, registry),
        _ => None,
    }
}

fn normalize_iface_type_id(ty: &TypeId) -> TypeId {
    match ty {
        TypeId::IEnumerable { inner } => {
            TypeId::Named(mangle_generic("IEnumerable", &[inner.as_ref().clone()]).into())
        }
        TypeId::IQueryable { inner } => {
            TypeId::Named(mangle_generic("IQueryable", &[inner.as_ref().clone()]).into())
        }
        // 可空引用（`object? o = ...; (IFace)o`）在 MIR/codegen 中即裸指针
        //（无 HasValue 判别）。剥开 Nullable 包装后递归归一，命中 Object / class
        // 分支；否则 `iface_wrap_rvalue` 返回 None → 接口 local 拿到裸对象指针，
        // 后续分派按 `{obj, itable}` 胖指针解引用 ACCESS_VIOLATION
        //（GenericInterfaceCastTests `(ICastable<int>)o` 实测）。
        TypeId::Nullable { inner } => normalize_iface_type_id(inner),
        // `object` 根类型：typeck 对方法返回 "object" 经 type_name_to_type_id
        // 归一为 `Named("object")`，而非内建 `TypeId::Object`。若不统一，
        // `iface_wrap_rvalue` 的 `src_ty == TypeId::Object` 检查失败、且
        // "object" 不是已注册 class → 返回 None，导致 `(IFace)obj.Method()`
        // 直接对方法调用结果强转接口时不生成 MakeIfaceDyn，接口 local 拿到
        // 裸对象指针，后续分派按胖指针解引用 ACCESS_VIOLATION。
        TypeId::Named(n) if n.as_str() == "object" => TypeId::Object,
        other => other.clone(),
    }
}

pub struct MirBuilder {
    next_local: u32,
    next_lambda: u32,
    lifted: Vec<(String, MirCfgBody)>,
}

impl MirBuilder {
    pub fn new() -> Self {
        Self {
            next_local: 0,
            next_lambda: 0,
            lifted: Vec::new(),
        }
    }

    pub(super) fn fresh_local(
        &mut self,
        name: &Ident,
        ty: TypeId,
        locals: &mut IndexMap<LocalId, (Ident, TypeId)>,
    ) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        locals.insert(id, (name.clone(), ty));
        id
    }

    pub fn lower_fn(
        &mut self,
        f: &TypedFn,
        fn_sigs: &HashMap<String, (Vec<TypeId>, TypeId)>,
        registry: &TypeRegistry,
        layouts: &ProgramLayouts,
        type_sizes: &typeck::TypeSizeTable,
        expr_types: &typeck::ExprTypeTable,
    ) -> MirCfgBody {
        self.next_local = 0;
        let mut locals = IndexMap::new();
        let mut scopes = vec![IndexMap::new()];

        for (name, ty) in &f.params {
            let id = self.fresh_local(name, ty.clone(), &mut locals);
            scopes.last_mut().unwrap().insert(name.clone(), id);
        }
        let param_count = f.params.len();

        let mut ctx = LowerCtx {
            scopes,
            loop_scopes: Vec::new(),
            locals: &mut locals,
            array_lengths: IndexMap::new(),
            owner: f.owner.clone(),
            class_fields: &f.class_fields,
            fn_sigs,
            registry,
            layouts,
            host_linkage: mir_linkage_from_fn(f.linkage),
            type_sizes,
            fn_ret: f.ret.clone(),
            expr_types,
        };

        let mut blocks = vec![MirBasicBlock { statements: vec![] }];
        if let Some(typed_body) = &f.typed_body {
            blocks[0].statements = self.lower_typed_block(typed_body, &mut ctx);
            for (id, (_, ty)) in locals.iter() {
                if (id.0 as usize) >= param_count && lower_type::is_class_type(ty, registry) {
                    blocks[0].statements.push(MirStatement::Drop(*id));
                }
            }
        } else if let Some(body) = &f.body {
            if std::env::var("ARC_DEBUG_RAW_BODY").is_ok() {
                eprintln!("[DIAG] lowering RAW body for fn `{}`", f.name);
            }
            blocks[0].statements = self.lower_block(body, &mut ctx);
            for (id, (_, ty)) in locals.iter() {
                if (id.0 as usize) >= param_count && lower_type::is_class_type(ty, registry) {
                    blocks[0].statements.push(MirStatement::Drop(*id));
                }
            }
        }

        // RFC 009 M3：async 函数对非 param locals 计算按需 spill 候选
        //（>SPILL_THRESHOLD 的大值类型跨 await 存活 → 堆槽指针）。
        // param 由 ctor 直接拷贝进 env 字段，不参与 spill。
        let spill_set = if f.is_async {
            let local_indices: Vec<(usize, TypeId)> = locals
                .iter()
                .filter(|(id, _)| (id.0 as usize) >= param_count)
                .map(|(id, (_, ty))| (id.0 as usize, ty.clone()))
                .collect();
            typeck::analyze_spill_candidates(true, &local_indices, type_sizes)
        } else {
            typeck::SpillSet::empty()
        };

        let body = MirBody {
            params: f.params.clone(),
            ret: f.ret.clone(),
            param_count,
            locals,
            blocks,
            is_async: f.is_async,
            owner: f.owner.clone(),
            class_fields: f.class_fields.clone(),
            is_ctor: f.is_ctor,
            // RFC 006 M2：透传 is_static 供 M3 区分静态/实例字段访问。
            is_static: f.is_static,
            captures: vec![],
            // RFC 024 M4-link Phase B：按 TypedFn.linkage 标注来源
            // （User → external / Monomorphized → linkonce_odr）。
            linkage: mir_linkage_from_fn(f.linkage),
            // RFC 039 M3：透传 `[Parallelize]` 标记，codegen 据此在 while 循环
            // backedge 附加 `!llvm.loop.vectorize.enable` metadata。
            parallelize: f.parallelize,
            spill_set,
        };
        body.to_cfg()
    }

    fn lower_lambda_to_body(
        &mut self,
        l: &LambdaExpr,
        params: &[(Ident, TypeId)],
        ret: &TypeId,
        fn_sigs: &HashMap<String, (Vec<TypeId>, TypeId)>,
        registry: &TypeRegistry,
        layouts: &ProgramLayouts,
        captures: &[LambdaCapture],
        refs_owner_static: bool,
        owner: Option<Ident>,
        class_fields: &[Ident],
        host_linkage: Linkage,
        type_sizes: &typeck::TypeSizeTable,
        expr_types: &typeck::ExprTypeTable,
    ) -> MirCfgBody {
        let saved = self.next_local;
        self.next_local = 0;
        let mut locals = IndexMap::new();
        let mut scopes = vec![IndexMap::new()];

        // RFC 023: when the lambda has captures, prepend a `__env__` parameter
        // (opaque pointer to the capture environment struct). Each captured
        // variable is bound to a local that codegen initializes via GEP+load
        // from `%__env__` in the function entry block.
        //
        // Captures are computed in `compute_captures` (MIR lowering) rather
        // than filled by typeck, because typeck receives `&Expr` (immutable)
        // and cannot mutate `LambdaExpr.captures`.
        let has_captures = !captures.is_empty();
        let mut all_params: Vec<(Ident, TypeId)> = Vec::new();
        let mut captures_info: Vec<(LocalId, usize, LambdaCapture)> = Vec::new();

        if has_captures {
            all_params.push(("__env__".into(), TypeId::Named("__env_ptr".into())));
        }
        all_params.extend(params.iter().cloned());

        for (name, ty) in &all_params {
            let id = self.fresh_local(name, ty.clone(), &mut locals);
            scopes.last_mut().unwrap().insert(name.clone(), id);
        }

        // Bind captured variable names to locals. Codegen emits the env-field
        // loads; MIR just registers the binding so body lowering resolves them.
        if has_captures {
            for (i, capture) in captures.iter().enumerate() {
                let id = self.fresh_local(&capture.name, capture.ty.clone(), &mut locals);
                scopes.last_mut().unwrap().insert(capture.name.clone(), id);
                captures_info.push((id, i, capture.clone()));
            }
        }

        let param_count = all_params.len();

        // RFC 023 L3: propagate the enclosing class `owner` and `class_fields`
        // into the lambda lowering context when `this` is captured, so that
        // `this.Field` / `this.Method()` / bare field names resolve correctly
        // through the captured `this` pointer. Without `owner`, field-access
        // class resolution returns "unknown" and codegen emits broken GEPs.
        //
        // RFC 008 L3 补充（裸静态成员引用）：owner 传播判据 = this 被捕获 或
        // lambda 体裸引用了 owner 类静态成员。后者不捕获 this（无 env 字段，
        // 保持零开销 FnPtr 路径），但裸名 → 限定符号解析
        // （resolve_class_static_method / StaticField operand）依赖 owner——
        // 缺 owner 时裸静态调用降级为自由函数调用（mangling 错位 + 可达性
        // 无边被树摇）。class_fields 仍仅随 this 传播：裸实例字段访问必须
        // 有 this 指针可用，静态上下文无从绑定。
        let this_captured = captures.iter().any(|c| c.name == "this");
        let lambda_owner = if this_captured || refs_owner_static {
            owner
        } else {
            None
        };
        let lambda_class_fields: &[Ident] = if this_captured { class_fields } else { &[] };

        let stmts = {
            let mut lambda_ctx = LowerCtx {
                scopes,
                loop_scopes: Vec::new(),
                locals: &mut locals,
                array_lengths: IndexMap::new(),
                owner: lambda_owner,
                class_fields: lambda_class_fields,
                fn_sigs,
                registry,
                layouts,
                host_linkage,
                type_sizes,
                fn_ret: ret.clone(),
                expr_types,
            };
            match &l.body {
                LambdaBody::Expr(e) => {
                    let mut stmts = Vec::new();
                    let rvalue = self.lower_return_value(&e.node, &mut lambda_ctx, &mut stmts);
                    stmts.push(MirStatement::Return(Some(rvalue)));
                    stmts
                }
                LambdaBody::Block(b) => {
                    let mut s = self.lower_block(b, &mut lambda_ctx);
                    if let Some(tail) = &b.tail {
                        let rvalue = self.lower_return_value(&tail.node, &mut lambda_ctx, &mut s);
                        s.push(MirStatement::Return(Some(rvalue)));
                    }
                    s
                }
            }
        };

        self.next_local = saved;

        // RFC 009 M3：async lambda 对非 param / 非 capture locals 计算按需
        // spill 候选。capture locals 的 env 字段由 ctor 从闭包 env 拷贝，
        // 不参与 spill（避免 ctor 中覆盖堆槽指针的路径）。
        let spill_set = if l.is_async {
            let capture_local_ids: HashSet<LocalId> =
                captures_info.iter().map(|(id, _, _)| *id).collect();
            let local_indices: Vec<(usize, TypeId)> = locals
                .iter()
                .filter(|(id, _)| (id.0 as usize) >= param_count && !capture_local_ids.contains(id))
                .map(|(id, (_, ty))| (id.0 as usize, ty.clone()))
                .collect();
            typeck::analyze_spill_candidates(true, &local_indices, type_sizes)
        } else {
            typeck::SpillSet::empty()
        };

        let body = MirBody {
            params: all_params,
            ret: ret.clone(),
            param_count,
            locals,
            blocks: vec![MirBasicBlock { statements: stmts }],
            is_async: l.is_async,
            owner: None,
            class_fields: vec![],
            is_ctor: false,
            // RFC 006 M2：lambda 不是静态方法（无类上下文）。
            is_static: false,
            captures: captures_info,
            // RFC 024 M4-link Phase B：lifted lambda 跟随宿主函数 linkage
            // （单态化方法体内的 lambda 也应是 linkonce_odr，跨 .o 弱符号去重）。
            linkage: host_linkage,
            // RFC 039 M3：lambda 不携带 [Parallelize] 标记。
            parallelize: false,
            spill_set,
        };
        body.to_cfg()
    }

    /// RFC 023: Analyze a lambda body for captured outer variables.
    ///
    /// Walks the lambda body AST to find identifier references that resolve to
    /// outer-scope locals (not lambda params). Returns a
    /// [`LambdaCaptureAnalysis`] with each captured variable's name, type, and
    /// mode, plus the `refs_owner_static` flag (see struct docs).
    ///
    /// L1: only reference types (class, string, interface) are captured, using
    /// `CaptureMode::ByRef` (pointer to the object).
    ///
    /// L2: value types (int/double/bool/struct/vector/nullable) are additionally
    /// captured using `CaptureMode::ByValue` (copying the value into the env
    /// struct). `Void` / `Generic` / `Infer` / `Error` are never captured.
    ///
    /// L3: `this` is captured when the lambda body references it explicitly
    /// (`this`, `this.Field`, `this.Method()`) or implicitly (a bare field name
    /// that resolves to a class field, or a bare instance method call that
    /// resolves to a class method of the enclosing class incl. base chain —
    /// body lowering rewrites both to `this.X`, so the captured `this` pointer
    /// must exist). `this` is always captured `ByValue` (RFC 006 G2: the
    /// pointer value, never the host frame slot address).
    ///
    /// 裸静态成员引用（owner 类静态方法/属性/字段）不捕获 `this`，但随结果
    /// 携带 `refs_owner_static` 标记：lambda 降级上下文须传播 owner 才能把
    /// 裸名解析为限定符号（`Owner::Method` / StaticField），否则裸调用降级为
    /// 自由函数调用（mangling 错位 + 可达性无边被树摇）。
    fn compute_captures(l: &LambdaExpr, ctx: &LowerCtx) -> LambdaCaptureAnalysis {
        let param_names: HashSet<Ident> = l.params.iter().map(|p| p.name.clone()).collect();
        let mut idents = Vec::new();
        collect_lambda_body_idents(&l.body, &mut idents);
        // 写捕获分析：lambda 体内被赋值的裸名（含嵌套 lambda 传递写）。
        // 写捕获标量须 ByRef（写传播），只读标量按 capture_mode_for 快照。
        let mut write_captured: HashSet<Ident> = HashSet::new();
        collect_lambda_body_assigned_idents(&l.body, &mut write_captured);

        let mut captures = Vec::new();
        let mut refs_owner_static = false;
        let mut seen: HashSet<Ident> = HashSet::new();
        for name in &idents {
            if param_names.contains(name) || seen.contains(name) {
                continue;
            }
            if let Some(local_id) = ctx.lookup(name) {
                if let Some((_, ty)) = ctx.locals.get(&local_id) {
                    // RFC 006 G2：`this` 捕获强制 **ByValue**（存 this 指针值而非
                    // 槽地址）。`this` 是方法内局部，按 class→ByRef 会存 this 栈槽
                    // 地址，宿主方法返回后悬垂 → 闭包延迟调用读垃圾（声明式
                    // `Click="Method"` fired=0 实测根因）。this 不可重赋值，按值
                    // 捕获即持有对象引用，语义等价 C# `this` 捕获。
                    //
                    // 循环体局部同理：其槽跨迭代复用（`while`/`for`/`foreach` 均
                    // 单 LocalId），ByRef 捕获存槽地址会让每个迭代的闭包读到**最后
                    // 一次**赋值的对象（web 连接线程读到下一个连接 → 交叉串连崩溃，
                    // `web_core_auth_concurrency_e2e` ConnectionReset 根因）。循环
                    // 内闭包捕获循环局部按 C# 逐迭代变量语义快照（ByValue），并
                    // 由 codegen 在闭包创建点对引用对象 rt_arc_inc 持有强引用。
                    let mode = if name.as_str() == "this" || ctx.is_loop_captured_local(name) {
                        Some(CaptureMode::ByValue)
                    } else if write_captured.contains(name) && is_scalar_primitive(ty) {
                        // 写捕获标量 → ByRef（写传播）：lambda 体内赋值必须
                        // 穿透至外层局部，否则对快照副本的赋值是死代码
                        //（ForEach_AppliesAction：sum 累加后 Assert.Equal(6, sum)）。
                        Some(CaptureMode::ByRef)
                    } else {
                        capture_mode_for(ty, ctx.registry)
                    };
                    if let Some(mode) = mode {
                        seen.insert(name.clone());
                        captures.push(LambdaCapture {
                            name: name.clone(),
                            ty: ty.clone(),
                            mode,
                        });
                    }
                }
            } else if (ctx.is_class_field(name)
                // RFC 045（closure_static 崩溃根因）：裸静态字段引用（`_hits`）
                // 虽经 class_fields 命中但**不捕获 this**——静态成员不经 this
                // 访问，走 StaticField operand + owner 传播（与下方静态分支
                // 对齐）；旧实现把静态字段当实例字段捕获 this → 多余 env
                // （零开销 FnPtr 路径被破坏）。
                && !ctx.owner.as_ref().is_some_and(|owner| {
                    lower_call::mir_class_has_static_member_named(ctx.registry, owner, name)
                }))
                || ctx.owner.as_ref().is_some_and(|owner| {
                    lower_call::mir_has_instance_method_named(ctx.registry, owner, name)
                })
            {
                // RFC 008 L3: implicit `this.field` / `this.Method()` access — the
                // bare name resolves to a class member (field, or instance method
                // incl. base chain) of the enclosing class, not an outer local, so
                // capture `this`. The lambda body lowering rewrites the bare name
                // to `this.X` via the captured `this` pointer. 捕获分析必须覆盖
                // 方法名：裸实例方法调用（`new Thread(() => WatchExit())`）若
                // 不触发 this 捕获，owner 不传播、调用降级为自由函数调用——
                // mangling 错位（@WatchExit 而非 @Probe_WatchExit）+ 可达性
                // 无边被树摇，符号丢失。
                //
                // RFC 006 G2：`this` 捕获强制 **ByValue**（存 this 指针值）。
                // 否则按 class→ByRef 会存「外层 this 局部变量槽地址」（栈 alloca），
                // 而该槽位于宿主方法栈帧——方法返回后悬垂，闭包延迟调用从悬垂
                // 地址读 this 得垃圾指针 → 声明式 `Click="Method"` 运行期不触发
                // （`_ => this.OnX()` δ[实测] fired=0）。this 是引用类型且不可
                // 重新赋值，按值捕获即持有对象引用，语义等价 C# `this` 捕获。
                let this_name: Ident = "this".into();
                if !seen.contains(&this_name) {
                    if let Some(this_id) = ctx.lookup(&this_name) {
                        if let Some((_, ty)) = ctx.locals.get(&this_id) {
                            let mode = Some(CaptureMode::ByValue);
                            if let Some(mode) = mode {
                                seen.insert(this_name.clone());
                                captures.push(LambdaCapture {
                                    name: this_name,
                                    ty: ty.clone(),
                                    mode,
                                });
                            }
                        }
                    }
                }
            } else if ctx.owner.as_ref().is_some_and(|owner| {
                lower_call::mir_class_has_static_member_named(ctx.registry, owner, name)
            }) {
                // 裸静态成员引用：不捕获 this（静态成员不经 this 访问，保持
                // 无 env 的零开销路径），仅标记 owner 传播——lambda 降级上下文
                // 需要 owner 把裸名解析为限定符号（见 lower_lambda_to_body）。
                refs_owner_static = true;
            }
        }
        LambdaCaptureAnalysis {
            captures,
            refs_owner_static,
        }
    }

    /// Infer the lambda block-body return type.
    ///
    /// Scans all `return expr` (including those nested inside `if`/loops/try),
    /// preferring the first **concrete** type. `return null` infers to `Infer`;
    /// a lambda like `(ctx) => { ...; return new UserPrincipal(...); return null; }`
    /// must not resolve to `Infer`, otherwise the lifted function is emitted with
    /// an `i32` (boxed) return while the call site invokes it through the Func's
    /// declared class return type (`ptr`) → caller `rt_arc_inc`s a boxed integer
    /// (0xC0000005). No valued return → `Void` (Action).
    fn infer_lambda_block_ret(block: &Block, ctx: &LowerCtx) -> TypeId {
        fn walk(
            stmts: &[Spanned<Stmt>],
            ctx: &LowerCtx,
            fallback: &mut Option<TypeId>,
        ) -> Option<TypeId> {
            for s in stmts {
                match &s.node {
                    Stmt::Return(Some(e)) => {
                        let inferred = lower_type::infer_type_from_spanned(e, ctx);
                        let ty = match inferred {
                            TypeId::Bool => TypeId::Int,
                            other => other,
                        };
                        // `return null` → Infer：暂记为兜底，继续找具体返回类型。
                        if matches!(ty, TypeId::Infer | TypeId::Error) {
                            fallback.get_or_insert(ty);
                        } else {
                            return Some(ty);
                        }
                    }
                    Stmt::While { body, .. }
                    | Stmt::For { body, .. }
                    | Stmt::ForC { body, .. }
                    | Stmt::Using { body, .. }
                    | Stmt::AwaitUsing { body, .. }
                    | Stmt::Lock { body, .. } => {
                        if let Some(ty) = walk(&body.stmts, ctx, fallback) {
                            return Some(ty);
                        }
                    }
                    Stmt::TryCatch {
                        try_body,
                        catch_body,
                        finally,
                        ..
                    } => {
                        for blk in std::iter::once(try_body)
                            .chain(std::iter::once(catch_body))
                            .chain(finally.iter())
                        {
                            if let Some(ty) = walk(&blk.stmts, ctx, fallback) {
                                return Some(ty);
                            }
                        }
                    }
                    Stmt::TryFinally { body, finally } => {
                        for blk in [body, finally] {
                            if let Some(ty) = walk(&blk.stmts, ctx, fallback) {
                                return Some(ty);
                            }
                        }
                    }
                    // `if` 是表达式形态：块内 `if (...) { return ...; }` 走这里。
                    Stmt::Expr(e) => {
                        if let Expr::If {
                            then_branch,
                            else_branch,
                            ..
                        } = &e.node
                        {
                            if let Some(ty) = walk(&then_branch.stmts, ctx, fallback) {
                                return Some(ty);
                            }
                            if let Some(eb) = else_branch {
                                if let Some(ty) = walk(&eb.stmts, ctx, fallback) {
                                    return Some(ty);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        let mut fallback = None;
        walk(&block.stmts, ctx, &mut fallback)
            .or(fallback)
            .unwrap_or(TypeId::Void)
    }

    /// Lift a lambda to a top-level function and return a `FnPtr` or `Closure` operand.
    /// Used for inline lambdas passed as method arguments (e.g., `list.Find(x => x > 0)`).
    /// Parameter types come from explicit annotations on the lambda, or from the
    /// expected delegate parameter types (`expected`, from the call-site formal
    /// parameter type), or fall back to `TypeId::Int`.
    /// Return type is inferred from the lambda body.
    ///
    /// RFC 008: when the lambda has captures, returns `MirOperand::Closure` and the
    /// lifted function receives a `void* __env__` first parameter. No-capture lambdas
    /// keep the zero-overhead `FnPtr` path (bare function pointer, no env parameter).
    pub(super) fn lower_lambda_to_fnptr(
        &mut self,
        l: &LambdaExpr,
        ctx: &LowerCtx,
        expected: Option<&[TypeId]>,
        expected_ret: Option<&TypeId>,
    ) -> MirOperand {
        let lambda_name = format!("__lambda_rt_{}", self.next_lambda);
        self.next_lambda += 1;
        let param_pairs: Vec<(Ident, TypeId)> = l
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let pty = if let Some(t) = p.ty.as_ref() {
                    lower_type::lower_type_name(&t.node)
                } else if let Some(exp) = expected {
                    exp.get(i).cloned().unwrap_or(TypeId::Int)
                } else {
                    TypeId::Int
                };
                (p.name.clone(), pty)
            })
            .collect();
        let ret_ty = match &l.body {
            LambdaBody::Expr(e) => {
                let inferred = lower_type::infer_type_from_spanned(e, ctx);
                match inferred {
                    TypeId::Bool => TypeId::Int,
                    other => other,
                }
            }
            // Block body：扫描 `return expr` 推断返回类型（`Task.Run<T>(() => { return x; })`）；
            // 无 return 表达式则 Void（Action）。
            LambdaBody::Block(b) => Self::infer_lambda_block_ret(b, ctx),
        };
        // RFC 009 M6: async lambda 的函数签名返回类型是 `Task<T>`，
        // body 实际返回 T，由 codegen emit_async_state_machine 包裹。
        let ret_ty = if l.is_async {
            TypeId::Task {
                inner: Box::new(ret_ty),
            }
        } else {
            ret_ty
        };
        // 期望委托返回类型为接口时按契约提升：body 推断只见具体类（如
        // `() => { ...; return new DisposableAction(...); }` 推断为
        // DisposableAction），而 `Func<IDisposable>` 契约要求闭包产出接口
        // fat pointer——否则调用方（`_disposer = _callback()`）按 {obj,itable}
        // 解引用裸对象 → 0xC0000005（chord EffectEntry 实证）。fn_ret 提升为
        // 接口后，Return 的 MakeIface 包裹路径（lower_return_value）随之生效。
        // body 推断已是该接口或契约非接口时保持不变。
        let ret_ty = match expected_ret {
            Some(TypeId::Named(n)) if ctx.registry.is_interface(n) => TypeId::Named(n.clone()),
            _ => ret_ty,
        };
        // RFC 008: compute captures from outer scope (replaces typeck-filled
        // `l.captures`). typeck receives `&Expr` and cannot mutate the AST, so
        // capture analysis runs here in MIR lowering where outer-scope locals
        // are already bound in `ctx`.
        let LambdaCaptureAnalysis {
            captures,
            refs_owner_static,
        } = Self::compute_captures(l, ctx);
        let fn_sigs = ctx.fn_sigs;
        let registry = ctx.registry;
        let layouts = ctx.layouts;
        let owner = ctx.owner.clone();
        let class_fields = ctx.class_fields;
        let host_linkage = ctx.host_linkage;
        let body = self.lower_lambda_to_body(
            l,
            &param_pairs,
            &ret_ty,
            fn_sigs,
            registry,
            layouts,
            &captures,
            refs_owner_static,
            owner,
            class_fields,
            host_linkage,
            ctx.type_sizes,
            ctx.expr_types,
        );
        self.lifted.push((lambda_name.clone(), body));

        if captures.is_empty() {
            MirOperand::FnPtr { name: lambda_name }
        } else {
            // Resolve each capture to its source operand (the outer local).
            let env: Vec<(LambdaCapture, MirOperand)> = captures
                .iter()
                .map(|c| {
                    let src = ctx
                        .lookup(&c.name)
                        .map(MirOperand::Local)
                        .unwrap_or(MirOperand::ConstNull);
                    (c.clone(), src)
                })
                .collect();
            MirOperand::Closure {
                fn_name: lambda_name,
                env,
            }
        }
    }

    /// RFC 009 D3：SoA struct 数组字段写融合 —— `soaArr[i].field = v`。
    ///
    /// 读路径（`MirRvalue::SoaFieldGet`）已在 `lower_call.rs` 融合；写路径若
    /// 走 AoS 回退（`operand_from_expr(arr[i])` 物化元素）会对 `rt_soa_array`
    /// 描述符按 AoS 布局 GEP，导致越界读写。此处直接融合：
    ///   1. `rt_soa_field_ptr(arr, field_idx)` → 字段数组首指针（ptr 临时）
    ///   2. `IndexSet`（按字段类型 GEP + store）写入第 `i` 个元素
    ///
    /// 仅使用既有 MIR 变体（`Call` + `IndexSet`），不新增 statement/rvalue
    /// 变体（否则须同步更新 `arc/src/pipeline.rs` 与 codegen 的穷尽匹配）。
    ///
    /// 返回 `true` 表示已处理（receiver 为 SoA 数组的元素字段访问）。
    fn try_lower_soa_field_set(
        &mut self,
        receiver: &Expr,
        field: &Ident,
        value: &Expr,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) -> bool {
        let Expr::Index {
            receiver: arr,
            index,
        } = receiver
        else {
            return false;
        };
        let Some(struct_name) = lower_type::soa_array_elem_struct(arr, ctx) else {
            return false;
        };
        let (mut prep, arr_op) = lower_call::lower_arg_operand(self, &arr.node, ctx);
        let (prep_i, idx_op) = lower_call::lower_arg_operand(self, &index.node, ctx);
        prep.extend(prep_i);
        let (mut val_prep, val_rv) = lower_expr::lower_expr_to_rvalue_with_binary(value, self, ctx);
        prep.append(&mut val_prep);
        stmts.append(&mut prep);

        let (field_idx, field_ty) = lower_type::soa_field_idx_ty(ctx, &struct_name, field);
        // 1) 字段数组首指针（ptr 临时 local）
        let farr = self.fresh_local(&"_soa_field_arr".into(), TypeId::Object, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: farr,
            rvalue: MirRvalue::Call {
                func: "rt_soa_field_ptr".into(),
                args: vec![arr_op, MirOperand::ConstInt(field_idx as i64)],
            },
        });
        // 2) 类型化 GEP + store 到第 idx 个元素
        stmts.push(MirStatement::IndexSet {
            array: MirOperand::Local(farr),
            index: idx_op,
            elem_type: field_ty,
            value: val_rv,
        });
        true
    }

    pub(super) fn lower_block(&mut self, block: &Block, ctx: &mut LowerCtx) -> Vec<MirStatement> {
        let mut stmts = Vec::new();
        for (idx, stmt) in block.stmts.iter().enumerate() {
            match &stmt.node {
                Stmt::Let { name, ty, init, .. } => {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type::lower_type_name(&t.node))
                        .or_else(|| {
                            init.as_ref()
                                .map(|i| lower_type::infer_type_from_spanned(i, ctx))
                        })
                        .unwrap_or(TypeId::Int);
                    self.lower_let(name, local_ty, init.as_ref(), ctx, &mut stmts);
                }
                Stmt::Return(val) => match val {
                    Some(v) => {
                        // 与 typed 路径对称走 lower_return_value：声明返回为接口时
                        // 把 class 返回物化为 fat pointer（raw 路径此前裸 Return，
                        // 接口返回的 raw 方法体/λ 体收到裸对象 → 调用方按
                        // {obj,itable} 解引用 AV——chord `Func<IDisposable>` 回调
                        // `return new DisposableAction(...)` 实证）。
                        let rvalue = self.lower_return_value(&v.node, ctx, &mut stmts);
                        stmts.push(MirStatement::Return(Some(rvalue)));
                    }
                    None => stmts.push(MirStatement::Return(None)),
                },
                Stmt::While { cond, body } => {
                    // Re-evaluate cond each iteration (incl. `a && b` short-circuit prep).
                    // Desugar: flag=true; while(flag) { prep; if(cond) body else flag=false }
                    let body_stmts = self.lower_loop_block(body, ctx);
                    self.lower_while_with_cond(&cond.node, body_stmts, ctx, &mut stmts);
                }
                Stmt::For { var, iter, body } => {
                    // RFC 005：Span / ReadOnlySpan 走索引 while（Field Length，零堆）。
                    let iter_ty = lower_type::infer_type_from_spanned(iter, ctx);
                    if matches!(iter_ty, TypeId::Span { .. }) {
                        self.lower_span_foreach_untyped(var, iter, body, ctx, &mut stmts);
                        continue;
                    }
                    // LINQ 链 / Query 必须先于 IEnumerable 接口源识别：P0 表达式
                    // 类型表会把 Query 的 span 记录为 IEnumerable<T>（typeck 语义
                    // 正确），若先查 is_enumerable_iface 会把 Query 抢先路由到
                    // GetEnumerator 协议路径，而该路径对裸 Query 无物化能力
                    //（operand_from_expr 禁止 Query）。try_lower_linq_chain 对
                    // 非 LINQ 形态表达式快速返回 None，真正的接口变量不受影响。
                    let chain = lower_linq::try_lower_linq_chain(&iter.node, ctx).or_else(|| {
                        if let Expr::Query(q) = &iter.node {
                            Some(lower_linq::lower_query(q, ctx))
                        } else {
                            None
                        }
                    });
                    if let Some(chain) = chain {
                        stmts.push(MirStatement::LinqForeach {
                            var: var.clone(),
                            chain,
                            body: self.lower_loop_block(body, ctx),
                        });
                        continue;
                    }
                    // RFC 044：`IEnumerable<T>` 接口源走 GetEnumerator 协议路径
                    //（与 typed 路径 `lower_enumerable_foreach` 对偶，见其注释）。
                    if lower_type::is_enumerable_iface(&iter_ty) {
                        let elem_ty = iter_ty.enumerable_elem().unwrap_or(TypeId::Infer);
                        self.lower_enumerable_foreach_untyped(
                            var, &elem_ty, iter, body, ctx, &mut stmts,
                        );
                        continue;
                    }
                    self.lower_list_foreach_untyped(var, iter, body, ctx, &mut stmts);
                }
                Stmt::Assign { target, value } => {
                    if let Expr::Ident(name) = &target.node {
                        // Locals / params shadow fields (align with operand_from_expr).
                        if let Some(place) = ctx.lookup(name) {
                            let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                &value.node,
                                self,
                                ctx,
                            );
                            stmts.append(&mut prep);
                            // Class → interface assignment: wrap in MakeIface / MakeIfaceDyn
                            let place_ty = ctx.locals.get(&place).map(|(_, ty)| ty.clone());
                            let iface_dest = place_ty
                                .as_ref()
                                .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                .map(|iface_name| {
                                    (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                });
                            if let Some((src_ty, iface_name)) = iface_dest {
                                let temp_id = self.fresh_local(
                                    &Ident::from("_iface_obj"),
                                    src_ty.clone(),
                                    ctx.locals,
                                );
                                stmts.push(MirStatement::Assign {
                                    place: temp_id,
                                    rvalue: rv,
                                });
                                if let Some(wrap) = iface_wrap_rvalue(
                                    ctx.registry,
                                    &src_ty,
                                    &iface_name,
                                    MirOperand::Local(temp_id),
                                ) {
                                    stmts.push(MirStatement::Assign {
                                        place,
                                        rvalue: wrap,
                                    });
                                } else {
                                    stmts.push(MirStatement::Assign {
                                        place,
                                        rvalue: MirRvalue::Use(MirOperand::Local(temp_id)),
                                    });
                                }
                            } else {
                                stmts.push(MirStatement::Assign { place, rvalue: rv });
                            }
                        } else if ctx.is_class_field(name) {
                            // RFC 006 M3：静态字段赋值走 `StaticFieldSet`（store 到
                            // `@__static_<class>_<field>` 全局变量），实例字段走 `FieldSet`。
                            // 先 clone owner，避免后续 mutable borrow 与 immutable borrow 冲突。
                            let owner_is_static = ctx
                                .owner
                                .as_ref()
                                .is_some_and(|o| ctx.is_static_field_of(o, name));
                            if owner_is_static {
                                let owner = ctx.owner.as_ref().unwrap().to_string();
                                let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                                stmts.append(&mut prep);
                                stmts.push(MirStatement::StaticFieldSet {
                                    class: owner,
                                    field: name.to_string(),
                                    value: rv,
                                });
                            } else if let Some(this) = ctx.lookup(&"this".into()) {
                                let owner = ctx.owner.as_ref().unwrap().to_string();
                                let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                                stmts.append(&mut prep);
                                // Class → interface field assignment (`this.D = ...`):
                                // wrap in MakeIface / MakeIfaceDyn before FieldSet.
                                let field_ty = ctx
                                    .registry
                                    .field_info(
                                        &Ident::from(owner.as_str()),
                                        &Ident::from(name.as_str()),
                                    )
                                    .map(|f| TypeId::Named(f.ty.clone()));
                                let iface_dest = field_ty
                                    .as_ref()
                                    .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                    .map(|iface_name| {
                                        (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                    });
                                if let Some((src_ty, iface_name)) = iface_dest {
                                    let temp_id = self.fresh_local(
                                        &Ident::from("_iface_obj"),
                                        src_ty.clone(),
                                        ctx.locals,
                                    );
                                    stmts.push(MirStatement::Assign {
                                        place: temp_id,
                                        rvalue: rv,
                                    });
                                    let set_value = if let Some(wrap) = iface_wrap_rvalue(
                                        ctx.registry,
                                        &src_ty,
                                        &iface_name,
                                        MirOperand::Local(temp_id),
                                    ) {
                                        wrap
                                    } else {
                                        MirRvalue::Use(MirOperand::Local(temp_id))
                                    };
                                    stmts.push(MirStatement::FieldSet {
                                        object: MirOperand::Local(this),
                                        class: owner,
                                        field: name.to_string(),
                                        value: set_value,
                                    });
                                } else {
                                    stmts.push(MirStatement::FieldSet {
                                        object: MirOperand::Local(this),
                                        class: ctx.owner.as_ref().unwrap().to_string(),
                                        field: name.to_string(),
                                        value: rv,
                                    });
                                }
                            }
                        }
                    } else if let Expr::Field { receiver, field } = &target.node {
                        // RFC 009 D3：SoA 字段写融合（`soaArr[i].field = v`）。
                        if self.try_lower_soa_field_set(
                            &receiver.node,
                            field,
                            &value.node,
                            ctx,
                            &mut stmts,
                        ) {
                            // 已按字段阵列直接写入，跳过 AoS 物化路径。
                        } else {
                            let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
                            let recv_class_ident: Ident = recv_class.as_str().into();
                            // RFC 006 M3：跨类静态字段赋值（`Counter._count = ...`）
                            // 直接 store 到 `@__static_<class>_<field>` 全局变量，
                            // 不经过 receiver operand（receiver 仅作类型解析用）。
                            if ctx.is_static_field_of(&recv_class_ident, field) {
                                let (mut val_prep, val_rv) =
                                    lower_expr::lower_expr_to_rvalue_with_binary(
                                        &value.node,
                                        self,
                                        ctx,
                                    );
                                stmts.append(&mut val_prep);
                                stmts.push(MirStatement::StaticFieldSet {
                                    class: recv_class,
                                    field: field.to_string(),
                                    value: val_rv,
                                });
                            } else if let Some(setter) =
                                lower_call::user_type_static_property_setter_func(
                                    &receiver.node,
                                    field,
                                    ctx,
                                )
                            {
                                // RFC 004 M2 对称路径：`类名.静态属性 = v`（自定义访问器）
                                // → 无 this 的静态 `set_*` 调用；receiver 是类名非表达式，
                                // 落入实例物化路径会以 unresolved ident ICE。
                                let (mut val_prep, val_op) =
                                    lower_call::lower_arg_operand(self, &value.node, ctx);
                                stmts.append(&mut val_prep);
                                let place = self.fresh_local(
                                    &"_setstaticprop".into(),
                                    TypeId::Void,
                                    ctx.locals,
                                );
                                stmts.push(MirStatement::Assign {
                                    place,
                                    rvalue: MirRvalue::Call {
                                        func: setter,
                                        args: vec![val_op],
                                    },
                                });
                            } else {
                                // CD-11：复杂接收体（`list[i]` 索引读取等）须经
                                // `lower_arg_operand` 物化为临时局部——`operand_from_expr`
                                // 无 prep 语句通道，`Expr::Index` 落入 catch-all panic
                                // （`this.Steps[i].Done = true` → MIR lower ICE）。
                                let (mut recv_prep, recv_op) =
                                    lower_call::lower_arg_operand(self, &receiver.node, ctx);
                                stmts.append(&mut recv_prep);
                                // 索引接收体的 `class_from_expr` 解析为 "unknown"，
                                // 从已物化 operand 的类型解析实际类（与
                                // `lower_arg_operand` 的 Field 分支对齐）。
                                let recv_class = if recv_class == "unknown" {
                                    lower_type::type_name_from_operand(
                                        &recv_op,
                                        &receiver.node,
                                        ctx,
                                    )
                                    .to_string()
                                } else {
                                    recv_class
                                };
                                let recv_class_ident: Ident = recv_class.as_str().into();
                                if is_custom_accessor_property(ctx.registry, &recv_class, field) {
                                    let (mut val_prep, val_op) =
                                        lower_call::lower_arg_operand(self, &value.node, ctx);
                                    stmts.append(&mut val_prep);
                                    let setter = format!("set_{field}");
                                    let set_params = vec![lower_type::type_name_from_operand(
                                        &val_op,
                                        &value.node,
                                        ctx,
                                    )
                                    .to_string()];
                                    let (impl_class, target_fn) = resolve_method_target(
                                        ctx.registry,
                                        &recv_class.clone().into(),
                                        &setter.clone().into(),
                                        ctx.owner.clone(),
                                    );
                                    let is_virtual = is_virtual_member(
                                        ctx.layouts,
                                        &recv_class,
                                        &setter,
                                        &set_params,
                                    );
                                    let rvalue = MirRvalue::MethodCall {
                                        receiver: recv_op,
                                        method: setter,
                                        args: vec![val_op],
                                        receiver_type: recv_class,
                                        impl_class,
                                        target_fn,
                                        is_virtual,
                                        params: set_params,
                                    };
                                    let place = self.fresh_local(
                                        &"_setprop".into(),
                                        TypeId::Void,
                                        ctx.locals,
                                    );
                                    stmts.push(MirStatement::Assign { place, rvalue });
                                } else {
                                    // Auto-property (get; set;) — the property name is
                                    // registered as a backing field (registry.rs:186),
                                    // so write directly to it. Without this branch the
                                    // assignment was silently dropped, leaving class-typed
                                    // fields as uninitialized garbage and crashing on use.
                                    let (mut val_prep, val_rv) =
                                        lower_expr::lower_expr_to_rvalue_with_binary(
                                            &value.node,
                                            self,
                                            ctx,
                                        );
                                    stmts.append(&mut val_prep);
                                    // Class → interface field assignment (`h.D = new Disp()`):
                                    // wrap in MakeIface / MakeIfaceDyn before FieldSet, matching
                                    // the local/param path. Without it the raw object pointer is
                                    // stored into the interface slot and the call site reads a
                                    // non-fat-pointer → 0xC0000005.
                                    let field_ty = ctx
                                        .registry
                                        .field_info(&recv_class_ident, field)
                                        .map(|f| TypeId::Named(f.ty.clone()));
                                    let iface_dest = field_ty
                                        .as_ref()
                                        .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                        .map(|iface_name| {
                                            (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                        });
                                    if let Some((src_ty, iface_name)) = iface_dest {
                                        let temp_id = self.fresh_local(
                                            &Ident::from("_iface_obj"),
                                            src_ty.clone(),
                                            ctx.locals,
                                        );
                                        stmts.push(MirStatement::Assign {
                                            place: temp_id,
                                            rvalue: val_rv,
                                        });
                                        if let Some(wrap) = iface_wrap_rvalue(
                                            ctx.registry,
                                            &src_ty,
                                            &iface_name,
                                            MirOperand::Local(temp_id),
                                        ) {
                                            stmts.push(MirStatement::FieldSet {
                                                object: recv_op,
                                                class: recv_class,
                                                field: field.to_string(),
                                                value: wrap,
                                            });
                                        } else {
                                            stmts.push(MirStatement::FieldSet {
                                                object: recv_op,
                                                class: recv_class,
                                                field: field.to_string(),
                                                value: MirRvalue::Use(MirOperand::Local(temp_id)),
                                            });
                                        }
                                    } else {
                                        stmts.push(MirStatement::FieldSet {
                                            object: recv_op,
                                            class: recv_class,
                                            field: field.to_string(),
                                            value: val_rv,
                                        });
                                    }
                                }
                            }
                        }
                    } else if let Expr::Index { receiver, index } = &target.node {
                        let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
                        if let Some(ix) = lower_type::resolve_indexer(&recv_class, &index.node, ctx)
                        {
                            let set_method = ix.set.unwrap_or_else(|| {
                                panic!(
                                    "MIR lower: write to read-only indexer `{recv_class}` \
                                     (get_Item declared without set_Item)"
                                )
                            });
                            // receiver/index 均须经 lower_arg_operand 物化：嵌套索引
                            // （`m[k][i] = v` 的 `m[k]` 是 get_Item 调用，rvalue 不可
                            // 作 operand）与 `i*2` 类索引表达式都要求 prep 语句落地。
                            // 与下方原生数组 `T[]` 分支同一纪律。
                            let (mut recv_prep, recv_op) =
                                lower_call::lower_arg_operand(self, &receiver.node, ctx);
                            stmts.append(&mut recv_prep);
                            let (mut idx_prep, idx_op) =
                                lower_call::lower_arg_operand(self, &index.node, ctx);
                            stmts.append(&mut idx_prep);
                            let (mut val_prep, val_op) =
                                lower_call::lower_arg_operand(self, &value.node, ctx);
                            stmts.append(&mut val_prep);
                            let rvalue = MirRvalue::MethodCall {
                                receiver: recv_op,
                                method: set_method.into(),
                                args: vec![idx_op, val_op],
                                receiver_type: recv_class.clone(),
                                impl_class: Some(recv_class.clone()),
                                target_fn: Some(format!("{recv_class}::{set_method}")),
                                is_virtual: false,
                                params: vec![],
                            };
                            let place =
                                self.fresh_local(&"_setidx".into(), TypeId::Void, ctx.locals);
                            stmts.push(MirStatement::Assign { place, rvalue });
                        } else {
                            let (mut prep, arr_op) =
                                lower_call::lower_arg_operand(self, &receiver.node, ctx);
                            let (prep_i, idx_op) =
                                lower_call::lower_arg_operand(self, &index.node, ctx);
                            prep.extend(prep_i);
                            let (mut val_prep, val_rv) =
                                lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                            prep.append(&mut val_prep);
                            stmts.append(&mut prep);
                            stmts.push(MirStatement::IndexSet {
                                array: arr_op,
                                index: idx_op,
                                elem_type: lower_type::index_elem_type_non_indexer(receiver, ctx),
                                value: val_rv,
                            });
                        }
                    } else if let Expr::NullCond { access } = &target.node {
                        // RFC 008：`recv?.member = value`
                        if let Expr::Field { receiver, field } = &access.node {
                            self.lower_null_cond_field_assign(
                                &receiver.node,
                                field,
                                &value.node,
                                ctx,
                                &mut stmts,
                            );
                        }
                    }
                }
                Stmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut stmts),
                Stmt::Break => stmts.push(MirStatement::Break),
                Stmt::Continue => stmts.push(MirStatement::Continue),
                Stmt::Throw { expr } => {
                    let (mut prep, value) =
                        lower_expr::lower_expr_to_rvalue_with_binary(&expr.node, self, ctx);
                    stmts.append(&mut prep);
                    stmts.push(MirStatement::Throw { value });
                }
                Stmt::TryCatch {
                    try_body,
                    catch_ty,
                    catch_name,
                    when_cond,
                    catch_body,
                    finally,
                } => {
                    let local_ty = lower_type::lower_type_name(&catch_ty.node);
                    let catch_var = self.fresh_local(catch_name, local_ty.clone(), ctx.locals);
                    // catch 变量仅在 catch 块内可见（typeck 同款作用域）。
                    ctx.push_scope();
                    ctx.bind(catch_name, catch_var);
                    let mut catch_stmts = self.lower_block(catch_body, ctx);
                    if let Some(w) = when_cond {
                        catch_stmts = self.wrap_catch_when(w, catch_var, catch_stmts, ctx);
                    }
                    ctx.pop_scope();
                    let try_catch = MirStatement::TryCatch {
                        try_body: self.lower_block(try_body, ctx),
                        catch_var,
                        catch_ty: local_ty,
                        catch_body: catch_stmts,
                    };
                    if let Some(f) = finally {
                        stmts.push(MirStatement::TryFinally {
                            body: vec![try_catch],
                            finally: self.lower_block(f, ctx),
                        });
                    } else {
                        stmts.push(try_catch);
                    }
                }
                Stmt::TryFinally { body, finally } => {
                    stmts.push(MirStatement::TryFinally {
                        body: self.lower_block(body, ctx),
                        finally: self.lower_block(finally, ctx),
                    });
                }
                Stmt::Using {
                    name,
                    ty,
                    init,
                    body,
                } => {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type::lower_type_name(&t.node))
                        .unwrap_or_else(|| lower_type::infer_type_from_spanned(init, ctx));
                    self.lower_let(name, local_ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let body_stmts = self.lower_block(body, ctx);
                    let dispose_stmt = self.build_dispose_call(resource_local, &local_ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                }
                // RFC 010：`using var` — 后续语句包进 TryFinally（嵌套 → LIFO Dispose）。
                Stmt::UsingVar { name, ty, init } => {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type::lower_type_name(&t.node))
                        .unwrap_or_else(|| lower_type::infer_type_from_spanned(init, ctx));
                    self.lower_let(name, local_ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let rest = Block {
                        stmts: block.stmts[idx + 1..].to_vec(),
                        tail: block.tail.clone(),
                    };
                    let body_stmts = self.lower_block(&rest, ctx);
                    let dispose_stmt = self.build_dispose_call(resource_local, &local_ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                    return stmts;
                }
                Stmt::AwaitUsing {
                    name,
                    ty,
                    init,
                    body,
                } => {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type::lower_type_name(&t.node))
                        .unwrap_or_else(|| lower_type::infer_type_from_spanned(init, ctx));
                    self.lower_let(name, local_ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let body_stmts = self.lower_block(body, ctx);
                    let dispose_stmt =
                        self.build_dispose_async_await(resource_local, &local_ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                }
                Stmt::AwaitUsingVar { name, ty, init } => {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type::lower_type_name(&t.node))
                        .unwrap_or_else(|| lower_type::infer_type_from_spanned(init, ctx));
                    self.lower_let(name, local_ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let rest = Block {
                        stmts: block.stmts[idx + 1..].to_vec(),
                        tail: block.tail.clone(),
                    };
                    let body_stmts = self.lower_block(&rest, ctx);
                    let dispose_stmt =
                        self.build_dispose_async_await(resource_local, &local_ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                    return stmts;
                }
                // RFC 044：yield 在 hir 脱糖为状态机，MIR 不应见到本节点——
                // 值表达式求值丢弃语义不可达，防御性跳过以保持编译期穷尽性。
                Stmt::YieldReturn { .. } | Stmt::YieldBreak => {}
                Stmt::Lock { expr, body } => {
                    // `lock (obj) { body }` 在原始（raw）路径脱糖为
                    // `Monitor.Enter` + `try/finally Monitor.Exit`（对标
                    // typeck::check_lock_stmt，RFC 009 §7.2；expr 求值一次）。
                    // 该路径覆盖 lambda 体 / 泛型单态化方法体的原始 lowering，
                    // 这些场景没有 typed body、锁未提前脱糖。
                    let tmp: Ident = format!("__lock_{}", stmt.span.start).into();
                    let enter_call = Stmt::Expr(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Ident(Ident::from("Monitor")),
                                stmt.span,
                            )),
                            method: Ident::from("Enter"),
                            args: vec![Spanned::new(Expr::Ident(tmp.clone()), stmt.span)],
                            type_args: vec![],
                            params_span: None,
                        },
                        stmt.span,
                    ));
                    let exit_call = Stmt::Expr(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Ident(Ident::from("Monitor")),
                                stmt.span,
                            )),
                            method: Ident::from("Exit"),
                            args: vec![Spanned::new(Expr::Ident(tmp.clone()), stmt.span)],
                            type_args: vec![],
                            params_span: None,
                        },
                        stmt.span,
                    ));
                    let desugared = Block {
                        stmts: vec![
                            Spanned::new(
                                Stmt::Let {
                                    mutable: false,
                                    name: tmp.clone(),
                                    ty: Some(Spanned::new(
                                        Type::Named {
                                            path: vec![Ident::from("Lock")],
                                            generics: vec![],
                                        },
                                        stmt.span,
                                    )),
                                    init: Some(expr.clone()),
                                },
                                stmt.span,
                            ),
                            Spanned::new(enter_call, stmt.span),
                            Spanned::new(
                                Stmt::TryFinally {
                                    body: body.clone(),
                                    finally: Block {
                                        stmts: vec![Spanned::new(exit_call, stmt.span)],
                                        tail: None,
                                    },
                                },
                                stmt.span,
                            ),
                        ],
                        tail: None,
                    };
                    stmts.extend(self.lower_block(&desugared, ctx));
                }
                Stmt::ForC {
                    init,
                    cond,
                    inc,
                    body,
                } => {
                    // 与 typed 路径对齐：for 语句自成一作用域，防止循环体内
                    // 嵌套同名变量劫持外层 for 的 inc/cond 解析（async 状态机
                    // 循环变量槽位分裂根因）。
                    ctx.push_scope();
                    if let Some(ref s) = init {
                        match &*s.node {
                            Stmt::Let {
                                name,
                                ty,
                                init: let_init,
                                ..
                            } => {
                                let local_ty = ty
                                    .as_ref()
                                    .map(|t| lower_type::lower_type_name(&t.node))
                                    .or_else(|| {
                                        let_init
                                            .as_ref()
                                            .map(|i| lower_type::infer_type_from_spanned(i, ctx))
                                    })
                                    .unwrap_or(TypeId::Int);
                                self.lower_let(name, local_ty, let_init.as_ref(), ctx, &mut stmts);
                            }
                            Stmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut stmts),
                            _ => {}
                        }
                    }
                    let mut body_stmts = self.lower_loop_block(body, ctx);
                    let mut inc_stmts: Vec<MirStatement> = Vec::new();
                    if let Some(ref s) = inc {
                        match &*s.node {
                            Stmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut inc_stmts),
                            Stmt::Assign { target, value } => {
                                if let Expr::Ident(name) = &target.node {
                                    if let Some(place) = ctx.lookup(name) {
                                        let (mut prep, rv) =
                                            lower_expr::lower_expr_to_rvalue_with_binary(
                                                &value.node,
                                                self,
                                                ctx,
                                            );
                                        inc_stmts.append(&mut prep);
                                        inc_stmts.push(MirStatement::Assign { place, rvalue: rv });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    // 在循环体每个本层 `continue` 前注入 increment，避免 continue 跳过 inc 死循环。
                    body_stmts = inject_for_increment(body_stmts, &inc_stmts, false);
                    body_stmts.extend(inc_stmts);
                    match cond.as_ref() {
                        Some(c) => self.lower_while_with_cond(&c.node, body_stmts, ctx, &mut stmts),
                        None => stmts.push(MirStatement::While {
                            cond: MirRvalue::Use(MirOperand::ConstInt(1)),
                            body: body_stmts,
                            foreach_source: None,
                        }),
                    }
                    ctx.pop_scope();
                }
                // RFC 004 M2：正常路径走 typed_body 脱糖；AST 回退路径不应到达。
                Stmt::DeconstructAssign { .. } => {
                    panic!(
                        "MIR lower: Stmt::DeconstructAssign reached untyped lower_block; \
                         typeck must expand it into Let + Deconstruct MethodCall"
                    );
                }
            }
        }
        // `else if` 链：Block.tail 中嵌套的 `Expr::If`。
        // parser 把 `if (a) X; else if (b) Y;` 编码为：
        //   Block { stmts: [], tail: Some(Expr::If { ... }) }
        // 必须在 lower_block 末尾显式处理 tail。
        if let Some(tail) = &block.tail {
            self.lower_stmt_expr(tail, ctx, &mut stmts);
        }
        stmts
    }

    /// 将返回表达式值降低为函数声明返回类型对应的 MIR rvalue。
    ///
    /// 当声明返回类型是接口而返回表达式是 class 值时，先落到临时 local，
    /// 再包裹 `MakeIface` / `MakeIfaceDyn` / `AdaptIface` 物化接口胖指针。
    /// 缺失此包裹时调用方收到裸对象指针，接口分派按胖指针 `{ptr,ptr}`
    /// 解引用导致 ACCESS_VIOLATION（covariance_e2e 标注的既有缺口）。
    ///
    /// 临时 local 以 `TypeId::Object` 登记——避免 `lower_fn` 结尾对其追加
    /// 自动 Drop：返回对象的所有权随胖指针转移给调用方，不能在此释放。
    fn lower_return_value(
        &mut self,
        v: &Expr,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) -> MirRvalue {
        let (mut prep, rvalue) = lower_expr::lower_expr_to_rvalue_with_binary(v, self, ctx);
        stmts.append(&mut prep);
        let Some(iface_name) = iface_dest_name(&ctx.fn_ret, ctx.registry) else {
            return rvalue;
        };
        let src_ty = class_ty_for_iface_wrap(v, ctx);
        let temp_id = self.fresh_local(&"_iface_ret".into(), TypeId::Object, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: temp_id,
            rvalue,
        });
        if let Some(wrap) = iface_wrap_rvalue(
            ctx.registry,
            &src_ty,
            &iface_name,
            MirOperand::Local(temp_id),
        ) {
            wrap
        } else {
            MirRvalue::Use(MirOperand::Local(temp_id))
        }
    }

    pub(super) fn lower_typed_block(
        &mut self,
        block: &TypedBlock,
        ctx: &mut LowerCtx,
    ) -> Vec<MirStatement> {
        let mut stmts = Vec::new();
        for (idx, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                TypedStmt::Let { name, ty, init } => {
                    self.lower_let(name, ty.clone(), init.as_ref(), ctx, &mut stmts);
                }
                TypedStmt::Return(val) => match val {
                    Some(v) => {
                        let rvalue = self.lower_return_value(&v.node, ctx, &mut stmts);
                        stmts.push(MirStatement::Return(Some(rvalue)));
                    }
                    None => stmts.push(MirStatement::Return(None)),
                },
                TypedStmt::While { cond, body } => {
                    let body_stmts = self.lower_loop_typed_block(body, ctx);
                    self.lower_while_with_cond(&cond.node, body_stmts, ctx, &mut stmts);
                }
                TypedStmt::For {
                    var,
                    iter,
                    body,
                    elem_ty,
                } => {
                    // RFC 005：Span / ReadOnlySpan 走索引 while（Field Length，零堆）。
                    let iter_ty = lower_type::infer_type_from_spanned(iter, ctx);
                    if matches!(iter_ty, TypeId::Span { .. }) {
                        self.lower_span_foreach(var, elem_ty, iter, body, ctx, &mut stmts);
                        continue;
                    }
                    // LINQ 链 / Query 先于 IEnumerable 接口源识别（同 untyped 路径
                    // 注释）：P0 类型表把 Query 记录为 IEnumerable<T>，先查接口
                    // 会把 LINQ 语法抢先路由到 GetEnumerator 协议路径。
                    let chain = lower_linq::try_lower_linq_chain(&iter.node, ctx).or_else(|| {
                        if let Expr::Query(q) = &iter.node {
                            Some(lower_linq::lower_query(q, ctx))
                        } else {
                            None
                        }
                    });
                    if let Some(chain) = chain {
                        // `lower_linq_foreach` handles three cases internally:
                        // Case 1 (compile-time array, source_len=Some) → while+IndexGet
                        // Case 2 (runtime List<T>) → while+Get+apply_linq_ops
                        // Case 3 (fallback) → LinqForeach statement
                        // Always calling it lets Case 2 inline Where/Select for
                        // runtime List<T> sources instead of falling through to
                        // the LinqForeach codegen that ignores LINQ operators.
                        self.lower_linq_foreach(var, chain, body, elem_ty, ctx, &mut stmts);
                        continue;
                    }
                    // RFC 044：`IEnumerable<T>` 接口源（yield 序列 / 自定义实现）走
                    // `GetEnumerator()`/`MoveNext()`/`Current` 协议路径（胖指针接口分派），
                    // 而非 `get_Count`/`get_Item` 索引路径。List_* / 数组等具体集合
                    // 仍走各自索引快路径。
                    if lower_type::is_enumerable_iface(&iter_ty) {
                        self.lower_enumerable_foreach(var, elem_ty, iter, body, ctx, &mut stmts);
                        continue;
                    }
                    self.lower_list_foreach(var, elem_ty, iter, body, ctx, &mut stmts);
                }
                TypedStmt::ForC {
                    init,
                    cond,
                    inc,
                    body,
                } => {
                    // C#/typeck 语义：`for` 语句自成一作用域（init/cond/body/inc
                    // 全部在其中解析）。MIR 曾不做语句作用域——循环体内嵌套的
                    // 同名变量（如内层 `for (int i...)`）会把 `i` 的绑定劫持到
                    // 内层局部，外层 for 的 inc/cond 落到错误槽：async 状态机
                    // 中表现为循环变量分裂为两个栈槽（body 索引恒 0、控制路径
                    // 递增另一槽）。进入语句作用域使 inc/cond 仍解析到外层 `i`。
                    ctx.push_scope();
                    // Desugar: for (init; cond; inc) { body }
                    //       → { init; while (cond) { body; inc; } }
                    if let Some(ref s) = init {
                        match &s.node {
                            Stmt::Let {
                                name,
                                ty,
                                init: let_init,
                                ..
                            } => {
                                let local_ty = ty
                                    .as_ref()
                                    .map(|t| lower_type::lower_type_name(&t.node))
                                    .or_else(|| {
                                        let_init
                                            .as_ref()
                                            .map(|i| lower_type::infer_type_from_spanned(i, ctx))
                                    })
                                    .unwrap_or(TypeId::Int);
                                self.lower_let(name, local_ty, let_init.as_ref(), ctx, &mut stmts);
                            }
                            Stmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut stmts),
                            _ => {}
                        }
                    }
                    let mut body_stmts = self.lower_loop_typed_block(body, ctx);
                    let mut inc_stmts: Vec<MirStatement> = Vec::new();
                    if let Some(ref s) = inc {
                        match &s.node {
                            Stmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut inc_stmts),
                            Stmt::Assign { target, value } => {
                                if let Expr::Ident(name) = &target.node {
                                    if let Some(place) = ctx.lookup(name) {
                                        let (mut prep, rv) =
                                            lower_expr::lower_expr_to_rvalue_with_binary(
                                                &value.node,
                                                self,
                                                ctx,
                                            );
                                        inc_stmts.append(&mut prep);
                                        inc_stmts.push(MirStatement::Assign { place, rvalue: rv });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    // 在循环体每个本层 `continue` 前注入 increment，避免 continue 跳过 inc 死循环。
                    body_stmts = inject_for_increment(body_stmts, &inc_stmts, false);
                    body_stmts.extend(inc_stmts);
                    match cond.as_ref() {
                        Some(c) => self.lower_while_with_cond(&c.node, body_stmts, ctx, &mut stmts),
                        None => stmts.push(MirStatement::While {
                            cond: MirRvalue::Use(MirOperand::ConstInt(1)),
                            body: body_stmts,
                            foreach_source: None,
                        }),
                    }
                    ctx.pop_scope();
                }
                TypedStmt::Assign { target, value } => {
                    if let Expr::Ident(name) = &target.node {
                        // Locals / params shadow fields (align with operand_from_expr).
                        if let Some(place) = ctx.lookup(name) {
                            let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                &value.node,
                                self,
                                ctx,
                            );
                            stmts.append(&mut prep);
                            // Class → interface assignment: wrap in MakeIface / MakeIfaceDyn
                            let place_ty = ctx.locals.get(&place).map(|(_, ty)| ty.clone());
                            let iface_dest = place_ty
                                .as_ref()
                                .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                .map(|iface_name| {
                                    (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                });
                            if let Some((src_ty, iface_name)) = iface_dest {
                                let temp_id = self.fresh_local(
                                    &Ident::from("_iface_obj"),
                                    src_ty.clone(),
                                    ctx.locals,
                                );
                                stmts.push(MirStatement::Assign {
                                    place: temp_id,
                                    rvalue: rv,
                                });
                                if let Some(wrap) = iface_wrap_rvalue(
                                    ctx.registry,
                                    &src_ty,
                                    &iface_name,
                                    MirOperand::Local(temp_id),
                                ) {
                                    stmts.push(MirStatement::Assign {
                                        place,
                                        rvalue: wrap,
                                    });
                                } else {
                                    stmts.push(MirStatement::Assign {
                                        place,
                                        rvalue: MirRvalue::Use(MirOperand::Local(temp_id)),
                                    });
                                }
                            } else {
                                stmts.push(MirStatement::Assign { place, rvalue: rv });
                            }
                        } else if ctx.is_class_field(name) {
                            // RFC 006 M3：静态字段赋值走 `StaticFieldSet`，实例字段走 `FieldSet`。
                            // 先 clone owner，避免后续 mutable borrow 与 immutable borrow 冲突。
                            let owner_is_static = ctx
                                .owner
                                .as_ref()
                                .is_some_and(|o| ctx.is_static_field_of(o, name));
                            if owner_is_static {
                                let owner = ctx.owner.as_ref().unwrap().to_string();
                                let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                                stmts.append(&mut prep);
                                stmts.push(MirStatement::StaticFieldSet {
                                    class: owner,
                                    field: name.to_string(),
                                    value: rv,
                                });
                            } else if let Some(this) = ctx.lookup(&"this".into()) {
                                let owner = ctx.owner.as_ref().unwrap().to_string();
                                let (mut prep, rv) = lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                                stmts.append(&mut prep);
                                // Class → interface field assignment (`this.D = ...`):
                                // wrap in MakeIface / MakeIfaceDyn before FieldSet.
                                let field_ty = ctx
                                    .registry
                                    .field_info(
                                        &Ident::from(owner.as_str()),
                                        &Ident::from(name.as_str()),
                                    )
                                    .map(|f| TypeId::Named(f.ty.clone()));
                                let iface_dest = field_ty
                                    .as_ref()
                                    .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                    .map(|iface_name| {
                                        (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                    });
                                if let Some((src_ty, iface_name)) = iface_dest {
                                    let temp_id = self.fresh_local(
                                        &Ident::from("_iface_obj"),
                                        src_ty.clone(),
                                        ctx.locals,
                                    );
                                    stmts.push(MirStatement::Assign {
                                        place: temp_id,
                                        rvalue: rv,
                                    });
                                    let set_value = if let Some(wrap) = iface_wrap_rvalue(
                                        ctx.registry,
                                        &src_ty,
                                        &iface_name,
                                        MirOperand::Local(temp_id),
                                    ) {
                                        wrap
                                    } else {
                                        MirRvalue::Use(MirOperand::Local(temp_id))
                                    };
                                    stmts.push(MirStatement::FieldSet {
                                        object: MirOperand::Local(this),
                                        class: owner,
                                        field: name.to_string(),
                                        value: set_value,
                                    });
                                } else {
                                    stmts.push(MirStatement::FieldSet {
                                        object: MirOperand::Local(this),
                                        class: ctx.owner.as_ref().unwrap().to_string(),
                                        field: name.to_string(),
                                        value: rv,
                                    });
                                }
                            }
                        }
                    } else if let Expr::Field { receiver, field } = &target.node {
                        // RFC 009 D3：SoA 字段写融合（`soaArr[i].field = v`）。
                        if self.try_lower_soa_field_set(
                            &receiver.node,
                            field,
                            &value.node,
                            ctx,
                            &mut stmts,
                        ) {
                            // 已按字段阵列直接写入，跳过 AoS 物化路径。
                        } else {
                            let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
                            let recv_class_ident: Ident = recv_class.as_str().into();
                            // RFC 006 M3：跨类静态字段赋值走 `StaticFieldSet`。
                            if ctx.is_static_field_of(&recv_class_ident, field) {
                                let (mut val_prep, val_rv) =
                                    lower_expr::lower_expr_to_rvalue_with_binary(
                                        &value.node,
                                        self,
                                        ctx,
                                    );
                                stmts.append(&mut val_prep);
                                stmts.push(MirStatement::StaticFieldSet {
                                    class: recv_class,
                                    field: field.to_string(),
                                    value: val_rv,
                                });
                            } else if let Some(setter) =
                                lower_call::user_type_static_property_setter_func(
                                    &receiver.node,
                                    field,
                                    ctx,
                                )
                            {
                                // RFC 004 M2 对称路径：`类名.静态属性 = v`（自定义访问器）
                                // → 无 this 的静态 `set_*` 调用；receiver 是类名非表达式，
                                // 落入实例物化路径会以 unresolved ident ICE。
                                let (mut val_prep, val_op) =
                                    lower_call::lower_arg_operand(self, &value.node, ctx);
                                stmts.append(&mut val_prep);
                                let place = self.fresh_local(
                                    &"_setstaticprop".into(),
                                    TypeId::Void,
                                    ctx.locals,
                                );
                                stmts.push(MirStatement::Assign {
                                    place,
                                    rvalue: MirRvalue::Call {
                                        func: setter,
                                        args: vec![val_op],
                                    },
                                });
                            } else {
                                // CD-11：复杂接收体（`list[i]` 索引读取等）须经
                                // `lower_arg_operand` 物化为临时局部——`operand_from_expr`
                                // 无 prep 语句通道，`Expr::Index` 落入 catch-all panic
                                // （`this.Steps[i].Done = true` → MIR lower ICE）。
                                let (mut recv_prep, recv_op) =
                                    lower_call::lower_arg_operand(self, &receiver.node, ctx);
                                stmts.append(&mut recv_prep);
                                // 索引接收体的 `class_from_expr` 解析为 "unknown"，
                                // 从已物化 operand 的类型解析实际类（与
                                // `lower_arg_operand` 的 Field 分支对齐）。
                                let recv_class = if recv_class == "unknown" {
                                    lower_type::type_name_from_operand(
                                        &recv_op,
                                        &receiver.node,
                                        ctx,
                                    )
                                    .to_string()
                                } else {
                                    recv_class
                                };
                                let recv_class_ident: Ident = recv_class.as_str().into();
                                if is_custom_accessor_property(ctx.registry, &recv_class, field) {
                                    let (mut val_prep, val_op) =
                                        lower_call::lower_arg_operand(self, &value.node, ctx);
                                    stmts.append(&mut val_prep);
                                    let setter = format!("set_{field}");
                                    let set_params = vec![lower_type::type_name_from_operand(
                                        &val_op,
                                        &value.node,
                                        ctx,
                                    )
                                    .to_string()];
                                    let (impl_class, target_fn) = resolve_method_target(
                                        ctx.registry,
                                        &recv_class.clone().into(),
                                        &setter.clone().into(),
                                        ctx.owner.clone(),
                                    );
                                    let is_virtual = is_virtual_member(
                                        ctx.layouts,
                                        &recv_class,
                                        &setter,
                                        &set_params,
                                    );
                                    let rvalue = MirRvalue::MethodCall {
                                        receiver: recv_op,
                                        method: setter,
                                        args: vec![val_op],
                                        receiver_type: recv_class,
                                        impl_class,
                                        target_fn,
                                        is_virtual,
                                        params: set_params,
                                    };
                                    let place = self.fresh_local(
                                        &"_setprop".into(),
                                        TypeId::Void,
                                        ctx.locals,
                                    );
                                    stmts.push(MirStatement::Assign { place, rvalue });
                                } else {
                                    // Auto-property (get; set;) — the property name is
                                    // registered as a backing field (registry.rs:186),
                                    // so write directly to it. Without this branch the
                                    // assignment was silently dropped, leaving class-typed
                                    // fields as uninitialized garbage and crashing on use.
                                    let (mut val_prep, val_rv) =
                                        lower_expr::lower_expr_to_rvalue_with_binary(
                                            &value.node,
                                            self,
                                            ctx,
                                        );
                                    stmts.append(&mut val_prep);
                                    // Class → interface field assignment (`h.D = new Disp()`):
                                    // wrap in MakeIface / MakeIfaceDyn before FieldSet, matching
                                    // the local/param path. Without it the raw object pointer is
                                    // stored into the interface slot and the call site reads a
                                    // non-fat-pointer → 0xC0000005.
                                    let field_ty = ctx
                                        .registry
                                        .field_info(&recv_class_ident, field)
                                        .map(|f| TypeId::Named(f.ty.clone()));
                                    let iface_dest = field_ty
                                        .as_ref()
                                        .and_then(|ty| iface_dest_name(ty, ctx.registry))
                                        .map(|iface_name| {
                                            (class_ty_for_iface_wrap(&value.node, ctx), iface_name)
                                        });
                                    if let Some((src_ty, iface_name)) = iface_dest {
                                        let temp_id = self.fresh_local(
                                            &Ident::from("_iface_obj"),
                                            src_ty.clone(),
                                            ctx.locals,
                                        );
                                        stmts.push(MirStatement::Assign {
                                            place: temp_id,
                                            rvalue: val_rv,
                                        });
                                        if let Some(wrap) = iface_wrap_rvalue(
                                            ctx.registry,
                                            &src_ty,
                                            &iface_name,
                                            MirOperand::Local(temp_id),
                                        ) {
                                            stmts.push(MirStatement::FieldSet {
                                                object: recv_op,
                                                class: recv_class,
                                                field: field.to_string(),
                                                value: wrap,
                                            });
                                        } else {
                                            stmts.push(MirStatement::FieldSet {
                                                object: recv_op,
                                                class: recv_class,
                                                field: field.to_string(),
                                                value: MirRvalue::Use(MirOperand::Local(temp_id)),
                                            });
                                        }
                                    } else {
                                        stmts.push(MirStatement::FieldSet {
                                            object: recv_op,
                                            class: recv_class,
                                            field: field.to_string(),
                                            value: val_rv,
                                        });
                                    }
                                }
                            }
                        }
                    } else if let Expr::Index { receiver, index } = &target.node {
                        let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
                        // C# 索引器写：`obj[i]=v` → MethodCall set_Item，codegen 内联为 rt_*。
                        if let Some(ix) = lower_type::resolve_indexer(&recv_class, &index.node, ctx)
                        {
                            let set_method = ix.set.unwrap_or_else(|| {
                                panic!(
                                    "MIR lower: write to read-only indexer `{recv_class}` \
                                     (get_Item declared without set_Item)"
                                )
                            });
                            // receiver/index 均须经 lower_arg_operand 物化：嵌套索引
                            // （`m[k][i] = v` 的 `m[k]` 是 get_Item 调用，rvalue 不可
                            // 作 operand）与 `i*2` 类索引表达式都要求 prep 语句落地。
                            // 与下方原生数组 `T[]` 分支同一纪律。
                            let (mut recv_prep, recv_op) =
                                lower_call::lower_arg_operand(self, &receiver.node, ctx);
                            stmts.append(&mut recv_prep);
                            let (mut idx_prep, idx_op) =
                                lower_call::lower_arg_operand(self, &index.node, ctx);
                            stmts.append(&mut idx_prep);
                            let (mut val_prep, val_op) =
                                lower_call::lower_arg_operand(self, &value.node, ctx);
                            stmts.append(&mut val_prep);
                            let rvalue = MirRvalue::MethodCall {
                                receiver: recv_op,
                                method: set_method.into(),
                                args: vec![idx_op, val_op],
                                receiver_type: recv_class.clone(),
                                impl_class: Some(recv_class.clone()),
                                target_fn: Some(format!("{recv_class}::{set_method}")),
                                is_virtual: false,
                                params: vec![],
                            };
                            let place =
                                self.fresh_local(&"_setidx".into(), TypeId::Void, ctx.locals);
                            stmts.push(MirStatement::Assign { place, rvalue });
                        } else {
                            // 原生 `T[]`：`arr[i]=v` → IndexSet（GEP+store）。
                            let (mut prep, arr_op) =
                                lower_call::lower_arg_operand(self, &receiver.node, ctx);
                            let (prep_i, idx_op) =
                                lower_call::lower_arg_operand(self, &index.node, ctx);
                            prep.extend(prep_i);
                            let (mut val_prep, val_rv) =
                                lower_expr::lower_expr_to_rvalue_with_binary(
                                    &value.node,
                                    self,
                                    ctx,
                                );
                            prep.append(&mut val_prep);
                            stmts.append(&mut prep);
                            stmts.push(MirStatement::IndexSet {
                                array: arr_op,
                                index: idx_op,
                                elem_type: lower_type::index_elem_type_non_indexer(receiver, ctx),
                                value: val_rv,
                            });
                        }
                    } else if let Expr::NullCond { access } = &target.node {
                        // RFC 008：`recv?.member = value`
                        if let Expr::Field { receiver, field } = &access.node {
                            self.lower_null_cond_field_assign(
                                &receiver.node,
                                field,
                                &value.node,
                                ctx,
                                &mut stmts,
                            );
                        }
                    }
                }
                TypedStmt::Expr(e) => self.lower_stmt_expr(e, ctx, &mut stmts),
                TypedStmt::Break => stmts.push(MirStatement::Break),
                TypedStmt::Continue => stmts.push(MirStatement::Continue),
                TypedStmt::Throw { expr } => {
                    let (mut prep, value) =
                        lower_expr::lower_expr_to_rvalue_with_binary(&expr.node, self, ctx);
                    stmts.append(&mut prep);
                    stmts.push(MirStatement::Throw { value });
                }
                TypedStmt::TryCatch {
                    try_body,
                    catch_ty,
                    catch_name,
                    when_cond,
                    catch_body,
                    finally,
                } => {
                    let catch_var = self.fresh_local(catch_name, catch_ty.clone(), ctx.locals);
                    // catch 变量仅在 catch 块内可见（typeck 同款作用域）。
                    ctx.push_scope();
                    ctx.bind(catch_name, catch_var);
                    let mut catch_stmts = self.lower_typed_block(catch_body, ctx);
                    if let Some(w) = when_cond {
                        catch_stmts = self.wrap_catch_when(w, catch_var, catch_stmts, ctx);
                    }
                    ctx.pop_scope();
                    let try_catch = MirStatement::TryCatch {
                        try_body: self.lower_typed_block(try_body, ctx),
                        catch_var,
                        catch_ty: catch_ty.clone(),
                        catch_body: catch_stmts,
                    };
                    if let Some(f) = finally {
                        stmts.push(MirStatement::TryFinally {
                            body: vec![try_catch],
                            finally: self.lower_typed_block(f, ctx),
                        });
                    } else {
                        stmts.push(try_catch);
                    }
                }
                TypedStmt::TryFinally { body, finally } => {
                    stmts.push(MirStatement::TryFinally {
                        body: self.lower_typed_block(body, ctx),
                        finally: self.lower_typed_block(finally, ctx),
                    });
                }
                TypedStmt::Using {
                    name,
                    ty,
                    init,
                    body,
                } => {
                    self.lower_let(name, ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let body_stmts = self.lower_typed_block(body, ctx);
                    let dispose_stmt = self.build_dispose_call(resource_local, ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                }
                TypedStmt::UsingVar { name, ty, init } => {
                    self.lower_let(name, ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let rest = TypedBlock {
                        stmts: block.stmts[idx + 1..].to_vec(),
                        tail: block.tail.clone(),
                    };
                    let body_stmts = self.lower_typed_block(&rest, ctx);
                    let dispose_stmt = self.build_dispose_call(resource_local, ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                    return stmts;
                }
                TypedStmt::AwaitUsing {
                    name,
                    ty,
                    init,
                    body,
                } => {
                    self.lower_let(name, ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let body_stmts = self.lower_typed_block(body, ctx);
                    let dispose_stmt = self.build_dispose_async_await(resource_local, ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                }
                TypedStmt::AwaitUsingVar { name, ty, init } => {
                    self.lower_let(name, ty.clone(), Some(init), ctx, &mut stmts);
                    let resource_local = ctx.lookup(name).unwrap();
                    let rest = TypedBlock {
                        stmts: block.stmts[idx + 1..].to_vec(),
                        tail: block.tail.clone(),
                    };
                    let body_stmts = self.lower_typed_block(&rest, ctx);
                    let dispose_stmt = self.build_dispose_async_await(resource_local, ty, ctx);
                    stmts.push(MirStatement::TryFinally {
                        body: body_stmts,
                        finally: vec![dispose_stmt],
                    });
                    return stmts;
                }
            }
        }
        stmts
    }

    fn lower_let(
        &mut self,
        name: &Ident,
        local_ty: TypeId,
        init: Option<&Spanned<Expr>>,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let id = self.fresh_local(name, local_ty.clone(), ctx.locals);
        ctx.bind(name, id);
        if let Some(init) = init {
            if let Expr::Await(inner) = &init.node {
                let (mut prep, task) =
                    lower_expr::lower_expr_to_rvalue_with_binary(&inner.node, self, ctx);
                stmts.append(&mut prep);
                stmts.push(MirStatement::Await { place: id, task });
                return;
            }
            if let Some(chain) = lower_linq::try_lower_linq_chain(&init.node, ctx) {
                // Materialize the LINQ chain into a fresh `List<T>` local
                // (where T comes from `local_ty`). Only `List_<T>` results
                // are supported; other result types (IEnumerable<T>) fall
                // through to the stub rvalue, which remains a known gap.
                if matches!(&local_ty, TypeId::Named(n) if n.starts_with("List_")) {
                    self.materialize_linq_chain_to_list(chain, id, &local_ty, ctx, stmts);
                    return;
                }
                stmts.push(MirStatement::Assign {
                    place: id,
                    rvalue: MirRvalue::LinqChain(chain),
                });
                return;
            }
            if let Expr::Query(q) = &init.node {
                let chain = lower_linq::lower_query(q, ctx);
                if matches!(&local_ty, TypeId::Named(n) if n.starts_with("List_")) {
                    self.materialize_linq_chain_to_list(chain, id, &local_ty, ctx, stmts);
                    return;
                }
                stmts.push(MirStatement::Assign {
                    place: id,
                    rvalue: MirRvalue::LinqChain(chain),
                });
                return;
            }
            if let TypeId::Func { params, ret } = &local_ty {
                if let Expr::Lambda(l) = &init.node {
                    let lambda_name = format!("__lambda_{}", self.next_lambda);
                    self.next_lambda += 1;
                    let param_pairs: Vec<(Ident, TypeId)> = l
                        .params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let pty =
                                p.ty.as_ref()
                                    .map(|t| lower_type::lower_type_name(&t.node))
                                    .unwrap_or_else(|| params[i].clone());
                            (p.name.clone(), pty)
                        })
                        .collect();
                    let fn_sigs = ctx.fn_sigs;
                    let registry = ctx.registry;
                    let layouts = ctx.layouts;
                    // RFC 008: compute captures here (see `lower_lambda_to_fnptr`).
                    let LambdaCaptureAnalysis {
                        captures,
                        refs_owner_static,
                    } = Self::compute_captures(l, ctx);
                    let owner = ctx.owner.clone();
                    let class_fields = ctx.class_fields;
                    let host_linkage = ctx.host_linkage;
                    let body = self.lower_lambda_to_body(
                        l,
                        &param_pairs,
                        ret,
                        fn_sigs,
                        registry,
                        layouts,
                        &captures,
                        refs_owner_static,
                        owner,
                        class_fields,
                        host_linkage,
                        ctx.type_sizes,
                        ctx.expr_types,
                    );
                    self.lifted.push((lambda_name.clone(), body));
                    // RFC 008: captured lambdas are stored as `Closure` operands
                    // (arc_closure { fn_ptr, env_ptr }); no-capture lambdas keep
                    // the bare `FnPtr` representation for zero overhead.
                    let rvalue = if captures.is_empty() {
                        MirRvalue::FnPtr { name: lambda_name }
                    } else {
                        let env: Vec<(LambdaCapture, MirOperand)> = captures
                            .iter()
                            .map(|c| {
                                let src = ctx
                                    .lookup(&c.name)
                                    .map(MirOperand::Local)
                                    .unwrap_or(MirOperand::ConstNull);
                                (c.clone(), src)
                            })
                            .collect();
                        MirRvalue::Use(MirOperand::Closure {
                            fn_name: lambda_name,
                            env,
                        })
                    };
                    stmts.push(MirStatement::Assign { place: id, rvalue });
                    return;
                }
            }
            let (prep, rv) = if matches!(local_ty, TypeId::Expression { .. })
                && matches!(init.node, Expr::Lambda(_))
            {
                if let Expr::Lambda(l) = &init.node {
                    // Collect visible outer-scope variables as captures (name, local_id, ty).
                    let captures: Vec<(Ident, i32, SmolStr)> = ctx
                        .scopes
                        .iter()
                        .flat_map(|s| s.iter())
                        .map(|(name, lid)| {
                            let ty = ctx
                                .locals
                                .get(lid)
                                .map(|(_, ty)| lower_type::type_id_name(ty))
                                .unwrap_or_else(|| "unknown".into());
                            (name.clone(), lid.0 as i32, ty)
                        })
                        .collect();
                    // 定位公理：树化失败须硬错误，禁止静默 Constant(true)。
                    let mut tree = ExpressionTree::from_lambda(l, &captures).unwrap_or_else(|| {
                        panic!(
                            "MIR lower: ExpressionTree::from_lambda failed \
                             (silent Constant(true) fallback is forbidden)"
                        )
                    });
                    lower_type::annotate_expression_tree(&mut tree, &local_ty, ctx);
                    (
                        vec![],
                        MirRvalue::ExpressionTreeConst {
                            name: "rt_expr_tree_summary_0".into(),
                            tree,
                        },
                    )
                } else {
                    lower_expr::lower_expr_to_rvalue_with_binary(&init.node, self, ctx)
                }
            } else if let Expr::CollectionExpr { elements } = &init.node {
                let has_spread = elements.iter().any(|e| e.is_spread());
                if !has_spread {
                    ctx.array_lengths.insert(id, elements.len());
                }
                let (prep_c, mut rv) =
                    lower_expr::lower_expr_to_rvalue_with_binary(&init.node, self, ctx);
                // Let 声明类型为 T[] 时覆写 ArrayLit.elem_type 为完整数组类型
                //（与 lower_collection 的 collection_array_type 约定一致；
                // 便于 Task[] 等 facade 的 emit_rvalue_typed expected）。
                if let (TypeId::Array { .. }, MirRvalue::ArrayLit { elem_type, .. }) =
                    (&local_ty, &mut rv)
                {
                    *elem_type = local_ty.clone();
                }
                (prep_c, rv)
            } else {
                lower_expr::lower_expr_to_rvalue_with_binary(&init.node, self, ctx)
            };
            stmts.extend(prep);
            // Class → interface assignment: wrap in MakeIface / MakeIfaceDyn.
            let iface_dest = iface_dest_name(&local_ty, ctx.registry)
                .map(|iface_name| (class_ty_for_iface_wrap(&init.node, ctx), iface_name));
            if let Some((src_ty, iface_name)) = iface_dest {
                let src_ty = normalize_iface_type_id(&src_ty);
                let temp_id =
                    self.fresh_local(&Ident::from("_iface_obj"), src_ty.clone(), ctx.locals);
                stmts.push(MirStatement::Assign {
                    place: temp_id,
                    rvalue: rv,
                });
                if let Some(wrap) = iface_wrap_rvalue(
                    ctx.registry,
                    &src_ty,
                    &iface_name,
                    MirOperand::Local(temp_id),
                ) {
                    stmts.push(MirStatement::Assign {
                        place: id,
                        rvalue: wrap,
                    });
                } else {
                    stmts.push(MirStatement::Assign {
                        place: id,
                        rvalue: MirRvalue::Use(MirOperand::Local(temp_id)),
                    });
                }
            } else {
                stmts.push(MirStatement::Assign {
                    place: id,
                    rvalue: rv,
                });
            }
        }
    }

    /// RFC 008：`P?.A = B` → 物化 `P` 一次；`if (P != null) { P.A = B }`（`B` 仅 then）。
    fn lower_null_cond_field_assign(
        &mut self,
        receiver: &Expr,
        field: &Ident,
        value: &Expr,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let recv_ty = lower_type::infer_type_from_expr(receiver, ctx);
        let recv_tmp = self.fresh_local(&"_nca_recv".into(), recv_ty.clone(), ctx.locals);
        let (mut prep, recv_rv) = lower_expr::lower_expr_to_rvalue_with_binary(receiver, self, ctx);
        stmts.append(&mut prep);
        stmts.push(MirStatement::Assign {
            place: recv_tmp,
            rvalue: recv_rv,
        });

        let nn = self.fresh_local(&"_nca_nn".into(), TypeId::Bool, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: nn,
            rvalue: MirRvalue::Binary {
                op: BinOp::NotEq,
                left: MirOperand::Local(recv_tmp),
                right: MirOperand::ConstNull,
            },
        });

        let recv_class = lower_type::type_id_name(&recv_ty).to_string();
        let mut then_body = Vec::new();
        if is_custom_accessor_property(ctx.registry, &recv_class, field) {
            let (mut val_prep, val_op) = lower_call::lower_arg_operand(self, value, ctx);
            then_body.append(&mut val_prep);
            let setter = format!("set_{field}");
            let set_params =
                vec![lower_type::type_name_from_operand(&val_op, value, ctx).to_string()];
            let (impl_class, target_fn) = resolve_method_target(
                ctx.registry,
                &recv_class.clone().into(),
                &setter.clone().into(),
                ctx.owner.clone(),
            );
            let is_virtual = is_virtual_member(ctx.layouts, &recv_class, &setter, &set_params);
            let rvalue = MirRvalue::MethodCall {
                receiver: MirOperand::Local(recv_tmp),
                method: setter,
                args: vec![val_op],
                receiver_type: recv_class,
                impl_class,
                target_fn,
                is_virtual,
                params: set_params,
            };
            let place = self.fresh_local(&"_setprop".into(), TypeId::Void, ctx.locals);
            then_body.push(MirStatement::Assign { place, rvalue });
        } else {
            let (mut val_prep, val_rv) =
                lower_expr::lower_expr_to_rvalue_with_binary(value, self, ctx);
            then_body.append(&mut val_prep);
            then_body.push(MirStatement::FieldSet {
                object: MirOperand::Local(recv_tmp),
                class: recv_class,
                field: field.to_string(),
                value: val_rv,
            });
        }

        stmts.push(MirStatement::If {
            cond: MirOperand::Local(nn),
            then_body,
            else_body: vec![],
        });
    }

    /// P1-B2：`catch (T e) when (cond)` → `if (cond) { catch_body } else { throw e }`。
    ///
    /// when 为 false 时 rethrow；若外层有同条 try 的 `finally`（已降为 `TryFinally` 包裹），
    /// codegen 的 `emit_finally_chain` 会在 throw 前执行 finally（C# 语义）。
    fn wrap_catch_when(
        &mut self,
        when: &Spanned<Expr>,
        catch_var: LocalId,
        catch_body: Vec<MirStatement>,
        ctx: &mut LowerCtx,
    ) -> Vec<MirStatement> {
        let (prep, cond_op) = lower_expr::lower_cond(self, &when.node, ctx);
        let mut out = prep;
        out.push(MirStatement::If {
            cond: cond_op,
            then_body: catch_body,
            else_body: vec![MirStatement::Throw {
                value: MirRvalue::Use(MirOperand::Local(catch_var)),
            }],
        });
        out
    }

    /// 构造 `resource.Dispose()` 方法调用语句（用于 using 语句的 finally 块）。
    fn build_dispose_call(
        &mut self,
        resource_local: LocalId,
        local_ty: &TypeId,
        ctx: &mut LowerCtx,
    ) -> MirStatement {
        let class_name = match local_ty {
            TypeId::Named(n) => n.to_string(),
            _ => String::new(),
        };
        let (impl_class, target_fn) = resolve_method_target(
            ctx.registry,
            &class_name.clone().into(),
            &"Dispose".into(),
            ctx.owner.clone(),
        );
        let is_virtual = is_virtual_member(ctx.layouts, &class_name, "Dispose", &[]);
        let dispose_rvalue = MirRvalue::MethodCall {
            receiver: MirOperand::Local(resource_local),
            method: "Dispose".to_string(),
            args: vec![],
            receiver_type: class_name,
            impl_class,
            target_fn,
            is_virtual,
            params: vec![],
        };
        let dispose_place = self.fresh_local(&"_dispose".into(), TypeId::Void, ctx.locals);
        MirStatement::Assign {
            place: dispose_place,
            rvalue: dispose_rvalue,
        }
    }

    /// 构造 `await r.DisposeAsync()` 语句（用于 await using 语句的 finally 块）。
    fn build_dispose_async_await(
        &mut self,
        resource_local: LocalId,
        local_ty: &TypeId,
        ctx: &mut LowerCtx,
    ) -> MirStatement {
        let class_name = match local_ty {
            TypeId::Named(n) => n.to_string(),
            _ => String::new(),
        };
        let (impl_class, target_fn) = resolve_method_target(
            ctx.registry,
            &class_name.clone().into(),
            &"DisposeAsync".into(),
            ctx.owner.clone(),
        );
        let is_virtual = is_virtual_member(ctx.layouts, &class_name, "DisposeAsync", &[]);
        let dispose_rvalue = MirRvalue::MethodCall {
            receiver: MirOperand::Local(resource_local),
            method: "DisposeAsync".to_string(),
            args: vec![],
            receiver_type: class_name,
            impl_class,
            target_fn,
            is_virtual,
            params: vec![],
        };
        let await_place = self.fresh_local(&"_dispose_async".into(), TypeId::Void, ctx.locals);
        MirStatement::Await {
            place: await_place,
            task: dispose_rvalue,
        }
    }

    /// Lower a loop body (untyped) inside a loop-capture scope. Names bound
    /// within get the per-iteration capture semantics (see `LowerCtx`).
    pub(super) fn lower_loop_block(
        &mut self,
        block: &Block,
        ctx: &mut LowerCtx,
    ) -> Vec<MirStatement> {
        ctx.enter_loop_body();
        let stmts = self.lower_block(block, ctx);
        ctx.exit_loop_body();
        stmts
    }

    /// Lower a loop body (typed) inside a loop-capture scope.
    pub(super) fn lower_loop_typed_block(
        &mut self,
        block: &TypedBlock,
        ctx: &mut LowerCtx,
    ) -> Vec<MirStatement> {
        ctx.enter_loop_body();
        let stmts = self.lower_typed_block(block, ctx);
        ctx.exit_loop_body();
        stmts
    }

    /// Desugar `while (cond) body` so condition prep (And/Or short-circuit, nested
    /// Binary, MethodCall) re-runs each iteration. Silent ConstInt(0) is forbidden.
    pub(super) fn lower_while_with_cond(
        &mut self,
        cond: &Expr,
        body: Vec<MirStatement>,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        let flag = self.fresh_local(&"_wflag".into(), TypeId::Bool, ctx.locals);
        stmts.push(MirStatement::Assign {
            place: flag,
            rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
        });
        let (cond_prep, cond_op) = lower_expr::lower_cond(self, cond, ctx);
        let mut loop_body = cond_prep;
        loop_body.push(MirStatement::If {
            cond: cond_op,
            then_body: body,
            else_body: vec![MirStatement::Assign {
                place: flag,
                rvalue: MirRvalue::Use(MirOperand::ConstBool(false)),
            }],
        });
        stmts.push(MirStatement::While {
            cond: MirRvalue::Use(MirOperand::Local(flag)),
            body: loop_body,
            foreach_source: None,
        });
    }

    fn lower_stmt_expr(
        &mut self,
        e: &Spanned<Expr>,
        ctx: &mut LowerCtx,
        stmts: &mut Vec<MirStatement>,
    ) {
        match &e.node {
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let (prep, cond_op) = lower_expr::lower_cond(self, &cond.node, ctx);
                stmts.extend(prep);
                stmts.push(MirStatement::If {
                    cond: cond_op,
                    then_body: self.lower_block(then_branch, ctx),
                    else_body: else_branch
                        .as_ref()
                        .map(|b| self.lower_block(b, ctx))
                        .unwrap_or_default(),
                });
            }
            Expr::Switch(s) => {
                stmts.extend(lower_match::lower_switch(self, s, ctx));
            }
            Expr::SwitchForm(s) => {
                let (prep, _result) = lower_match::lower_switch_form(self, s, ctx);
                stmts.extend(prep);
            }
            Expr::Call {
                func,
                args,
                type_args,
                params_span,
            } => {
                // 非 Ident 被调方（实例委托字段 `f.Callback(...)` / 显式 `this.f(...)`）：
                // 须走 IndirectCall，禁止静默丢弃或被 mangle 成自由函数 Call。
                // 与表达式级路径（lower_expr.rs `Expr::Call` 分支）对称，
                // 见 lower_call::try_lower_delegate_invoke。
                if !matches!(func.node, Expr::Ident(_)) {
                    if let Some((mut dprep, drv, ret_ty)) =
                        lower_call::try_lower_delegate_invoke(self, func, args, ctx)
                    {
                        stmts.append(&mut dprep);
                        stmts.push(MirStatement::Assign {
                            place: self.fresh_local(&"_tmp".into(), ret_ty, ctx.locals),
                            rvalue: drv,
                        });
                        return;
                    }
                }
                if let Expr::Ident(fname) = &func.node {
                    if let Some(local_id) = ctx.lookup(fname) {
                        // RFC 037 M1: 局部变量持有委托时需间接调用。
                        // typeck 限制 #1：类方法级泛型参数未 push 到 type_param_scope，
                        // 导致 `Func<T, T, bool>` 等被 mangle 为 `Named("Func_T_T_bool")`
                        // 而非 `TypeId::Func { .. }`。识别 mangled 委托名以路由 IndirectCall。
                        let delegate_ty = ctx
                            .locals
                            .get(&local_id)
                            .map(|(_, ty)| ty.clone())
                            .filter(lower_type::is_delegate_type);
                        if let Some(delegate_ty) = delegate_ty {
                            // RFC 039：委托形参为接口时，class 实参须包装为接口胖指针
                            //（如 `configure(this._services)` → Action<IServiceCollection>）。
                            let params =
                                lower_type::delegate_params_of(&delegate_ty, args.len(), &|s| {
                                    ctx.registry.types.contains_key(s)
                                });
                            let mut call_args = Vec::with_capacity(args.len());
                            for (i, a) in args.iter().enumerate() {
                                let (mut p, op) = lower_call::lower_arg_operand(self, &a.node, ctx);
                                stmts.append(&mut p);
                                let op = if let Some(pt) = params.as_ref().and_then(|ps| ps.get(i))
                                {
                                    let arg_ty =
                                        lower_type::type_name_from_operand(&op, &a.node, ctx);
                                    lower_call::maybe_box_iface(op, &arg_ty, pt, ctx)
                                } else {
                                    op
                                };
                                call_args.push(op);
                            }
                            // 结果临时按委托返回类型建（Void 默认会把 `Func<IDisposable>`
                            // 调用结果存 i32 → 指针截断，见 try_lower_delegate_invoke 注释）。
                            let ret_ty = lower_type::delegate_return_type(&delegate_ty, &|s| {
                                ctx.registry.types.contains_key(s)
                            })
                            .unwrap_or_else(|| TypeId::Named("object".into()));
                            stmts.push(MirStatement::Assign {
                                place: self.fresh_local(&"_tmp".into(), ret_ty, ctx.locals),
                                rvalue: MirRvalue::IndirectCall {
                                    func: MirOperand::Local(local_id),
                                    args: call_args,
                                },
                            });
                            return;
                        }
                    }
                    // 实例委托字段（`_f(x)` 裸调用）：须 IndirectCall，禁止
                    // 自由函数 `Call { func: "_f" }`（链接失败 / 半物化 AV）。
                    // 在自由函数回退前拦截，见 lower_call::try_lower_delegate_invoke。
                    if let Some((mut dprep, drv, ret_ty)) =
                        lower_call::try_lower_delegate_invoke(self, func, args, ctx)
                    {
                        stmts.append(&mut dprep);
                        stmts.push(MirStatement::Assign {
                            place: self.fresh_local(&"_tmp".into(), ret_ty, ctx.locals),
                            rvalue: drv,
                        });
                        return;
                    }
                    // 裸实例方法调用（`_bump()` → `this._bump()`）：C# 允许实例
                    // 方法内省略 `this.`。若名字非自由函数、未被局部遮蔽，且当前类
                    // 存在 arity 匹配的实例方法，重写为 MethodCall 走语句级分派
                    //（与 lower_expr.rs 表达式级路径对称，target_fn 须为
                    // `Owner::Method` 而非裸名）。
                    if ctx.fn_sigs.get(fname.as_str()).is_none() && ctx.lookup(fname).is_none() {
                        if let Some(owner) = ctx.owner.clone() {
                            if lower_call::mir_has_instance_method(
                                ctx.registry,
                                &owner,
                                fname,
                                args.len(),
                            ) {
                                let mc = Expr::MethodCall {
                                    receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                                    method: fname.clone(),
                                    args: args.to_vec(),
                                    type_args: type_args.clone(),
                                    params_span: None,
                                };
                                let sp = Spanned::new(mc, e.span);
                                self.lower_stmt_expr(&sp, ctx, stmts);
                                return;
                            }
                        }
                    }
                    // 语句级自由函数/静态调用须与表达式级路径（lower_expr.rs
                    // `Expr::Call` 分支）对称地解析静态方法限定名（`Owner::Method`），
                    // 否则树剪枝（`filter_reachable_mir_fns` 的 `name_to_id`）看不到
                    // 该调用边，被调函数被剪掉而调用点仍引用 → LLVM undefined symbol
                    // （如 `PointerRouter.RegisterSlot` 裸名 `"RegisterSlot"`）。
                    let func_name = if !type_args.is_empty() {
                        lower_type::resolve_instantiated_type_name_from_args(fname, type_args)
                    } else if !ctx.fn_sigs.contains_key(fname.as_str()) {
                        lower_call::resolve_class_static_method(fname, args, ctx)
                            .unwrap_or_else(|| fname.to_string())
                    } else {
                        fname.to_string()
                    };
                    let (arg_prep, call_args) =
                        lower_call::lower_call_args(self, fname, args, params_span.as_ref(), ctx);
                    stmts.extend(arg_prep);
                    stmts.push(MirStatement::Assign {
                        place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                        rvalue: MirRvalue::Call {
                            func: func_name,
                            args: call_args,
                        },
                    });
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                type_args,
                params_span,
            } => {
                // RFC 005：语句形式 `span.CopyTo(dst)` / `span.Slice(...)` 等须走
                // 专用 rvalue（与表达式路径一致）；不可落到空 stub MethodCall。
                {
                    let recv_ty = lower_type::infer_type_from_spanned(receiver, ctx);
                    if matches!(recv_ty, TypeId::Span { .. })
                        && matches!(
                            method.as_str(),
                            "CopyTo"
                                | "TryCopyTo"
                                | "ToArray"
                                | "Slice"
                                | "Fill"
                                | "Clear"
                                | "AsReadOnly"
                                | "AsSpan"
                                | "AsReadOnlySpan"
                        )
                    {
                        let (mut prep, rv) =
                            lower_expr::lower_expr_to_rvalue_with_binary(&e.node, self, ctx);
                        stmts.append(&mut prep);
                        stmts.push(MirStatement::Assign {
                            place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                            rvalue: rv,
                        });
                        return;
                    }
                }
                if let Some((chain, kind)) = lower_linq::try_parse_linq_terminal(
                    &Expr::MethodCall {
                        receiver: receiver.clone(),
                        method: method.clone(),
                        args: args.clone(),
                        type_args: type_args.clone(),
                        params_span: params_span.clone(),
                    },
                    ctx,
                ) {
                    if let Some((prep, _local)) = self.lower_linq_terminal(kind, chain, ctx) {
                        stmts.extend(prep);
                        return;
                    }
                }
                if let Some(func) = lower_linq::builtin_static_method(&receiver.node, method) {
                    let (arg_prep, call_args): (Vec<_>, Vec<_>) = args
                        .iter()
                        .map(|a| lower_call::lower_arg_operand(self, &a.node, ctx))
                        .unzip();
                    for mut prep in arg_prep {
                        stmts.append(&mut prep);
                    }
                    stmts.push(MirStatement::Assign {
                        place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                        rvalue: MirRvalue::Call {
                            func,
                            args: call_args,
                        },
                    });
                } else if let Some((func, params)) = {
                    let stripped: Vec<ast::Type> =
                        type_args.iter().map(|t| t.node.clone()).collect();
                    lower_call::user_type_static_method_sig(
                        &receiver.node,
                        method,
                        &stripped,
                        args,
                        ctx,
                    )
                } {
                    // RFC 004 M2：用户类型静态方法调用（如 `Vector2.Add(a, b)`）。
                    // 静态方法无 `this` 参数，降级为 `MirRvalue::Call`（无 receiver），
                    // codegen `mangle_fn_name` 将 `Vector2::Add` mangle 为 `@Vector2_Add`。
                    // 泛型实参时名为 `Class::Method__T`，由 try_create_mono_body 克隆。
                    // RFC 039：静态方法接口形参须包装 class 实参为接口胖指针。
                    let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
                    for (i, a) in args.iter().enumerate() {
                        let (mut p, op) = lower_call::lower_arg_operand(self, &a.node, ctx);
                        stmts.append(&mut p);
                        let arg_ty = lower_type::type_name_from_operand(&op, &a.node, ctx);
                        let op = if let Some(pt) = params.get(i) {
                            lower_call::maybe_box_iface(
                                op,
                                &arg_ty,
                                &TypeId::Named(pt.clone()),
                                ctx,
                            )
                        } else {
                            op
                        };
                        call_args.push(op);
                    }
                    stmts.push(MirStatement::Assign {
                        place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                        rvalue: MirRvalue::Call {
                            func,
                            args: call_args,
                        },
                    });
                } else {
                    // Use `lower_arg_operand` (not `operand_from_expr`) for the
                    // receiver so that chained calls (e.g.
                    // `sb.Append("a").Append("b")`) materialize the inner
                    // MethodCall to a temp local instead of collapsing to
                    // `ConstInt(0)`.
                    let (mut recv_prep, recv) =
                        lower_call::lower_arg_operand(self, &receiver.node, ctx);
                    // C#：`d.Invoke(args)` ≡ `d(args)`。typeck 通常已改写；此处兜底。
                    if method.as_str() == "Invoke" {
                        let recv_ty_id = match &recv {
                            MirOperand::Local(id) => ctx.locals.get(id).map(|(_, ty)| ty.clone()),
                            _ => None,
                        };
                        if recv_ty_id
                            .as_ref()
                            .is_some_and(lower_type::is_delegate_type)
                        {
                            let (arg_prep, call_args): (Vec<_>, Vec<_>) = args
                                .iter()
                                .map(|a| lower_call::lower_arg_operand(self, &a.node, ctx))
                                .unzip();
                            stmts.append(&mut recv_prep);
                            for mut prep in arg_prep {
                                stmts.append(&mut prep);
                            }
                            stmts.push(MirStatement::Assign {
                                place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                                rvalue: MirRvalue::IndirectCall {
                                    func: recv,
                                    args: call_args,
                                },
                            });
                            return;
                        }
                    }
                    let recv_ty = lower_type::type_name_from_operand(&recv, &receiver.node, ctx);
                    // Use `method_call_rvalue_with_prep` so that complex argument
                    // expressions (e.g. `people.Add(new Person() { ... })`) are
                    // materialized to temp locals with their prep statements
                    // preserved. The previous `method_call_rvalue` + redundant
                    // `lower_arg_operand` pattern materialized args but discarded
                    // the resulting locals, so the rvalue used `ConstInt(0)`.
                    let stripped_type_args: Vec<ast::Type> =
                        type_args.iter().map(|t| t.node.clone()).collect();
                    let (mut arg_prep, rvalue) = lower_call::method_call_rvalue_with_prep(
                        self,
                        receiver,
                        method,
                        args,
                        &stripped_type_args,
                        params_span.as_ref(),
                        ctx,
                        recv,
                        &recv_ty,
                    );
                    stmts.append(&mut recv_prep);
                    stmts.append(&mut arg_prep);
                    stmts.push(MirStatement::Assign {
                        place: self.fresh_local(&"_tmp".into(), TypeId::Void, ctx.locals),
                        rvalue,
                    });
                }
            }
            // RFC 009 M4：bare `await foo();` 语句（非 `let x = await foo()`）。
            // 此前 _ => {} 静默丢弃 Expr::Await，导致 __async_main 被编译为同步函数，
            // 所有的 async 测试都因错误原因通过（没有真实异步行为被测试）。
            // 在此显式发射 MirStatement::Await，使状态机正确生成。
            Expr::Await(inner) => {
                let place = self.fresh_local(&"_await".into(), TypeId::Void, ctx.locals);
                let (mut prep, task) =
                    lower_expr::lower_expr_to_rvalue_with_binary(&inner.node, self, ctx);
                stmts.append(&mut prep);
                stmts.push(MirStatement::Await { place, task });
            }
            _ => {}
        }
    }
}

impl Default for MirBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn lower_module(
    fns: &[TypedFn],
    registry: &TypeRegistry,
    expr_types: &typeck::ExprTypeTable,
) -> Vec<(String, MirCfgBody)> {
    let layouts = typeck::layouts_from_registry(registry);
    // RFC 009 M3：一次构建 TypeSizeTable，供所有 async spill 判定复用。
    let type_sizes = typeck::TypeSizeTable::build(registry);
    let mut fn_sigs: HashMap<String, (Vec<TypeId>, TypeId)> = HashMap::new();
    for f in fns {
        if f.owner.is_none() {
            fn_sigs.insert(
                f.name.to_string(),
                (
                    f.params.iter().map(|(_, t)| t.clone()).collect(),
                    f.ret.clone(),
                ),
            );
        }
    }
    let mut builder = MirBuilder::new();
    let mut result: Vec<(String, MirCfgBody)> = fns
        .iter()
        .map(|f| {
            (
                f.name.to_string(),
                builder.lower_fn(f, &fn_sigs, registry, &layouts, &type_sizes, expr_types),
            )
        })
        .collect();
    result.append(&mut builder.lifted);

    // 泛型方法单态化：扫描 MethodCall.target_fn 与 Call.func 中的 `__` 后缀
    // （表示泛型方法的具体类型实例化），创建具体参数类型的 MIR body clone。
    // 多轮 fixpoint：mono 体内可能再调用其它泛型方法。
    //
    // RFC 006「接口泛型方法分派」：同轮收集**接口接收者**的泛型方法实例化
    // （`IGetter::Get__Seed`），并按实例化为**全部实现类**单态化
    // `C::Get__Seed`（模板克隆 + 类型实参替换），供 itable 槽位引用。
    // 单态化后的封闭程序中实例化集有限且确定；嵌套泛型（`Sink<int>.Run`
    // 体内 `g.Get<T>`）在 `Sink<int>` 单态化后收敛为 `g.Get<int>`，由 fixpoint
    // 在下一轮重新收集。
    {
        let mut guard = 0;
        loop {
            let name_to_idx: HashMap<String, usize> = result
                .iter()
                .enumerate()
                .map(|(i, (name, _))| (name.clone(), i))
                .collect();
            let mut mono_bodies: Vec<(String, MirCfgBody)> = Vec::new();
            // (iface, 基础方法名, 类型实参后缀)，如 ("IGetter", "Get", "Seed")。
            let mut iface_insts: Vec<(String, String, String)> = Vec::new();
            for (_, body) in &result {
                collect_mono_targets(body, &name_to_idx, &result, registry, &mut mono_bodies);
                collect_iface_instantiations_in_body(body, registry, &mut iface_insts);
            }
            // RFC 006：全实现者单态化——为每个实例化键生成接口全部实现类的 mono body。
            for (iface, method_name, suffix) in &iface_insts {
                generate_iface_instantiation_monos(
                    iface,
                    method_name,
                    suffix,
                    &name_to_idx,
                    &result,
                    registry,
                    &layouts,
                    &mut mono_bodies,
                );
            }
            let before = result.len();
            for (name, body) in mono_bodies {
                if !name_to_idx.contains_key(name.as_str()) {
                    result.push((name, body));
                }
            }
            if result.len() == before {
                break;
            }
            guard += 1;
            if guard > 64 {
                panic!(
                    "MIR lower: generic method monomorphization exceeded fixpoint limit; \
                     possible cyclic generic method expansion"
                );
            }
        }
        reject_missing_generic_method_monos(&result, registry);
    }

    // 泛型类构造函数单态化：扫描所有 New rvalue 中引用的泛型类单态化实例，
    // 从泛型模板的构造函数 body 克隆并替换类型参数。
    // 需要此步骤是因为 typeck 在泛型方法体内看到 Signal<T>（含占位符 T）
    // 时仅注册 stub，不会触发完整单态化；MIR lower 需在克隆泛型方法体后
    // 补全被引用泛型类的构造函数 body。
    //
    // 多轮 fixpoint：生成的 ctor 体内可能还有嵌套 `new Other<U>`。
    // 若模板缺失则硬错误——禁止「关掉 mono」却让程序编过再 runtime crash。
    {
        let mut guard = 0;
        loop {
            let before = result.len();
            generate_generic_class_ctors(&mut result, registry);
            if result.len() == before {
                break;
            }
            guard += 1;
            if guard > 64 {
                panic!(
                    "MIR lower: generate_generic_class_ctors exceeded fixpoint limit; \
                     possible cyclic generic ctor expansion"
                );
            }
        }
        reject_missing_generic_ctors(&result, registry);
    }

    // 泛型类方法单态化（C2 相关）：`Element.SetValue<T>` 等泛型方法 mono clone
    // 体内引用具体泛型类方法（`Signal_double::Set`）。若 typeck 未实例化该类
    // （如程序未写 `new Signal<int>(0)` 预温），具体类方法 body 缺失——从泛型
    // 模板（`Signal_T::Set`）克隆。多轮 fixpoint：克隆出的方法体可能继续引用
    // 其它具体泛型类方法/构造函数。
    {
        let mut guard = 0;
        loop {
            let before = result.len();
            generate_generic_class_methods(&mut result, registry);
            generate_generic_class_ctors(&mut result, registry);
            // 类方法 mono 克隆体会把扩展调用名里的 `TRequest` 换成具体类型
            // （`MediatorExtensions::SendAsync_GetUserRequest_UserDto`），但扩展
            // 单态体此前只在 typeck 调用点生成——嵌套路径无调用点 → 缺 body。
            // 此处按「模板名把 generics 换成 concrete == 目标」从已有占位模板克隆。
            generate_substituted_call_monos(&mut result, registry);
            if result.len() == before {
                break;
            }
            guard += 1;
            if guard > 64 {
                panic!(
                    "MIR lower: generate_generic_class_methods exceeded fixpoint limit; \
                     possible cyclic generic class method expansion"
                );
            }
        }
        reject_missing_generic_class_methods(&result, registry);
    }

    // 泛型方法模板后处理：剔除无法独立成函数的模板。
    // 见 `drop_non_emittable_generic_templates` 的注释。
    drop_non_emittable_generic_templates(&mut result, registry);

    // 模板级联剔除：模板剔除后，其 lowering 期产生的 lifted λ（`__lambda_rt_N`）
    // 与占位单态体仍留在 result——body 内含未替换的 `__T`/`_T` 型符号引用
    //（如 `BindingRegistry_ApplyValue__T`——λ 内泛型方法调用以模板形参为实参）。
    // 这类函数只能被已剔除模板引用，无法独立链接；`--dynamic`（无入口）下
    // tree-shake 全量保留，不剔则 arc-prune-001。
    drop_placeholder_tainted(&mut result);

    result
}

/// 剔除 body 引用「占位形态符号」（含单大写原子类型实参，如 `Foo__T`/`Bar_T`，
/// 或接收者 `Enum_T`）的残留函数。具体类型实参均为已注册多字符类型名；
/// 单大写原子只可能来自未单态化的模板形参占位。模板体自身由
/// [`drop_non_emittable_generic_templates`] 先行剔除。
fn drop_placeholder_tainted(result: &mut Vec<(String, MirCfgBody)>) {
    fn has_placeholder_atom(name: &str) -> bool {
        // 跳过函数名前缀部分：仅检查实参段/类型名中的单大写原子。
        // `TextBuffer_get_LineCount` 等非泛型名无此类原子。
        name.split(['_', ':']).any(|seg| {
            seg.chars().count() == 1 && seg.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        })
    }
    fn rv_tainted(rv: &MirRvalue) -> bool {
        // 类型名位（class/receiver）：单大写原子即占位（`EnumOptions_T` 的 T）。
        // 函数名位（func/target_fn）：仅 `__` 类型实参后缀内判单大写——普通段
        // 单大写可能是**单字母属性名**（`FkCounter_set_X` 的 X，不可误判）。
        let fn_tainted = |name: &str| {
            name.rsplit_once("__")
                .is_some_and(|(_, suffix)| has_placeholder_atom(suffix))
        };
        match rv {
            MirRvalue::New { class, .. } => has_placeholder_atom(class),
            MirRvalue::Call { func, .. } => fn_tainted(func),
            MirRvalue::MethodCall {
                receiver_type,
                target_fn,
                ..
            } => {
                has_placeholder_atom(receiver_type) || target_fn.as_deref().is_some_and(fn_tainted)
            }
            MirRvalue::NullCondMethod {
                receiver_type,
                target_fn,
                ..
            }
            | MirRvalue::ForceDerefMethod {
                receiver_type,
                target_fn,
                ..
            } => {
                has_placeholder_atom(receiver_type) || target_fn.as_deref().is_some_and(fn_tainted)
            }
            _ => false,
        }
    }
    fn stmts_tainted(stmts: &[MirStatement]) -> bool {
        stmts.iter().any(|s| match s {
            MirStatement::Assign { rvalue, .. }
            | MirStatement::Return(Some(rvalue))
            | MirStatement::FieldSet { value: rvalue, .. }
            | MirStatement::StaticFieldSet { value: rvalue, .. } => rv_tainted(rvalue),
            _ => false,
        })
    }
    result.retain(|(name, body)| {
        // 非占位形态名的 fn 若 body 无占位引用则保留。
        if body.blocks.values().any(|b| stmts_tainted(&b.statements)) {
            if std::env::var("ARC_DEBUG_TEMPLATES").is_ok() {
                eprintln!("[drop_tainted] {name}");
            }
            return false;
        }
        true
    });
}

/// 收集 MIR body 中引用的具体泛型类方法目标（`Class_concrete::Method`）。
fn collect_generic_class_method_targets(rv: &MirRvalue, out: &mut HashSet<String>) {
    match rv {
        MirRvalue::MethodCall {
            target_fn: Some(t), ..
        }
        | MirRvalue::NullCondMethod {
            target_fn: Some(t), ..
        }
        | MirRvalue::ForceDerefMethod {
            target_fn: Some(t), ..
        } => {
            out.insert(t.clone());
        }
        MirRvalue::Call { func, .. } => {
            out.insert(func.clone());
        }
        _ => {}
    }
}

/// 泛型类方法 mono 之后：为「由类型参数替换得到的 Call 目标」补单态体。
///
/// 典型：`EndpointDispatcher_TReq_TResp::Dispatch` 内
/// `MediatorExtensions::SendAsync_TRequest_TResponse` 经类参数替换变为
/// `…SendAsync_GetUserRequest_UserDto`，而扩展方法模板只在 typeck 调用点
/// 实例化——嵌套路径无调用点。此处若模块内已有带 generics 后缀的模板
/// （检查泛型类模板时 typeck 以占位实参实例化留下的），则克隆并替换。
fn generate_substituted_call_monos(
    result: &mut Vec<(String, MirCfgBody)>,
    registry: &TypeRegistry,
) {
    let existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();
    let mut needed: HashSet<String> = HashSet::new();
    for (_, body) in result.iter() {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. }
                    | MirStatement::Return(Some(rvalue)) => {
                        if let MirRvalue::Call { func, .. } = rvalue {
                            needed.insert(func.clone());
                        }
                        if let MirRvalue::MethodCall {
                            target_fn: Some(tfn),
                            ..
                        } = rvalue
                        {
                            needed.insert(tfn.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut additions: Vec<(String, MirCfgBody)> = Vec::new();
    for tfn in &needed {
        if existing.contains(tfn.as_str()) || additions.iter().any(|(n, _)| n == tfn) {
            continue;
        }
        // `__` 方法级泛型仍由 try_create_mono_body 处理。
        if tfn.contains("__") {
            continue;
        }
        let Some((template_name, template_body, generics, concrete)) =
            find_substitutable_call_template(tfn, result, registry)
        else {
            continue;
        };
        let _ = template_name;
        let mut cloned = template_body.clone();
        cloned.ret = substitute_type_id(&cloned.ret, &generics, &concrete);
        for (_, ty) in &mut cloned.params {
            *ty = substitute_type_id(ty, &generics, &concrete);
        }
        for (_, (_, ty)) in &mut cloned.locals {
            *ty = substitute_type_id(ty, &generics, &concrete);
        }
        for block in cloned.blocks.values_mut() {
            for stmt in &mut block.statements {
                substitute_in_statement(stmt, &generics, &concrete);
            }
            substitute_in_terminator(&mut block.terminator, &generics, &concrete);
        }
        cloned.linkage = Linkage::LinkonceOdr;
        additions.push((tfn.clone(), cloned));
    }
    result.extend(additions);
}

/// 在 `result` 中找一个模板名 `N`，把其泛型形参换成 concrete 后恰等于 `tfn`。
///
/// 扩展方法单态名是 `mangle_base`+`_`+实参（如
/// `MediatorExtensions::SendAsync_GetUserRequest_UserDto`），**不是**
/// `Class::Method` 简单名——`get_method_generics` 查不到。此处优先扫
/// `registry.extensions` 的 `mangle_base`/`generic_params`，再回退类方法路径。
fn find_substitutable_call_template<'a>(
    tfn: &str,
    result: &'a [(String, MirCfgBody)],
    registry: &TypeRegistry,
) -> Option<(String, &'a MirCfgBody, Vec<Ident>, Vec<Ident>)> {
    for ems in registry.extensions.values() {
        for em in ems {
            if em.generic_params.is_empty() {
                continue;
            }
            let generics = em.generic_params.clone();
            let open_args: Vec<TypeId> =
                generics.iter().map(|g| TypeId::Named(g.clone())).collect();
            let open_name = mangle_generic(em.mangle_base.as_str(), &open_args);
            let Some((_, body)) = result.iter().find(|(n, _)| n == &open_name) else {
                continue;
            };
            let Some(concrete) = infer_concrete_args_from_template_mono(&open_name, tfn, &generics)
            else {
                continue;
            };
            if typeck::registry::substitute_generic_in_ty_name(&open_name, &generics, &concrete)
                != tfn
            {
                continue;
            }
            return Some((open_name, body, generics, concrete));
        }
    }

    for (name, body) in result {
        let Some(generics) = get_method_generics(registry, name) else {
            continue;
        };
        if generics.is_empty() {
            continue;
        }
        let Some(concrete) = infer_concrete_args_from_template_mono(name, tfn, &generics) else {
            continue;
        };
        if typeck::registry::substitute_generic_in_ty_name(name, &generics, &concrete) != tfn {
            continue;
        }
        return Some((name.clone(), body, generics, concrete));
    }
    None
}

/// 从模板名 / 单态名对推断 concrete 实参（要求模板名以 `_<g1>_<g2>…` 结尾）。
fn infer_concrete_args_from_template_mono(
    template: &str,
    mono: &str,
    generics: &[Ident],
) -> Option<Vec<Ident>> {
    let gen_suffix = generics
        .iter()
        .map(|g| g.as_str())
        .collect::<Vec<_>>()
        .join("_");
    let base = template.strip_suffix(&format!("_{gen_suffix}"))?;
    if base.is_empty() {
        return None;
    }
    let rest = mono.strip_prefix(base)?.strip_prefix('_')?;
    let parts: Vec<&str> = rest.split('_').collect();
    if parts.len() != generics.len() || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(parts.iter().map(|p| (*p).into()).collect())
}

/// 收集 MIR body 中引用的**类名**（如 `Signal_int`、`List_int`），供 pipeline
/// 在 `layouts_from_registry` 之前对 registry 缺失的泛型类实例强制单态化。
///
/// 背景（C2 单态化完整性）：`try_create_mono_body` / `generate_generic_class_ctors`
/// / `generate_generic_class_methods` 克隆出的 body 会引用具体泛型类（例如
/// `Element.SetValue<int>` 克隆体内的 `new Signal<int>()` / `Signal_int::Set`），
/// 而 typeck 在类型解析路径可能未实例化该类（用户源码无显式 `Signal<int>` 注解）
/// → `layouts_from_registry` 缺该类布局 → codegen 字段访问回退 `(16, "int")`
/// 产生错位与 LLVM 类型错误。本收集器枚举这些类名供 pipeline 强制实例化。
///
/// 泛型模板自身（如 `Signal_T::TrySet`）的 body 不在扫描范围——它们引用的
/// `List_Func_T_T_bool` 等参数化 stub 已由 `register_parametrized_generic_stub`
/// 注册；对这类**非具体**类强实例化会产生伪类，故跳过。
pub fn collect_concrete_class_refs(
    fns: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
    out: &mut HashSet<String>,
) {
    for (name, body) in fns {
        // 跳过泛型模板类自身的 body（类名含泛型参数且 registry 中 generic_params 非空）。
        if let Some(pos) = name.rfind("::") {
            let class = &name[..pos];
            if registry
                .types
                .get(class)
                .is_some_and(|t| !t.generic_params.is_empty())
            {
                continue;
            }
        }
        for block in body.blocks.values() {
            for stmt in &block.statements {
                collect_stmt_class_refs(stmt, out);
            }
            collect_terminator_class_refs(&block.terminator, out);
        }
    }
}

fn collect_stmt_class_refs(stmt: &MirStatement, out: &mut HashSet<String>) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_rvalue_class_refs(rvalue, out),
        MirStatement::Return(Some(rvalue)) => collect_rvalue_class_refs(rvalue, out),
        MirStatement::Return(None) => {}
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body.iter().chain(else_body) {
                collect_stmt_class_refs(s, out);
            }
        }
        MirStatement::While { body, cond, .. } => {
            collect_rvalue_class_refs(cond, out);
            for s in body {
                collect_stmt_class_refs(s, out);
            }
        }
        MirStatement::FieldSet { class, value, .. } => {
            out.insert(class.clone());
            collect_rvalue_class_refs(value, out);
        }
        MirStatement::StaticFieldSet { class, value, .. } => {
            out.insert(class.clone());
            collect_rvalue_class_refs(value, out);
        }
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_stmt_class_refs(s, out);
            }
        }
        MirStatement::Await { task, .. } => collect_rvalue_class_refs(task, out),
        MirStatement::Throw { value } => collect_rvalue_class_refs(value, out),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body.iter().chain(catch_body) {
                collect_stmt_class_refs(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body.iter().chain(finally) {
                collect_stmt_class_refs(s, out);
            }
        }
        MirStatement::Drop(_) => {}
        MirStatement::IndexSet { value, .. } => collect_rvalue_class_refs(value, out),
        MirStatement::Break | MirStatement::Continue => {}
    }
}

fn collect_terminator_class_refs(term: &MirTerminator, out: &mut HashSet<String>) {
    match term {
        MirTerminator::Return(Some(op)) | MirTerminator::Throw(op) => {
            collect_operand_class_refs(op, out);
        }
        MirTerminator::CondBr { cond, .. } => collect_operand_class_refs(cond, out),
        MirTerminator::Goto(_) | MirTerminator::Return(None) | MirTerminator::Unreachable => {}
    }
}

fn collect_operand_class_refs(op: &MirOperand, out: &mut HashSet<String>) {
    match op {
        MirOperand::Local(_)
        | MirOperand::ConstInt(_)
        | MirOperand::ConstBool(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::AddrOf(_)
        | MirOperand::ConstNull
        | MirOperand::TypeInfoPtr { .. } => {}
        MirOperand::FnPtr { .. } => {}
        MirOperand::Field { class, .. } => {
            out.insert(class.clone());
        }
        MirOperand::Iface { class, iface, .. } => {
            out.insert(class.clone());
            out.insert(iface.clone());
        }
        MirOperand::UnboxIface { class, .. } => {
            out.insert(class.clone());
        }
        MirOperand::UnboxString { object: _ } => {}
        MirOperand::UnboxGeneric { object, type_name } => {
            collect_operand_class_refs(object, out);
            // `(T)obj` 单态化后 type_name 可能为具体类/泛型实例名，
            // 收集以供泛型类强制实例化（与 TypeId/ConstDefault 分支同语义）。
            out.insert(type_name.clone());
        }
        MirOperand::StaticField { class, .. } => {
            out.insert(class.clone());
        }
        MirOperand::Closure { .. } => {}
        MirOperand::TypeId { type_name } => {
            out.insert(type_name.clone());
        }
        MirOperand::ConstDefault { type_name } => {
            // `default(T)` 单态化后 type_name 可能为具体类/泛型实例名，
            // 收集以供泛型类强制实例化（与 TypeId 分支同语义）。
            out.insert(type_name.clone());
        }
    }
}

fn collect_rvalue_class_refs(rv: &MirRvalue, out: &mut HashSet<String>) {
    match rv {
        MirRvalue::New { class, .. } => {
            out.insert(class.clone());
        }
        MirRvalue::FieldGet { class, .. } => {
            out.insert(class.clone());
        }
        MirRvalue::MethodCall {
            receiver_type,
            target_fn,
            ..
        } => {
            out.insert(receiver_type.clone());
            if let Some(t) = target_fn {
                if let Some(pos) = t.rfind("::") {
                    out.insert(t[..pos].to_string());
                }
            }
        }
        MirRvalue::NullCondField { class, .. } => {
            out.insert(class.clone());
        }
        MirRvalue::NullCondMethod {
            receiver_type,
            target_fn,
            ..
        } => {
            out.insert(receiver_type.clone());
            if let Some(t) = target_fn {
                if let Some(pos) = t.rfind("::") {
                    out.insert(t[..pos].to_string());
                }
            }
        }
        MirRvalue::ForceDerefField { class, .. } => {
            out.insert(class.clone());
        }
        MirRvalue::ForceDerefMethod {
            receiver_type,
            target_fn,
            ..
        } => {
            out.insert(receiver_type.clone());
            if let Some(t) = target_fn {
                if let Some(pos) = t.rfind("::") {
                    out.insert(t[..pos].to_string());
                }
            }
        }
        MirRvalue::MakeIface { class, iface, .. } => {
            out.insert(class.clone());
            out.insert(iface.clone());
        }
        MirRvalue::MakeIfaceDyn { iface, .. } => {
            out.insert(iface.clone());
        }
        MirRvalue::AdaptIface {
            from_iface,
            to_iface,
            ..
        } => {
            out.insert(from_iface.clone());
            out.insert(to_iface.clone());
        }
        _ => {}
    }
}

/// 泛型类方法单态化：扫描所有 MethodCall target，若具体类方法（如
/// `Signal_int::Set`）body 缺失而泛型模板（`Signal_T::Set`）存在，则从模板
/// 克隆并替换类型参数。
///
/// 背景：typeck 在泛型方法体内看到 `Signal<T>` 时仅注册 stub，不会触发
/// `instantiate_generic_class` 生成具体类方法 body。MIR lower 克隆泛型方法体
/// 为具体类型后（`try_create_mono_body`），需补全被引用泛型类的方法 body。
fn generate_generic_class_methods(result: &mut Vec<(String, MirCfgBody)>, registry: &TypeRegistry) {
    let existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();

    let mut needed: HashSet<String> = HashSet::new();
    for (_, body) in result.iter() {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                        collect_generic_class_method_targets(rvalue, &mut needed);
                    }
                    MirStatement::Return(Some(rvalue)) => {
                        collect_generic_class_method_targets(rvalue, &mut needed);
                    }
                    _ => {}
                }
            }
        }
    }

    for tfn in &needed {
        if existing.contains(tfn.as_str()) {
            continue;
        }
        // 方法级泛型单态化目标（`__` 后缀）由 try_create_mono_body 处理。
        if tfn.contains("__") {
            continue;
        }
        let Some(pos) = tfn.rfind("::") else {
            continue;
        };
        let class = &tfn[..pos];
        let Some((_, type_args, gen_params)) =
            resolve_generic_class_template_by_name(class, registry)
        else {
            continue;
        };
        if gen_params.is_empty() || gen_params.len() != type_args.len() {
            continue;
        }
        let concrete_idents: Vec<Ident> = type_args
            .iter()
            .map(typeck::type_id_to_field_name)
            .collect();
        // 模板 body：前向替换匹配——模板名 `Signal_T::Set` 经 gen_params 替换
        // 后恰等于目标 `Signal_int::Set`（`::` 感知，见 registry_resolve.rs）。
        let Some((_, template_body)) = result.iter().find(|(n, _)| {
            n.contains("::")
                && typeck::registry::substitute_generic_in_ty_name(n, &gen_params, &concrete_idents)
                    == *tfn
        }) else {
            continue;
        };
        let mut cloned = template_body.clone();
        cloned.ret = substitute_type_id(&cloned.ret, &gen_params, &concrete_idents);
        for (_, ty) in &mut cloned.params {
            *ty = substitute_type_id(ty, &gen_params, &concrete_idents);
        }
        for (_, (_, ty)) in &mut cloned.locals {
            *ty = substitute_type_id(ty, &gen_params, &concrete_idents);
        }
        for block in cloned.blocks.values_mut() {
            for stmt in &mut block.statements {
                substitute_in_statement(stmt, &gen_params, &concrete_idents);
            }
            substitute_in_terminator(&mut block.terminator, &gen_params, &concrete_idents);
        }
        cloned.linkage = Linkage::LinkonceOdr;
        result.push((tfn.clone(), cloned));
    }
}

/// After generic class method mono, hard-error if a referenced concrete class
/// method still lacks a body while its generic template method exists in the
/// module (silent runtime crash forbidden — mirror of the ctor/method rejects).
fn reject_missing_generic_class_methods(result: &[(String, MirCfgBody)], registry: &TypeRegistry) {
    let existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();
    let mut needed: HashSet<String> = HashSet::new();
    for (_, body) in result {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                        collect_generic_class_method_targets(rvalue, &mut needed);
                    }
                    MirStatement::Return(Some(rvalue)) => {
                        collect_generic_class_method_targets(rvalue, &mut needed);
                    }
                    _ => {}
                }
            }
        }
    }
    for tfn in &needed {
        if existing.contains(tfn.as_str()) || tfn.contains("__") {
            continue;
        }
        let Some(pos) = tfn.rfind("::") else {
            continue;
        };
        let class = &tfn[..pos];
        let Some((_, type_args, gen_params)) =
            resolve_generic_class_template_by_name(class, registry)
        else {
            continue;
        };
        if gen_params.is_empty() || gen_params.len() != type_args.len() {
            continue;
        }
        let concrete_idents: Vec<Ident> = type_args
            .iter()
            .map(typeck::type_id_to_field_name)
            .collect();
        let template_present = result.iter().any(|(n, _)| {
            n.contains("::")
                && typeck::registry::substitute_generic_in_ty_name(n, &gen_params, &concrete_idents)
                    == *tfn
        });
        if template_present {
            panic!(
                "MIR lower: generic class method `{tfn}` requires a monomorphized body \
                 but cloning failed while generic template method exists"
            );
        }
    }
}

fn collect_mono_targets(
    body: &MirCfgBody,
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            match stmt {
                MirStatement::Assign { rvalue, .. }
                | MirStatement::FieldSet { value: rvalue, .. }
                | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                    try_create_mono_body(rvalue, name_to_idx, result, registry, mono_bodies);
                }
                MirStatement::Return(Some(rvalue)) => {
                    try_create_mono_body(rvalue, name_to_idx, result, registry, mono_bodies);
                }
                _ => {}
            }
        }
    }
}

/// 将单态化调用名 `{base}__{T0}__{T1}` 拆分为 `(base, suffix)`。
///
/// 模板基底名可自身以 `_` 结尾——0 实参泛型方法被重载 mangling 为
/// `Registrar::Register_`（`method_link_name` 对空实参后缀产生尾下划线），与
/// `__` 分隔符叠成 `Register___Base` 的**三下划线歧义**。若用 `rfind("__")` 切
/// 会取到 `Base__Impl` 处，得到错误基底 `Registrar::Register___Base`。
///
/// 本函数扫描所有 `__` 边界（含重叠——三下划线 `___` 含两个相邻 `__`），返回
/// **基底最长的既有泛型方法模板**切分点：对 `Register___Base__Impl`，
/// 边界 `Register`(非模板)、`Register_`(命中模板)、`Register___Base`(非模板)，
/// 最长的模板基底为 `Registrar::Register_` → 正确基底。用 `get_method_generics`
/// 判定「模板」而非普通函数，可排除 name_to_idx 中已生成的 mono body（mono 名
/// 的 `__` 后缀无法被 method_link_name 反查）。
fn split_mono_name<'a>(
    tfn: &'a str,
    contains: &impl Fn(&str) -> bool,
    registry: &TypeRegistry,
) -> Option<(&'a str, &'a str)> {
    let bytes = tfn.as_bytes();
    let mut best: Option<usize> = None;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'_' && bytes[i + 1] == b'_' {
            let base = &tfn[..i];
            if contains(base) && get_method_generics(registry, base).is_some() {
                // 取基底最长者：三下划线时 `Register_`(i=20) 优于 `Register`(i=19)。
                if best.is_none_or(|b| i > b) {
                    best = Some(i);
                }
            }
        }
        i += 1;
    }
    let pos = best?;
    Some((&tfn[..pos], &tfn[pos + 2..]))
}

fn try_create_mono_body(
    rv: &MirRvalue,
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    // 实例泛型：MethodCall.target_fn；静态泛型：Call.func（均用 `__` 分隔实参）。
    let tfn: &str = match rv {
        MirRvalue::MethodCall {
            target_fn: Some(tfn),
            ..
        } => tfn.as_str(),
        MirRvalue::Call { func, .. } => func.as_str(),
        _ => return,
    };
    let Some((base_name, suffix)) =
        split_mono_name(tfn, &|b| name_to_idx.contains_key(b), registry)
    else {
        return;
    };
    if name_to_idx.contains_key(tfn) || mono_bodies.iter().any(|(n, _)| n == tfn) {
        return;
    }
    let concrete_names: Vec<Ident> = suffix.split("__").map(|s| s.into()).collect();
    let Some(&idx) = name_to_idx.get(base_name) else {
        return;
    };
    let (_, orig_body) = &result[idx];
    let Some(generics) = get_method_generics(registry, base_name) else {
        return;
    };
    if generics.len() != concrete_names.len() {
        return;
    }
    // 模板体内 `this.Leaf<T>(…)` 会 mangle 为 `Leaf__T`（实参仍是方法型参）。
    // 禁止克隆出 identity mono——等外层方法 mono（`Wrap__int`）把 target 改写
    // 为 `Leaf__int` 后再克隆；否则留下死符号，且 reject 误把 `__T` 当必需。
    if generics == concrete_names {
        return;
    }
    let mut cloned = orig_body.clone();
    cloned.ret = substitute_type_id(&cloned.ret, &generics, &concrete_names);
    for (_, ty) in &mut cloned.params {
        *ty = substitute_type_id(ty, &generics, &concrete_names);
    }
    for (_, (_, ty)) in &mut cloned.locals {
        *ty = substitute_type_id(ty, &generics, &concrete_names);
    }
    for block in cloned.blocks.values_mut() {
        for stmt in &mut block.statements {
            substitute_in_statement(stmt, &generics, &concrete_names);
        }
        substitute_in_terminator(&mut block.terminator, &generics, &concrete_names);
    }
    // RFC 006 M4：方法 mono 体内提升的闭包（如 `Signal.Subscribe` 回调）经
    // `substitute_in_operand` 重命名为 `__lambda_N__concrete`，但其函数体仍是
    // 泛型模板（`__lambda_N`，体内调用 `BindingRegistry.ApplyValue<T>`）。须从
    // 模板克隆闭包体并替换类型参数，否则 codegen 链接 `__lambda_N__concrete`
    // 时报 undefined symbol（`BindingRegistry_ApplyValue__T` 未解除）。
    collect_closure_mono_targets(
        &cloned,
        &generics,
        &concrete_names,
        name_to_idx,
        result,
        mono_bodies,
    );
    cloned.linkage = Linkage::LinkonceOdr;
    mono_bodies.push((tfn.to_string(), cloned));
}

/// RFC 006 M4：扫描已 mono 的方法体，收集被重命名（带 `__concrete` 后缀）的
/// 闭包引用，并从模板克隆其函数体、替换类型参数后加入 `mono_bodies`。
///
/// 闭包体与泛型方法模板同源（`lower_lambda_to_body` 在方法泛型上下文中提升），
/// 故使用与外包方法相同的 `generics`/`concrete` 对替换。嵌套闭包由外层
/// fixpoint（`lower_module` 循环）在下一轮扫描已加入的闭包 mono 体时递归处理。
fn collect_closure_mono_targets(
    body: &MirCfgBody,
    generics: &[Ident],
    concrete: &[Ident],
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            collect_closure_monos_in_stmt(
                stmt,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
    }
}

fn collect_closure_monos_in_stmt(
    stmt: &MirStatement,
    generics: &[Ident],
    concrete: &[Ident],
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => {
            collect_closure_monos_in_rvalue(
                rvalue,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::FieldSet { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::StaticFieldSet { value, .. } => {
            collect_closure_monos_in_rvalue(
                value,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            collect_closure_monos_in_operand(
                array,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                index,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_rvalue(
                value,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::Return(Some(rv)) => {
            collect_closure_monos_in_rvalue(
                rv,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::Return(None) => {}
        MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            collect_closure_monos_in_operand(
                cond,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            for s in then_body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            for s in else_body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirStatement::While {
            cond,
            body,
            foreach_source,
        } => {
            collect_closure_monos_in_rvalue(
                cond,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            if let Some(src) = foreach_source {
                collect_closure_monos_in_operand(
                    src,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            for s in body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            for s in catch_body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            for s in finally {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                collect_closure_monos_in_stmt(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirStatement::Throw { value } => {
            collect_closure_monos_in_rvalue(
                value,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirStatement::Drop(_)
        | MirStatement::Await { .. }
        | MirStatement::Break
        | MirStatement::Continue => {}
    }
}

fn collect_closure_monos_in_rvalue(
    rv: &MirRvalue,
    generics: &[Ident],
    concrete: &[Ident],
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    match rv {
        MirRvalue::Use(op) => {
            collect_closure_monos_in_operand(
                op,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::Binary { left, right, .. } => {
            collect_closure_monos_in_operand(
                left,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                right,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::New { args, .. }
        | MirRvalue::Call { args, .. }
        | MirRvalue::IndirectCall { args, .. } => {
            for a in args {
                collect_closure_monos_in_operand(
                    a,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_closure_monos_in_operand(
                receiver,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            for a in args {
                collect_closure_monos_in_operand(
                    a,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for el in elements {
                if let ArrayLitElement::Value(rv) = el {
                    collect_closure_monos_in_rvalue(
                        rv,
                        generics,
                        concrete,
                        name_to_idx,
                        result,
                        mono_bodies,
                    );
                }
            }
        }
        MirRvalue::NewArray { length, .. } => {
            collect_closure_monos_in_operand(
                length,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_closure_monos_in_operand(
                cond,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                then_val,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                else_val,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::Coalesce { left, right } => {
            collect_closure_monos_in_operand(
                left,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                right,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::NullCondField {
            receiver, default, ..
        } => {
            collect_closure_monos_in_operand(
                receiver,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                default,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            collect_closure_monos_in_operand(
                receiver,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            for a in args {
                collect_closure_monos_in_operand(
                    a,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            collect_closure_monos_in_operand(
                default,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::ForceDerefField { receiver, .. } => {
            collect_closure_monos_in_operand(
                receiver,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            collect_closure_monos_in_operand(
                receiver,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            for a in args {
                collect_closure_monos_in_operand(
                    a,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            collect_closure_monos_in_operand(
                array,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                index,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            collect_closure_monos_in_operand(
                array,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            if let Some(s) = start {
                collect_closure_monos_in_operand(
                    s,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
            if let Some(l) = length {
                collect_closure_monos_in_operand(
                    l,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            collect_closure_monos_in_operand(
                span,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                start,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            if let Some(l) = length {
                collect_closure_monos_in_operand(
                    l,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::SoaFieldGet { array, index, .. } => {
            collect_closure_monos_in_operand(
                array,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                index,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                collect_closure_monos_in_operand(
                    p,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::VariantExtract { scrutinee, .. } => {
            collect_closure_monos_in_operand(
                scrutinee,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::VariantTag { scrutinee, .. } => {
            collect_closure_monos_in_operand(
                scrutinee,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::Box { src, .. }
        | MirRvalue::Unbox { src, .. }
        | MirRvalue::SpanCopyTo { src, .. }
        | MirRvalue::SpanTryCopyTo { src, .. } => {
            collect_closure_monos_in_operand(
                src,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::SpanFill { span, value, .. } => {
            collect_closure_monos_in_operand(
                span,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
            collect_closure_monos_in_operand(
                value,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::SpanClear { span, .. } | MirRvalue::SpanToArray { span, .. } => {
            collect_closure_monos_in_operand(
                span,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::StructLit { fields, .. } => {
            for (_, op) in fields {
                collect_closure_monos_in_operand(
                    op,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::FieldGet { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::MakeIface { object, .. }
        | MirRvalue::MakeIfaceDyn { object, .. }
        | MirRvalue::AdaptIface { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            for e in elements {
                collect_closure_monos_in_operand(
                    e,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirRvalue::FnPtr { .. }
        | MirRvalue::ExpressionTreeConst { .. }
        | MirRvalue::LinqChain(_) => {}
    }
}

fn collect_closure_monos_in_operand(
    op: &MirOperand,
    generics: &[Ident],
    concrete: &[Ident],
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    match op {
        MirOperand::Closure { fn_name, env } => {
            // 被 `substitute_in_operand` 重命名的闭包（带 `__{concrete}` 后缀），
            // 若其模板体存在于 result 且全名尚未生成，则克隆 + 替换。
            // concrete 可含多个类型实参（`__A__B`），`rfind("__")` 会切到最后一个
            // 分隔符而把模板名误判为 `{base}__A`（未注册）→ 闭包单态永不生成 →
            // codegen 引用未定义符号。逐 `__` 边界试切，前缀须是已注册模板名。
            let mut split: Option<usize> = None;
            let bytes = fn_name.as_bytes();
            let mut i = 0;
            while i + 1 < bytes.len() {
                if bytes[i] == b'_'
                    && bytes[i + 1] == b'_'
                    && i > 0
                    && name_to_idx.contains_key(&fn_name[..i])
                {
                    split = Some(i);
                    break;
                }
                i += 1;
            }
            if let Some(pos) = split {
                let base = &fn_name[..pos];
                let full = fn_name.clone();
                if name_to_idx.contains_key(base)
                    && !name_to_idx.contains_key(&full)
                    && !mono_bodies.iter().any(|(n, _)| n == &full)
                {
                    if let Some(&idx) = name_to_idx.get(base) {
                        let (_, orig) = &result[idx];
                        let mut cloned = orig.clone();
                        cloned.ret = substitute_type_id(&cloned.ret, generics, concrete);
                        for (_, ty) in &mut cloned.params {
                            *ty = substitute_type_id(ty, generics, concrete);
                        }
                        for (_, (_, ty)) in &mut cloned.locals {
                            *ty = substitute_type_id(ty, generics, concrete);
                        }
                        for block in cloned.blocks.values_mut() {
                            for stmt in &mut block.statements {
                                substitute_in_statement(stmt, generics, concrete);
                            }
                            substitute_in_terminator(&mut block.terminator, generics, concrete);
                        }
                        cloned.linkage = Linkage::LinkonceOdr;
                        // 嵌套闭包递归：克隆体（如 `__lambda_rt_37__Greeter`）的
                        // body 内还有 `Closure{ fn_name: "__lambda_rt_38__Greeter" }`
                        // 操作数（泛型方法体里的外层 λ 再建内层 λ）。闭包克隆产物
                        // 只进 mono_bodies，不会被后续 fixpoint 轮按方法克隆路径
                        // 再扫（`try_create_mono_body` 只看 Call/MethodCall）——
                        // 此处立即递归收集，否则内层闭包 mono 缺失
                        //（arc-prune-001：IR 引用 `__lambda_rt_38__Greeter` 无定义）。
                        collect_closure_mono_targets(
                            &cloned,
                            generics,
                            concrete,
                            name_to_idx,
                            result,
                            mono_bodies,
                        );
                        mono_bodies.push((full, cloned));
                    }
                }
            }
            for (_, op) in env {
                collect_closure_monos_in_operand(
                    op,
                    generics,
                    concrete,
                    name_to_idx,
                    result,
                    mono_bodies,
                );
            }
        }
        MirOperand::Field { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirOperand::Iface { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        MirOperand::UnboxIface { object, .. } => {
            collect_closure_monos_in_operand(
                object,
                generics,
                concrete,
                name_to_idx,
                result,
                mono_bodies,
            );
        }
        _ => {}
    }
}

/// After method mono fixpoint, hard-error if a mangled generic call still
/// lacks a body while the generic template method exists in the module.
fn reject_missing_generic_method_monos(result: &[(String, MirCfgBody)], registry: &TypeRegistry) {
    let existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();
    let mut needed: HashSet<String> = HashSet::new();
    for (_, body) in result {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                        collect_mono_call_targets(rvalue, &mut needed);
                    }
                    MirStatement::Return(Some(rvalue)) => {
                        collect_mono_call_targets(rvalue, &mut needed);
                    }
                    _ => {}
                }
            }
        }
    }
    for tfn in &needed {
        if existing.contains(tfn.as_str()) {
            continue;
        }
        let Some((base_name, suffix)) = split_mono_name(tfn, &|b| existing.contains(b), registry)
        else {
            continue;
        };
        let Some(generics) = get_method_generics(registry, base_name) else {
            continue;
        };
        let concrete: Vec<Ident> = suffix.split("__").map(|s| s.into()).collect();
        // 模板体内未替换的 `Method__T` 不是真实缺口——外层 mono 会改写后缀。
        if generics == concrete {
            continue;
        }
        panic!(
            "MIR lower: generic method `{tfn}` requires a monomorphized body but cloning \
             from template `{base_name}` failed (silent runtime crash forbidden)"
        );
    }
}

fn collect_mono_call_targets(rv: &MirRvalue, out: &mut HashSet<String>) {
    match rv {
        MirRvalue::MethodCall {
            target_fn: Some(tfn),
            ..
        } => {
            out.insert(tfn.clone());
        }
        MirRvalue::Call { func, .. } => {
            out.insert(func.clone());
        }
        _ => {}
    }
}

// =========================================================================
// RFC 006「接口泛型方法分派」：实例化收集 + 全实现者单态化
// =========================================================================

/// 扫描 MIR body，收集接口接收者的泛型方法实例化调用站点。
///
/// 当 `receiver_type` 为接口且 `target_fn` 形如 `Iface::Method__Suffix` 时，
/// 说明调用点经接口引用调用泛型方法。`target_fn` 的 `__` 后缀即类型实参。
/// 收集 `(iface, 基础方法名, 类型实参后缀)` 供全实现者单态化使用。
fn collect_iface_instantiations_in_body(
    body: &MirCfgBody,
    registry: &TypeRegistry,
    out: &mut Vec<(String, String, String)>,
) {
    for block in body.blocks.values() {
        for stmt in &block.statements {
            collect_iface_insts_in_stmt(stmt, registry, out);
        }
    }
}

fn collect_iface_insts_in_stmt(
    stmt: &MirStatement,
    registry: &TypeRegistry,
    out: &mut Vec<(String, String, String)>,
) {
    match stmt {
        MirStatement::Assign { rvalue, .. }
        | MirStatement::FieldSet { value: rvalue, .. }
        | MirStatement::StaticFieldSet { value: rvalue, .. } => {
            collect_iface_insts_in_rvalue(rvalue, registry, out);
        }
        MirStatement::Return(Some(rv)) => {
            collect_iface_insts_in_rvalue(rv, registry, out);
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
            for s in else_body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
        }
        MirStatement::While { body, .. } => {
            for s in body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
        }
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
            for s in catch_body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
        }
        MirStatement::TryFinally { body, finally, .. } => {
            for s in body {
                collect_iface_insts_in_stmt(s, registry, out);
            }
            for s in finally {
                collect_iface_insts_in_stmt(s, registry, out);
            }
        }
        _ => {}
    }
}

fn collect_iface_insts_in_rvalue(
    rv: &MirRvalue,
    registry: &TypeRegistry,
    out: &mut Vec<(String, String, String)>,
) {
    if let MirRvalue::MethodCall {
        receiver_type,
        target_fn: Some(tfn),
        ..
    } = rv
    {
        let iface_ident: Ident = receiver_type.as_str().into();
        if !registry.is_interface(&iface_ident) {
            return;
        }
        // target_fn 形如 "IGetter::Get__Seed"
        if let Some((method, suffix)) = parse_iface_generic_inst(tfn, &iface_ident, registry) {
            let entry = (receiver_type.clone(), method, suffix);
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
}

/// 从 `Iface::Method__Suffix` 中提取 `(Method, Suffix)`。
/// 仅当 Method 是 Iface 上的泛型方法时返回——避免误匹配重载后缀（`_` 单下划线）。
fn parse_iface_generic_inst(
    tfn: &str,
    iface: &Ident,
    registry: &TypeRegistry,
) -> Option<(String, String)> {
    let pos = tfn.rfind("::")?;
    let actual_iface = &tfn[..pos];
    if actual_iface != iface.as_str() {
        return None;
    }
    let method_part = &tfn[pos + 2..];
    // 扫描所有 `__` 边界，取能使前缀成为泛型方法的最长基底。
    // 关键：直接查 registry.types（绕过访问检查），因为这是编译期内部操作，
    // 不应受 `AccessContext` 可见性限制。QIF 测试类型非 public 时，
    // `resolve_method` 带空访问上下文会失败 → 实例化收集静默跳过。
    let bytes = method_part.as_bytes();
    let mut best: Option<usize> = None;
    let iface_nom = registry.types.get(iface.as_str());
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'_' && bytes[i + 1] == b'_' {
            let base = &method_part[..i];
            let method_ident: Ident = base.into();
            let is_generic = iface_nom.is_some_and(|nom| {
                nom.methods
                    .get(&method_ident)
                    .is_some_and(|sigs| sigs.iter().any(|sig| !sig.generics.is_empty()))
            });
            if is_generic {
                best = Some(i);
            }
        }
        i += 1;
    }
    let pos = best?;
    Some((
        method_part[..pos].to_string(),
        method_part[pos + 2..].to_string(),
    ))
}

/// 为每个接口泛型方法实例化键生成**全部实现类**的单态化方法体。
///
/// 对实例化键 `(iface, method, suffix)`：
/// 1. 枚举 registry 中所有直接实现 `iface` 的具体类
/// 2. 对每个实现类 C，从模板 `C::method` 克隆 + 类型实参替换 → `C::method__suffix`
///
/// 这与 `try_create_mono_body` 的克隆逻辑一致，但作用于全部实现者而非单一调用点。
fn generate_iface_instantiation_monos(
    iface: &str,
    method_name: &str,
    suffix: &str,
    name_to_idx: &HashMap<String, usize>,
    result: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
    _layouts: &ProgramLayouts,
    mono_bodies: &mut Vec<(String, MirCfgBody)>,
) {
    let iface_ident: Ident = iface.into();
    let concrete_names: Vec<Ident> = suffix.split("__").map(|s| s.into()).collect();

    // 枚举所有直接实现该接口的具体类
    for (type_name, type_info) in &registry.types {
        if !matches!(
            type_info.kind,
            typeck::TypeKind::Class | typeck::TypeKind::Struct
        ) {
            continue;
        }
        let class_ident: Ident = type_name.clone();
        if !registry.implements_interface(&class_ident, &iface_ident) {
            continue;
        }
        // 查找模板体 C::method
        let template_name = format!("{type_name}::{method_name}");
        let Some(&idx) = name_to_idx.get(template_name.as_str()) else {
            continue;
        };
        let (_, orig_body) = &result[idx];
        let Some(generics) = get_method_generics(registry, &template_name) else {
            continue;
        };
        if generics.len() != concrete_names.len() {
            continue;
        }
        // identity mono 无意义
        if generics == concrete_names {
            continue;
        }
        let mono_name = format!("{template_name}__{suffix}");
        if name_to_idx.contains_key(mono_name.as_str())
            || mono_bodies.iter().any(|(n, _)| n == &mono_name)
        {
            continue;
        }
        // 克隆 + 替换（与 try_create_mono_body 完全一致）
        let mut cloned = orig_body.clone();
        cloned.ret = substitute_type_id(&cloned.ret, &generics, &concrete_names);
        for (_, ty) in &mut cloned.params {
            *ty = substitute_type_id(ty, &generics, &concrete_names);
        }
        for (_, (_, ty)) in &mut cloned.locals {
            *ty = substitute_type_id(ty, &generics, &concrete_names);
        }
        for block in cloned.blocks.values_mut() {
            for stmt in &mut block.statements {
                substitute_in_statement(stmt, &generics, &concrete_names);
            }
            substitute_in_terminator(&mut block.terminator, &generics, &concrete_names);
        }
        // 提升的闭包也需克隆
        collect_closure_mono_targets(
            &cloned,
            &generics,
            &concrete_names,
            name_to_idx,
            result,
            mono_bodies,
        );
        cloned.linkage = Linkage::LinkonceOdr;
        mono_bodies.push((mono_name, cloned));
    }
}

/// 公开 API：从已 lower 的 MIR 中收集接口泛型方法实例化信息。
///
/// 返回 `BTreeMap<iface_name, BTreeSet<(method_name, suffix)>>`，
/// 供 pipeline 填充 `InterfaceLayout.generic_instances`。
/// 使用 BTreeMap/BTreeSet 保证跨编译单元的槽位顺序确定性。
pub fn collect_iface_generic_instances(
    mir_fns: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<(String, String)>> {
    let mut insts: Vec<(String, String, String)> = Vec::new();
    for (_, body) in mir_fns {
        collect_iface_instantiations_in_body(body, registry, &mut insts);
    }
    let mut map: std::collections::BTreeMap<String, std::collections::BTreeSet<(String, String)>> =
        std::collections::BTreeMap::new();
    for (iface, method, suffix) in insts {
        map.entry(iface).or_default().insert((method, suffix));
    }
    map
}

fn get_method_generics(registry: &TypeRegistry, mangled_name: &str) -> Option<Vec<Ident>> {
    // ctor 模板（`__ctor::Class[_arity]`）不是方法：其泛型是**类**泛型参数，
    // 由 generic-class 单态化路径（generate_generic_class_ctors）处理，不走
    // 方法泛型解析。按 `::` 拆分会把 owner 解析为不存在的类型 `__ctor`，
    // 对 registry 做必然失败的方法查询（重载收集报 UndefinedType）。
    if mangled_name.starts_with("__ctor::") {
        return None;
    }
    if let Some(pos) = mangled_name.rfind("::") {
        let class: Ident = mangled_name[..pos].into();
        let method_part = &mangled_name[pos + 2..];
        let ctx = AccessContext {
            current_type: None,
            extension_scope: ExtensionScope {
                imported: vec![],
                enclosing: vec![],
            },
            enclosing_namespace: vec![],
            current_package: None,
            skip_type_visibility: false,
        };
        // 无重载：`Assert::Empty` → 按简单名解析。
        let method: Ident = method_part.into();
        if let Ok(sig) = registry.resolve_method(&class, &method, &ctx) {
            if !sig.generics.is_empty() {
                return Some(sig.generics.clone());
            }
        }
        // Fallback 1：直接按方法名查 registry.types（绕过访问检查）。
        // `resolve_method` 可能因 `AccessContext` 缺失（无 current_package）
        // 拒绝访问 internal 类型，但模板过滤是编译期内部操作，不应受访问限制。
        let nom = registry.types.get(&class)?;
        if let Some(sigs) = nom.methods.get(&method) {
            for sig in sigs {
                if !sig.generics.is_empty() {
                    return Some(sig.generics.clone());
                }
            }
        }
        // Fallback 2：有重载时模板名含参数占位（如 `Contains_T_List_T`），
        // 简单名不在 methods 表——按 link 名反查。
        for sigs in nom.methods.values() {
            for sig in sigs {
                if sig.generics.is_empty() {
                    continue;
                }
                if registry.method_link_name_for(&class, sig) == mangled_name {
                    return Some(sig.generics.clone());
                }
            }
        }
    }
    None
}

/// 泛型方法模板后处理：从 `result` 剔除无法独立成函数的泛型方法模板。
///
/// 一个泛型方法模板（`get_method_generics` 非空）若其函数体直接对自身泛型
/// 形参做构造或方法调用（`new T()` → `@__ctor_T`、`value.ReadJson(reader)` →
/// `@T_ReadJson`），在未单态化（`T` 未定）时无法编译为可链接的独立函数——
/// 这些 `@T_*` 符号在链接期未解析。模板仅作单态化克隆源，从不被裸名直接
/// 调用（泛型方法总以 `Class::Method__T` 单态名调用；C# 亦禁止泛型方法
/// virtual/override，故不进入 vtable）。
///
/// 唯一强制模板存活的外部引用来自接口 itable（接口可声明泛型方法，如
/// `IConfiguration.Get<T>` → `@Configuration_Get`）。此类模板被 itable 引用却
/// 无法独立成函数，codegen 在 `emit_itables` 中据 `fn_names` 自动跳过对应
/// 槽位（见 codegen），故此处直接从 `result` 剔除模板，避免 codegen 发射
/// 未解析的 `@T_*` 符号。
///
/// 保留的泛型方法模板：函数体不直接触碰泛型形参的类型成员（如
/// `Mediator.SendAsync<T>` 仅通过接口调度 `handler.HandleAsync(request, ct)`，不产生
/// `@T_*` 符号），可独立成函数，且可能被 itable 引用，必须保留。
fn drop_non_emittable_generic_templates(
    result: &mut Vec<(String, MirCfgBody)>,
    registry: &TypeRegistry,
) {
    result.retain(|(name, body)| {
        let Some(generics) = get_method_generics(registry, name) else {
            // 非泛型方法：保留。
            return true;
        };
        // 泛型方法模板：若函数体直接触碰泛型形参的类型成员（构造/方法调用），
        // 无法独立成函数 → 剔除。
        let drop = body_refs_generic_param_as_type(body, &generics);
        if std::env::var("ARC_DEBUG_TEMPLATES").is_ok() {
            eprintln!("[drop_non_emittable] {name} generics={generics:?} drop={drop}");
        }
        !drop
    });
}

/// 扫描 `body`，判断其函数体是否直接以泛型形参作为「类型所有者」做构造或
/// 方法调用（`new T()` / `value.ReadJson(reader)` 且 `value: T`）。
fn body_refs_generic_param_as_type(body: &MirCfgBody, generics: &[Ident]) -> bool {
    fn scan_stmts(stmts: &[MirStatement], generics: &[Ident]) -> bool {
        stmts.iter().any(|s| scan_stmt(s, generics))
    }
    fn scan_stmt(s: &MirStatement, generics: &[Ident]) -> bool {
        match s {
            MirStatement::Assign { rvalue, .. } => rvalue_refs_generic_param(rvalue, generics),
            MirStatement::Return(Some(rv)) => rvalue_refs_generic_param(rv, generics),
            MirStatement::If {
                then_body,
                else_body,
                ..
            } => scan_stmts(then_body, generics) || scan_stmts(else_body, generics),
            MirStatement::While { cond, body, .. } => {
                rvalue_refs_generic_param(cond, generics) || scan_stmts(body, generics)
            }
            MirStatement::FieldSet { value, .. } => rvalue_refs_generic_param(value, generics),
            MirStatement::StaticFieldSet { value, .. } => {
                rvalue_refs_generic_param(value, generics)
            }
            MirStatement::IndexSet { value, .. } => rvalue_refs_generic_param(value, generics),
            MirStatement::TryCatch {
                try_body,
                catch_body,
                ..
            } => scan_stmts(try_body, generics) || scan_stmts(catch_body, generics),
            MirStatement::TryFinally { body, finally, .. } => {
                scan_stmts(body, generics) || scan_stmts(finally, generics)
            }
            MirStatement::LinqForeach { body, .. } => scan_stmts(body, generics),
            _ => false,
        }
    }
    for block in body.blocks.values() {
        if scan_stmts(&block.statements, generics) {
            return true;
        }
    }
    false
}

/// 判断单个 rvalue 是否以泛型形参作为「类型所有者」作构造或方法调用。
fn rvalue_refs_generic_param(rv: &MirRvalue, generics: &[Ident]) -> bool {
    let is_gen = |s: &str| generics.iter().any(|g| g.as_str() == s);
    // 泛型形参嵌入**嵌套泛型类**形态（`EnumOptions_T`/`Signal_T`）时同样无法
    // 独立成函数：body 内 `options.Count`/`options.Get(i)`（`options: EnumOptions<T>`）
    // 的调用目标为未单态化符号 `EnumOptions_T_get_Count`/`EnumOptions_T_Get`
    //（arc-prune-001：UI `From_EnumOptions_T` 模板被发射即报 22 符号缺失）。
    // 模板仅作单态化克隆源——克隆体经 `substitute_in_rvalue` 把接收者类型与
    // 目标名中的 T 原子替换为具体类型后才可发射，模板本身须剔除。
    let name_has_generic_atom = |s: &str| {
        s.split('_').any(|atom| {
            generics
                .iter()
                .any(|g| !g.as_str().is_empty() && g.as_str() == atom)
        })
    };
    // `seed.Value()`（`seed: T`）：target_fn 常为 `T::Value`，但约束接口分派时
    // 解析为接口方法（target_fn = `ISeed::Value`），此时仅靠 target_fn 前缀漏检。
    // 必须同时检查 `receiver_type` 是否即泛型形参——否则模板不被剔除，codegen
    // 发射 `call @T_Value`（undefined value）。RFC 006「接口泛型方法分派」。
    let method_refs = |receiver_type: &str, target_fn: &Option<String>| {
        generics.iter().any(|g| {
            let gs = g.as_str();
            receiver_type == gs
                || name_has_generic_atom(receiver_type)
                || target_fn.as_deref().is_some_and(|tf| {
                    tf.starts_with(&format!("{gs}::")) || name_has_generic_atom(tf)
                })
        })
    };
    match rv {
        // `new T()` → `MirRvalue::New { class: "T" }` → codegen `@__ctor_T`。
        MirRvalue::New { class, .. } => is_gen(class) || name_has_generic_atom(class),
        MirRvalue::MethodCall {
            receiver_type,
            target_fn,
            ..
        } => method_refs(receiver_type, target_fn),
        MirRvalue::NullCondMethod {
            receiver_type,
            target_fn,
            ..
        } => method_refs(receiver_type, target_fn),
        MirRvalue::ForceDerefMethod {
            receiver_type,
            target_fn,
            ..
        } => method_refs(receiver_type, target_fn),
        // 兜底：裸 `Call` 目标为 `__ctor_T` 或 `T::...`。
        MirRvalue::Call { func, .. } => generics.iter().any(|g| {
            let gs = g.as_str();
            func == &format!("__ctor_{gs}")
                || func.starts_with(&format!("{gs}::"))
                || name_has_generic_atom(func)
        }),
        _ => false,
    }
}

fn substitute_type_id(ty: &TypeId, generics: &[Ident], concrete: &[Ident]) -> TypeId {
    match ty {
        TypeId::Named(n) => {
            let substituted =
                typeck::registry::substitute_generic_in_ty_name(n, generics, concrete);
            field_name_to_type_id(&substituted)
        }
        TypeId::Generic(g) => {
            if let Some(pos) = generics.iter().position(|gen| gen == g) {
                field_name_to_type_id(&concrete[pos])
            } else {
                ty.clone()
            }
        }
        TypeId::Array { elem } => TypeId::Array {
            elem: Box::new(substitute_type_id(elem, generics, concrete)),
        },
        TypeId::Span { elem, mutable } => TypeId::Span {
            elem: Box::new(substitute_type_id(elem, generics, concrete)),
            mutable: *mutable,
        },
        TypeId::Ref {
            inner,
            mutable,
            kind,
        } => TypeId::Ref {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
            mutable: *mutable,
            kind: *kind,
        },
        TypeId::Func { params, ret } => TypeId::Func {
            params: params
                .iter()
                .map(|p| substitute_type_id(p, generics, concrete))
                .collect(),
            ret: Box::new(substitute_type_id(ret, generics, concrete)),
        },
        TypeId::Nullable { inner } => TypeId::Nullable {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
        },
        TypeId::Task { inner } => TypeId::Task {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
        },
        TypeId::IEnumerable { inner } => TypeId::IEnumerable {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
        },
        TypeId::IQueryable { inner } => TypeId::IQueryable {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
        },
        TypeId::Expression { inner } => TypeId::Expression {
            inner: Box::new(substitute_type_id(inner, generics, concrete)),
        },
        TypeId::Vector { elem, n } => TypeId::Vector {
            elem: Box::new(substitute_type_id(elem, generics, concrete)),
            n: *n,
        },
        other => other.clone(),
    }
}

fn substitute_in_statement(stmt: &mut MirStatement, generics: &[Ident], concrete: &[Ident]) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => {
            substitute_in_rvalue(rvalue, generics, concrete);
        }
        MirStatement::FieldSet { object, class, .. } => {
            substitute_in_operand(object, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
        }
        MirStatement::StaticFieldSet { value, .. } => {
            substitute_in_rvalue(value, generics, concrete);
        }
        MirStatement::IndexSet {
            array,
            index,
            elem_type,
            value,
        } => {
            substitute_in_operand(array, generics, concrete);
            substitute_in_operand(index, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
            substitute_in_rvalue(value, generics, concrete);
        }
        MirStatement::Return(rv) => {
            if let Some(rv) = rv {
                substitute_in_rvalue(rv, generics, concrete);
            }
        }
        MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            substitute_in_operand(cond, generics, concrete);
            for s in then_body {
                substitute_in_statement(s, generics, concrete);
            }
            for s in else_body {
                substitute_in_statement(s, generics, concrete);
            }
        }
        MirStatement::While {
            cond,
            body,
            foreach_source,
        } => {
            substitute_in_rvalue(cond, generics, concrete);
            if let Some(src) = foreach_source {
                substitute_in_operand(src, generics, concrete);
            }
            for s in body {
                substitute_in_statement(s, generics, concrete);
            }
        }
        MirStatement::TryCatch {
            catch_ty,
            try_body,
            catch_body,
            ..
        } => {
            *catch_ty = substitute_type_id(catch_ty, generics, concrete);
            for s in try_body {
                substitute_in_statement(s, generics, concrete);
            }
            for s in catch_body {
                substitute_in_statement(s, generics, concrete);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                substitute_in_statement(s, generics, concrete);
            }
            for s in finally {
                substitute_in_statement(s, generics, concrete);
            }
        }
        MirStatement::Drop(_) => {}
        MirStatement::Break | MirStatement::Continue => {}
        MirStatement::LinqForeach { body, .. } => {
            for s in body {
                substitute_in_statement(s, generics, concrete);
            }
        }
        MirStatement::Await { .. } => {}
        MirStatement::Throw { value } => {
            substitute_in_rvalue(value, generics, concrete);
        }
    }
}

fn substitute_in_terminator(term: &mut MirTerminator, generics: &[Ident], concrete: &[Ident]) {
    match term {
        MirTerminator::Goto(_) => {}
        MirTerminator::CondBr { cond, .. } => {
            substitute_in_operand(cond, generics, concrete);
        }
        MirTerminator::Return(rv) => {
            if let Some(rv) = rv {
                substitute_in_operand(rv, generics, concrete);
            }
        }
        MirTerminator::Throw(op) => {
            substitute_in_operand(op, generics, concrete);
        }
        MirTerminator::Unreachable => {}
    }
}

fn substitute_in_rvalue(rv: &mut MirRvalue, generics: &[Ident], concrete: &[Ident]) {
    match rv {
        MirRvalue::Use(op) => substitute_in_operand(op, generics, concrete),
        MirRvalue::Binary { left, right, .. } => {
            substitute_in_operand(left, generics, concrete);
            substitute_in_operand(right, generics, concrete);
        }
        MirRvalue::New { class, args, .. } => {
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
        }
        MirRvalue::Call { func, args } => {
            // RFC 006 M4：静态泛型调用（`BindingRegistry.ApplyValue<T>`）的符号
            // 名含 `T` 占位符，方法 mono 时须替换为具体类型（`ApplyValue_string`），
            // 否则 codegen 链接 `ApplyValue__T` 报 undefined value。
            *func = typeck::registry::substitute_generic_in_ty_name(func, generics, concrete);
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
        }
        MirRvalue::MethodCall {
            receiver,
            args,
            receiver_type,
            target_fn,
            ..
        } => {
            substitute_in_operand(receiver, generics, concrete);
            *receiver_type =
                typeck::registry::substitute_generic_in_ty_name(receiver_type, generics, concrete);
            if let Some(tfn) = target_fn {
                *tfn = typeck::registry::substitute_generic_in_ty_name(tfn, generics, concrete);
            }
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
        }
        MirRvalue::ArrayLit {
            elem_type,
            elements,
        } => {
            *elem_type = substitute_type_id(elem_type, generics, concrete);
            for el in elements {
                if let ArrayLitElement::Value(rv) = el {
                    substitute_in_rvalue(rv, generics, concrete);
                }
            }
        }
        MirRvalue::NewArray { elem_type, length } => {
            *elem_type = substitute_type_id(elem_type, generics, concrete);
            substitute_in_operand(length, generics, concrete);
        }
        MirRvalue::StructLit { .. } => {}
        MirRvalue::FnPtr { .. } => {}
        MirRvalue::ExpressionTreeConst { .. } => {}
        MirRvalue::IndirectCall { args, .. } => {
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            substitute_in_operand(cond, generics, concrete);
            substitute_in_operand(then_val, generics, concrete);
            substitute_in_operand(else_val, generics, concrete);
        }
        MirRvalue::Coalesce { left, right } => {
            substitute_in_operand(left, generics, concrete);
            substitute_in_operand(right, generics, concrete);
        }
        MirRvalue::NullCondField {
            receiver,
            class,
            default,
            ..
        } => {
            substitute_in_operand(receiver, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
            substitute_in_operand(default, generics, concrete);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            receiver_type,
            target_fn,
            default,
            ..
        } => {
            substitute_in_operand(receiver, generics, concrete);
            *receiver_type =
                typeck::registry::substitute_generic_in_ty_name(receiver_type, generics, concrete);
            if let Some(tfn) = target_fn {
                *tfn = typeck::registry::substitute_generic_in_ty_name(tfn, generics, concrete);
            }
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
            substitute_in_operand(default, generics, concrete);
        }
        MirRvalue::ForceDerefField {
            receiver, class, ..
        } => {
            substitute_in_operand(receiver, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
        }
        MirRvalue::ForceDerefMethod {
            receiver,
            args,
            receiver_type,
            target_fn,
            ..
        } => {
            substitute_in_operand(receiver, generics, concrete);
            *receiver_type =
                typeck::registry::substitute_generic_in_ty_name(receiver_type, generics, concrete);
            if let Some(tfn) = target_fn {
                *tfn = typeck::registry::substitute_generic_in_ty_name(tfn, generics, concrete);
            }
            for a in args {
                substitute_in_operand(a, generics, concrete);
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            substitute_in_operand(array, generics, concrete);
            substitute_in_operand(index, generics, concrete);
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            substitute_in_operand(array, generics, concrete);
            if let Some(s) = start {
                substitute_in_operand(s, generics, concrete);
            }
            if let Some(l) = length {
                substitute_in_operand(l, generics, concrete);
            }
        }
        MirRvalue::SpanFromStack {
            elements,
            elem_type,
            ..
        } => {
            for e in elements {
                substitute_in_operand(e, generics, concrete);
            }
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            substitute_in_operand(span, generics, concrete);
            substitute_in_operand(start, generics, concrete);
            if let Some(length) = length {
                substitute_in_operand(length, generics, concrete);
            }
        }
        MirRvalue::SpanCopyTo {
            src,
            dest,
            elem_type,
        } => {
            substitute_in_operand(src, generics, concrete);
            substitute_in_operand(dest, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
        MirRvalue::Box { src, src_ty } => {
            substitute_in_operand(src, generics, concrete);
            *src_ty = substitute_type_id(src_ty, generics, concrete);
        }
        MirRvalue::Unbox { src, target_ty } => {
            substitute_in_operand(src, generics, concrete);
            *target_ty = substitute_type_id(target_ty, generics, concrete);
        }
        MirRvalue::MakeIface { class, iface, .. } => {
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
            *iface = typeck::registry::substitute_generic_in_ty_name(iface, generics, concrete);
        }
        MirRvalue::MakeIfaceDyn { iface, .. } => {
            *iface = typeck::registry::substitute_generic_in_ty_name(iface, generics, concrete);
        }
        MirRvalue::AdaptIface {
            from_iface,
            to_iface,
            ..
        } => {
            *from_iface =
                typeck::registry::substitute_generic_in_ty_name(from_iface, generics, concrete);
            *to_iface =
                typeck::registry::substitute_generic_in_ty_name(to_iface, generics, concrete);
        }
        MirRvalue::LinqChain(_) => {}
        MirRvalue::FieldGet { object, class, .. } => {
            substitute_in_operand(object, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
        }
        MirRvalue::SoaFieldGet { array, index, .. } => {
            substitute_in_operand(array, generics, concrete);
            substitute_in_operand(index, generics, concrete);
        }
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(p) = payload {
                substitute_in_operand(p, generics, concrete);
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } => {
            substitute_in_operand(scrutinee, generics, concrete);
        }
        MirRvalue::VariantExtract {
            scrutinee,
            payload_ty,
            ..
        } => {
            substitute_in_operand(scrutinee, generics, concrete);
            *payload_ty = substitute_type_id(payload_ty, generics, concrete);
        }
        MirRvalue::SpanFill {
            span,
            value,
            elem_type,
            ..
        } => {
            substitute_in_operand(span, generics, concrete);
            substitute_in_operand(value, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
        MirRvalue::SpanClear {
            span, elem_type, ..
        } => {
            substitute_in_operand(span, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
        MirRvalue::SpanTryCopyTo {
            src,
            dest,
            elem_type,
            ..
        } => {
            substitute_in_operand(src, generics, concrete);
            substitute_in_operand(dest, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
        MirRvalue::SpanToArray {
            span, elem_type, ..
        } => {
            substitute_in_operand(span, generics, concrete);
            *elem_type = substitute_type_id(elem_type, generics, concrete);
        }
    }
}

fn substitute_in_operand(op: &mut MirOperand, generics: &[Ident], concrete: &[Ident]) {
    match op {
        MirOperand::Field { object, class, .. } => {
            substitute_in_operand(object, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
        }
        MirOperand::Iface {
            object,
            class,
            iface,
        } => {
            substitute_in_operand(object, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
            *iface = typeck::registry::substitute_generic_in_ty_name(iface, generics, concrete);
        }
        MirOperand::UnboxIface { object, class } => {
            substitute_in_operand(object, generics, concrete);
            *class = typeck::registry::substitute_generic_in_ty_name(class, generics, concrete);
        }
        MirOperand::UnboxString { object } => {
            substitute_in_operand(object, generics, concrete);
        }
        MirOperand::UnboxGeneric { object, type_name } => {
            substitute_in_operand(object, generics, concrete);
            let substituted =
                typeck::registry::substitute_generic_in_ty_name(type_name, generics, concrete);
            *type_name = substituted;
        }
        MirOperand::AddrOf(_) => {}
        MirOperand::FnPtr { .. } => {}
        MirOperand::Closure { fn_name, env } => {
            // RFC 006 M4：泛型方法（如 `SetBinding<T>`）体内提升的闭包在方法
            // 单态化时须一并单态化——闭包体可能调用模板内泛型方法
            // （如 `BindingRegistry.ApplyValue<T>`）。重命名闭包符号为
            // `{fn_name}__{concrete}`，并对其 env 捕获递归替换（捕获值类型
            // 可能含 `T`，跨单态化保持一致）。
            if !concrete.is_empty() {
                *fn_name = format!("{}__{}", fn_name, concrete.join("__"));
            }
            for (_, op) in env {
                substitute_in_operand(op, generics, concrete);
            }
        }
        MirOperand::Local(_) => {}
        MirOperand::ConstInt(_) => {}
        MirOperand::ConstFloat(_) => {}
        MirOperand::ConstString(_) => {}
        MirOperand::ConstBool(_) => {}
        MirOperand::ConstNull => {}
        MirOperand::ConstDefault { type_name } => {
            let substituted =
                typeck::registry::substitute_generic_in_ty_name(type_name, generics, concrete);
            *type_name = substituted;
        }
        MirOperand::StaticField { .. } => {}
        MirOperand::TypeId { type_name } => {
            let substituted =
                typeck::registry::substitute_generic_in_ty_name(type_name, generics, concrete);
            *type_name = substituted;
        }
        MirOperand::TypeInfoPtr { type_name } => {
            let substituted =
                typeck::registry::substitute_generic_in_ty_name(type_name, generics, concrete);
            *type_name = substituted;
        }
    }
}

/// 查找泛型类模板构造函数 MIR body。
///
/// 泛型类 `Signal<T>` 的 ctor 在 typed_fns 中可能是 `__ctor::Signal_1`
/// 或 `__ctor::Signal_T_1`（占位符型参名嵌入 mangling）。单态化克隆时须
/// 逐一尝试，避免 `Element.SetValue<double>` 等路径仅见 `New Signal_double`
/// 却找不到模板而漏发 `__ctor::Signal_double_1`。
fn find_generic_template_ctor_body<'a>(
    result: &'a [(String, MirCfgBody)],
    template_name: &str,
    gen_params: &[Ident],
    arity: usize,
) -> Option<&'a MirCfgBody> {
    let mut candidates: Vec<String> = Vec::new();
    if arity == 0 {
        candidates.push(format!("__ctor::{template_name}"));
    } else {
        candidates.push(format!("__ctor::{template_name}_{arity}"));
        for gp in gen_params {
            candidates.push(format!("__ctor::{template_name}_{gp}_{arity}"));
        }
    }
    for name in &candidates {
        if let Some((_, body)) = result.iter().find(|(n, _)| n == name) {
            return Some(body);
        }
    }
    None
}

/// 泛型类构造函数单态化：扫描所有 MIR body 中的 `MirRvalue::New`，
/// 若 class 为泛型类的单态化实例且构造函数 body 不存在，从泛型模板克隆。
///
/// 使用名称启发式（无需 `mono_origins`）：按 `_` 拆分 class 名，
/// 匹配 Registry 中具有泛型参数的模板类。
fn generate_generic_class_ctors(result: &mut Vec<(String, MirCfgBody)>, registry: &TypeRegistry) {
    let mut existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();

    // 收集所有 (class, arity)
    let mut needed: HashSet<(String, usize)> = HashSet::new();
    for (_, body) in result.iter() {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                        collect_new_rvalue_classes(rvalue, &mut needed);
                    }
                    MirStatement::Return(Some(rvalue)) => {
                        collect_new_rvalue_classes(rvalue, &mut needed);
                    }
                    _ => {}
                }
            }
        }
    }

    // Element DP 路径：`SetValue<T>`/`GetValue<T>`/`Observe<T>` 单态化方法名
    // 反推 `Signal<T>` ctor 需求（MIR 名 `Element::SetValue__double` 等）。
    for (name, _) in result.iter() {
        for prefix in [
            "Element::SetValue__",
            "Element::GetValue__",
            "Element::Observe__",
            "Element_SetValue__",
            "Element_GetValue__",
            "Element_Observe__",
        ] {
            if let Some(type_suffix) = name.strip_prefix(prefix) {
                needed.insert((format!("Signal_{type_suffix}"), 1));
            }
        }
    }

    for (class, arity) in &needed {
        let ctor_name = if *arity == 0 {
            format!("__ctor::{class}")
        } else {
            format!("__ctor::{class}_{arity}")
        };
        if existing.contains(&ctor_name) {
            continue;
        }

        // 名称启发式：按 `_` 拆分并匹配模板类
        let Some((template_name, type_args, gen_params)) =
            resolve_generic_class_template_by_name(class, registry)
        else {
            if std::env::var("ARC_DBG_CTOR").is_ok() {
                eprintln!(
                    "[ctor-dbg] generate_generic_class_ctors: no template for {class}/{arity}"
                );
            }
            // 非泛型 mono 名（或无法解析模板）——跳过；由 reject_missing 再筛
            continue;
        };
        if gen_params.is_empty() || gen_params.len() != type_args.len() {
            if std::env::var("ARC_DBG_CTOR").is_ok() {
                eprintln!(
                    "[ctor-dbg] generate_generic_class_ctors: arity mismatch {class} gp={} ta={}",
                    gen_params.len(),
                    type_args.len()
                );
            }
            continue;
        }

        let Some(template_body) =
            find_generic_template_ctor_body(result, template_name.as_str(), &gen_params, *arity)
        else {
            if std::env::var("ARC_DBG_CTOR").is_ok() {
                let names: Vec<String> = result
                    .iter()
                    .filter(|(n, _)| n.contains(template_name.as_str()))
                    .map(|(n, _)| n.clone())
                    .collect();
                eprintln!("[ctor-dbg] generate_generic_class_ctors: template body NOT FOUND for {class} template={template_name} gp={gen_params:?} arity={arity}; matching={names:?}");
            }
            // 模板 ctor 不在本模块 MIR 中。两类情况：
            //   1. 泛型类**无显式构造函数且无实例字段初始化器**（如
            //      `EndpointDispatcher<T,U>` 的隐式无参 ctor）——此时 typeck
            //      不合成模板 ctor body，codegen 第 6 步对非泛型无参类补发
            //      空默认 ctor；此处对**无参（arity==0）**泛型 mono 类同样
            //      合成空 ctor body（vtable/itable 指针由 emit_new 写入）。
            //   2. 有参 ctor（arity>0）但模板 body 缺失——属真实缺陷，
            //      不可静默合成（会吞掉参数初始化），交由 reject_missing 兜底。
            if *arity > 0 {
                continue;
            }
            let ctor_name = format!("__ctor::{class}");
            let this_ty = TypeId::Named(class.clone().into());
            let mut blocks = IndexMap::new();
            blocks.insert(
                BlockId(0),
                MirBlock {
                    id: BlockId(0),
                    statements: vec![],
                    terminator: MirTerminator::Return(None),
                },
            );
            let mut empty = MirCfgBody {
                params: vec![("this".into(), this_ty.clone())],
                ret: TypeId::Void,
                param_count: 1,
                locals: IndexMap::new(),
                entry: BlockId(0),
                blocks,
                is_async: false,
                owner: Some(class.clone().into()),
                class_fields: vec![],
                is_ctor: true,
                is_static: false,
                captures: vec![],
                linkage: Linkage::LinkonceOdr,
                parallelize: false,
                loop_backedges: HashSet::new(),
                foreach_loops: Vec::new(),
                spill_set: SpillSet::default(),
            };
            // `this` 占位 LocalId(0)，与 codegen 对 ctor 的 `(ptr %self)` 约定一致。
            empty.locals.insert(LocalId(0), ("this".into(), this_ty));
            existing.insert(ctor_name.clone());
            result.push((ctor_name, empty));
            continue;
        };
        let mut cloned = template_body.clone();

        // TypeId → Ident 转换（substitute_type_id 用 Ident 作 concrete 参数）
        let concrete_idents: Vec<Ident> = type_args
            .iter()
            .map(typeck::type_id_to_field_name)
            .collect();

        cloned.ret = substitute_type_id(&cloned.ret, &gen_params, &concrete_idents);
        for (_, ty) in &mut cloned.params {
            *ty = substitute_type_id(ty, &gen_params, &concrete_idents);
        }
        for (_, (_, ty)) in &mut cloned.locals {
            *ty = substitute_type_id(ty, &gen_params, &concrete_idents);
        }
        for block in cloned.blocks.values_mut() {
            for stmt in &mut block.statements {
                substitute_in_statement(stmt, &gen_params, &concrete_idents);
            }
            substitute_in_terminator(&mut block.terminator, &gen_params, &concrete_idents);
        }
        cloned.linkage = Linkage::LinkonceOdr;
        existing.insert(ctor_name.clone());
        result.push((ctor_name, cloned));
    }
}

/// 名称启发式：从 class 名解析泛型模板。
pub fn resolve_generic_class_template_by_name(
    class: &str,
    registry: &TypeRegistry,
) -> Option<(Ident, Vec<TypeId>, Vec<Ident>)> {
    let parts: Vec<&str> = class.split('_').collect();
    if parts.len() < 2 {
        return None;
    }
    for prefix_len in (1..parts.len()).rev() {
        let prefix = parts[..prefix_len].join("_");
        if let Some(nom) = registry.types.get(prefix.as_str()) {
            let num_gen = nom.generic_params.len();
            if num_gen > 0 && prefix_len + num_gen == parts.len() {
                let gen_params = nom.generic_params.clone();
                let concrete_idents: Vec<Ident> = parts[prefix_len..]
                    .iter()
                    .map(|&s| Ident::from(s))
                    .collect();
                let concrete_type_ids: Vec<TypeId> = concrete_idents
                    .iter()
                    .map(|n| field_name_to_type_id(n.as_str()))
                    .collect();
                return Some((Ident::from(prefix), concrete_type_ids, gen_params));
            }
        }
    }
    // 兜底：单类型参数模板 + 多段实参名（如 `List<Func<int,int,bool>>` →
    // `List_Func_int_int_bool`）。`_` 分段计数法无法区分多段实参，此处按
    // 「模板名前缀 + 余下整段 = 唯一实参」匹配（取最长模板名避免歧义）。
    let mut best: Option<(usize, Ident, Vec<TypeId>, Vec<Ident>)> = None;
    for (tname, nom) in registry.types.iter() {
        if nom.generic_params.len() != 1 {
            continue;
        }
        if let Some(rest) = class.strip_prefix(tname.as_str()) {
            if rest.starts_with('_') && rest.len() > 1 {
                let arg = &class[tname.len() + 1..];
                let arg_tid = field_name_to_type_id(arg);
                let gen_params = nom.generic_params.clone();
                match &best {
                    Some((blen, ..)) if *blen >= tname.len() => {}
                    _ => best = Some((tname.len(), tname.clone(), vec![arg_tid], gen_params)),
                }
            }
        }
    }
    if let Some((_, tname, type_args, gen_params)) = best {
        return Some((tname, type_args, gen_params));
    }
    None
}

fn collect_new_rvalue_classes(rv: &MirRvalue, out: &mut HashSet<(String, usize)>) {
    if let MirRvalue::New { class, args, .. } = rv {
        out.insert((class.clone(), args.len()));
    }
    if let MirRvalue::Call { func, .. } = rv {
        collect_ctor_call_target(func, out);
    }
}

/// 从 `__ctor::Class_Ty_arity` 调用名反推泛型类单态 ctor 需求。
fn collect_ctor_call_target(func: &str, out: &mut HashSet<(String, usize)>) {
    let Some(rest) = func.strip_prefix("__ctor::") else {
        return;
    };
    let Some(uscore) = rest.rfind('_') else {
        return;
    };
    let (class, arity_s) = rest.split_at(uscore);
    let Ok(arity) = arity_s.trim_start_matches('_').parse::<usize>() else {
        return;
    };
    if !class.is_empty() {
        out.insert((class.to_string(), arity));
    }
}

/// After fixpoint mono, hard-error if a `new MonoGeneric(...)` still lacks a
/// ctor body while the generic template ctor is present in the module.
///
/// Stub-handled types (template ctor absent — codegen emits ABI) are skipped.
fn reject_missing_generic_ctors(result: &[(String, MirCfgBody)], registry: &TypeRegistry) {
    let existing: HashSet<String> = result.iter().map(|(n, _)| n.clone()).collect();
    let mut needed: HashSet<(String, usize)> = HashSet::new();
    for (_, body) in result {
        for block in body.blocks.values() {
            for stmt in &block.statements {
                match stmt {
                    MirStatement::Assign { rvalue, .. }
                    | MirStatement::FieldSet { value: rvalue, .. }
                    | MirStatement::StaticFieldSet { value: rvalue, .. } => {
                        collect_new_rvalue_classes(rvalue, &mut needed);
                    }
                    MirStatement::Return(Some(rvalue)) => {
                        collect_new_rvalue_classes(rvalue, &mut needed);
                    }
                    _ => {}
                }
            }
        }
    }
    for (class, arity) in &needed {
        let Some((template_name, type_args, gen_params)) =
            resolve_generic_class_template_by_name(class, registry)
        else {
            continue;
        };
        if gen_params.is_empty() || gen_params.len() != type_args.len() {
            continue;
        }
        let ctor_name = if *arity == 0 {
            format!("__ctor::{class}")
        } else {
            format!("__ctor::{class}_{arity}")
        };
        if existing.contains(&ctor_name) {
            continue;
        }
        if find_generic_template_ctor_body(result, template_name.as_str(), &gen_params, *arity)
            .is_none()
        {
            // Template ctor not in MIR (codegen stub / external) — OK.
            continue;
        }
        panic!(
            "MIR lower: generic class `{class}` requires `{ctor_name}` but monomorphization \
             failed while generic template ctor exists (silent runtime crash forbidden)"
        );
    }
}

fn field_name_to_type_id(name: &str) -> TypeId {
    match name {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "double" => TypeId::Double,
        "float" => TypeId::Float,
        "bool" => TypeId::Bool,
        "string" => TypeId::String,
        "void" => TypeId::Void,
        "object" => TypeId::Object,
        _ => TypeId::Named(name.into()),
    }
}

// ===========================================================================
// RFC 023 (L1): Lambda capture analysis helpers.
//
// typeck receives `&Expr` (immutable) and cannot fill `LambdaExpr.captures`,
// so capture analysis runs in MIR lowering via `compute_captures`. These
// helpers walk the lambda body AST to collect identifier references; any name
// that resolves to an outer-scope local (not a lambda parameter) and whose
// type is a reference type (L1: class/string/interface) becomes a
// `ByRef` capture.
// ===========================================================================

/// lambda 捕获分析结果（`compute_captures` 返回值）。
///
/// 除捕获列表外携带 `refs_owner_static`：lambda 体裸引用了 owner 类静态
/// 成员（方法/属性/字段）。静态引用不需要捕获 `this`（保持无 env 的零开销
/// FnPtr 路径），但 lambda 降级上下文必须传播 owner 才能把裸名解析为限定
/// 符号——见 `lower_lambda_to_body` 的 `lambda_owner` 判据。
pub(super) struct LambdaCaptureAnalysis {
    pub(super) captures: Vec<LambdaCapture>,
    pub(super) refs_owner_static: bool,
}

/// Walk a lambda body and collect every identifier reference in source order.
/// Duplicates are preserved so `compute_captures` can de-duplicate via `seen`.
fn collect_lambda_body_idents(body: &LambdaBody, idents: &mut Vec<Ident>) {
    match body {
        LambdaBody::Expr(e) => collect_expr_idents(&e.node, idents),
        LambdaBody::Block(b) => collect_block_idents(b, idents),
    }
}

fn collect_block_idents(block: &Block, idents: &mut Vec<Ident>) {
    for stmt in &block.stmts {
        collect_stmt_idents(&stmt.node, idents);
    }
    if let Some(tail) = &block.tail {
        collect_expr_idents(&tail.node, idents);
    }
}

fn collect_stmt_idents(stmt: &Stmt, idents: &mut Vec<Ident>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(i) = init {
                collect_expr_idents(&i.node, idents);
            }
        }
        Stmt::Expr(e) => collect_expr_idents(&e.node, idents),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_expr_idents(&e.node, idents);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr_idents(&cond.node, idents);
            collect_block_idents(body, idents);
        }
        Stmt::For { iter, body, .. } => {
            collect_expr_idents(&iter.node, idents);
            collect_block_idents(body, idents);
        }
        Stmt::Assign { target, value } => {
            collect_expr_idents(&target.node, idents);
            collect_expr_idents(&value.node, idents);
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::Throw { expr } => collect_expr_idents(&expr.node, idents),
        Stmt::TryCatch {
            try_body,
            when_cond,
            catch_body,
            finally,
            ..
        } => {
            collect_block_idents(try_body, idents);
            if let Some(w) = when_cond {
                collect_expr_idents(&w.node, idents);
            }
            collect_block_idents(catch_body, idents);
            if let Some(f) = finally {
                collect_block_idents(f, idents);
            }
        }
        Stmt::TryFinally { body, finally } => {
            collect_block_idents(body, idents);
            collect_block_idents(finally, idents);
        }
        Stmt::Using { init, body, .. } => {
            collect_expr_idents(&init.node, idents);
            collect_block_idents(body, idents);
        }
        Stmt::UsingVar { init, .. } => {
            collect_expr_idents(&init.node, idents);
        }
        Stmt::AwaitUsing { init, body, .. } => {
            collect_expr_idents(&init.node, idents);
            collect_block_idents(body, idents);
        }
        Stmt::AwaitUsingVar { init, .. } => {
            collect_expr_idents(&init.node, idents);
        }
        Stmt::YieldReturn { value } => {
            collect_expr_idents(&value.node, idents);
        }
        Stmt::YieldBreak => {}
        Stmt::Lock { expr, body } => {
            collect_expr_idents(&expr.node, idents);
            collect_block_idents(body, idents);
        }
        Stmt::ForC {
            init,
            cond,
            inc,
            body,
        } => {
            if let Some(s) = init {
                collect_stmt_idents(&s.node, idents);
            }
            if let Some(c) = cond {
                collect_expr_idents(&c.node, idents);
            }
            if let Some(i) = inc {
                collect_stmt_idents(&i.node, idents);
            }
            collect_block_idents(body, idents);
        }
        Stmt::DeconstructAssign { value, .. } => {
            collect_expr_idents(&value.node, idents);
        }
    }
}

/// Recursively collect identifier references from an expression.
///
/// Method names (`MethodCall.method`) and field names (`Field.field`) are
/// intentionally NOT collected — they are member names, not variable
/// references. Nested lambda bodies ARE recursed into (transitive capture):
/// identifiers a nested lambda references that resolve to this lambda's
/// enclosing scope must be captured here and re-exported through the closure
/// env, matching C# nested-closure semantics. The nested lambda's own
/// parameters are local to it and excluded at the collection point; deeper
/// nesting bubbles up through recursion.
fn collect_expr_idents(expr: &Expr, idents: &mut Vec<Ident>) {
    match expr {
        Expr::Ident(name) => idents.push(name.clone()),
        Expr::Path(path) => {
            // Qualified path (e.g. `System.Console`); collect the last segment
            // so fully-qualified locals still resolve. Member paths typically
            // do not refer to outer-scope locals, but this is conservative.
            if let Some(name) = path.last() {
                idents.push(name.clone());
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_idents(&left.node, idents);
            collect_expr_idents(&right.node, idents);
        }
        // 赋值表达式：目标与值均引用外层变量（写捕获经闭包 env 快照传递，
        // 与读引用同一收集点——闭包对捕获变量持共享语义）。
        Expr::Assign { target, value } => {
            collect_expr_idents(&target.node, idents);
            collect_expr_idents(&value.node, idents);
        }
        Expr::Unary { expr, .. } => collect_expr_idents(&expr.node, idents),
        Expr::Call { func, args, .. } => {
            collect_expr_idents(&func.node, idents);
            for a in args {
                collect_expr_idents(&a.node, idents);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_idents(&receiver.node, idents);
            for a in args {
                collect_expr_idents(&a.node, idents);
            }
        }
        Expr::Field { receiver, .. } => {
            collect_expr_idents(&receiver.node, idents);
        }
        Expr::Index { receiver, index } => {
            collect_expr_idents(&receiver.node, idents);
            collect_expr_idents(&index.node, idents);
        }
        Expr::Lambda(l) => {
            // RFC 008 L4（传递捕获）：递归收集嵌套 lambda 体内引用的标识符。
            // 解析到本层宿主作用域的变量（嵌套参数除外）由本层捕获，经闭包
            // env 再导出给嵌套层——C# 嵌套闭包语义（编译器自动传递捕获）。
            // 嵌套 lambda 自身参数对它是局部的，在收集点排除；更深层嵌套
            // 的引用沿递归继续冒泡至本层（流式契约：块 lambda 引用外层
            // 流 lambda 捕获的 CancellationToken 即此路径）。
            let nested_params: HashSet<Ident> = l.params.iter().map(|p| p.name.clone()).collect();
            let mut nested = Vec::new();
            collect_lambda_body_idents(&l.body, &mut nested);
            idents.extend(nested.into_iter().filter(|n| !nested_params.contains(n)));
        }
        Expr::ExpressionLit(_) => {}
        Expr::Await(e) => collect_expr_idents(&e.node, idents),
        Expr::Block(b) => collect_block_idents(b, idents),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_idents(&cond.node, idents);
            collect_block_idents(then_branch, idents);
            if let Some(eb) = else_branch {
                collect_block_idents(eb, idents);
            }
        }
        Expr::Switch(s) => {
            collect_expr_idents(&s.scrutinee.node, idents);
            for case in &s.cases {
                collect_block_idents(&case.body, idents);
                if let Some(w) = &case.when {
                    collect_expr_idents(&w.node, idents);
                }
            }
        }
        Expr::SwitchForm(s) => {
            collect_expr_idents(&s.scrutinee.node, idents);
            for arm in &s.arms {
                if let Some(w) = &arm.when {
                    collect_expr_idents(&w.node, idents);
                }
                collect_expr_idents(&arm.body.node, idents);
            }
        }
        Expr::CollectionExpr { elements } => {
            for el in elements {
                collect_expr_idents(&el.expr().node, idents);
            }
        }
        Expr::Cast { expr, .. } => collect_expr_idents(&expr.node, idents),
        Expr::Comptime(inner) => collect_expr_idents(&inner.node, idents),
        Expr::New { args, obj_init, .. } => {
            for a in args {
                collect_expr_idents(&a.node, idents);
            }
            if let Some(init) = obj_init {
                for (_, e) in init {
                    collect_expr_idents(&e.node, idents);
                }
            }
        }
        // RFC 023 L3: collect `this` so it can be captured when the lambda
        // body references it (explicit `this`, `this.Field`, `this.Method()`).
        Expr::This => idents.push("this".into()),
        Expr::Base => {}
        Expr::Query(_) => {
            // LINQ query: its internal lambdas are lowered separately; skip.
        }
        Expr::RefArg { expr, .. } => collect_expr_idents(&expr.node, idents),
        Expr::NamedArg { expr, .. } => collect_expr_idents(&expr.node, idents),
        Expr::StackSpanLit { elements, .. } => {
            for e in elements {
                collect_expr_idents(&e.node, idents);
            }
        }
        Expr::Null => {}
        Expr::Coalesce { left, right } => {
            collect_expr_idents(&left.node, idents);
            collect_expr_idents(&right.node, idents);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_idents(&cond.node, idents);
            collect_expr_idents(&then_branch.node, idents);
            collect_expr_idents(&else_branch.node, idents);
        }
        Expr::NullCond { access } => collect_expr_idents(&access.node, idents),
        Expr::ForceDeref { access } => collect_expr_idents(&access.node, idents),
        // FFI Marshal 装箱/拆箱节点：递归收集内部表达式的 idents。
        Expr::Box { expr, .. } | Expr::Unbox { expr, .. } => {
            collect_expr_idents(&expr.node, idents)
        }
        Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::BoolLit(_)
        | Expr::StringLit(_)
        | Expr::CharLit(_)
        | Expr::Default { .. }
        | Expr::TypeOf(_) => {}
        Expr::InterpolatedString { parts } => {
            for p in parts {
                if let InterpPart::Expr(hole) = p {
                    collect_expr_idents(&hole.expr.node, idents);
                }
            }
        }
        // RFC 036 M1: `expr is pattern` — 递归收集 inner expr 的 idents。
        // pattern 中的类型名/绑定名不收集（类型名不是变量，绑定名由 lower 引入局部）。
        Expr::Is { expr, .. } => collect_expr_idents(&expr.node, idents),
        // RFC 006 M2：正常路径 typeck 已脱糖；防御性收集。
        Expr::With { receiver, inits } => {
            collect_expr_idents(&receiver.node, idents);
            for (_, e) in inits {
                collect_expr_idents(&e.node, idents);
            }
        }
        // `new T[n]`：长度表达式可能引用外层局部。
        Expr::NewArray { length, .. } => collect_expr_idents(&length.node, idents),
    }
}

/// 写捕获分析：收集 lambda 体内赋值语句的**裸名目标**（`x = ...`）。捕获
/// 模式判定（compute_captures）据此把写捕获的标量提升 ByRef——按值快照下
/// 对快照副本赋值是死代码，外部读不到写入（ForEach_AppliesAction：
/// `x => { sum = sum + x; }` 后 Assert.Equal(6, sum) 要求写传播至外层局部）；
/// 只读捕获的标量保持 ByValue 快照（LambdaCaptureTests.Int_ByValueCapture /
/// LoopVariable_ByValueSnapshot 断言的语义）。成员赋值（`this.f = ...`、
/// `arr[i] = ...`）写入的是引用指向的存储而非裸名重绑定，不收集；嵌套
/// lambda 的写按传递捕获语义（RFC 008 L4）一并收集——内层写经闭包 env
/// 再导出，本层须 ByRef 才能让写穿透至宿主局部。遍历覆盖面与
/// collect_stmt_idents / collect_expr_idents 逐一对应。
fn collect_lambda_body_assigned_idents(body: &LambdaBody, out: &mut HashSet<Ident>) {
    match body {
        LambdaBody::Expr(e) => collect_expr_assigned_idents(&e.node, out),
        LambdaBody::Block(b) => collect_block_assigned_idents(b, out),
    }
}

fn collect_block_assigned_idents(block: &Block, out: &mut HashSet<Ident>) {
    for stmt in &block.stmts {
        collect_stmt_assigned_idents(&stmt.node, out);
    }
    if let Some(tail) = &block.tail {
        collect_expr_assigned_idents(&tail.node, out);
    }
}

fn collect_stmt_assigned_idents(stmt: &Stmt, out: &mut HashSet<Ident>) {
    match stmt {
        Stmt::Assign { target, value } => {
            if let Expr::Ident(name) = &target.node {
                out.insert(name.clone());
            } else {
                collect_expr_assigned_idents(&target.node, out);
            }
            collect_expr_assigned_idents(&value.node, out);
        }
        Stmt::Let { init, .. } => {
            if let Some(i) = init {
                collect_expr_assigned_idents(&i.node, out);
            }
        }
        Stmt::Expr(e) => collect_expr_assigned_idents(&e.node, out),
        Stmt::Return(e) => {
            if let Some(e) = e {
                collect_expr_assigned_idents(&e.node, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr_assigned_idents(&cond.node, out);
            collect_block_assigned_idents(body, out);
        }
        Stmt::For { iter, body, .. } => {
            collect_expr_assigned_idents(&iter.node, out);
            collect_block_assigned_idents(body, out);
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::Throw { expr } => collect_expr_assigned_idents(&expr.node, out),
        Stmt::TryCatch {
            try_body,
            when_cond,
            catch_body,
            finally,
            ..
        } => {
            collect_block_assigned_idents(try_body, out);
            if let Some(w) = when_cond {
                collect_expr_assigned_idents(&w.node, out);
            }
            collect_block_assigned_idents(catch_body, out);
            if let Some(f) = finally {
                collect_block_assigned_idents(f, out);
            }
        }
        Stmt::TryFinally { body, finally } => {
            collect_block_assigned_idents(body, out);
            collect_block_assigned_idents(finally, out);
        }
        Stmt::Using { init, body, .. } => {
            collect_expr_assigned_idents(&init.node, out);
            collect_block_assigned_idents(body, out);
        }
        Stmt::UsingVar { init, .. } => {
            collect_expr_assigned_idents(&init.node, out);
        }
        Stmt::AwaitUsing { init, body, .. } => {
            collect_expr_assigned_idents(&init.node, out);
            collect_block_assigned_idents(body, out);
        }
        Stmt::AwaitUsingVar { init, .. } => {
            collect_expr_assigned_idents(&init.node, out);
        }
        Stmt::YieldReturn { value } => {
            collect_expr_assigned_idents(&value.node, out);
        }
        Stmt::YieldBreak => {}
        Stmt::Lock { expr, body } => {
            collect_expr_assigned_idents(&expr.node, out);
            collect_block_assigned_idents(body, out);
        }
        Stmt::ForC {
            init,
            cond,
            inc,
            body,
        } => {
            if let Some(s) = init {
                collect_stmt_assigned_idents(&s.node, out);
            }
            if let Some(c) = cond {
                collect_expr_assigned_idents(&c.node, out);
            }
            if let Some(i) = inc {
                collect_stmt_assigned_idents(&i.node, out);
            }
            collect_block_assigned_idents(body, out);
        }
        Stmt::DeconstructAssign { value, .. } => {
            collect_expr_assigned_idents(&value.node, out);
        }
    }
}

/// 见 collect_lambda_body_assigned_idents：递归表达式收集嵌套 lambda / 语句
/// 容器（块 / if / switch）内的赋值目标裸名。裸名**读**（Ident/Path）不是
/// 记录点——与 collect_expr_idents 的关键差异。
fn collect_expr_assigned_idents(expr: &Expr, out: &mut HashSet<Ident>) {
    match expr {
        Expr::Lambda(l) => collect_lambda_body_assigned_idents(&l.body, out),
        Expr::Block(b) => collect_block_assigned_idents(b, out),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_assigned_idents(&cond.node, out);
            collect_block_assigned_idents(then_branch, out);
            if let Some(eb) = else_branch {
                collect_block_assigned_idents(eb, out);
            }
        }
        Expr::Switch(s) => {
            collect_expr_assigned_idents(&s.scrutinee.node, out);
            for case in &s.cases {
                collect_block_assigned_idents(&case.body, out);
                if let Some(w) = &case.when {
                    collect_expr_assigned_idents(&w.node, out);
                }
            }
        }
        Expr::SwitchForm(s) => {
            collect_expr_assigned_idents(&s.scrutinee.node, out);
            for arm in &s.arms {
                if let Some(w) = &arm.when {
                    collect_expr_assigned_idents(&w.node, out);
                }
                collect_expr_assigned_idents(&arm.body.node, out);
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_assigned_idents(&left.node, out);
            collect_expr_assigned_idents(&right.node, out);
        }
        // 赋值表达式：目标裸名是记录点（与语句层 Assign 目标同语义），
        // 复合目标递归；值侧递归。
        Expr::Assign { target, value } => {
            if let Expr::Ident(name) = &target.node {
                out.insert(name.clone());
            } else {
                collect_expr_assigned_idents(&target.node, out);
            }
            collect_expr_assigned_idents(&value.node, out);
        }
        Expr::Unary { expr, .. } => collect_expr_assigned_idents(&expr.node, out),
        Expr::Call { func, args, .. } => {
            collect_expr_assigned_idents(&func.node, out);
            for a in args {
                collect_expr_assigned_idents(&a.node, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr_assigned_idents(&receiver.node, out);
            for a in args {
                collect_expr_assigned_idents(&a.node, out);
            }
        }
        Expr::Field { receiver, .. } => {
            collect_expr_assigned_idents(&receiver.node, out);
        }
        Expr::Index { receiver, index } => {
            collect_expr_assigned_idents(&receiver.node, out);
            collect_expr_assigned_idents(&index.node, out);
        }
        Expr::ExpressionLit(_) => {}
        Expr::Await(e) => collect_expr_assigned_idents(&e.node, out),
        Expr::CollectionExpr { elements } => {
            for el in elements {
                collect_expr_assigned_idents(&el.expr().node, out);
            }
        }
        Expr::Cast { expr, .. } => collect_expr_assigned_idents(&expr.node, out),
        Expr::Comptime(inner) => collect_expr_assigned_idents(&inner.node, out),
        Expr::New { args, obj_init, .. } => {
            for a in args {
                collect_expr_assigned_idents(&a.node, out);
            }
            if let Some(init) = obj_init {
                for (_, e) in init {
                    collect_expr_assigned_idents(&e.node, out);
                }
            }
        }
        Expr::Query(_) => {}
        Expr::RefArg { expr, .. } => collect_expr_assigned_idents(&expr.node, out),
        Expr::NamedArg { expr, .. } => collect_expr_assigned_idents(&expr.node, out),
        Expr::StackSpanLit { elements, .. } => {
            for e in elements {
                collect_expr_assigned_idents(&e.node, out);
            }
        }
        Expr::Coalesce { left, right } => {
            collect_expr_assigned_idents(&left.node, out);
            collect_expr_assigned_idents(&right.node, out);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_assigned_idents(&cond.node, out);
            collect_expr_assigned_idents(&then_branch.node, out);
            collect_expr_assigned_idents(&else_branch.node, out);
        }
        Expr::NullCond { access } => collect_expr_assigned_idents(&access.node, out),
        Expr::ForceDeref { access } => collect_expr_assigned_idents(&access.node, out),
        Expr::Box { expr, .. } | Expr::Unbox { expr, .. } => {
            collect_expr_assigned_idents(&expr.node, out)
        }
        Expr::InterpolatedString { parts } => {
            for p in parts {
                if let InterpPart::Expr(hole) = p {
                    collect_expr_assigned_idents(&hole.expr.node, out);
                }
            }
        }
        Expr::Is { expr, .. } => collect_expr_assigned_idents(&expr.node, out),
        Expr::With { receiver, inits } => {
            collect_expr_assigned_idents(&receiver.node, out);
            for (_, e) in inits {
                collect_expr_assigned_idents(&e.node, out);
            }
        }
        Expr::NewArray { length, .. } => collect_expr_assigned_idents(&length.node, out),
        // 叶子（字面量 / 裸名读 / this / default / typeof）：无嵌套语句容器，
        // 裸名读不收集。
        Expr::Ident(_)
        | Expr::Path(_)
        | Expr::This
        | Expr::Base
        | Expr::Null
        | Expr::IntLit(_)
        | Expr::FloatLit(_)
        | Expr::BoolLit(_)
        | Expr::StringLit(_)
        | Expr::CharLit(_)
        | Expr::Default { .. }
        | Expr::TypeOf(_) => {}
    }
}

/// 标量基元（≤ 8 字节，可安全放入 byref_captured_locals 的 malloc(i64 8) 堆槽）。
fn is_scalar_primitive(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
            | TypeId::Bool
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    )
}

/// RFC 023 L2: decide the capture mode for a variable of type `ty`.
///
/// Returns `Some(ByRef)` for reference types (class/string/interface/...) and
/// scalar primitives (int/double/bool/... — 变量捕获：lambda 内变异写回外层
/// 局部), `Some(ByValue)` for larger value types (struct/vector/nullable/Func),
/// and `None` for types that cannot be captured (`Void` / `Generic` / `Infer`
/// / `Error`).
///
/// `Named` is checked against the registry so structs are treated as value
/// types while classes are treated as reference types.
fn capture_mode_for(ty: &TypeId, registry: &TypeRegistry) -> Option<CaptureMode> {
    match ty {
        // Reference types → by-reference capture (pointer).
        // RFC 023: Func 委托按 ByValue 捕获（存闭包指针值本身），而非 ByRef 存栈
        // 参数槽地址。委托参数在栈上，若 ByRef 捕获其地址，订阅函数返回后地址悬垂
        // → use-after-free（Signal.Subscribe 闭包逃逸）。ByValue 存闭包指针值，
        // 闭包体本身在堆上（emit_operand_as_closure → emit_closure_value_heap）。
        TypeId::String
        | TypeId::Object
        | TypeId::Array { .. }
        | TypeId::Task { .. }
        | TypeId::IEnumerable { .. }
        | TypeId::IQueryable { .. }
        | TypeId::Expression { .. } => Some(CaptureMode::ByRef),
        TypeId::Named(n) => {
            if registry.is_class(n) {
                Some(CaptureMode::ByRef)
            } else {
                // structs and other named value types
                Some(CaptureMode::ByValue)
            }
        }
        TypeId::Ref { inner, .. } => {
            // 标量 Ref（`ref int` 局部）保持 ByValue：Ref 槽存「指向被引变量的
            // 指针」而非标量值本身，ByRef 化后 lambda 读路径
            // `load i32, ptr <槽>`（llvm_type_of(Ref{int}) = i32）会把 8 字节
            // ref 指针当标量读 → 垃圾。引用 Ref（`ref class`）递归保持现状。
            if is_scalar_primitive(inner) {
                Some(CaptureMode::ByValue)
            } else {
                capture_mode_for(inner, registry)
            }
        }
        // 标量基元（≤ 8 字节）→ ByValue 快照捕获（建 lambda 时刻的值副本，
        // 外层后续赋值对 lambda 不可见——LambdaCaptureTests.Int_ByValueCapture /
        // MixedCapture / LoopVariable_ByValueSnapshot 断言的语义）。lambda 体内
        // **写**该变量的场景由 compute_captures 的写捕获分析单独提升 ByRef
        //（写传播：ForEach_AppliesAction `x => { sum = sum + x; }` 后
        // Assert.Equal(6, sum) 要求写穿透至外层局部——对快照副本赋值是死代码）。
        // byref_captured_locals 槽统一 malloc(i64 8)（emit_fn.rs），标量基元均
        // ≤ 8 字节安全容纳；Vector(16B)/Nullable/Named struct 可 >8 字节，
        // 保持 ByValue（堆槽溢出）。
        TypeId::Int
        | TypeId::Long
        | TypeId::Short
        | TypeId::Byte
        | TypeId::Char
        | TypeId::Float
        | TypeId::Double
        | TypeId::Bool
        | TypeId::UInt
        | TypeId::ULong
        | TypeId::UShort
        | TypeId::SByte => Some(CaptureMode::ByValue),
        TypeId::Vector { .. }
        | TypeId::Nullable { .. }
        // RFC 023: Func 委托按 ByValue 捕获（存闭包指针值本身），避免 ByRef 存栈
        // 参数槽地址导致订阅函数返回后悬垂（use-after-free）。
        | TypeId::Func { .. } => Some(CaptureMode::ByValue),
        // RFC 005 B3：Span 禁止捕获进堆上闭包。
        TypeId::Span { .. } => None,
        // RFC 040 M-C：泛型方法体（如 `Mediator.SendAsync<TRequest,TResponse>`）内
        // 闭包捕获泛型形参（`() => handler.Handle(request)`，request: TRequest）。
        // 单态化后 concrete 替换闭包体 locals 类型；ByRef 捕获 env 字段恒为 ptr
        //（存外层变量槽地址），codegen `emit_operand` 对捕获局部二次解引用——
        // class 经 `load ptr, ptr <slot>` 得对象指针，值类型经 `load T, ptr <slot>`
        // 得值，两种情形均正确，故 Generic 一律 ByRef（不捕获则闭包体无法解析
        // 泛型形参 → MIR lower unresolved ident）。
        TypeId::Generic(_) => Some(CaptureMode::ByRef),
        // Not capturable.
        TypeId::Void | TypeId::Infer | TypeId::Error => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 对齐 C#：`for` 循环体顶层的 `continue` 前必须注入 increment，
    /// 否则 CFG 将 `continue` 跳回循环头时跳过 increment 导致死循环。
    #[test]
    fn inject_for_increment_before_continue() {
        let inc = vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
        }];
        let body = vec![
            MirStatement::If {
                cond: MirOperand::ConstBool(true),
                then_body: vec![MirStatement::Continue],
                else_body: vec![],
            },
            MirStatement::Continue,
        ];
        let out = inject_for_increment(body, &inc, false);
        // 顶层 continue 前应紧跟 increment。
        match &out[1] {
            MirStatement::Assign { .. } => {}
            other => panic!("top-level continue 前应注入 increment，got {:?}", other),
        }
        assert!(matches!(&out[2], MirStatement::Continue));
        // If 分支内的 continue（同样属于本层 for）前也应注入 increment。
        match &out[0] {
            MirStatement::If { then_body, .. } => {
                assert!(matches!(&then_body[0], MirStatement::Assign { .. }));
                assert!(matches!(&then_body[1], MirStatement::Continue));
            }
            other => panic!("expected If, got {:?}", other),
        }
    }

    /// 嵌套循环内的 `continue` 属于内层循环，不应注入外层 for 的 increment。
    #[test]
    fn inject_for_increment_skips_nested_loop() {
        let inc = vec![MirStatement::Assign {
            place: LocalId(0),
            rvalue: MirRvalue::Use(MirOperand::ConstInt(1)),
        }];
        let body = vec![MirStatement::While {
            cond: MirRvalue::Use(MirOperand::ConstBool(true)),
            body: vec![MirStatement::Continue],
            foreach_source: None,
        }];
        let out = inject_for_increment(body, &inc, false);
        // 嵌套 While 体中的 continue 前不应注入 increment。
        let has_nested_inc = matches!(
            &out[0],
            MirStatement::While { body, .. }
                if matches!(&body[0], MirStatement::Assign { .. })
        );
        assert!(
            !has_nested_inc,
            "嵌套循环内的 continue 不应注入外层 increment"
        );
    }
}
