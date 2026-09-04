//! Native 契约类型校验与符号注册（RFC 016 M1 / M3 扩展）。
//!
//! 将 `NativeModule` 注册为 `StaticClass` 到 `TypeRegistry`，使其方法调用
//! 复用现有 OOP 静态方法分派路径。M1 仅校验白名单类型，不做符号存在性验证。
//!
//! 白名单（M1）：`int`/`long`/`short`/`byte`/`char`/`float`/`double`/`bool`/
//! `string`/`string?`/`void`。其他类型（class/interface/struct/数组等）不允许。
//!
//! 白名单扩展（M3，RFC 016 M2 同期推进）：`object`——FFI Marshal 专用根类型，
//! 对应 C 侧 `void*`。值类型实参 → object 形参时由 typeck 自动插入 `Expr::Box`；
//! object 返回值 → 值类型期望时由 typeck 自动插入 `Expr::Unbox`。
//!
//! 白名单扩展（M3，RFC 016 §3.3）：`NativePtr` 内置透明指针 + 契约 struct 名。
//! `NativePtr` 对应 C `void*`，按值传递；契约 struct 由 `.ani` 的 `native type`
//! 声明，按值传递（LLVM `%struct.<Name>`）。二者均通过 `is_contract_type` 判定。
//!
//! 白名单扩展（M3 §3.3 List<T> marshal）：`List_<T>` 单态化命名。
//! `List<T>` 在 typeck lower 为 `TypeId::Named("List_int")` 等单态化命名；
//! native 形参声明为 `List<T>` 时，codegen 展开为 `ptr buffer, i32 size` 两个
//! LLVM 参数（零拷贝，调用 `rt_list_buffer_and_size` ABI）。typeck 仅需放行
//! `List_<T>` 命名，参数数量保持不变（codegen 层展开）。

use ast::*;

use crate::mangle_generic;
use crate::type_id::TypeId;
use crate::{NominalType, OopMethodSig, ParamSig, TypeChecker, TypeError, TypeKind, TypeRegistry};

/// 从 AST `Type` 提取类型名（用于 native 参数类型）。
///
/// 若为 `Type::Named { path: ["CmpFn"], generics: [] }` 返回 `Some("CmpFn")`；
/// 其他形式返回 `None`。
fn native_param_type_name(ty: &Type) -> Option<Ident> {
    match ty {
        Type::Named { path, generics } if path.len() == 1 && generics.is_empty() => {
            Some(path[0].clone())
        }
        _ => None,
    }
}

/// 判断 `TypeId` 是否在 native 白名单内。
///
/// M1：基元 + string/string?/void。
/// M3 扩展（RFC 016 M2）：加入 `object`——FFI Marshal 专用根类型，对应 C `void*`。
/// M3 扩展（RFC 016 §3.3）：加入 `NativePtr` 内置透明指针 + 契约 struct 名
/// （通过 `contract_types` 参数传入当前模块声明的 struct 名 + 内置 `NativePtr`）。
fn is_whitelist(ty: &TypeId, contract_types: &[Ident]) -> bool {
    match ty {
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
        | TypeId::String
        | TypeId::Void
        | TypeId::Object => true,
        TypeId::Nullable { inner } if inner.as_ref() == &TypeId::String => true,
        // RFC 025 S4：`byte[]`（RtArrayHeader 载体）作为 native 契约参数类型。
        // 与 C shim（rt_crypto_*/rt_quic_*）`arr_len(data)` 读取 header 的形态一致；
        // codegen 直接传 payload 指针（header 位于 payload-8）。
        TypeId::Array { elem } if **elem == TypeId::Byte => true,
        TypeId::Named(name) => is_contract_type(name, contract_types),
        _ => false,
    }
}

/// 判断命名类型是否为契约类型（RFC 016 M3 §3.3）。
///
/// `NativePtr` 是内置的透明指针类型，无需 `.ani` 声明即可使用；
/// 契约 struct 由 `native type Name { ... };` 声明，名收集自 `NativeModule.types`。
///
/// RFC 016 M3 §3.3 List<T> marshal：`List_<T>` 单态化命名也视为契约类型。
/// typeck lower `List<int>` 为 `TypeId::Named("List_int")`，此处放行使其
/// 通过白名单；参数展开（`ptr buffer, i32 size`）由 codegen 层负责。
fn is_contract_type(name: &Ident, contract_types: &[Ident]) -> bool {
    name == "NativePtr" || contract_types.iter().any(|t| t == name) || is_list_type_name(name)
}

/// 判断命名是否为 `List<T>` 类型名（RFC 016 M3 §3.3 List<T> marshal）。
///
/// 同时识别两种形式：
/// - `List_<T>`：单态化后的命名（如 `List_int`、`List_string`），class_templates
///   已加载时 `lower_type(List<int>)` 的结果。
/// - `List`：未单态化的泛型名，class_templates 未加载时 `lower_type` 的 fallback。
///   native module 在 `check_module` 前后注册两次，第一次时 class_templates 为空，
///   `List<int>` 会 fallback 为 `TypeId::Named("List")`；第二次注册会用单态化
///   结果覆盖 ParamSig。两种形式都放行确保两次注册都不报白名单错误。
fn is_list_type_name(name: &Ident) -> bool {
    name == "List" || name.starts_with("List_")
}

