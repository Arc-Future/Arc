use super::check_builtin::is_builtin_static_abstract_iface;
use super::check_native::is_boxable_primitive;
use super::*;

impl TypeChecker {
    pub(crate) fn access_ctx(&self) -> AccessContext {
        AccessContext {
            current_type: self.current_class.clone(),
            extension_scope: ExtensionScope {
                imported: self.extension_imports.clone(),
                enclosing: self.enclosing_namespace.clone(),
            },
            enclosing_namespace: self.enclosing_namespace.clone(),
            current_package: self
                .current_package
                .clone()
                .or_else(|| self.default_package.clone()),
            skip_type_visibility: self.mono_depth > 0,
        }
    }

    /// RFC 025 M2：按声明 span 的 FileId 切换当前包上下文。
    pub(crate) fn enter_package_for_span(&mut self, span: ast::Span) {
        if let Some(pkg) = self.file_packages.get(&span.file_id) {
            self.current_package = Some(pkg.clone());
        } else if self.default_package.is_some() {
            self.current_package = self.default_package.clone();
        }
    }

    /// RFC 025：当前访问上下文下类型名是否可见（`internal class` 包边界）。
    pub(crate) fn ensure_type_accessible(&self, name: &Ident) -> Result<(), TypeError> {
        // 泛型类单态化期间跳过：编译器驱动的实例化（`instantiate_generic_class`）
        // 在消费端重建库内 internal 模板的签名（如 `EndpointDispatcher.Dispatch`
        // 引用 internal `DispatchContext`），模板本身已在归属包内通过可见性校验，
        // 此处门禁只会误杀合法的库内泛型类实例化（RFC 019 M-B）。
        if self.mono_depth > 0 {
            return Ok(());
        }
        let ctx = self.access_ctx();
        if self.registry.can_access_type(name, &ctx) {
            Ok(())
        } else {
            Err(TypeError::Oop(
                crate::OopError::InaccessibleType {
                    ty: name.to_string(),
                }
                .to_string(),
            ))
        }
    }

    pub(crate) fn type_name_of(&self, ty: &TypeId) -> Option<Ident> {
        match ty {
            TypeId::Named(n) => Some(n.clone()),
            // Expression<T> (内置编译期类型) 在运行时映射为 Expression 类。
            // 方法重载解析时，Expression<Func<...>> 应匹配 Expression 参数。
            TypeId::Expression { .. } => Some("Expression".into()),
            TypeId::Nullable { inner } => self.type_name_of(inner),
            // 基元类型映射为注册名（与 mangle_type_suffix/type_id_to_field_name 一致）
            TypeId::Int => Some("int".into()),
            TypeId::Long => Some("long".into()),
            TypeId::Short => Some("short".into()),
            TypeId::Byte => Some("byte".into()),
            TypeId::Char => Some("char".into()),
            TypeId::Float => Some("float".into()),
            TypeId::Double => Some("double".into()),
            TypeId::Bool => Some("bool".into()),
            TypeId::UInt => Some("uint".into()),
            TypeId::ULong => Some("ulong".into()),
            TypeId::UShort => Some("ushort".into()),
            TypeId::SByte => Some("sbyte".into()),
            TypeId::String => Some("string".into()),
            // RFC 006 M1: object 根类型映射为注册名 "object"
            TypeId::Object => Some("object".into()),
            // Builtin `IEnumerable<T>` / `IQueryable<T>` → std 单态接口名（方法解析 / itable）。
            TypeId::IEnumerable { inner } => {
                Some(mangle_generic("IEnumerable", &[inner.as_ref().clone()]).into())
            }
            TypeId::IQueryable { inner } => {
                Some(mangle_generic("IQueryable", &[inner.as_ref().clone()]).into())
            }
            _ => None,
        }
    }

    /// Whether `ty` is a type that can be declared as nullable (`T?`).
    /// Only reference types (string, object, class instances, delegates, etc.) can be nullable;
    /// value types (primitives, structs, enums) cannot.
    pub(crate) fn is_nullable_ref_type(&self, ty: &TypeId) -> bool {
        self.is_reference_type(ty)
    }

    /// Whether `ty` is a reference type (assignable from `null`).
    pub(crate) fn is_reference_type(&self, ty: &TypeId) -> bool {
        match ty {
            TypeId::String
            // RFC 006 D2: object 是所有引用类型的根，自身也是引用类型
            | TypeId::Object
            | TypeId::Task { .. }
            | TypeId::IEnumerable { .. }
            | TypeId::IQueryable { .. }
            | TypeId::Array { .. }
            | TypeId::Func { .. }
            | TypeId::Expression { .. } => true,
            // 可空引用类型（`T?`）在 C# 语义下与 `T` 是同一运行时类型，可空性
            // 仅是编译期流分析注解——引用性沿 inner 递归判定：`string?` 是引用
            // 类型（满足 `T : class` 约束、`string? <: object`、可与 `string`
            // 直接比较）；裸 `null`（Nullable{Infer}）与可空值类型（`int?`）仍
            // 为 false，不因解包而放宽。
            TypeId::Nullable { inner } => self.is_reference_type(inner),
            TypeId::Named(n) => {
                // C# 语义：enum 与 variant（栈上标签联合）均为值类型——
                // `where T : class` 拒绝二者（Weak<T> 等引用类型约束的
                // 严密性前提）。
                !self.registry.is_struct(n)
                    && !self.registry.is_enum(n)
                    && !self.registry.is_variant(n)
            }
            _ => false,
        }
    }