/// 提取 `List<T>` 中 `T` 的引用（RFC 016 M3 §3.3 List<T> marshal）。
///
/// 仅识别 AST 形式 `Type::Named { path: ["List"], generics: [T] }`。
/// 返回 `Some([T])` 时调用方直接 mangle 为 `List_<T>`，绕过 `lower_type`
/// 的 class_templates 依赖——避免 native module 注册时机早于 class_templates
/// 填充导致 fallback 为 `Named("List")`，进而与用户代码 lower 出的
/// `Named("List_int")` 类型不匹配。
fn list_generic_args(ty: &Type) -> Option<Vec<&Type>> {
    match ty {
        Type::Named { path, generics }
            if path.len() == 1 && path[0] == "List" && !generics.is_empty() =>
        {
            Some(generics.iter().map(|g| &g.node as &Type).collect())
        }
        _ => None,
    }
}

/// 将 `ParamSig.ty`（Ident 字符串）转回 `TypeId`，用于调用时参数类型校验。
fn param_sig_to_type_id(name: &Ident) -> TypeId {
    match name.as_str() {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "uint" => TypeId::UInt,
        "ulong" => TypeId::ULong,
        "ushort" => TypeId::UShort,
        "sbyte" => TypeId::SByte,
        "string" => TypeId::String,
        "string?" => TypeId::Nullable {
            inner: Box::new(TypeId::String),
        },
        // RFC 016 M2 / RFC 016 M3：object 是 FFI Marshal 专用根类型，对应 C `void*`
        "object" => TypeId::Object,
        "void" => TypeId::Void,
        // RFC 025 S4：`byte[]` 参数（RtArrayHeader 载体）→ 还原为 Array{elem:Byte}，
        // 使调用点实参类型 `byte[]` 与形参匹配（`TypeId::Array{elem:Byte}.display()` = "byte[]"）。
        "byte[]" => TypeId::Array {
            elem: Box::new(TypeId::Byte),
        },
        other => TypeId::Named(other.into()),
    }
}

/// 将 `TypeId` 反向构造为 `ast::Type`，用于构建 `Expr::Box`/`Expr::Unbox` 节点的 `value_ty`。
///
/// 仅覆盖 FFI 装箱场景涉及的类型：基元类型与命名类型（struct/class）。
/// 复合类型（数组、泛型等）不在 FFI Marshal 装箱范围内，遇之返回 `Named("unknown")`。
pub(crate) fn type_id_to_ast_type(ty: &TypeId) -> Type {
    match ty {
        TypeId::Int => Type::Named {
            path: vec!["int".into()],
            generics: vec![],
        },
        TypeId::Long => Type::Named {
            path: vec!["long".into()],
            generics: vec![],
        },
        TypeId::Short => Type::Named {
            path: vec!["short".into()],
            generics: vec![],
        },
        TypeId::Byte => Type::Named {
            path: vec!["byte".into()],
            generics: vec![],
        },
        TypeId::Char => Type::Named {
            path: vec!["char".into()],
            generics: vec![],
        },
        TypeId::Float => Type::Named {
            path: vec!["float".into()],
            generics: vec![],
        },
        TypeId::Double => Type::Named {
            path: vec!["double".into()],
            generics: vec![],
        },
        TypeId::Bool => Type::Named {
            path: vec!["bool".into()],
            generics: vec![],
        },
        TypeId::UInt => Type::Named {
            path: vec!["uint".into()],
            generics: vec![],
        },
        TypeId::ULong => Type::Named {
            path: vec!["ulong".into()],
            generics: vec![],
        },
        TypeId::UShort => Type::Named {
            path: vec!["ushort".into()],
            generics: vec![],
        },
        TypeId::SByte => Type::Named {
            path: vec!["sbyte".into()],
            generics: vec![],
        },
        TypeId::String => Type::Named {
            path: vec!["string".into()],
            generics: vec![],
        },
        TypeId::Object => Type::Named {
            path: vec!["object".into()],
            generics: vec![],
        },
        TypeId::Named(name) => Type::Named {
            path: vec![name.clone()],
            generics: vec![],
        },
        // RFC 037 M1 配套：`typed_block_to_block`（Expr::If 分支重建）需把
        // 委托局部（`Action a = ...`）的 `TypeId::Func` 反向还原为 `ast::Type::Func`，
        // 否则落入下方 `_ => "unknown"`，MIR lower 把 `a()` 误判为直接函数调用
        // `call void @a()` → LLVM undefined symbol（data_driven_unsubscribe_e2e）。
        // 数组/泛型同理需保真，避免 if 分支内 `T[]` 等声明降级为 unknown。
        TypeId::Func { params, ret } => Type::Func {
            params: params
                .iter()
                .map(|p| Spanned::new(type_id_to_ast_type(p), Span::DUMMY))
                .collect(),
            ret: Box::new(Spanned::new(type_id_to_ast_type(ret), Span::DUMMY)),
        },
        TypeId::Array { elem } => Type::Array {
            inner: Box::new(Spanned::new(type_id_to_ast_type(elem), Span::DUMMY)),
        },
        // `Type? rt` 等可空局部经 `typed_block_to_block` 重建为 `Stmt::Let` 时，
        // 必须保真还原为 `Type::Nullable`。否则落入 `_ => "unknown"`，MIR 将
        // `rt` 局部类型标成 `Named("unknown")`，`rt.Name` 无法识别为 custom
        // accessor（get_Name getter）而走直接字段读取（错位 i32），触发 LLVM
        // 类型不匹配（i32 vs ptr）。参考同文件 RFC 037 M1 对 Func/Array 的保真。
        TypeId::Nullable { inner } => Type::Nullable {
            inner: Box::new(Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)),
        },
        // try/catch 等经 `typed_block_to_block` 重建时必须保真 `Task`/`Span` 等内建类型。
        // 否则落入 `_ => Named("unknown")`，MIR 将 `Task authTask` 标成 `Named("unknown")`，
        // `authTask.Wait()` 触发 codegen「unresolved receiver」panic（web_mb / web_core_auth）。
        TypeId::Task { inner } => {
            // 非泛型 `Task` ≡ `Task<void>`：与 check_type 拦截对齐，不写 void 泛参，
            // 避免 MIR `lower_type_name` 产出 `Named("Task")` 与内建 `TypeId::Task` 分叉。
            if matches!(inner.as_ref(), TypeId::Void) {
                Type::Named {
                    path: vec!["Task".into()],
                    generics: vec![],
                }
            } else {
                Type::Named {
                    path: vec!["Task".into()],
                    generics: vec![Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)],
                }
            }
        }
        TypeId::Span { elem, mutable } => Type::Named {
            path: vec![if *mutable { "Span" } else { "ReadOnlySpan" }.into()],
            generics: vec![Spanned::new(type_id_to_ast_type(elem), Span::DUMMY)],
        },
        TypeId::IEnumerable { inner } => Type::Named {
            path: vec!["IEnumerable".into()],
            generics: vec![Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)],
        },
        TypeId::IQueryable { inner } => Type::Named {
            path: vec!["IQueryable".into()],
            generics: vec![Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)],
        },
        TypeId::Expression { inner } => Type::Named {
            path: vec!["Expression".into()],
            generics: vec![Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)],
        },
        TypeId::Vector { elem, n } => Type::Named {
            path: vec!["Vector".into()],
            generics: vec![
                Spanned::new(type_id_to_ast_type(elem), Span::DUMMY),
                Spanned::new(Type::ConstInt(*n as i64), Span::DUMMY),
            ],
        },
        TypeId::Void => Type::Named {
            path: vec!["void".into()],
            generics: vec![],
        },
        TypeId::Generic(n) => Type::Named {
            path: vec![n.clone()],
            generics: vec![],
        },
        TypeId::Ref { inner, mutable, .. } => Type::Ref {
            inner: Box::new(Spanned::new(type_id_to_ast_type(inner), Span::DUMMY)),
            mutable: *mutable,
        },
        _ => Type::Named {
            path: vec!["unknown".into()],
            generics: vec![],
        },
    }
}

/// 值类型 → object 自动装箱的统一入口（RFC 004 P0 Phase 1）。
///
/// 当目标位置类型为 `object` 且实参为可装箱类型时，将表达式包装为 `Expr::Box`
/// （codegen 据此发射装箱：`string` → `rt_string_box`，使 object 槽持有 string
/// 有合法 vtable → `o is string` 可识别；基元 → `rt_box_create` + vtable store，
/// 使 `o is int` 可识别、`(int)o` 可拆箱；struct → 深拷贝装箱）。不匹配时原样返回 `expr`。
///
/// `param_ty` 为形参类型名（如 `"object"`）。enum 装箱留待 Phase 4，不在本入口覆盖。
pub(crate) fn box_to_object(
    registry: &TypeRegistry,
    expr: Expr,
    val_ty: &TypeId,
    param_ty: &str,
    span: ast::Span,
) -> Expr {
    if param_ty == "object" && is_boxable_value_type(registry, val_ty) {
        let value_ty = ast::Spanned::new(type_id_to_ast_type(val_ty), span);
        Expr::Box {
            expr: Box::new(ast::Spanned::new(expr, span)),
            value_ty,
        }
    } else {
        expr
    }
}

/// RFC 004 P0 Phase 2：可装箱值类型判定（基元 + struct + string；enum 留待 Phase 4）。
///
/// `string` 经 `rt_string_box` 装箱；基元经 `rt_box_create` + vtable store；
/// struct 经 `rt_box_create` + 逐字段深拷贝。class/array/Task/Func 已是引用类型，
/// 直接透传无需装箱。
pub(crate) fn is_boxable_value_type(registry: &TypeRegistry, ty: &TypeId) -> bool {
    if *ty == TypeId::String || is_boxable_primitive(ty) {
        return true;
    }
    matches!(ty, TypeId::Named(n) if registry.is_struct(n))
}

/// 判断 `TypeId` 是否为 Phase 1 可装箱基元（`object o = 5` 之类）。
///
/// 仅含拥有运行时 typeinfo（`@rt_typeinfo_<prim>`）的 8 个基元；
/// `uint`/`ulong`/`ushort`/`sbyte` 无运行时 typeinfo，struct/enum 需 Phase 2/4
/// 的深拷贝/哈希语义，均不在其列。
pub(crate) fn is_boxable_primitive(ty: &TypeId) -> bool {
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
    )
}

/// 判断 `TypeId` 是否为可装箱的值类型（用于 FFI `object` 形参的自动装箱决策）。
///
/// M2 范围：基元类型 + struct（Named）。class/string 已是引用类型，无需装箱；
/// `object` 本身也无需装箱（直接透传）。具体类/struct 的区分由 typeck 时
/// `registry.is_class`/`is_struct` 决定，但此处采用宽松判断——任何 Named
/// 都视为可装箱值类型，由 codegen 在 size 推导时区分。
pub(crate) fn is_value_type_for_boxing(ty: &TypeId) -> bool {
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
            | TypeId::Named(_)
    )
}

impl TypeChecker {
    /// 注册 native 契约模块到类型注册表，并缓存以便 `check_module` 重建 registry 后重注册。
    ///
    /// 每个 `NativeModule` 注册为 `StaticClass`，其函数注册为静态方法。
    /// 应在 `check_module` 之前调用；`check_module` 重建 registry 后会自动
    /// 用缓存的 native_modules 重注册，保证 `libc.puts(...)` 等分派可用。
    pub fn register_native_modules(&mut self, modules: &[NativeModule]) {
        self.native_modules = modules.to_vec();
        for module in modules {
            self.check_and_register_native_module(module);
        }
    }