    /// Whether `ty` is a value type (C# `where T : struct` 的判定依据)。
    ///
    /// 值类型 = 基元（int/long/short/byte/char/float/double/bool）或 struct/enum。
    /// 注意：`string` 在 C# 中是引用类型，此处返回 false。
    ///
    /// RFC 037 M1: 此方法同时用于 `Expr::Cast` 的拆箱决策——
    /// `(Signal<T>)box` 中 target_ty=Named("Signal_T")，registry.is_struct
    /// 返回 false（Signal 是 class），故不触发拆箱，保持 Cast 原语义（指针重解释）。
    /// `(int)box` 中 target_ty=Int，返回 true，触发 Unbox → rt_box_unbox。
    /// `(SomeStruct)box` 中 target_ty=Named("SomeStruct")，is_struct 返回 true，触发 Unbox。
    pub(crate) fn is_value_type(&self, ty: &TypeId) -> bool {
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
            | TypeId::SByte => true,
            TypeId::Named(n) => self.registry.is_struct(n) || self.registry.is_enum(n),
            // RFC 005：Span 为 ref-like 值视图，非可装箱普通值类型。
            TypeId::Span { .. } => false,
            _ => false,
        }
    }

    pub(crate) fn is_subtype(&self, sub: &TypeId, sup: &TypeId) -> bool {
        if sub == sup || matches!(sub, TypeId::Infer) {
            return true;
        }
        // RFC 016 D2: 所有引用类型隐式是 object 的子类型。
        // 适用于 class / string / array / Task / IEnumerable / IQueryable / Func / Expression。
        // 值类型（int/bool/struct 等）不在此列——它们需要装箱（M2 实现）才能赋值给 object。
        if *sup == TypeId::Object && self.is_reference_type(sub) {
            return true;
        }
        // RFC 004 P0 Phase 1: 基元 → object 自动装箱（隐式转换）。
        // `object o = 5` 的放行点；enum 装箱留待 Phase 4。
        // RFC 004 P0 Phase 2: struct → object 自动装箱（隐式转换）。
        // `object o = point` 的放行点——struct 是值类型，需装箱为堆盒。
        if *sup == TypeId::Object
            && (is_boxable_primitive(sub)
                || matches!(sub, TypeId::Named(n) if self.registry.is_struct(n)))
        {
            return true;
        }
        // Expression<T> (内置编译期类型) 是 Expression (运行时类) 的子类型。
        // codegen 会将 Expression<T> 变量构造为运行时 Expression 对象树。
        if let TypeId::Expression { .. } = sub {
            if let TypeId::Named(n) = sup {
                if n == "Expression" {
                    return true;
                }
            }
        }
        // Builtin `IEnumerable<T>`：对标 std `IEnumerable<out T>` 单态名做实现/协变判定。
        if let TypeId::IEnumerable { inner } = sup {
            let mangled: Ident = mangle_generic("IEnumerable", &[inner.as_ref().clone()]).into();
            if let Some(s) = self.type_name_of(sub) {
                if self.registry.is_subtype(&s, &mangled) {
                    return true;
                }
                if let Some(sub_ty) = self.registry.types.get(&s) {
                    for base in &sub_ty.bases {
                        if self.variance_compatible_named(&mangled, base) {
                            return true;
                        }
                    }
                }
                // RFC 044 M3：模板态泛型 stub 的接口 bases 被清除（
                // `register_parametrized_generic_stub` 纯化 stub 防 itable 虚分派），
                // 子类型判定回退到类模板的 AST bases——合成状态机类
                // `__Yield_X_0_T` 由此可证明实现 `IEnumerable<T>`。
                if self.template_subtype_of_interface(&s, &mangled) {
                    return true;
                }
                if self.registry.is_subtype(&s, &"IEnumerable".into()) {
                    return true;
                }
            }
            if let TypeId::IEnumerable { inner: sub_inner } = sub {
                return self.is_subtype(sub_inner, inner);
            }
            if let TypeId::Named(n) = sub {
                if self.variance_compatible_named(&mangled, n) {
                    return true;
                }
            }
        }
        if let (Some(s), Some(p)) = (self.type_name_of(sub), self.type_name_of(sup)) {
            if self.registry.is_subtype(&s, &p) {
                return true;
            }
            // RFC 044 M3：模板态泛型 stub 的接口 bases 被清除，回退类模板
            // AST bases 判定。`IAsyncEnumerable` 无内置 TypeId（与 `IEnumerable`
            // 不同），走通用 mangle 形态 `IAsyncEnumerable_T`，此路径由此可证明
            // `__Yield_Flow_0_T` 实现 `IAsyncEnumerable<T>` / `IAsyncEnumerator<T>`。
            if self.template_subtype_of_interface(&s, &p) {
                return true;
            }
            return false;
        }
        false
    }

    /// RFC 044 M3：模板态泛型 stub 是否实现目标接口（按模板 AST bases 判定）。
    ///
    /// `register_parametrized_generic_stub` 会清除 stub `registry.types` 条目的
    /// 接口 bases（防 stub 生成 itable 虚分派），故 `registry.is_subtype` 无法
    /// 证明 `__Yield_X_0_T` 实现了 `IEnumerable<T>`。此处经 `mono_origins` 还原
    /// stub 的类模板与实参，逐接口 base 实例化后与目标接口名比对（精确名或
    /// variance 兼容），仅作子类型判定，不产生 itable。
    fn template_subtype_of_interface(&self, s: &Ident, iface_mangled: &Ident) -> bool {
        let Some((template_name, args)) = self.mono_origins.get(s.as_str()) else {
            return false;
        };
        let Some(template) = self.class_templates.get(template_name) else {
            return false;
        };
        for base in &template.bases {
            let Type::Named { path, generics } = base else {
                continue;
            };
            let Some(base_name) = path.last() else {
                continue;
            };
            if !self.registry.is_interface(base_name) {
                continue;
            }
            let inst: Ident = if generics.is_empty() {
                base_name.clone()
            } else {
                mangle_generic(base_name, args).into()
            };
            if inst == *iface_mangled || self.variance_compatible_named(iface_mangled, &inst) {
                return true;
            }
        }
        false
    }

    pub(crate) fn lower_type(&mut self, ty: &Type) -> Result<TypeId, TypeError> {
        match ty {
            Type::Named { path, generics } => {
                let name = path.last().cloned().unwrap_or_else(|| "unknown".into());

                // 非泛型 `Task` 是 `Task<Void>` 的零成本别名（RFC 009 §4.1）。
                // 不引入新 TypeId 变体，复用 TypeId::Task { inner: Void }。
                // 必须在 resolve_type_path 之前拦截，避免 std/Arc/Tasks/Task.as 中的
                // `class Task` stub 被识别为 TypeId::Named("Task")。
                if generics.is_empty() && path.len() == 1 && name == "Task" {
                    return Ok(TypeId::Task {
                        inner: Box::new(TypeId::Void),
                    });
                }

                // RFC 016 M1: `object` 是内置根类型，解析为 TypeId::Object。
                // 必须在 resolve_type_path 之前拦截，避免 std/Arc/Object.as 中的
                // `class Object` stub 被识别为 TypeId::Named("Object")。
                // 小写 `object` 是预定义类型标识符（与 int/string/bool 一致），
                // 大写 `Object` 不被视为根类型（保留给用户自定义类型）。
                if generics.is_empty() && path.len() == 1 && name == "object" {
                    return Ok(TypeId::Object);
                }

                if generics.is_empty() && path.len() == 1 {
                    if let Some(param_ty) = self.resolve_type_param(&name) {
                        return Ok(param_ty);
                    }
                }

                if !generics.is_empty() {
                    // Built-in generic types take precedence over user-defined templates
                    match name.as_str() {
                        "Task" if generics.len() == 1 => {
                            return Ok(TypeId::Task {
                                inner: Box::new(self.lower_type(&generics[0].node)?),
                            })
                        }
                        "IEnumerable" if generics.len() == 1 => {
                            let inner = self.lower_type(&generics[0].node)?;
                            // 副作用：注册 std `IEnumerable_<T>` 单态（itable / variance）。
                            if self.registry.is_generic_template(&"IEnumerable".into()) {
                                let _ = self.instantiate_generic_interface(
                                    &"IEnumerable".into(),
                                    std::slice::from_ref(&inner),
                                );
                            }
                            return Ok(TypeId::IEnumerable {
                                inner: Box::new(inner),
                            })
                        }
                        "IQueryable" if generics.len() == 1 => {
                            return Ok(TypeId::IQueryable {
                                inner: Box::new(self.lower_type(&generics[0].node)?),
                            })
                        }
                        "Func" => {
                            let ret = self.lower_type(&generics[generics.len() - 1].node)?;
                            let params = generics[..generics.len() - 1]
                                .iter()
                                .map(|g| self.lower_type(&g.node))
                                .collect::<Result<_, _>>()?;
                            return Ok(TypeId::Func {
                                params,
                                ret: Box::new(ret),
                            });
                        }
                        // RFC 037 / RFC 009: Action<T1, T2, ...> 是 Func<T1, T2, ..., void> 的语法糖
                        // 对齐 C# System.Action 委托家族——无返回值的委托统一用 Action 表达。
                        "Action" => {
                            let params = generics
                                .iter()
                                .map(|g| self.lower_type(&g.node))
                                .collect::<Result<_, _>>()?;
                            return Ok(TypeId::Func {
                                params,
                                ret: Box::new(TypeId::Void),
                            });
                        }
                        "Expression" if generics.len() == 1 => {
                            return Ok(TypeId::Expression {
                                inner: Box::new(self.lower_type(&generics[0].node)?),
                            })
                        }
                        "Vector" if generics.len() == 2 => {
                            let elem = self.lower_type(&generics[0].node)?;
                            let n = match &generics[1].node {
                                Type::ConstInt(n) => *n as u32,
                                _ => {
                                    return Err(TypeError::Generic(
                                        "Vector const generic N must be an integer literal"
                                            .into(),
                                    ))
                                }
                            };
                            if !matches!(elem, TypeId::Float | TypeId::Double) {
                                return Err(TypeError::Generic(format!(
                                    "Vector element type must be float or double, got {}",
                                    elem.display()
                                )));
                            }
                            if !matches!(n, 4 | 8 | 16) {
                                return Err(TypeError::Generic(format!(
                                    "Vector length N must be 4, 8, or 16, got {n}"
                                )));
                            }
                            return Ok(TypeId::Vector {
                                elem: Box::new(elem),
                                n,
                            });
                        }
                        // RFC 005：`Span<T>` / `ReadOnlySpan<T>` 语言内建切片视图。
                        "Span" if generics.len() == 1 => {
                            let elem = self.lower_type(&generics[0].node)?;
                            return Ok(TypeId::Span {
                                elem: Box::new(elem),
                                mutable: true,
                            });
                        }
                        "ReadOnlySpan" if generics.len() == 1 => {
                            let elem = self.lower_type(&generics[0].node)?;
                            return Ok(TypeId::Span {
                                elem: Box::new(elem),
                                mutable: false,
                            });
                        }
                        _ => {}
                    }

                    let args: Vec<TypeId> = generics
                        .iter()
                        .map(|g| self.lower_type(&g.node))
                        .collect::<Result<_, _>>()?;
                    if self.class_templates.contains_key(&name) {
                        return self.instantiate_generic_class(&name, &args);
                    }
                    if self.registry.is_generic_template(&name) {
                        return self.instantiate_generic_interface(&name, &args);
                    }
                    // GAP #5 扩展：泛型委托引用点单态化（`Map<int, string>`）。
                    if self.delegate_templates.contains_key(&name) {
                        return self.instantiate_generic_delegate(&name, &args);
                    }
                    // RFC 004 M1：内置 static abstract 接口（INumber/IEquatable 等）
                    // 未注册到 registry 时，直接 mangle 为 `IFace_<arg>` 形式。
                    // 约束校验通过 `is_primitive_satisfiable_interface` 识别 mangle 形态；
                    // 方法调用校验通过 `check_static_abstract_call` 内置接口表查询。
                    if is_builtin_static_abstract_iface(&name) {
                        let mangled = mangle_generic(&name, &args);
                        return Ok(TypeId::Named(mangled.into()));
                    }
                }

                if let Some(resolved) = self.resolve_type_path(path) {
                    if generics.is_empty() {
                        if self.registry.is_generic_template(&name) {
                            return Err(TypeError::GenericTypeNeedsArgs(name.to_string()));
                        }
                        self.ensure_type_accessible(&name)?;
                        return Ok(resolved);
                    }
                }

                match name.as_str() {
                    "int" => Ok(TypeId::Int),
                    "long" => Ok(TypeId::Long),
                    "short" => Ok(TypeId::Short),
                    "byte" => Ok(TypeId::Byte),
                    "char" => Ok(TypeId::Char),
                    "bool" => Ok(TypeId::Bool),
                    "string" => Ok(TypeId::String),
                    "uint" => Ok(TypeId::UInt),
                    "ulong" => Ok(TypeId::ULong),
                    "ushort" => Ok(TypeId::UShort),
                    "sbyte" => Ok(TypeId::SByte),
                    // RFC 006 M1: 将 `object` 标识符解析为 TypeId::Object
                    "object" => Ok(TypeId::Object),
                    "void" => Ok(TypeId::Void),
                    "float" => Ok(TypeId::Float),
                    "double" => Ok(TypeId::Double),
                    // RFC 037 / RFC 009: 非泛型 `Action`（无参 void 委托）= Func<void>
                    // 对齐 C# System.Action（无泛型版本）。
                    "Action" => Ok(TypeId::Func {
                        params: Vec::new(),
                        ret: Box::new(TypeId::Void),
                    }),
                    _ => {
                        // GAP #5：delegate 委托类型别名解析——将委托名解析为 TypeId::Func。
                        if let Some(type_id) = self.registry.delegate_aliases.get(name.as_str()) {
                            return Ok(type_id.clone());
                        }
                        // GAP #5 扩展：泛型委托裸名引用（缺实参）报错。
                        if self.delegate_templates.contains_key(name.as_str()) {
                            return Err(TypeError::GenericTypeNeedsArgs(name.to_string()));
                        }
                        // 未走 resolve_type_path 命中、但仍在 registry 的命名类型
                        // （如前向引用）同样须过类型级可见性门禁。
                        if self.registry.types.contains_key(&name) {
                            self.ensure_type_accessible(&name)?;
                        }
                        Ok(TypeId::Named(name))
                    }
                }
            }
            Type::Ref { inner, mutable } => Ok(TypeId::Ref {
                inner: Box::new(self.lower_type(&inner.node)?),
                mutable: *mutable,
                kind: ast::RefKind::Var,
            }),
            Type::Func { params, ret } => Ok(TypeId::Func {
                params: params
                    .iter()
                    .map(|p| self.lower_type(&p.node))
                    .collect::<Result<_, _>>()?,
                ret: Box::new(self.lower_type(&ret.node)?),
            }),
            Type::Array { inner } => Ok(TypeId::Array {
                elem: Box::new(self.lower_type(&inner.node)?),
            }),
            Type::Nullable { inner } => {
                // RFC 009 L2：允许任意 `T?`（包括值类型 `int?` / `long?` 等）。
                // - 引用类型 `T?`：codegen 用 `ptr`（null = null ptr，复用既有基础设施）
                // - 值类型 `T?`：codegen 用 `ptr`（null = null ptr；有值 = ptr to alloca'd T，
                //   由 codegen 在赋值点插入 alloca+store 装箱，在读取点插入 load 解箱）。
                let inner_ty = self.lower_type(&inner.node)?;
                Ok(TypeId::Nullable {
                    inner: Box::new(inner_ty),
                })
            }
            Type::Infer => Ok(TypeId::Infer),
            Type::ConstInt(n) => Err(TypeError::Generic(format!(
                "const integer `{n}` is only valid as a generic argument (e.g. Vector<T, N>), not as a standalone type"
            ))),
        }
    }

    pub(crate) fn canonical_type(&self, ty: &TypeId) -> TypeId {
        match ty {
            TypeId::Named(n) => match n.as_str() {
                "string" => TypeId::String,
                // RFC 006 M1: 将 Named("object") 规范化为 TypeId::Object，
                // 确保经 registry 反序列化或 AST 路径解析的 object 标识符
                // 在类型兼容性判定中被正确识别。
                "object" => TypeId::Object,
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
                "void" => TypeId::Void,
                // RFC 009 §4.1: Task / Task<T> 是非泛型 `Task` 为 `Task<Void>`。
                // OOP registry 以 mangle 名存储："Task" = Task<Void>，"Task_int" = Task<int>。
                "Task" => TypeId::Task {
                    inner: Box::new(TypeId::Void),
                },
                other if other.starts_with("Task_") => {
                    let inner_name = &other[5..];
                    let inner = match inner_name {
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
                        "object" => TypeId::Object,
                        "void" => TypeId::Void,
                        _ => TypeId::Named(inner_name.into()),
                    };
                    TypeId::Task {
                        inner: Box::new(inner),
                    }
                }
                // RFC 005：registry 将 `Span<T>` mangle 为 `Span_T`；归约为 TypeId::Span。
                // 注意：inner 保持 primitive_or_named 语义（数组等复合 mangle 名
                // 不在此归一）——`Array↔Named("{elem}_arr")` 的兼容互认见
                // `types_compatible`，全局归一会破坏泛型 `T[]` 的字符串互认路径。
                other if other.starts_with("Span_") => {
                    let inner_name = &other[5..];
                    TypeId::Span {
                        elem: Box::new(Self::primitive_or_named(inner_name)),
                        mutable: true,
                    }
                }
                other if other.starts_with("ReadOnlySpan_") => {
                    let inner_name = &other["ReadOnlySpan_".len()..];
                    TypeId::Span {
                        elem: Box::new(Self::primitive_or_named(inner_name)),
                        mutable: false,
                    }
                }
                "Span" => TypeId::Span {
                    elem: Box::new(TypeId::Infer),
                    mutable: true,
                },
                "ReadOnlySpan" => TypeId::Span {
                    elem: Box::new(TypeId::Infer),
                    mutable: false,
                },
                // 数组 mangle 名（`{elem}_arr`）**不在 canonical 全局归一**：
                // `types_compatible` 已有 `Array ↔ Named("{elem}_arr")` 字符串
                // 互认（泛型 `T[]` 字段依赖该路径——全局归一会把互认变成
                // `Array{Named}` vs `Array{Generic}` 严格比较而拒绝）。
                // 数组结构化仅在成员访问 receiver 处按需归一（check_expr Field）。
                other => TypeId::Named((*other).into()),
            },
            TypeId::Nullable { inner } => TypeId::Nullable {
                inner: Box::new(self.canonical_type(inner)),
            },
            TypeId::Span { elem, mutable } => TypeId::Span {
                elem: Box::new(self.canonical_type(elem)),
                mutable: *mutable,
            },
            other => other.clone(),
        }
    }

    fn primitive_or_named(name: &str) -> TypeId {
        match name {
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
            "object" => TypeId::Object,
            "void" => TypeId::Void,
            other => TypeId::Named(other.into()),
        }
    }

    pub(crate) fn types_compatible(&self, expected: &TypeId, found: &TypeId) -> bool {
        let expected = self.canonical_type(expected);
        let found = self.canonical_type(found);
        if expected == found {
            return true;
        }
        match (&expected, &found) {
            (TypeId::Nullable { inner: e_inner }, TypeId::Nullable { inner: f_inner }) => {
                return self.types_compatible(e_inner, f_inner);
            }
            (TypeId::Nullable { inner: e_inner }, _) => {
                return self.types_compatible(e_inner, &found);
            }
            (_, TypeId::Nullable { inner: f_inner }) if matches!(**f_inner, TypeId::Infer) => {
                // null literal — compatible with any reference type
                return self.is_reference_type(&expected);
            }
            (_, TypeId::Nullable { inner: f_inner }) => {
                // 可空引用类型（`T?`）与基础引用类型在签名兼容性上等价（仅编译期
                // 标注，非独立类型——见 `registry::type_path_name` 归约）。registry
                // 把 `object?` 存为 "object"，调用点 aty 仍为 `Nullable<Object>`，
                // 此处需退化为 inner 比较以匹配接口/类方法参数声明。
                // 仅当 expected 是引用类型时接受（值类型 `int?` ≠ `int` 仍拒绝）。
                if self.is_reference_type(&expected) && self.types_compatible(&expected, f_inner) {
                    return true;
                }
                return false;
            }
            // 递归比较复合类型内部的 Infer：
            // Task<Infer> 兼容 Task<ParallelResult>（因 Infer 兼容一切）
            // Func { params: [Infer], ret: Infer } 兼容 Func { params: [Int], ret: Void }
            (TypeId::Task { inner: e_inner }, TypeId::Task { inner: f_inner }) => {
                return self.types_compatible(e_inner, f_inner);
            }
            (
                TypeId::Func {
                    params: ep,
                    ret: er,
                },
                TypeId::Func {
                    params: fp,
                    ret: fr,
                },
            ) => {
                if ep.len() != fp.len() {
                    return false;
                }
                return ep
                    .iter()
                    .zip(fp.iter())
                    .all(|(e, f)| self.types_compatible(e, f))
                    && self.types_compatible(er, fr);
            }
            // 委托互认：registry/OOP 形参侧的 Func/Action 以 mangle 名
            // （`Func_P..._ret`；`Action_P...` 与 `Func_P..._void` 同一 mangle）
            // 到达，而局部声明/lambda 实参侧 lower 为结构化 `TypeId::Func`——
            // 同一委托的两种表示必须互认（与 `Array↔Named("{elem}_arr")` 同族）。
            // 判定：demangle 名义名（Nullable 已在 mangle 侧归一，段切分无歧义）
            // 后逐位比较——结构侧 Infer 通配（未类型化 lambda 形参）。
            // demangle 失败（嵌套 Func_ 段）回退：结构侧重 mangle 严格比对。
            (
                TypeId::Named(n),
                TypeId::Func {
                    params: fp,
                    ret: fr,
                },
            ) if n.starts_with("Func_") || n.starts_with("Action_") => {
                if let Some(TypeId::Func {
                    params: ep,
                    ret: er,
                }) = crate::check_expr::demangle_func_type_with(n, fp.len(), &|s| {
                    self.registry.types.contains_key(s)
                }) {
                    return ep.len() == fp.len()
                        && ep
                            .iter()
                            .zip(fp.iter())
                            .all(|(e, f)| self.types_compatible(e, f))
                        && self.types_compatible(&er, fr);
                }
                let canonical_func = TypeId::Func {
                    params: fp.iter().map(|p| self.canonical_type(p)).collect(),
                    ret: Box::new(self.canonical_type(fr)),
                };
                return crate::generics::mangle_type_suffix(&canonical_func) == n.as_str();
            }
            (
                TypeId::Func {
                    params: ep,
                    ret: er,
                },
                TypeId::Named(n),
            ) if n.starts_with("Func_") || n.starts_with("Action_") => {
                if let Some(TypeId::Func {
                    params: fp,
                    ret: fr,
                }) = crate::check_expr::demangle_func_type_with(n, ep.len(), &|s| {
                    self.registry.types.contains_key(s)
                }) {
                    return ep.len() == fp.len()
                        && fp
                            .iter()
                            .zip(ep.iter())
                            .all(|(f, e)| self.types_compatible(e, f))
                        && self.types_compatible(er.as_ref(), fr.as_ref());
                }
                let canonical_func = TypeId::Func {
                    params: ep.iter().map(|p| self.canonical_type(p)).collect(),
                    ret: Box::new(self.canonical_type(er)),
                };
                return crate::generics::mangle_type_suffix(&canonical_func) == n.as_str();
            }
            // RFC 005 项 3 / RFC 002：数组元素 **invariant**——拒 C# `Dog[]→Animal[]`
            // 危险协变（免存时类型检查；写元素否则可破坏实际缓冲类型）。
            // Infer 仍放行（集合表达式等推导路径）。
            (TypeId::Array { elem: ee, .. }, TypeId::Array { elem: fe, .. }) => {
                let ee = self.canonical_type(ee);
                let fe = self.canonical_type(fe);
                return ee == fe || matches!(ee, TypeId::Infer) || matches!(fe, TypeId::Infer);
            }
            // RFC 005 V3：`Span<T>` → `ReadOnlySpan<T>` 隐式转换；反向禁止。
            (
                TypeId::Span {
                    elem: ee,
                    mutable: false,
                },
                TypeId::Span {
                    elem: fe,
                    mutable: true,
                },
            ) => {
                return self.types_compatible(ee, fe);
            }
            (
                TypeId::Span {
                    elem: ee,
                    mutable: em,
                },
                TypeId::Span {
                    elem: fe,
                    mutable: fm,
                },
            ) if em == fm => {
                return self.types_compatible(ee, fe);
            }
            // registry / OOP 形参常存 Named("{elem}_arr")，集合表达式实参为 Array{elem}。
            (TypeId::Array { .. }, TypeId::Named(n)) => {
                if type_id_to_field_name(&expected).as_str() == n.as_str() {
                    return true;
                }
            }
            (TypeId::Named(n), TypeId::Array { .. })
                if n.as_str() == type_id_to_field_name(&found).as_str() =>
            {
                return true;
            }
            _ => {}
        }
        if let TypeId::Generic(e) = &expected {
            if let TypeId::Generic(f) = &found {
                return e == f;
            }
            if let TypeId::Named(n) = &found {
                return e == n;
            }
        }
        if let TypeId::Generic(f) = &found {
            if let TypeId::Named(n) = &expected {
                return f == n;
            }
            // C# 装箱语义：任意类型实参（未约束或已约束）隐式转换为 object
            // （如泛型扩展方法 `Provide<T>(ctx, name, instance)` 内
            // `ctx.Provide(name, instance)` 的 `T → object?`）。
            if matches!(&expected, TypeId::Object) {
                return true;
            }
        }
        if let TypeId::Func { params, ret } = &found {
            if let TypeId::Named(n) = &expected {
                // Fast path: exact string match for non-Infer Func types
                if n.as_str() == type_id_to_field_name(&found).as_str() {
                    return true;
                }
                // If the Func contains Infer (unbound lambda), it's compatible only
                // with Func/Action-shaped params — NOT with arbitrary Named classes.
                // Otherwise `new ServiceDescriptor(typeof(T), (sp) => …, …)` 的
                // (Type?, …) 与 (Func<…>, …) 重载会因 lambda 类型 Func_Infer_Infer
                // 同时命中而歧义。
                if is_func_mangled_name(n.as_str())
                    && (matches!(ret.as_ref(), TypeId::Infer)
                        || params.iter().any(|p| matches!(p, TypeId::Infer)))
                {
                    return true;
                }
            }
        }
        if let TypeId::Func { params, ret } = &expected {
            if let TypeId::Named(n) = &found {
                if n.as_str() == type_id_to_field_name(&expected).as_str() {
                    return true;
                }
                if is_func_mangled_name(n.as_str())
                    && (matches!(ret.as_ref(), TypeId::Infer)
                        || params.iter().any(|p| matches!(p, TypeId::Infer)))
                {
                    return true;
                }
            }
        }
        // RFC 009 P1-C2：泛型接口 `in`/`out` 赋值兼容（单态化名经 mono_origins）。
        if let (TypeId::Named(e), TypeId::Named(f)) = (&expected, &found) {
            if self.variance_compatible_named(e, f) {
                return true;
            }
        }
        // Builtin `IEnumerable<T>` ↔ 单态名 / 另一 `IEnumerable<U>`（`out T`）。
        match (&expected, &found) {
            (TypeId::IEnumerable { inner: e }, TypeId::IEnumerable { inner: f }) => {
                if self.is_subtype(f, e) || self.types_compatible(e, f) {
                    return true;
                }
            }
            (TypeId::IEnumerable { inner: e }, TypeId::Named(f))
            | (TypeId::Named(f), TypeId::IEnumerable { inner: e }) => {
                let mangled: Ident = mangle_generic("IEnumerable", &[e.as_ref().clone()]).into();
                if f == &mangled || self.variance_compatible_named(&mangled, f) {
                    return true;
                }
            }
            _ => {}
        }
        expected == found
            || matches!(expected, TypeId::Infer)
            || matches!(found, TypeId::Infer)
            || matches!(expected, TypeId::Void)
            || self.is_subtype(&found, &expected)
            || numeric_implicit_convertible(&expected, &found)
    }

    pub(crate) fn resolve_value_name(&self, name: &Ident) -> Option<TypeId> {
        if let Some(flow) = &self.null_flow {
            if let Some(ty) = flow.narrowed_ty(name) {
                return Some(ty.clone());
            }
        }
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    pub(crate) fn resolve_type_path(&self, path: &[Ident]) -> Option<TypeId> {
        if let Some(name) = path.last() {
            // CD-30 批处理扩容（阶段 B·typeck 侧）：**显式命名空间限定名**沿 FQN
            // 解析。`namespace Med { class Shape {} }` 下的 `Med.Shape`（path=[Med,Shape]）
            // ——短名 `types["Shape"]` 被碰撞胜者占用时，输家按其 FQN 存于
            // `shadowed_types["Med.Shape"]`。整条限定路径拼出的 FQN 命中 shadowed_types
            // 即确系碰撞输家 → 返回 FQN，使 `new`/ctor/方法/静态成员沿 `classes[FQN]`
            // 成员表解析而非误落到短名胜者。std 无碰撞（shadowed_types 恒空）零行为变化。
            if path.len() > 1 {
                let qualified_fqn =
                    crate::oop_types::type_fqn(&path[..path.len() - 1], name.as_str());
                if self.registry.shadowed_types.contains_key(&qualified_fqn) {
                    return Some(TypeId::Named(qualified_fqn.into()));
                }
            }
            if self.registry.types.contains_key(name) {
                return Some(
                    self.resolve_collision_fqn(name)
                        .map(|f| TypeId::Named(f.into()))
                        .unwrap_or(TypeId::Named(name.clone())),
                );
            }
            if let Some(type_id) = self.registry.delegate_aliases.get(name.as_str()) {
                return Some(type_id.clone());
            }
            return self.resolve_value_name(name);
        }
        None
    }

    /// CD-30 批处理扩容（阶段 B·typeck 侧）：碰撞输家引用沿 FQN 路由。
    ///
    /// 跨命名空间同名类（`namespace A { class T }` 与 `namespace B { class T }`）时，
    /// 短名 `types[T]` 被碰撞**胜者**占用，输家按其 FQN 存于 `shadowed_types`。
    /// 若本引用沿调用点 namespace 链（`lookup_type`，深度 = 当前 ns → 父 ns → 全局，
    /// 对齐 C# 名称查找）解析到**被遮蔽输家**（其 FQN 恰在 `shadowed_types`），
    /// 则返回 FQN（如 `A.T`），使 MIR/codegen 沿 `classes[FQN]` 键解析而非误落到
    /// 短名胜者。胜者与无碰撞引用的命中等价于短名返回——std 单入口包
    /// `shadowed_types` 恒为空，本函数对 std 零行为变化（回归绿）。
    fn resolve_collision_fqn(&self, name: &Ident) -> Option<String> {
        if self.enclosing_namespace.is_empty() {
            return None;
        }
        let nom = self.registry.lookup_type(name, &self.enclosing_namespace)?;
        let fqn = crate::oop_types::type_fqn(&nom.namespace, nom.name.as_str());
        if fqn == name.as_str() {
            // 命中短名主索引（胜者/无碰撞）或全局类型：维持短名，不 FQN 化。
            return None;
        }
        // 仅当该引用确系被遮蔽输家（FQN 键存在于 shadowed_types）才 FQN 化；
        // 否则（如 namespace 内自引、父 ns 命中）维持短名，防 std 波级。
        if self.registry.shadowed_types.contains_key(&fqn) {
            Some(fqn)
        } else {
            None
        }
    }
}