    /// RFC 016 M1: 重注册缓存的 native 模块到当前 registry。
    ///
    /// `check_module` 用 `TypeRegistry::from_module` 重建 registry 后调用此方法，
    /// 将之前 `register_native_modules` 缓存的 native 契约重新注册，保证
    /// native 方法分派可用。不更新 `self.native_modules` 缓存。
    pub(crate) fn reregister_native_modules(&mut self) {
        let modules = std::mem::take(&mut self.native_modules);
        for module in &modules {
            self.check_and_register_native_module(module);
        }
        self.native_modules = modules;
    }

    fn check_and_register_native_module(&mut self, module: &NativeModule) {
        if self.registry.types.contains_key(&module.name) {
            self.errors.push(TypeError::Oop(format!(
                "duplicate native module `{}`",
                module.name
            )));
            return;
        }

        // RFC 016 M3 §3.4 能力 gating Phase 1+：记录 native 模块的 capability 标签。
        // `check_native_method` 调用时据此校验调用方 namespace 是否声明了对应能力。
        // Phase 0 兼容：`capability = None` 表示无能力要求，任何 namespace 都可调用。
        self.native_caps
            .insert(module.name.clone(), module.capability.clone());

        // RFC 016 M4 / RFC 016（2026-08-03 用户裁决简化）：`library` 字面量形态
        // 的编译期校验——路径**非空**；不做存在性强制（编译期允许指向尚未安装/
        // 尚未创建的目录，由链接/运行期决定）。相对路径基准=执行程序根目录。
        if let Some(lib) = &module.library {
            if lib.as_os_str().is_empty() {
                self.errors.push(TypeError::Oop(format!(
                    "native module `{}`: `library` 字面量路径不能为空串",
                    module.name
                )));
            }
        }

        // RFC 016（2026-08-03 扩展）：`library = Environment.GetEnvironmentVariable(...)`
        // 环境变量形式的编译期强类型检测。
        if let Some(env_var) = &module.library_env_var {
            // 接收者类存在性：`Environment` 必须是已登记的 builtin facade 类
            //（`std/Arc/Environment.as` 的 `GetEnvironmentVariable` 映射到
            // `rt_env_get_var`，见 Environment.as 注释）。
            if crate::builtin_facade::classify_builtin_facade("Environment").is_none() {
                self.errors.push(TypeError::Oop(format!(
                    "native module `{}`: `library` 环境变量形式要求接收者 `Environment` \
                     静态类存在（std/Arc/Environment.as）；当前编译未提供该 facade",
                    module.name
                )));
            }
            // 环境变量名必须为非空字符串字面量。
            if env_var.is_empty() {
                self.errors.push(TypeError::Oop(format!(
                    "native module `{}`: `library` 环境变量名不能为空串",
                    module.name
                )));
            }
            // 环境变量形式是运行时解析语义——static 链接需要编译期可定位的
            // 字面量路径，二者语义互斥（单一惯用法：字面量 XOR 环境变量）。
            if module.load == LoadStrategy::Static {
                self.errors.push(TypeError::Oop(format!(
                    "native module `{}`: `library` 环境变量形式 \
                     （Environment.GetEnvironmentVariable）仅适用于 `load = \"runtime\"` 或 \
                     `load = \"auto\"`；static 链接需要编译期可定位的字面量 `library` 路径",
                    module.name
                )));
            }
        }

        // RFC 016 M1：注册 native callback 类型到 typeck。
        for cb in &module.callbacks {
            self.native_callbacks.insert(cb.name.clone(), cb.clone());
        }

        // RFC 016 M3 §3.3：收集契约类型名（内置 NativePtr + 模块声明的 struct/OpaquePtr 名
        // + RFC 016 M1 callback 名）。
        // 传给 `check_native_fn` 供 `is_whitelist` 判定命名类型是否为契约类型。
        let contract_types: Vec<Ident> = std::iter::once("NativePtr".into())
            .chain(module.types.iter().map(|t| t.name.clone()))
            .chain(module.callbacks.iter().map(|c| c.name.clone()))
            .collect();

        let mut methods: indexmap::IndexMap<Ident, Vec<OopMethodSig>> = indexmap::IndexMap::new();
        for fn_decl in &module.functions {
            match self.check_native_fn(fn_decl, &contract_types) {
                Ok(sig) => {
                    methods.entry(sig.name.clone()).or_default().push(sig);
                }
                Err(e) => {
                    self.errors.push(e);
                }
            }
        }

        self.registry.types.insert(
            module.name.clone(),
            NominalType {
                name: module.name.clone(),
                kind: TypeKind::StaticClass,
                vis: Visibility::Public,
                is_abstract: false,
                is_record: false,
                is_readonly: false,
                fields: indexmap::IndexMap::new(),
                methods,
                bases: vec![],
                base_types: vec![],
                span: Span::DUMMY,
                variants: vec![],
                generic_params: vec![],
                namespace: vec![],
                const_values: indexmap::IndexMap::new(),
                constructors: vec![],
                soa: false,
                required_props: Default::default(),
            },
        );
    }

    fn check_native_fn(
        &mut self,
        fn_decl: &NativeFn,
        contract_types: &[Ident],
    ) -> Result<OopMethodSig, TypeError> {
        let mut params = Vec::new();
        for p in &fn_decl.params {
            // RFC 016 M1：若参数类型名匹配 native callback 注册表，直接放行（callback 是函数指针类型）。
            let cb_name = native_param_type_name(&p.ty.node);
            let is_callback = cb_name
                .as_ref()
                .is_some_and(|n| self.native_callbacks.contains_key(n.as_str()));

            // RFC 016 M3 §3.3 List<T> marshal：对 `List<T>` 形参直接 mangle 为
            // `Named("List_<T>")`，绕过 `lower_type` 的 class_templates 依赖。
            // native module 注册发生在 `check_module` 之前/早期（reregister 在
            // `from_module` 后但仍在 `check_module_items` 之前），此时
            // `class_templates` 未填充，`lower_type(List<int>)` 会 fallback 为
            // `Named("List")`，与用户代码 `new List<int>()` lower 出的
            // `Named("List_int")` 不一致，导致调用点类型不匹配。
            let ty_id = if is_callback {
                // Callback 类型：直接使用 Named(callback_name)，跳过 lower_type。
                TypeId::Named(cb_name.unwrap())
            } else if let Some(args) = list_generic_args(&p.ty.node) {
                let mangled_args: Vec<TypeId> = args
                    .iter()
                    .map(|t| self.lower_type(t))
                    .collect::<Result<_, _>>()?;
                TypeId::Named(mangle_generic("List", &mangled_args).into())
            } else {
                self.lower_type(&p.ty.node)?
            };
            if !is_whitelist(&ty_id, contract_types) {
                return Err(TypeError::Oop(format!(
                    "native contract parameter `{}` type `{}` not in whitelist \
                     (int/long/short/byte/char/float/double/bool/string/string?/object/NativePtr/contract struct/List<T>)",
                    p.name,
                    ty_id.display()
                )));
            }
            params.push(ParamSig {
                name: p.name.clone(),
                ty: ty_id.display().into(),
                is_ref: false,
                is_out: false,
                is_in: false,
                is_params: false,
                default: None,
            });
        }

        let ret = match &fn_decl.ret {
            Some(t) => {
                // RFC 016 M3 §3.3：返回值若为 `List<T>` 同样直接 mangle。
                let ty_id = if let Some(args) = list_generic_args(&t.node) {
                    let mangled_args: Vec<TypeId> = args
                        .iter()
                        .map(|t2| self.lower_type(t2))
                        .collect::<Result<_, _>>()?;
                    TypeId::Named(mangle_generic("List", &mangled_args).into())
                } else {
                    self.lower_type(&t.node)?
                };
                if !is_whitelist(&ty_id, contract_types) {
                    return Err(TypeError::Oop(format!(
                        "native contract return type `{}` not in whitelist",
                        ty_id.display()
                    )));
                }
                ty_id.display().into()
            }
            None => Ident::from("void"),
        };

        Ok(OopMethodSig {
            name: fn_decl.name.clone(),
            vis: Visibility::Public,
            params,
            ret,
            modifier: MethodModifier::Static,
            is_async: false,
            generics: vec![],
            is_static_abstract: false,
        })
    }