/// Func/Action 的 mangled 名（`Func_T_T_bool` / `Action<T…>` 归约为 `Func_…_void`）。
pub(crate) fn is_func_mangled_name(name: &str) -> bool {
    name.starts_with("Func_") || name == "Action"
}

/// Numeric implicit conversions for assignment/parameter compatibility (RFC 007).
///
/// `expected` is the target type (LHS of let, parameter, return), `found` is the source.
/// Allows:
/// - Safe widening (C# standard): `Int → Long`, `Int → Float`, `Int → Double`,
///   `Long → Float`, `Long → Double`, `Short/Byte → Int/Long/Float/Double`,
///   `Float → Double`.
/// - Narrowing (RFC 007 design — no literal suffixes for `float`/`short`/`byte`):
///   `Double → Float`, `Int/Long → Short/Byte`, `Long → Int`. Variables of these
///   types obtain values via explicit declaration + implicit narrowing from literals.
/// - Floating → integer is NOT allowed (matches C#; requires explicit cast).
fn numeric_implicit_convertible(expected: &TypeId, found: &TypeId) -> bool {
    let is_int = |t: &TypeId| {
        matches!(
            t,
            TypeId::Int
                | TypeId::Long
                | TypeId::Short
                | TypeId::Byte
                | TypeId::Char
                | TypeId::UInt
                | TypeId::ULong
                | TypeId::UShort
                | TypeId::SByte
        )
    };
    let is_float = |t: &TypeId| matches!(t, TypeId::Float | TypeId::Double);
    // Integer ↔ integer: widening and narrowing both allowed (RFC 007).
    if is_int(expected) && is_int(found) {
        return true;
    }
    // Integer → floating: safe widening.
    if is_float(expected) && is_int(found) {
        return true;
    }
    // Floating ↔ floating: widening and narrowing both allowed (RFC 007).
    if is_float(expected) && is_float(found) {
        return true;
    }
    false
}