    /// 从 registry 查找 native/static 方法并校验调用参数。
    ///
    /// 在 `check_builtin_static_method` 的 `_ =>` fallback 中调用：
    /// - `Ok(Some(ty))`：匹配成功，返回返回类型；`args` 可能已被就地修改（RFC 016 M2）
    /// - `Ok(None)`：`type_name` 或 `method` 未在 registry 中注册，交由上层继续分派
    /// - `Err(...)`：找到签名但参数不匹配，编译错误
    ///
    /// RFC 016 v2 M2 / RFC 016 M3：FFI `object` 形参自动装箱。
    /// 当形参类型为 `object` 且实参为值类型时，将实参就地包装为 `Expr::Box`。
    /// 实参本身已是 `object` 时直接透传（无需装箱）。返回类型为 `object` 时
    /// 不在此处拆箱——拆箱需调用点上下文（赋值目标/强转目标），由
    /// `Expr::Cast` 分支单独处理（typeck 在 Cast 源为 object、目标为值类型时
    /// 转化为 `Expr::Unbox`）。
    pub(crate) fn check_native_method(
        &mut self,
        type_name: &Ident,
        method: &Ident,
        args: &mut [Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        // 仅处理 native 契约模块。用户 `static class`（如 `Assert`）也是
        // StaticClass，但须走普通重载解析（按实参类型），否则会按 arity
        // 误取首个重载（`Assert.Equal("a","b")` → `Equal(int,int)`）。
        if !self.native_caps.contains_key(type_name) {
            return Ok(None);
        }
        // 先用不可变借用查 registry，避免提前借用 self
        let (sigs, ret_ty) = {
            let Some(nom) = self.registry.types.get(type_name) else {
                return Ok(None);
            };
            if nom.kind != TypeKind::StaticClass {
                return Ok(None);
            }
            let Some(sigs) = nom.methods.get(method) else {
                return Ok(None);
            };
            // 收集参数数量匹配的候选签名的返回类型
            let matched: Option<&OopMethodSig> = sigs.iter().find(|s| s.params.len() == args.len());
            match matched {
                Some(sig) => (sig.clone(), param_sig_to_type_id(&sig.ret)),
                None => {
                    return Err(TypeError::Mismatch {
                        expected: format!(
                            "`{}.{}` expects {} argument(s)",
                            type_name,
                            method,
                            sigs.first().map(|s| s.params.len()).unwrap_or(0)
                        ),
                        found: format!("{} arguments", args.len()),
                    });
                }
            }
        };

        // RFC 016 M3 §3.4 能力 gating Phase 1+（[4.4 能力系统]）：
        // 若 native 模块声明了 `capability`，调用方所在 namespace 的有效能力集
        //（沿父链继承的并集）必须包含该 capability，否则报错。
        // Phase 0 兼容：`None` 表示无能力要求，任何 namespace 都可调用。
        if let Some(Some(required_cap)) = self.native_caps.get(type_name) {
            let current_caps = self.current_namespace_caps();
            if !current_caps.iter().any(|c| c == required_cap) {
                return Err(TypeError::Oop(format!(
                    "namespace {:?} 未声明能力 `{}`，无法调用 native 模块 `{}` 的方法 `{}`；\
                     请在 namespace 声明处添加 `capability {}`，或使用已声明该能力的 namespace",
                    self.enclosing_namespace, required_cap, type_name, method, required_cap,
                )));
            }
        }

        // 检查实参类型；对 `object` 形参放宽类型检查（允许任意值类型或 object）。
        // 若形参为 `object` 且实参为值类型，就地包装为 `Expr::Box`（非用户书写）。
        // RFC 016 M1：若形参是 native callback 类型，接受 Func（lambda）实参。
        for (i, expected) in sigs.params.iter().enumerate() {
            let expected_ty = param_sig_to_type_id(&expected.ty);
            let arg_ty = self.check_expr_at(args[i].span, &args[i].node)?.ty;

            // RFC 016 M1：native callback 形参 → 接受 lambda（Func）类型。
            let is_cb_param = self
                .native_callbacks
                .contains_key::<str>(expected.ty.as_ref());
            let arg_is_lambda = matches!(&args[i].node, Expr::Lambda(_));

            let compatible = if is_cb_param && arg_is_lambda {
                // Lambda 实参数对 callback 形参：接受（M1 仅 no-capture，codegen 检查）。
                true
            } else if is_cb_param && matches!(&arg_ty, TypeId::Func { .. }) {
                // 变量引用（如 `let f = (a, b) => ...; lib.qsort(arr, n, size, f);`）
                // 也接受；codegen 根据 MIR operand 类型决定走 trampoline 或报错。
                true
            } else if expected_ty == TypeId::Object {
                // object 形参：接受值类型（需装箱）或 object 本身（透传）
                arg_ty == TypeId::Object || is_value_type_for_boxing(&arg_ty)
            } else {
                self.types_compatible(&expected_ty, &arg_ty)
            };
            if !compatible {
                return Err(TypeError::Mismatch {
                    expected: expected_ty.display(),
                    found: arg_ty.display(),
                });
            }

            // FFI 装箱：object 形参 + 值类型实参 → Expr::Box
            if expected_ty == TypeId::Object && arg_ty != TypeId::Object {
                let original = std::mem::replace(
                    &mut args[i].node,
                    Expr::IntLit(0), // 占位，待替换
                );
                let value_ty = Spanned::new(type_id_to_ast_type(&arg_ty), args[i].span);
                args[i].node = Expr::Box {
                    expr: Box::new(Spanned::new(original, args[i].span)),
                    value_ty,
                };
            }
        }

        Ok(Some(ret_ty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{Span, Spanned};

    fn make_native_module() -> NativeModule {
        NativeModule {
            name: "libc".into(),
            functions: vec![
                NativeFn {
                    name: "puts".into(),
                    symbol: None,
                    params: vec![NativeParam {
                        name: "s".into(),
                        ty: Spanned::new(
                            Type::Named {
                                path: vec!["string".into()],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        direction: ParamDirection::default(),
                    }],
                    ret: Some(Spanned::new(
                        Type::Named {
                            path: vec!["int".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    )),
                    calling_conv: CallingConv::default(),
                },
                NativeFn {
                    name: "getenv".into(),
                    symbol: None,
                    params: vec![NativeParam {
                        name: "name".into(),
                        ty: Spanned::new(
                            Type::Named {
                                path: vec!["string".into()],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        direction: ParamDirection::default(),
                    }],
                    ret: Some(Spanned::new(
                        Type::Nullable {
                            inner: Box::new(Spanned::new(
                                Type::Named {
                                    path: vec!["string".into()],
                                    generics: vec![],
                                },
                                Span::DUMMY,
                            )),
                        },
                        Span::DUMMY,
                    )),
                    calling_conv: CallingConv::default(),
                },
            ],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        }
    }

    #[test]
    fn register_native_module_basic() {
        let mut tc = TypeChecker::new();
        tc.register_native_modules(&[make_native_module()]);
        assert!(
            tc.errors.is_empty(),
            "expected no errors, got: {:?}",
            tc.errors
        );

        let libc = tc
            .registry()
            .types
            .get("libc")
            .expect("libc not registered");
        assert_eq!(libc.kind, TypeKind::StaticClass);
        assert!(libc.methods.contains_key("puts"));
        assert!(libc.methods.contains_key("getenv"));
    }

    #[test]
    fn register_native_module_whitelist_rejects_non_primitive() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "bad".into(),
            functions: vec![NativeFn {
                name: "f".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "x".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["SomeClass".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(!tc.errors.is_empty(), "expected whitelist rejection error");
    }

    #[test]
    fn register_native_module_duplicate_rejected() {
        let mut tc = TypeChecker::new();
        tc.register_native_modules(&[make_native_module()]);
        tc.errors.clear();
        tc.register_native_modules(&[make_native_module()]);
        assert!(!tc.errors.is_empty(), "expected duplicate module error");
    }

    #[test]
    fn check_native_method_returns_correct_type() {
        let mut tc = TypeChecker::new();
        tc.register_native_modules(&[make_native_module()]);
        assert!(tc.errors.is_empty());

        // libc.puts(string) -> int
        let arg = Spanned::new(Expr::StringLit("hello".into()), Span::DUMMY);
        let ty = tc
            .check_native_method(&"libc".into(), &"puts".into(), &mut [arg])
            .unwrap()
            .expect("expected puts to match");
        assert_eq!(ty, TypeId::Int);
    }

    #[test]
    fn check_native_method_returns_none_for_unknown_module() {
        let mut tc = TypeChecker::new();
        let result = tc.check_native_method(&"nonexistent".into(), &"f".into(), &mut Vec::new());
        assert!(matches!(result, Ok(None)));
    }

    /// RFC 016 M3 §3.3：`NativePtr` 内置透明指针应在白名单内。
    #[test]
    fn register_native_module_whitelist_accepts_native_ptr() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "libc".into(),
            functions: vec![NativeFn {
                name: "malloc".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "size".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["int".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["NativePtr".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            tc.errors.is_empty(),
            "NativePtr return should be whitelisted, got: {:?}",
            tc.errors
        );
    }

    /// RFC 016 M3 §3.3：契约 struct 名应在白名单内（按值传递）。
    #[test]
    fn register_native_module_whitelist_accepts_contract_struct() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "libc".into(),
            functions: vec![NativeFn {
                name: "make_point".into(),
                symbol: None,
                params: vec![
                    NativeParam {
                        name: "x".into(),
                        ty: Spanned::new(
                            Type::Named {
                                path: vec!["int".into()],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        direction: ParamDirection::default(),
                    },
                    NativeParam {
                        name: "y".into(),
                        ty: Spanned::new(
                            Type::Named {
                                path: vec!["int".into()],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        direction: ParamDirection::default(),
                    },
                ],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["Point".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![NativeTypeDecl {
                name: "Point".into(),
                kind: NativeTypeKind::Struct {
                    fields: vec![
                        (
                            "x".into(),
                            Spanned::new(
                                Type::Named {
                                    path: vec!["int".into()],
                                    generics: vec![],
                                },
                                Span::DUMMY,
                            ),
                        ),
                        (
                            "y".into(),
                            Spanned::new(
                                Type::Named {
                                    path: vec!["int".into()],
                                    generics: vec![],
                                },
                                Span::DUMMY,
                            ),
                        ),
                    ],
                },
            }],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            tc.errors.is_empty(),
            "contract struct Point should be whitelisted, got: {:?}",
            tc.errors
        );
    }

    /// RFC 016 M3 §3.4：`capability` 字段 Phase 0 仅记录不强制。
    #[test]
    fn register_native_module_capability_phase0_recorded_not_enforced() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "libsqlite3".into(),
            functions: vec![NativeFn {
                name: "sqlite3_open".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "filename".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["string".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: Some("io.Db".into()),
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        // Phase 0：capability 仅记录，不产生错误
        assert!(
            tc.errors.is_empty(),
            "Phase 0 should not enforce capability, got: {:?}",
            tc.errors
        );
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+：调用带 capability 的 native 方法
    /// 时，当前 namespace 有效能力集必须包含该 capability，否则报错。
    ///
    /// 此测试不声明任何 namespace capability（栈底为根层空 Vec），
    /// 调用 `io.Db` 能力的 `libsqlite3.open` 应被拒绝。
    #[test]
    fn capability_phase1_rejects_call_without_namespace_cap() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "libsqlite3".into(),
            functions: vec![NativeFn {
                name: "open".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "filename".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["string".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: Some("io.Db".into()),
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(tc.errors.is_empty(), "registration should not error");

        // 根 namespace 无 capability 声明 → 调用应被拒绝
        let arg = Spanned::new(Expr::StringLit("test.db".into()), Span::DUMMY);
        let result = tc.check_native_method(&"libsqlite3".into(), &"open".into(), &mut [arg]);
        assert!(
            matches!(result, Err(TypeError::Oop(_))),
            "expected capability rejection, got: {:?}",
            result
        );
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+：当前 namespace 声明了对应能力时，
    /// 调用带 capability 的 native 方法应通过。
    #[test]
    fn capability_phase1_passes_when_namespace_has_cap() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "libsqlite3".into(),
            functions: vec![NativeFn {
                name: "open".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "filename".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["string".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: Some("io.Db".into()),
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);

        // 模拟进入一个声明了 `io.Db` 能力的 namespace
        tc.namespace_caps_stack.push(vec!["io.Db".into()]);
        let arg = Spanned::new(Expr::StringLit("test.db".into()), Span::DUMMY);
        let result = tc.check_native_method(&"libsqlite3".into(), &"open".into(), &mut [arg]);
        assert!(
            result.is_ok(),
            "expected capability check to pass, got: {:?}",
            result
        );
        tc.namespace_caps_stack.pop();
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+：native 模块无 capability 标签时
    ///（Phase 0 兼容场景），任何 namespace 都可调用，不强制能力声明。
    #[test]
    fn capability_phase1_passes_when_native_has_no_cap() {
        let mut tc = TypeChecker::new();
        // capability = None 的 native module（如 libc）
        let module = NativeModule {
            name: "libc".into(),
            functions: vec![NativeFn {
                name: "puts".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "s".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["string".into()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);

        // 根 namespace 无任何 capability 声明，调用无 capability 要求的 libc.puts 应通过
        let arg = Spanned::new(Expr::StringLit("hi".into()), Span::DUMMY);
        let result = tc.check_native_method(&"libc".into(), &"puts".into(), &mut [arg]);
        assert!(
            result.is_ok(),
            "Phase 0 compat: no-capability native call should pass, got: {:?}",
            result
        );
    }

    /// RFC 016 M3 §3.4 能力 gating Phase 1+：子 namespace 继承父 namespace 的
    /// capabilities。`current_namespace_caps` 返回栈中所有层级的并集。
    #[test]
    fn current_namespace_caps_inherits_parent_layers() {
        let mut tc = TypeChecker::new();
        // 根层（栈底）：空 Vec
        assert!(tc.current_namespace_caps().is_empty());

        // 父 namespace 声明 io
        tc.namespace_caps_stack.push(vec!["io".into()]);
        assert_eq!(tc.current_namespace_caps(), ["io"]);

        // 子 namespace 声明 db —— 应继承父的 io
        tc.namespace_caps_stack.push(vec!["db".into()]);
        let caps = tc.current_namespace_caps();
        assert!(
            caps.contains(&"io".into()),
            "child should inherit parent's io"
        );
        assert!(caps.contains(&"db".into()), "child should have its own db");

        // 离开子 namespace，仅剩根 + 父
        tc.namespace_caps_stack.pop();
        assert_eq!(tc.current_namespace_caps(), ["io"]);

        // 离开父 namespace，仅剩根
        tc.namespace_caps_stack.pop();
        assert!(tc.current_namespace_caps().is_empty());
    }

    /// RFC 016 M3 §3.3 List<T> marshal：`List<int>` 形参应在白名单内。
    ///
    /// 验证：`check_native_fn` 对 `List<int>` 形参直接 mangle 为
    /// `TypeId::Named("List_int")`，绕过 `lower_type` 的 class_templates 依赖。
    /// 这样无论 native module 在 `check_module` 之前还是之后注册，
    /// ParamSig.ty 都是 `"List_int"`，与用户代码 `new List<int>()` lower 出的
    /// `Named("List_int")` 一致，避免调用点类型不匹配。
    #[test]
    fn register_native_module_whitelist_accepts_list_generic() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "arc_test".into(),
            functions: vec![NativeFn {
                name: "sum_list".into(),
                symbol: None,
                params: vec![NativeParam {
                    name: "xs".into(),
                    ty: Spanned::new(
                        Type::Named {
                            path: vec!["List".into()],
                            generics: vec![Spanned::new(
                                Type::Named {
                                    path: vec!["int".into()],
                                    generics: vec![],
                                },
                                Span::DUMMY,
                            )],
                        },
                        Span::DUMMY,
                    ),
                    direction: ParamDirection::default(),
                }],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: None,
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            tc.errors.is_empty(),
            "List<int> parameter should be whitelisted, got: {:?}",
            tc.errors
        );

        // RFC 016 M3 §3.3：验证 ParamSig.ty 被正确 mangle 为 "List_int"，
        // 与用户代码 `new List<int>()` lower 出的 `Named("List_int")` 一致。
        let arc_test = tc
            .registry()
            .types
            .get("arc_test")
            .expect("arc_test module should be registered");
        let sum_list_sigs = arc_test
            .methods
            .get("sum_list")
            .expect("sum_list method should be registered");
        assert_eq!(
            sum_list_sigs.len(),
            1,
            "expected exactly one sum_list signature"
        );
        assert_eq!(
            sum_list_sigs[0].params[0].ty.as_str(),
            "List_int",
            "ParamSig.ty should be mangled to 'List_int', got: {:?}",
            sum_list_sigs[0].params[0].ty
        );
    }

    /// RFC 016（2026-08-03）：`library` 环境变量形式 + `load = "runtime"` 注册通过。
    #[test]
    fn register_native_module_env_var_library_runtime_ok() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "gpu".into(),
            functions: vec![NativeFn {
                name: "init".into(),
                symbol: None,
                params: vec![],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: Some("ARC_GPU_LIB".into()),
            source: None,
            load: LoadStrategy::Runtime,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            tc.errors.is_empty(),
            "env-var library with runtime load should register, got: {:?}",
            tc.errors
        );
    }

    /// RFC 016（2026-08-03）：`library` 环境变量形式 + `load = "auto"` 注册通过。
    #[test]
    fn register_native_module_env_var_library_auto_ok() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "gpu_auto".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: Some("ARC_GPU_PATH".into()),
            source: None,
            load: LoadStrategy::Auto,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            tc.errors.is_empty(),
            "env-var library with auto load should register, got: {:?}",
            tc.errors
        );
    }

    /// RFC 016（2026-08-03）：`library` 环境变量形式 + static（缺省）→ 编译期强类型
    /// 检测报错：运行时语义与 static 链接互斥。
    #[test]
    fn register_native_module_env_var_library_rejects_static_load() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "gpu_static".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: Some("ARC_GPU_LIB".into()),
            source: None,
            load: LoadStrategy::Static,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(
            !tc.errors.is_empty(),
            "env-var library with static load must be rejected"
        );
    }

    /// RFC 016（2026-08-03）：环境变量名为空串 → 编译期强类型检测报错。
    #[test]
    fn register_native_module_env_var_library_rejects_empty_name() {
        let mut tc = TypeChecker::new();
        let module = NativeModule {
            name: "gpu_empty".into(),
            functions: vec![],
            types: vec![],
            capability: None,
            library: None,
            library_env_var: Some(String::new()),
            source: None,
            load: LoadStrategy::Runtime,
            callbacks: vec![],
        };
        tc.register_native_modules(&[module]);
        assert!(!tc.errors.is_empty(), "empty env-var name must be rejected");
    }
}
