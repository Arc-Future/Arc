use super::*;
use crate::check_expr::resolve_named_type_id;

/// RFC 004 M1：判定接口名是否为编译器内置 static abstract 接口。
///
/// 这些接口的基元类型实现由编译器隐式提供（无需加载 .as 源码）：
/// - `INumber<T>` / `IAddable<T>` / `ISubtractable<T>` / `IMultiplicable<T>` / `IDivisible<T>`
/// - `IEquatable<T>` / `IHashable<T>` / `IComparable<T>`
///
/// 用户源码中的同名接口（`std/Arc/INumber.as` 等）是 facade——
/// 仅声明接口契约，不参与 typeck 方法签名校验。
pub(crate) fn is_builtin_static_abstract_iface(name: &str) -> bool {
    matches!(
        name,
        "INumber"
            | "IAddable"
            | "ISubtractable"
            | "IMultiplicable"
            | "IDivisible"
            | "IEquatable"
            | "IHashable"
            | "IComparable"
    )
}

impl TypeChecker {
    /// RFC 004 M1：检测 `T.Method(...)` 形式的 static abstract 接口调用。
    ///
    /// 当 receiver 是当前作用域的泛型参数（`resolve_type_param` 返回 Some），
    /// 查询 `where_clause_scope` 找到 `where T : IFace<T>` 约束，验证 IFace
    /// 含 `static abstract Method` 成员。匹配成功返回方法的返回类型
    /// （T 仍为泛型参数，单态化时由 `substitute_expr` 替换）。
    ///
    /// 返回值：
    /// - `Ok(Some(ret_ty))` — 找到匹配的 static abstract 方法
    /// - `Ok(None)` — receiver 不是泛型参数，或无匹配约束（让其他路径处理）
    /// - `Err(...)` — receiver 是泛型参数但约束缺失（编译错误）
    pub(crate) fn check_static_abstract_call(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &mut [Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        // 仅识别 `Expr::Ident(name)` 形式的 receiver。
        let Expr::Ident(name) = receiver else {
            return Ok(None);
        };
        // 必须是当前作用域的泛型参数。
        let type_param_ty = match self.resolve_type_param(name) {
            Some(ty) => ty,
            None => return Ok(None),
        };
        // 从 where_clause_scope 顶到底查找 `param == name` 的 Type 约束。
        // 约束形式：`where T : IFace<T>` —— IFace 是接口名，generics 含 T 自身。
        let mut matched_iface: Option<Ident> = None;
        for clause in self.where_clause_scope.iter().rev() {
            for c in clause.iter() {
                if c.param != *name {
                    continue;
                }
                let ast::ConstraintKind::Type(ty_node) = &c.kind else {
                    continue;
                };
                let ast::Type::Named { path, generics: _ } = &ty_node.node else {
                    continue;
                };
                let Some(iface_name) = path.last() else {
                    continue;
                };
                // 必须是已注册的接口模板，或 RFC 004 内置接口（不需要加载 .as 源码）。
                if !self.interface_templates.contains_key(iface_name)
                    && !is_builtin_static_abstract_iface(iface_name)
                {
                    continue;
                }
                matched_iface = Some(iface_name.clone());
                break;
            }
            if matched_iface.is_some() {
                break;
            }
        }
        let Some(iface_name) = matched_iface else {
            // T 是泛型参数但 where_clause 中无对应接口约束——
            // 让 check_builtin_static_method 继续处理（可能匹配其他 builtin 路径），
            // 若仍无匹配则最终走 Err 路径（"T has no static member 'Method'"）。
            return Ok(None);
        };

        // 优先尝试内置接口（INumber/IAddable/IEquatable 等）——这些接口的
        // 方法签名由编译器内置，不依赖 .as 源码加载。
        if let Some(ret_ty) =
            self.lookup_builtin_static_abstract_method(&iface_name, name, method, args)?
        {
            return Ok(Some(ret_ty));
        }

        // 退路：从 interface_templates 查找 method（用户自定义接口或已加载的 .as 接口）。
        let iface_def = match self.interface_templates.get(&iface_name) {
            Some(d) => d,
            None => return Ok(None),
        };
        let mut matched_ret: Option<ast::Spanned<ast::Type>> = None;
        let mut matched_property = false;
        for sig in &iface_def.methods {
            if sig.name != *method || !sig.is_static_abstract {
                continue;
            }
            if args.len() != sig.params.len() {
                return Err(TypeError::Mismatch {
                    expected: format!("{} arguments", sig.params.len()),
                    found: format!("{} arguments", args.len()),
                });
            }
            matched_ret = sig.ret.clone();
            break;
        }
        if matched_ret.is_none() {
            for prop in &iface_def.properties {
                if prop.name != *method || !prop.is_static_abstract {
                    continue;
                }
                matched_ret = Some(prop.ty.clone());
                matched_property = true;
                break;
            }
        }
        let Some(ret_ty_ast) = matched_ret else {
            return Ok(None);
        };
        let ret_ty = self.lower_type(&ret_ty_ast.node).unwrap_or(TypeId::Void);
        let _ = matched_property;
        let _ = type_param_ty;
        Ok(Some(ret_ty))
    }

    /// RFC 004 修复刀 2：泛型参数实例接口方法分派。
    ///
    /// 处理 `where T : IFace<T>` 约束下对**泛型局部变量**（`TypeId::Generic`）的
    /// **实例**接口方法调用，如 `T t = new T(); t.ReadJson(reader)`（Deserialize<T>）
    /// 或 `t.Create()`。与 `check_static_abstract_call` 同构，但查找的是接口的
    /// **非 static abstract** 实例方法，返回其返回类型（保留泛型参数，单态化时
    /// 由 substitute_expr 替换）。
    ///
    /// 返回 `Ok(Some(ret_ty))` 匹配成功；`Ok(None)` 无约束接口 / 方法不匹配
    /// （交由既有路径处理或静默 Void 回退）。
    pub(crate) fn check_generic_constraint_method_call(
        &mut self,
        recv_ty: &TypeId,
        method: &Ident,
        args_len: usize,
    ) -> Result<Option<TypeId>, TypeError> {
        // 仅识别泛型参数类型（`TypeId::Generic`）。
        let TypeId::Generic(type_param_name) = recv_ty else {
            return Ok(None);
        };
        // 从 where_clause_scope 顶到底查找 `param == name` 的 Type 约束。
        // 约束形式：`where T : IFace<T>` —— IFace 是接口名。
        let mut matched_iface: Option<Ident> = None;
        for clause in self.where_clause_scope.iter().rev() {
            for c in clause.iter() {
                if c.param != *type_param_name {
                    continue;
                }
                let ast::ConstraintKind::Type(ty_node) = &c.kind else {
                    continue;
                };
                let ast::Type::Named { path, generics: _ } = &ty_node.node else {
                    continue;
                };
                let Some(iface_name) = path.last() else {
                    continue;
                };
                // 约束接口须为编译器内置 static abstract 接口（interface_templates）
                // 或**用户自定义接口**（registry 中的 interface）。此前仅认内置接口，
                // 用户接口约束（如 `where T : ISeed`）下 `t.Member()` 静默落 Void →
                // `int v = t.Value()` 报 expected int, found void。
                if !self.interface_templates.contains_key(iface_name)
                    && !self.registry.is_interface(iface_name)
                {
                    continue;
                }
                matched_iface = Some(iface_name.clone());
                break;
            }
            if matched_iface.is_some() {
                break;
            }
        }
        let Some(iface_name) = matched_iface else {
            return Ok(None);
        };
        // 内置 static abstract 接口：查 interface_templates 方法表。
        if let Some(iface_def) = self.interface_templates.get(&iface_name) {
            let mut matched_ret: Option<ast::Spanned<ast::Type>> = None;
            for sig in &iface_def.methods {
                if sig.name != *method || sig.is_static_abstract {
                    continue;
                }
                if args_len != sig.params.len() {
                    return Err(TypeError::Mismatch {
                        expected: format!("{} arguments", sig.params.len()),
                        found: format!("{} arguments", args_len),
                    });
                }
                matched_ret = sig.ret.clone();
                break;
            }
            let Some(ret_ty_ast) = matched_ret else {
                return Ok(None);
            };
            let ret_ty = self.lower_type(&ret_ty_ast.node).unwrap_or(TypeId::Void);
            return Ok(Some(ret_ty));
        }
        // 用户自定义接口：经 registry 解析实例方法签名，取返回类型。
        // 接口方法可自身泛型（`IGetter.Get<T>`）——返回类型中的方法级泛型占位符
        // 保留为 `TypeId::Named`，由调用点单态化路径替换。
        if let Ok(sig) = self
            .registry
            .resolve_method(&iface_name, method, &self.access_ctx())
        {
            if args_len != sig.params.len() {
                return Err(TypeError::Mismatch {
                    expected: format!("{} arguments", sig.params.len()),
                    found: format!("{} arguments", args_len),
                });
            }
            return Ok(Some(resolve_named_type_id(sig.ret.clone())));
        }
        Ok(None)
    }

    /// RFC 004 M1：查询内置 static abstract 接口的方法签名。
    ///
    /// 返回 `Ok(Some(ret_ty))` 表示匹配成功；`Ok(None)` 表示接口非内置或方法不匹配。
    /// 参数数量校验在内置方法表中进行，不匹配则返回 `Err`。
    fn lookup_builtin_static_abstract_method(
        &mut self,
        iface_name: &Ident,
        type_param_name: &Ident,
        method: &Ident,
        args: &mut [Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        if !is_builtin_static_abstract_iface(iface_name.as_str()) {
            return Ok(None);
        }
        // T 的 TypeId（保留为泛型参数，单态化时由 substitute_expr 替换）。
        let t_ty = TypeId::Generic(type_param_name.clone());
        match method.as_str() {
            // INumber<T> / IAddable<T>：T Add(T a, T b)
            "Add" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(t_ty))
            }
            // ISubtractable<T>：T Subtract(T a, T b)
            "Subtract" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(t_ty))
            }
            // IMultiplicable<T>：T Multiply(T a, T b)
            "Multiply" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(t_ty))
            }
            // IDivisible<T>：T Divide(T a, T b)
            "Divide" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(t_ty))
            }
            // INumber<T>：T Negate(T a)
            "Negate" => {
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(t_ty))
            }
            // IEquatable<T>：bool Equals(T a, T b)
            "Equals" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(TypeId::Bool))
            }
            // IHashable<T>：int GetHashCode(T value)
            "GetHashCode" => {
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(TypeId::Int))
            }
            // IComparable<T>：int Compare(T a, T b)
            "Compare" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(TypeId::Int))
            }
            _ => Ok(None),
        }
    }

    /// RFC 004 M1：检测 `T.Prop` 形式的 static abstract 接口属性访问。
    ///
    /// 与 `check_static_abstract_call` 类似，但作用于 `Expr::Field` 路径
    /// （无括号属性访问，如 `T.Zero` / `T.One`）。匹配成功返回属性类型。
    /// 同时处理单态化后基元类型的属性访问（`int.Zero` / `double.One` 等）。
    pub(crate) fn check_static_abstract_field(
        &mut self,
        receiver: &Expr,
        field: &Ident,
    ) -> Result<Option<TypeId>, TypeError> {
        let Expr::Ident(name) = receiver else {
            return Ok(None);
        };
        // 单态化后场景：receiver 已是具体基元类型名（如 `int.Zero`）。
        // 此时 T 已替换为 int，直接走基元类型 static abstract 属性识别。
        let numeric_primitives = [
            "int", "long", "short", "byte", "float", "double", "uint", "ulong", "ushort",
        ];
        if numeric_primitives.contains(&name.as_str()) {
            return self.check_primitive_static_field(name, field);
        }
        // 泛型模板期场景：receiver 是泛型参数 T。
        if self.resolve_type_param(name).is_none() {
            return Ok(None);
        }
        // 从 where_clause_scope 查找 `where T : IFace<T>` 约束。
        let mut matched_iface: Option<Ident> = None;
        for clause in self.where_clause_scope.iter().rev() {
            for c in clause.iter() {
                if c.param != *name {
                    continue;
                }
                let ast::ConstraintKind::Type(ty_node) = &c.kind else {
                    continue;
                };
                let ast::Type::Named { path, generics: _ } = &ty_node.node else {
                    continue;
                };
                let Some(iface_name) = path.last() else {
                    continue;
                };
                if !self.interface_templates.contains_key(iface_name)
                    && !is_builtin_static_abstract_iface(iface_name)
                {
                    continue;
                }
                matched_iface = Some(iface_name.clone());
                break;
            }
            if matched_iface.is_some() {
                break;
            }
        }
        let Some(iface_name) = matched_iface else {
            return Ok(None);
        };
        // RFC 004 M1：内置接口的 Zero/One 属性——直接返回 T 类型。
        if is_builtin_static_abstract_iface(&iface_name) && matches!(field.as_str(), "Zero" | "One")
        {
            return Ok(Some(TypeId::Generic(name.clone())));
        }
        // 内置接口的其他属性（无）：让其他路径处理。
        // 退路：从 interface_templates 查找属性（用户自定义接口或已加载 .as 接口）。
        // 先克隆属性类型 AST，避免 `interface_templates` immutable borrow
        // 与 `lower_type` 的 mutable self borrow 冲突。
        let matched_prop_ty: Option<ast::Spanned<ast::Type>> = {
            let iface_def = match self.interface_templates.get(&iface_name) {
                Some(d) => d,
                None => return Ok(None),
            };
            iface_def
                .properties
                .iter()
                .find(|p| p.name == *field && p.is_static_abstract)
                .map(|p| p.ty.clone())
        };
        let Some(prop_ty_ast) = matched_prop_ty else {
            return Ok(None);
        };
        let ret_ty = self.lower_type(&prop_ty_ast.node).unwrap_or(TypeId::Void);
        Ok(Some(ret_ty))
    }

    /// RFC 004 M1：识别基元类型 static abstract 方法调用。
    ///
    /// 单态化后 `T.Add(a, b)` 已被 `substitute_expr` 替换为 `int.Add(a, b)` 等。
    /// typeck 在此识别基元类型的 `INumber<T>`/`IAddable<T>`/`ISubtractable<T>`
    /// /`IMultiplicable<T>`/`IDivisible<T>`/`IEquatable<T>`/`IHashable<T>`
    /// /`IComparable<T>` 方法，返回相应基元类型。
    ///
    /// codegen `try_emit_primitive_static` 拦截器直接发射 LLVM 指令（零运行时开销）。
    fn check_primitive_static_abstract(
        &mut self,
        type_name: &Ident,
        method: &Ident,
        args: &mut [Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        // 基元数值类型集合（int/long/short/byte/float/double）。
        let numeric_primitives = [
            "int", "long", "short", "byte", "float", "double", "uint", "ulong", "ushort", "sbyte",
        ];
        if !numeric_primitives.contains(&type_name.as_str()) {
            // bool/char/string 也可实现部分接口（如 IEquatable/IHashable/IComparable），
            // 但不是 INumber。这里统一识别为基元类型集合。
            if !matches!(type_name.as_str(), "bool" | "char" | "string") {
                return Ok(None);
            }
        }
        let prim_ty = match type_name.as_str() {
            "int" => TypeId::Int,
            "long" => TypeId::Long,
            "short" => TypeId::Short,
            "byte" => TypeId::Byte,
            "float" => TypeId::Float,
            "double" => TypeId::Double,
            "bool" => TypeId::Bool,
            "char" => TypeId::Char,
            "string" => TypeId::String,
            "uint" => TypeId::UInt,
            "ulong" => TypeId::ULong,
            "ushort" => TypeId::UShort,
            "sbyte" => TypeId::SByte,
            _ => return Ok(None),
        };
        match method.as_str() {
            // INumber<T> / IAddable<T>：T Add(T a, T b) → 返回 T
            "Add" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(prim_ty))
            }
            // ISubtractable<T>：T Subtract(T a, T b)
            "Subtract" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(prim_ty))
            }
            // IMultiplicable<T>：T Multiply(T a, T b)
            "Multiply" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(prim_ty))
            }
            // IDivisible<T>：T Divide(T a, T b)
            "Divide" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(prim_ty))
            }
            // INumber<T>：T Negate(T a)
            "Negate" => {
                if !numeric_primitives.contains(&type_name.as_str()) {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(prim_ty))
            }
            // IEquatable<T>：bool Equals(T a, T b)
            "Equals" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(TypeId::Bool))
            }
            // IHashable<T>：int GetHashCode(T value)
            "GetHashCode" => {
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(TypeId::Int))
            }
            // IComparable<T>：int Compare(T a, T b)
            "Compare" => {
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(TypeId::Int))
            }
            // Parse(string) → prim_ty（int/long/float/double/bool/char/short/byte/uint/ulong/ushort/sbyte 支持）
            "Parse" => {
                if !matches!(
                    type_name.as_str(),
                    "int"
                        | "long"
                        | "float"
                        | "double"
                        | "bool"
                        | "char"
                        | "short"
                        | "byte"
                        | "uint"
                        | "ulong"
                        | "ushort"
                        | "sbyte"
                ) {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 1, method)?;
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(prim_ty))
            }
            // TryParse(string, out prim_ty) → bool
            "TryParse" => {
                if !matches!(
                    type_name.as_str(),
                    "int"
                        | "long"
                        | "float"
                        | "double"
                        | "bool"
                        | "char"
                        | "short"
                        | "byte"
                        | "uint"
                        | "ulong"
                        | "ushort"
                        | "sbyte"
                ) {
                    return Ok(None);
                }
                if args.len() < 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments for TryParse".into(),
                        found: format!("{} argument(s)", args.len()),
                    });
                }
                let s = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(&TypeId::String, &s.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: s.ty.display(),
                    });
                }
                // out 参数：不校验类型（codegen 处理指针传递）
                Ok(Some(TypeId::Bool))
            }
            // ToString(T) → string；ToString(T, string format) → string（RFC 007 M2a 数字格式）
            // ToString(T, string format, IFormatProvider provider) → string（RFC 034 M5 文化感知）
            "ToString" => {
                if !matches!(
                    type_name.as_str(),
                    "int"
                        | "long"
                        | "short"
                        | "byte"
                        | "float"
                        | "double"
                        | "bool"
                        | "char"
                        | "uint"
                        | "ulong"
                        | "ushort"
                        | "sbyte"
                ) {
                    return Ok(None);
                }
                match args.len() {
                    1 => Ok(Some(TypeId::String)),
                    2 => {
                        // 格式重载仅数值族（bool/char 拒绝）
                        if matches!(type_name.as_str(), "bool" | "char") {
                            return Err(TypeError::Mismatch {
                                expected: "ToString(value) without format for bool/char".into(),
                                found: "ToString(value, format)".into(),
                            });
                        }
                        self.require_string_arg(&args[..], 1)?;
                        Ok(Some(TypeId::String))
                    }
                    3 => {
                        // 文化感知重载仅数值族（bool/char 拒绝；格式字符串 + IFormatProvider）
                        if matches!(type_name.as_str(), "bool" | "char") {
                            return Err(TypeError::Mismatch {
                                expected: "ToString(value) without format for bool/char".into(),
                                found: "ToString(value, format, provider)".into(),
                            });
                        }
                        self.require_string_arg(&args[..], 1)?;
                        self.require_format_provider_arg(&args[..], 2)?;
                        Ok(Some(TypeId::String))
                    }
                    n => Err(TypeError::Mismatch {
                        expected: "1, 2 or 3 arguments for ToString".into(),
                        found: format!("{n} argument(s)"),
                    }),
                }
            }
            // Char.IsDigit/IsLetter/IsWhiteSpace/IsUpper/IsLower(char) → bool
            "IsDigit" | "IsLetter" | "IsWhiteSpace" | "IsUpper" | "IsLower" => {
                if type_name.as_str() != "char" {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(TypeId::Bool))
            }
            // Char.ToUpper/ToLower(char) → char
            "ToUpper" | "ToLower" => {
                if type_name.as_str() != "char" {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(TypeId::Char))
            }
            // string.IsNullOrEmpty / IsNullOrWhiteSpace(string) → bool
            "IsNullOrEmpty" | "IsNullOrWhiteSpace" => {
                if type_name.as_str() != "string" {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 1, method)?;
                Ok(Some(TypeId::Bool))
            }
            // string.FromCharCount(char, int) → string
            "FromCharCount" => {
                if type_name.as_str() != "string" {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 2, method)?;
                Ok(Some(TypeId::String))
            }
            // string.Format(string, ...) → string (可变参数，最多 4 个插值)
            "Format" => {
                if type_name.as_str() != "string" {
                    return Ok(None);
                }
                if args.len() < 2 || args.len() > 5 {
                    return Err(TypeError::Mismatch {
                        expected: "2 to 5 arguments for Format".into(),
                        found: format!("{} argument(s)", args.len()),
                    });
                }
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::String))
            }
            // string.Concat(string, string) → string
            "Concat" => {
                if type_name.as_str() != "string" {
                    return Ok(None);
                }
                self.require_arg_count(&args[..], 2, method)?;
                self.require_string_arg(&args[..], 0)?;
                self.require_string_arg(&args[..], 1)?;
                Ok(Some(TypeId::String))
            }
            _ => Ok(None),
        }
    }

    /// RFC 004 M1：识别基元类型 static abstract 属性访问（`int.Zero` / `int.One`）。
    ///
    /// 单态化后 `T.Zero` 已被 `substitute_expr` 替换为 `int.Zero` 等。
    /// typeck 在此识别基元类型的 `INumber<T>.Zero` / `INumber<T>.One` 属性，
    /// 以及内置常量字段（MinValue/MaxValue/Epsilon/NaN/Infinity）。
    /// 返回相应基元类型本身。
    pub(crate) fn check_primitive_static_field(
        &mut self,
        type_name: &Ident,
        field: &Ident,
    ) -> Result<Option<TypeId>, TypeError> {
        // 所有数值基元类型（含 bool，用于 TrueString/FalseString 等）。
        let numeric_primitives = [
            "int", "long", "short", "byte", "float", "double", "uint", "ulong", "ushort", "sbyte",
        ];
        let all_primitives = [
            "int", "long", "short", "byte", "float", "double", "bool", "char", "uint", "ulong",
            "ushort", "sbyte",
        ];
        let is_numeric = numeric_primitives.contains(&type_name.as_str());
        let is_float = matches!(type_name.as_str(), "float" | "double");
        let is_primitive = all_primitives.contains(&type_name.as_str());
        if !is_primitive {
            return Ok(None);
        }
        let prim_ty = match type_name.as_str() {
            "int" => TypeId::Int,
            "long" => TypeId::Long,
            "short" => TypeId::Short,
            "byte" => TypeId::Byte,
            "float" => TypeId::Float,
            "double" => TypeId::Double,
            "bool" => TypeId::Bool,
            "uint" => TypeId::UInt,
            "ulong" => TypeId::ULong,
            "ushort" => TypeId::UShort,
            "sbyte" => TypeId::SByte,
            _ => return Ok(None),
        };
        match field.as_str() {
            "Zero" | "One" if is_numeric => Ok(Some(prim_ty)),
            "MinValue" | "MaxValue" if is_numeric => Ok(Some(prim_ty)),
            "Epsilon" if is_float => Ok(Some(prim_ty)),
            "NaN" | "PositiveInfinity" | "NegativeInfinity" if is_float => Ok(Some(prim_ty)),
            _ => Ok(None),
        }
    }

    fn static_type_name(&self, receiver: &Expr) -> Option<Ident> {
        match receiver {
            Expr::Ident(name) => Some(name.clone()),
            Expr::Path(path) => path.last().cloned(),
            Expr::Call {
                func,
                args,
                type_args,
                ..
            } if args.is_empty() && !type_args.is_empty() => {
                if let Expr::Ident(name) = &func.node {
                    return Some(name.clone());
                }
                None
            }
            Expr::Field { receiver, field } => {
                // `this.F` / `base.F` 是实例字段访问，绝不是静态类型路径。
                // 若走 `resolve_type_path`，其 value-name 回退会把作用域中的字段
                // `X: int` 解析成类型名 `int`，从而把 `this.X.GetHashCode()` 误判为
                // 静态 `int.GetHashCode`（期望 1 实参）——record/class 合成哈希会炸。
                if matches!(receiver.node, Expr::This | Expr::Base) {
                    return None;
                }
                let mut segments = Vec::new();
                self.field_path_segments(&receiver.node, &mut segments);
                segments.push(field.clone());
                // 仅 registry 类型名；禁止 value-name 回退（同上误判）。
                let name = segments.last()?;
                if self.registry.types.contains_key(name) {
                    return Some(name.clone());
                }
                None
            }
            _ => None,
        }
    }

    fn field_path_segments(&self, expr: &Expr, out: &mut Vec<Ident>) {
        match expr {
            Expr::Ident(name) => out.push(name.clone()),
            Expr::Field { receiver, field } => {
                self.field_path_segments(&receiver.node, out);
                out.push(field.clone());
            }
            Expr::Path(path) => out.extend(path.clone()),
            _ => {}
        }
    }

    pub(crate) fn check_builtin_static_method(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &mut Vec<Spanned<Expr>>,
        type_args: &[Spanned<Type>],
    ) -> Result<Option<TypeId>, TypeError> {
        let type_name = match self.static_type_name(receiver) {
            Some(name) => name,
            None => return Ok(None),
        };
        // Native contract call (RFC 016 M1): try native method dispatch before
        // builtin match. check_native_method returns Ok(None) if `type_name`
        // is not a registered native module, falling through to builtin match.
        // RFC 016 v2 M2：native 调用的 `object` 形参可能就地包装 args（Expr::Box）。
        if let Some(ty) = self.check_native_method(&type_name, method, args)? {
            return Ok(Some(ty));
        }
        // RFC 004 M1：基元类型 static abstract 调用——单态化后 `T.Add(...)` 已被
        // `substitute_expr` 替换为 `int.Add(...)` / `double.Add(...)` 等。
        // typeck 在此识别基元类型的 INumber/IAddable/ISubtractable 等方法，
        // 返回相应类型。codegen `try_emit_primitive_static` 拦截器直接发射 LLVM 指令。
        if let Some(ret_ty) = self.check_primitive_static_abstract(&type_name, method, args)? {
            return Ok(Some(ret_ty));
        }
        match (type_name.as_str(), method.as_str()) {
            ("Console", "WriteLine") => {
                // WriteLine() 空行 / WriteLine(string) 带换行输出
                match args.len() {
                    0 => Ok(Some(TypeId::Void)),
                    1 => {
                        let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                        if !self.types_compatible(&TypeId::String, &arg.ty) {
                            return Err(TypeError::Mismatch {
                                expected: "string".into(),
                                found: arg.ty.display(),
                            });
                        }
                        Ok(Some(TypeId::Void))
                    }
                    n => Err(TypeError::Mismatch {
                        expected: "0 or 1 argument(s)".into(),
                        found: format!("{n} arguments"),
                    }),
                }
            }
            ("Console", "Write") => {
                // Write(string) 无换行输出
                self.require_arg_count(&args[..], 1, method)?;
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::Void))
            }
            ("Console", "ReadLine") => {
                // ReadLine() → string（EOF 返回空串）
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::String))
            }
            ("Console", "Read") => {
                // Read() → int（EOF 返回 -1）
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Int))
            }
            ("Console", "SetForegroundColor" | "SetBackgroundColor") => {
                // 接受 int（与 runtime ABI int32_t 对齐）
                self.require_arg_count(&args[..], 1, method)?;
                self.require_int_arg(&args[..], 0)?;
                Ok(Some(TypeId::Void))
            }
            ("Console", "GetForegroundColor" | "GetBackgroundColor") => {
                // 返回 int（与 runtime ABI int32_t 对齐）
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Int))
            }
            ("Console", "ResetColor") => {
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Void))
            }
            ("Window", "Run") => {
                if args.len() != 3 {
                    return Err(TypeError::Mismatch {
                        expected: "3 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let title = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(&TypeId::String, &title.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: title.ty.display(),
                    });
                }
                let width = self.check_expr_at(args[1].span, &args[1].node)?;
                let height = self.check_expr_at(args[2].span, &args[2].node)?;
                if !self.types_compatible(&TypeId::Int, &width.ty)
                    || !self.types_compatible(&TypeId::Int, &height.ty)
                {
                    return Err(TypeError::Mismatch {
                        expected: "int".into(),
                        found: format!("{}, {}", width.ty.display(), height.ty.display()),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Window", "RunWithText") => {
                // RFC 037 ARML 端到端 demo：与 Window.Run 相同，但追加第 4
                // 参数 text:string，让平台后端在窗口中央绘制该文本。
                if args.len() != 4 {
                    return Err(TypeError::Mismatch {
                        expected: "4 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let title = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(&TypeId::String, &title.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: title.ty.display(),
                    });
                }
                let width = self.check_expr_at(args[1].span, &args[1].node)?;
                let height = self.check_expr_at(args[2].span, &args[2].node)?;
                if !self.types_compatible(&TypeId::Int, &width.ty)
                    || !self.types_compatible(&TypeId::Int, &height.ty)
                {
                    return Err(TypeError::Mismatch {
                        expected: "int".into(),
                        found: format!("{}, {}", width.ty.display(), height.ty.display()),
                    });
                }
                let text = self.check_expr_at(args[3].span, &args[3].node)?;
                if !self.types_compatible(&TypeId::String, &text.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: text.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("File", "ReadAllText") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let path = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(&TypeId::String, &path.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: path.ty.display(),
                    });
                }
                Ok(Some(TypeId::String))
            }
            ("File", "WriteAllText") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let path = self.check_expr_at(args[0].span, &args[0].node)?;
                let content = self.check_expr_at(args[1].span, &args[1].node)?;
                if !self.types_compatible(&TypeId::String, &path.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: path.ty.display(),
                    });
                }
                if !self.types_compatible(&TypeId::String, &content.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: content.ty.display(),
                    });
                }
                Ok(Some(TypeId::Bool))
            }
            ("string", "Compare") | ("string", "CompareOrdinal") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let left = self.check_expr_at(args[0].span, &args[0].node)?;
                let right = self.check_expr_at(args[1].span, &args[1].node)?;
                if !self.types_compatible(&TypeId::String, &left.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: left.ty.display(),
                    });
                }
                if !self.types_compatible(&TypeId::String, &right.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "string".into(),
                        found: right.ty.display(),
                    });
                }
                Ok(Some(TypeId::Int))
            }
            ("string", "Join") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let sep = self.check_expr_at(args[0].span, &args[0].node)?;
                // Join(string, string[]) | Join(char, string[]) — char = UTF-8 码元（与 Length 对齐）
                if !self.types_compatible(&TypeId::String, &sep.ty)
                    && !self.types_compatible(&TypeId::Char, &sep.ty)
                {
                    return Err(TypeError::Mismatch {
                        expected: "string or char".into(),
                        found: sep.ty.display(),
                    });
                }
                let arr = self.check_expr_at(args[1].span, &args[1].node)?;
                if !matches!(arr.ty, TypeId::Array { ref elem } if **elem == TypeId::String) {
                    return Err(TypeError::Mismatch {
                        expected: "string[]".into(),
                        found: arr.ty.display(),
                    });
                }
                Ok(Some(TypeId::String))
            }
            // P5-F: Array static methods
            ("Array", "Copy") => {
                if args.len() != 5 {
                    return Err(TypeError::Mismatch {
                        expected: "5 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Array", "Clear") => {
                if args.len() != 3 {
                    return Err(TypeError::Mismatch {
                        expected: "3 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Array", "Reverse") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Array", "IndexOf") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Int))
            }
            ("Array", "Sort") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Array", "BinarySearch") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Int))
            }
            ("Array", "FindAll") | ("Array", "ConvertAll") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                Ok(Some(TypeId::Array {
                    elem: Box::new(TypeId::Int),
                }))
            }
            ("Vector", "Add" | "Sub" | "Mul") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let a = self.check_expr_at(args[0].span, &args[0].node)?;
                let b = self.check_expr_at(args[1].span, &args[1].node)?;
                match (&a.ty, &b.ty) {
                    (TypeId::Vector { elem, n }, TypeId::Vector { elem: e2, n: n2 })
                        if elem == e2 && n == n2 =>
                    {
                        Ok(Some(TypeId::Vector {
                            elem: elem.clone(),
                            n: *n,
                        }))
                    }
                    _ => Err(TypeError::Mismatch {
                        expected: a.ty.display(),
                        found: b.ty.display(),
                    }),
                }
            }
            ("Vector", "Fma") => {
                if args.len() != 3 {
                    return Err(TypeError::Mismatch {
                        expected: "3 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let a = self.check_expr_at(args[0].span, &args[0].node)?;
                let b = self.check_expr_at(args[1].span, &args[1].node)?;
                let c = self.check_expr_at(args[2].span, &args[2].node)?;
                match (&a.ty, &b.ty, &c.ty) {
                    (
                        TypeId::Vector { elem, n },
                        TypeId::Vector { elem: e2, n: n2 },
                        TypeId::Vector { elem: e3, n: n3 },
                    ) if elem == e2 && elem == e3 && n == n2 && n == n3 => {
                        Ok(Some(TypeId::Vector {
                            elem: elem.clone(),
                            n: *n,
                        }))
                    }
                    _ => Err(TypeError::Mismatch {
                        expected: a.ty.display(),
                        found: format!("{}, {}", b.ty.display(), c.ty.display()),
                    }),
                }
            }
            ("Vector", "Get") => {
                if args.len() != 2 {
                    return Err(TypeError::Mismatch {
                        expected: "2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let v = self.check_expr_at(args[0].span, &args[0].node)?;
                let i = self.check_expr_at(args[1].span, &args[1].node)?;
                if !self.types_compatible(&TypeId::Int, &i.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "int".into(),
                        found: i.ty.display(),
                    });
                }
                match v.ty {
                    TypeId::Vector { elem, .. } => Ok(Some(*elem)),
                    ref other => Err(TypeError::Mismatch {
                        expected: "Vector".into(),
                        found: other.display(),
                    }),
                }
            }
            ("Vector", "Set") => {
                if args.len() != 3 {
                    return Err(TypeError::Mismatch {
                        expected: "3 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let v = self.check_expr_at(args[0].span, &args[0].node)?;
                let i = self.check_expr_at(args[1].span, &args[1].node)?;
                let val = self.check_expr_at(args[2].span, &args[2].node)?;
                if !self.types_compatible(&TypeId::Int, &i.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "int".into(),
                        found: i.ty.display(),
                    });
                }
                match &v.ty {
                    TypeId::Vector { elem, n } => {
                        if !self.types_compatible(elem, &val.ty) {
                            return Err(TypeError::Mismatch {
                                expected: elem.display(),
                                found: val.ty.display(),
                            });
                        }
                        Ok(Some(TypeId::Vector {
                            elem: elem.clone(),
                            n: *n,
                        }))
                    }
                    other => Err(TypeError::Mismatch {
                        expected: "Vector".into(),
                        found: other.display(),
                    }),
                }
            }
            // Arc.Security crypto facades（RFC 026 M3 现代形态）：公开方法（ComputeHash/
            // ToHex/GetBytes）为**真实 .as 体**（null 判空 + CryptographicException），
            // 仅私有 `_ComputeHash`/`_GetBytes` 为 `[Builtin(ABI)]` stub——codegen 按
            // `SHA256::_ComputeHash` 等发射 `@rt_crypto_*_arr`。故公开调用**不得**在此
            // 硬编码拦截（旧表按 string 时代签名 `ComputeHash(string)->string` 校验，
            // 与现行 `byte[]` API 冲突，曾致所有 `ComputeHash(byte[])` 调用点报
            // `expected string, found byte[]`），一律回落 registry 按真实签名解析。
            // 私有 ABI stub 的拦截与类型校验见 builtin_registry 属性路径（无需此处）。
            // Arc.Net network facade (RFC 025 M4): Dns 静态方法。
            ("Dns", "Resolve") | ("Dns", "GetHostAddresses") => {
                self.require_arg_count(&args[..], 1, method)?;
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::String))
            }
            ("Dns", "GetHostEntry") => {
                self.require_arg_count(&args[..], 1, method)?;
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::Named("IPHostEntry".into())))
            }
            ("Dns", "GetHostName") => {
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::String))
            }
            // Task facade (RFC 009 M1): Task静态方法分派。
            // std/Arc/Tasks/Task.as 中 Task 类为 stub，方法体不执行；typeck 在此校验
            // 参数形状并返回 Task<T>/Task<Void> 类型，codegen 拦截后直接发射 rt_task_* ABI。
            // M1 仅实现同步路径 API：FromResult/WhenAll/WhenAny/CompletedTask。
            // Run/Delay 依赖 M2/M3 的 EventLoop 调度器，M1 不支持。
            ("Task", "FromResult") => {
                self.require_arg_count(&args[..], 1, method)?;
                let val = self.check_expr_at(args[0].span, &args[0].node)?;
                Ok(Some(TypeId::Task {
                    inner: Box::new(val.ty),
                }))
            }
            ("Task", "WhenAll") => {
                // RFC 005 dogfood：`params ReadOnlySpan<Task>` → 栈脱糖 / ROS 直传。
                // 正道：`WhenAll(t1,t2,…)` / `WhenAll()` / `WhenAll(ros)`（禁止堆上 Task[]）。
                self.bind_params_task_span(args)?;
                Ok(Some(TypeId::Task {
                    inner: Box::new(TypeId::Void),
                }))
            }
            ("Task", "WhenAny") => {
                // RFC 005 dogfood：与 WhenAll 同形；返回 Task（M4 简化，非 Task<Task>）。
                self.bind_params_task_span(args)?;
                Ok(Some(TypeId::Task {
                    inner: Box::new(TypeId::Void),
                }))
            }
            ("Task", "CompletedTask") => {
                // 属性 getter 被作为无参方法调用时（typeck 兜底）：返回 Task<Void>
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Task {
                    inner: Box::new(TypeId::Void),
                }))
            }
            ("Task", "FromCanceled") => {
                // M5.7: Task.FromCanceled(CancellationToken) / Task.FromCanceled<T>(CancellationToken)
                // 返回 Task<Void> / Task<T>（codegen 统一为 rt_task_from_canceled，类型在消费端区分）。
                // 显式泛型实参 `<T>` 经调用点 MethodCall.type_args 携带，typeck 在此落为 Task<T>；
                // 未携带类型实参时为非泛型 Task（Task<Void> 零成本别名）。
                self.require_arg_count(&args[..], 1, method)?;
                let ct = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&ct.ty, TypeId::Named(n) if n.as_str() == "CancellationToken") {
                    return Err(TypeError::Mismatch {
                        expected: "CancellationToken".into(),
                        found: ct.ty.display().to_string(),
                    });
                }
                let inner = match type_args {
                    [t] => self.lower_type(&t.node)?,
                    [] => TypeId::Void,
                    _ => {
                        return Err(TypeError::Mismatch {
                            expected: "0 or 1 type argument".into(),
                            found: format!("{} type argument(s)", type_args.len()),
                        });
                    }
                };
                Ok(Some(TypeId::Task {
                    inner: Box::new(inner),
                }))
            }
            ("Task", "FromException") => {
                // Task.FromException(Exception) → FAULTED Task（rt_task_from_exception）
                self.require_arg_count(&args[..], 1, method)?;
                let ex = self.check_expr_at(args[0].span, &args[0].node)?;
                let ok = matches!(
                    &ex.ty,
                    TypeId::Named(n) if n.as_str() == "Exception"
                        || n.ends_with("Exception")
                );
                if !ok {
                    return Err(TypeError::Mismatch {
                        expected: "Exception".into(),
                        found: ex.ty.display().to_string(),
                    });
                }
                Ok(Some(TypeId::Task {
                    inner: Box::new(TypeId::Void),
                }))
            }
            ("Task", "WaitAll") => {
                // M5.7 + RFC 005：`params ReadOnlySpan<Task>` → void
                self.bind_params_task_span(args)?;
                Ok(Some(TypeId::Void))
            }
            ("Task", "WaitAny") => {
                // M5.7 + RFC 005：`params ReadOnlySpan<Task>` → int（首个完成索引）
                self.bind_params_task_span(args)?;
                Ok(Some(TypeId::Int))
            }
            ("Task", "Delay") => {
                // M3: Task.Delay(int ms) → Task<Void>（定时器异步等待）
                // M4: Task.Delay(int ms, CancellationToken ct) → Task<Void>（取消传播）
                match args.len() {
                    1 => {
                        self.require_int_arg(&args[..], 0)?;
                        Ok(Some(TypeId::Task {
                            inner: Box::new(TypeId::Void),
                        }))
                    }
                    2 => {
                        self.require_int_arg(&args[..], 0)?;
                        let ct = self.check_expr_at(args[1].span, &args[1].node)?;
                        if !matches!(&ct.ty, TypeId::Named(n) if n.as_str() == "CancellationToken")
                        {
                            return Err(TypeError::Mismatch {
                                expected: "CancellationToken".into(),
                                found: ct.ty.display().to_string(),
                            });
                        }
                        Ok(Some(TypeId::Task {
                            inner: Box::new(TypeId::Void),
                        }))
                    }
                    _ => Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    }),
                }
            }
            ("Task", "Run") => {
                // M5.7: Task.Run(Action) / Task.Run(Action, ThreadPoolScheduler) → Task<Void>
                // M5.7: Task.Run<T>(Func<T>) / Task.Run<T>(Func<T>, CancellationToken) → Task<T>
                // 通过 Func 返回类型区分：void ret → Action 路径，非 void → Func<T> 路径
                match args.len() {
                    1 => {
                        let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                        match &arg.ty {
                            TypeId::Func { params: _, ret } => {
                                Ok(Some(TypeId::Task { inner: ret.clone() }))
                            }
                            _ => Err(TypeError::Mismatch {
                                expected: "Action / Func delegate".into(),
                                found: arg.ty.display(),
                            }),
                        }
                    }
                    2 => {
                        // 无捕获 block lambda 的 ret 常为 Infer（非已解析 Void）；
                        // 须按第二参类型分派，禁止 Infer 误入 CancellationToken 分支。
                        let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                        let second = self.check_expr_at(args[1].span, &args[1].node)?;
                        match &arg.ty {
                            TypeId::Func { params: _, ret } => match &second.ty {
                                TypeId::Named(n) if n.as_str() == "ThreadPoolScheduler" => {
                                    // Action (+ Infer/Void) + ThreadPoolScheduler
                                    let _ = ret;
                                    Ok(Some(TypeId::Task {
                                        inner: Box::new(TypeId::Void),
                                    }))
                                }
                                TypeId::Named(n) if n.as_str() == "CancellationToken" => {
                                    // Func<T> + CancellationToken
                                    Ok(Some(TypeId::Task { inner: ret.clone() }))
                                }
                                _ => Err(TypeError::Mismatch {
                                    expected: "ThreadPoolScheduler or CancellationToken".into(),
                                    found: second.ty.display(),
                                }),
                            },
                            _ => Err(TypeError::Mismatch {
                                expected: "Action / Func delegate".into(),
                                found: arg.ty.display(),
                            }),
                        }
                    }
                    _ => Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{} arguments", args.len()),
                    }),
                }
            }
            // ThreadPoolScheduler.Run (RFC 009 M5.7): pool.Run(action) → Task
            ("ThreadPoolScheduler", "Run") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Func { .. }) {
                    return Err(TypeError::Mismatch {
                        expected: "Action / Func delegate".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Task {
                    inner: Box::new(TypeId::Void),
                }))
            }
            // Parallel.For (RFC 009 §5.3 / RFC 009 M5.7):
            //   For(int, int, Action<int>)                  → 3 参
            //   For(int, int, ParallelOptions, Action<int>) → 4 参
            // 两者返回 ParallelResult。
            ("Parallel", "For") => {
                let (body_idx, opts_idx) = match args.len() {
                    3 => (2, None),
                    4 => (3, Some(2)),
                    n => {
                        return Err(TypeError::Mismatch {
                            expected: "3 or 4 arguments for `Parallel.For`".into(),
                            found: format!("{n} argument(s)"),
                        });
                    }
                };
                self.require_int_arg(&args[..], 0)?;
                self.require_int_arg(&args[..], 1)?;
                if let Some(idx) = opts_idx {
                    let opt = self.check_expr_at(args[idx].span, &args[idx].node)?;
                    if !matches!(&opt.ty, TypeId::Named(n) if n.as_str() == "ParallelOptions") {
                        return Err(TypeError::Mismatch {
                            expected: "ParallelOptions".into(),
                            found: opt.ty.display(),
                        });
                    }
                }
                let body = self.check_expr_at(args[body_idx].span, &args[body_idx].node)?;
                if !matches!(&body.ty, TypeId::Func { .. }) {
                    return Err(TypeError::Mismatch {
                        expected: "Action<int> delegate".into(),
                        found: body.ty.display(),
                    });
                }
                Ok(Some(TypeId::Named("ParallelResult".into())))
            }
            // Parallel.ForEach<T> (RFC 009 §5.3 / RFC 009 M5.7):
            //   ForEach<T>(IEnumerable<T>, Action<T>)                  → 2 参
            //   ForEach<T>(IEnumerable<T>, ParallelOptions, Action<T>) → 3 参
            // 元素类型 T 从 source 的 IEnumerable<T> 推断；body 须为 Action<T>。
            // 两者返回 ParallelResult。
            ("Parallel", "ForEach") => {
                let (body_idx, opts_idx) = match args.len() {
                    2 => (1, None),
                    3 => (2, Some(1)),
                    n => {
                        return Err(TypeError::Mismatch {
                            expected: "2 or 3 arguments for `Parallel.ForEach`".into(),
                            found: format!("{n} argument(s)"),
                        });
                    }
                };
                let source = self.check_expr_at(args[0].span, &args[0].node)?;
                let elem_ty = match &source.ty {
                    TypeId::IEnumerable { inner } => (**inner).clone(),
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: "IEnumerable<T>".into(),
                            found: other.display(),
                        });
                    }
                };
                if let Some(idx) = opts_idx {
                    let opt = self.check_expr_at(args[idx].span, &args[idx].node)?;
                    if !matches!(&opt.ty, TypeId::Named(n) if n.as_str() == "ParallelOptions") {
                        return Err(TypeError::Mismatch {
                            expected: "ParallelOptions".into(),
                            found: opt.ty.display(),
                        });
                    }
                }
                let body = self.check_expr_at(args[body_idx].span, &args[body_idx].node)?;
                match &body.ty {
                    TypeId::Func { params, ret } => {
                        if params.len() != 1 || !self.types_compatible(&elem_ty, &params[0]) {
                            return Err(TypeError::Mismatch {
                                expected: format!("Action<{}>", elem_ty.display()),
                                found: body.ty.display(),
                            });
                        }
                        if !matches!(ret.as_ref(), TypeId::Void) {
                            return Err(TypeError::Mismatch {
                                expected: "Action (void-returning delegate)".into(),
                                found: body.ty.display(),
                            });
                        }
                    }
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: format!("Action<{}>", elem_ty.display()),
                            found: other.display(),
                        });
                    }
                }
                Ok(Some(TypeId::Named("ParallelResult".into())))
            }
            // Arc.Text facade (RFC 021 §4.3 M4): Base64/Hex codecs.
            // 静态方法，方法体为空 stub，codegen 拦截后直接发射 @rt_text_* ABI。
            ("Base64", "Encode" | "Decode")
            | ("Hex", "Encode" | "Decode")
            | ("Url", "Encode" | "Decode") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::String))
            }
            // Encoding UTF-8 (std readiness P0): GetBytes(string)→byte[], GetString(byte[])→string.
            ("Encoding", "GetBytes") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::Array {
                    elem: Box::new(TypeId::Byte),
                }))
            }
            ("Encoding", "GetString") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                let a = self.check_expr_at(args[0].span, &args[0].node)?;
                // 接受 `TypeId::Array{Byte}` 与其 name 归约 `Named("byte_arr")`
                // （例如 `List<byte>.ToArray()` 的返回类型）两种等价表示。
                let want = TypeId::Array {
                    elem: Box::new(TypeId::Byte),
                };
                if !self.types_compatible(&want, &a.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "byte[]".into(),
                        found: a.ty.display(),
                    });
                }
                Ok(Some(TypeId::String))
            }
            ("Encoding", "GetByteCount") => {
                if args.len() != 1 {
                    return Err(TypeError::Mismatch {
                        expected: "1 argument".into(),
                        found: format!("{} arguments", args.len()),
                    });
                }
                self.require_string_arg(&args[..], 0)?;
                Ok(Some(TypeId::Int))
            }
            // Thread 静态方法（RFC 009 M5.5）：Sleep / CurrentThread / ManagedThreadId
            ("Thread", "Sleep") => {
                self.require_arg_count(&args[..], 1, method)?;
                self.require_int_arg(&args[..], 0)?;
                Ok(Some(TypeId::Void))
            }
            ("Thread", "CurrentThread") => {
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Named("Thread".into())))
            }
            ("Thread", "ManagedThreadId") => {
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Int))
            }
            // 默认 Task.Run 池：报告前 join，避免 WriteResults 与 worker 堆竞态（H1）。
            ("ThreadPoolScheduler", "ShutdownDefaultPool") => {
                self.require_arg_count(&args[..], 0, method)?;
                Ok(Some(TypeId::Void))
            }
            // Monitor 静态方法（RFC 009 M5.5）：Enter/Exit/TryEnter/Wait/Pulse/PulseAll
            ("Monitor", "Enter") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                    return Err(TypeError::Mismatch {
                        expected: "Lock".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Monitor", "Exit") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                    return Err(TypeError::Mismatch {
                        expected: "Lock".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Monitor", "TryEnter") => match args.len() {
                1 => {
                    let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                    if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                        return Err(TypeError::Mismatch {
                            expected: "Lock".into(),
                            found: arg.ty.display(),
                        });
                    }
                    Ok(Some(TypeId::Bool))
                }
                2 => {
                    let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                    if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                        return Err(TypeError::Mismatch {
                            expected: "Lock".into(),
                            found: arg.ty.display(),
                        });
                    }
                    self.require_int_arg(&args[..], 1)?;
                    Ok(Some(TypeId::Bool))
                }
                _ => Err(TypeError::Mismatch {
                    expected: "1 or 2 arguments".into(),
                    found: format!("{} arguments", args.len()),
                }),
            },
            ("Monitor", "Wait") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                    return Err(TypeError::Mismatch {
                        expected: "Lock".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Monitor", "Pulse") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                    return Err(TypeError::Mismatch {
                        expected: "Lock".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            ("Monitor", "PulseAll") => {
                self.require_arg_count(&args[..], 1, method)?;
                let arg = self.check_expr_at(args[0].span, &args[0].node)?;
                if !matches!(&arg.ty, TypeId::Named(n) if n.as_str() == "Lock") {
                    return Err(TypeError::Mismatch {
                        expected: "Lock".into(),
                        found: arg.ty.display(),
                    });
                }
                Ok(Some(TypeId::Void))
            }
            // RFC 016 M1：未匹配任何内置静态方法时，尝试 native 契约分派。
            // check_native_method 对未注册的 type_name 返回 Ok(None)，行为与
            // 原 `_ => Ok(None)` 等价；对已注册的 native 模块返回其方法返回类型。
            _ => self.check_native_method(&type_name, method, args),
        }
    }

    fn classify_match_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_ty: &TypeId,
        enum_name: Option<&Ident>,
    ) -> Result<MatchPat, TypeError> {
        match pattern {
            Pattern::Wildcard => Ok(MatchPat::Wildcard),
            Pattern::Null => Ok(MatchPat::Null),
            Pattern::Var(name) => Ok(MatchPat::Binding(name.clone())),
            Pattern::Literal(lit) => {
                let lit_ty = match &lit.node {
                    Expr::IntLit(_) => TypeId::Int,
                    Expr::BoolLit(_) => TypeId::Bool,
                    Expr::StringLit(_) => TypeId::String,
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: "literal pattern".into(),
                            found: format!("{other:?}"),
                        });
                    }
                };
                if !self.types_compatible(scrutinee_ty, &lit_ty) {
                    return Err(TypeError::Mismatch {
                        expected: scrutinee_ty.display(),
                        found: lit_ty.display(),
                    });
                }
                Ok(MatchPat::Wildcard)
            }
            Pattern::Type { ty, binding } => {
                let pat_ty = self.lower_type(&ty.node)?;
                Ok(MatchPat::Type {
                    ty: pat_ty,
                    binding: binding.clone(),
                })
            }
            Pattern::Ident(name) => {
                if let Some(en) = enum_name {
                    if self.registry.enum_variant(en, name).is_some() {
                        return Ok(MatchPat::Variant {
                            case: name.clone(),
                            binding: None,
                        });
                    }
                    return Err(TypeError::UnknownEnumVariant {
                        enum_name: en.to_string(),
                        variant: name.to_string(),
                    });
                }
                // 非 enum：若 Ident 是已注册类型 → 无绑定类型模式
                if self.registry.types.contains_key(name)
                    || matches!(
                        name.as_str(),
                        "int"
                            | "long"
                            | "short"
                            | "byte"
                            | "bool"
                            | "char"
                            | "float"
                            | "double"
                            | "string"
                            | "object"
                    )
                {
                    let pat_ty = TypeId::Named(name.clone());
                    let pat_ty = match name.as_str() {
                        "int" => TypeId::Int,
                        "long" => TypeId::Long,
                        "short" => TypeId::Short,
                        "byte" => TypeId::Byte,
                        "bool" => TypeId::Bool,
                        "char" => TypeId::Char,
                        "float" => TypeId::Float,
                        "double" => TypeId::Double,
                        "string" => TypeId::String,
                        _ => pat_ty,
                    };
                    return Ok(MatchPat::Type {
                        ty: pat_ty,
                        binding: None,
                    });
                }
                Err(TypeError::Oop(format!(
                    "unknown switch pattern `{name}` (expected type, enum variant, null, var, or `_`)"
                )))
            }
            // RFC 004 M1：variant case 模式 `Type.Case(binding)` / `Type.Case`
            // 也用于 enum 变体模式 `EnumType.VariantName`（解析器对两者均产出 `Pattern::Variant`）。
            Pattern::Variant {
                path,
                type_args,
                case,
                binding,
            } => {
                let template_name = path.last().ok_or_else(|| TypeError::Mismatch {
                    expected: "variant pattern with type name".into(),
                    found: "empty path".into(),
                })?;
                // RFC 004 M2：`Option<int>.Some(n)` — 按 type_args 单态化到 `Option_int`
                let variant_name: Ident = if !type_args.is_empty() {
                    if !self.registry.is_variant(template_name)
                        || !self.registry.is_generic_template(template_name)
                    {
                        return Err(TypeError::Oop(format!(
                            "`{}` is not a generic variant type",
                            template_name
                        )));
                    }
                    let arg_tys: Vec<TypeId> = type_args
                        .iter()
                        .map(|t| self.lower_type(&t.node))
                        .collect::<Result<Vec<_>, _>>()?;
                    let mangled = crate::generics::mangle_generic(template_name.as_str(), &arg_tys);
                    let mangled_ident: Ident = mangled.as_str().into();
                    if !self.registry.is_variant(&mangled_ident) {
                        if let Some(tmpl) = self.registry.types.get(template_name).cloned() {
                            let map: IndexMap<Ident, TypeId> = tmpl
                                .generic_params
                                .iter()
                                .zip(arg_tys.iter())
                                .map(|(p, t)| (p.clone(), t.clone()))
                                .collect();
                            let inst_cases: Vec<_> = tmpl
                                .variants
                                .iter()
                                .map(|c| {
                                    let mut copy = c.clone();
                                    if let Some(ref p) = c.payload {
                                        copy.payload =
                                            Some(crate::generics::substitute_type_name(p, &map));
                                    }
                                    copy
                                })
                                .collect();
                            let inst = crate::oop_types::NominalType {
                                name: mangled_ident.clone(),
                                kind: crate::oop_types::TypeKind::Variant,
                                variants: inst_cases,
                                ..tmpl
                            };
                            self.registry.types.insert(mangled_ident.clone(), inst);
                        }
                    }
                    if !self.registry.is_variant(&mangled_ident) {
                        return Err(TypeError::Oop(format!(
                            "failed to instantiate generic variant `{}`",
                            template_name
                        )));
                    }
                    mangled_ident
                } else {
                    template_name.clone()
                };
                // CD-30：pattern 的变体/枚举类型名须与**注解侧**同一规范化（`lower_type`
                // 经 `resolve_type_path`/`resolve_collision_fqn` 沿调用点 namespace 链把
                // 同名类型解析到 FQN）。scrutinee 若来自注解（`Content c` / `Content Make()`）
                // 已是 `BatchVariants.CaseN.Content`（FQN），而 pattern 的 `template_name`
                // 是原始短名 `Content`——直接以短名构造 `expected_ty` 与 scrutinee 做
                // `types_compatible` 会字符串失配（批处理跨 case 同名 variant 实测
                // `expected X.Content, found Content`）。此处用 `resolve_type_path` 解析出
                // 与 annotation 相同的 TypeId 表示；泛型（`Option<int>.Some`）维持 mangled
                // 短名（注册键在 `types`，无 FQN 化）。
                let pattern_variant_ty = if type_args.is_empty() {
                    self
                        .resolve_type_path(path)
                        .unwrap_or_else(|| TypeId::Named(variant_name.clone()))
                } else {
                    TypeId::Named(variant_name.clone())
                };
                // enum 变体模式：`EnumType.VariantName`
                // 解析器与 variant 共用 `Pattern::Variant` AST；typeck 在此分流到 enum 路径。
                if self.registry.is_enum(&variant_name) {
                    if self.registry.enum_variant(&variant_name, case).is_none() {
                        return Err(TypeError::UnknownEnumVariant {
                            enum_name: variant_name.to_string(),
                            variant: case.to_string(),
                        });
                    }
                    let expected_ty = pattern_variant_ty.clone();
                    if !self.types_compatible(scrutinee_ty, &expected_ty) {
                        return Err(TypeError::Mismatch {
                            expected: scrutinee_ty.display(),
                            found: expected_ty.display(),
                        });
                    }
                    // enum 变体模式不支持 payload binding（与 RFC 004 variant 不同）。
                    if binding.is_some() {
                        return Err(TypeError::Oop(format!(
                            "enum variant `{}.{}` does not take a binding",
                            variant_name, case
                        )));
                    }
                    return Ok(MatchPat::Variant {
                        case: case.clone(),
                        binding: None,
                    });
                }
                if !self.registry.is_variant(&variant_name) {
                    return Err(TypeError::Oop(format!(
                        "`{}` is not a variant type",
                        variant_name
                    )));
                }
                let case_info = self.registry.variant_case(&variant_name, case).ok_or_else(|| {
                    TypeError::Oop(format!(
                        "variant `{}` has no case `{}`",
                        variant_name, case
                    ))
                })?;
                let expected_ty = pattern_variant_ty.clone();
                if !self.types_compatible(scrutinee_ty, &expected_ty) {
                    return Err(TypeError::Mismatch {
                        expected: scrutinee_ty.display(),
                        found: expected_ty.display(),
                    });
                }
                let binding_info = match (binding, &case_info.payload) {
                    (Some(b), Some(payload_ty)) => {
                        Some((b.clone(), TypeId::Named(payload_ty.clone())))
                    }
                    (Some(_), None) => {
                        return Err(TypeError::Oop(format!(
                            "variant case `{}.{}` has no payload; cannot bind",
                            variant_name, case
                        )));
                    }
                    (None, Some(_)) => None,
                    (None, None) => None,
                };
                Ok(MatchPat::Variant {
                    case: case.clone(),
                    binding: binding_info,
                })
            }
            Pattern::Positional(_) => Err(TypeError::Oop(
                "internal: positional pattern must be rewritten before match classification (RFC 004 M3)"
                    .into(),
            )),
        }
    }

    pub(crate) fn check_match_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_ty: &TypeId,
        enum_name: Option<&Ident>,
    ) -> Result<MatchPat, TypeError> {
        self.classify_match_pattern(pattern, scrutinee_ty, enum_name)
    }

    /// Type-check a builtin `string` instance method (P2).
    ///
    /// Returns `Ok(Some(ret))` when `method` is a recognised string method
    /// and the arguments type-check; `Ok(None)` when the method is not a
    /// string builtin so the caller can fall through to registry resolution.
    pub(crate) fn check_builtin_string_method(
        &mut self,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        Ok(Some(match method.as_str() {
            "Split" => {
                // Split(char|string)
                // Split(char|string, StringSplitOptions)
                // Split(params char...) / Split(char[]) — 多分隔符
                // Split(char|string|char[], int count, StringSplitOptions)
                // Split(params char..., StringSplitOptions) — 多分隔符+选项（末参 Options）
                let string_arr = TypeId::Array {
                    elem: Box::new(TypeId::String),
                };
                let is_opts = |ty: &TypeId| matches!(ty, TypeId::Named(n) if n.as_str() == "StringSplitOptions");
                let is_sep = |ty: &TypeId, this: &Self| {
                    this.types_compatible(&TypeId::String, ty)
                        || this.types_compatible(&TypeId::Char, ty)
                        || matches!(ty, TypeId::Array { elem } if this.types_compatible(&TypeId::Char, elem))
                };
                let is_char_like = |ty: &TypeId, this: &Self| {
                    this.types_compatible(&TypeId::Char, ty)
                        || matches!(ty, TypeId::Array { elem } if this.types_compatible(&TypeId::Char, elem))
                };
                match args.len() {
                    1 => {
                        let a = self.check_expr_at(args[0].span, &args[0].node)?;
                        if !is_sep(&a.ty, self) {
                            return Err(TypeError::Mismatch {
                                expected: "string, char, or char[]".into(),
                                found: a.ty.display(),
                            });
                        }
                        string_arr
                    }
                    2 => {
                        let a = self.check_expr_at(args[0].span, &args[0].node)?;
                        let b = self.check_expr_at(args[1].span, &args[1].node)?;
                        if is_opts(&b.ty) {
                            if !is_sep(&a.ty, self) {
                                return Err(TypeError::Mismatch {
                                    expected: "string, char, or char[]".into(),
                                    found: a.ty.display(),
                                });
                            }
                        } else if is_char_like(&a.ty, self)
                            && self.types_compatible(&TypeId::Char, &b.ty)
                        {
                            // params char...（≥2）
                        } else {
                            return Err(TypeError::Mismatch {
                                expected: "StringSplitOptions or char (multi-separator)".into(),
                                found: b.ty.display(),
                            });
                        }
                        string_arr
                    }
                    n if n >= 3 => {
                        let last = self.check_expr_at(args[n - 1].span, &args[n - 1].node)?;
                        if is_opts(&last.ty) {
                            // (sep, count, options) 或 (c0..cN, options)
                            if n == 3 {
                                let a = self.check_expr_at(args[0].span, &args[0].node)?;
                                let mid = self.check_expr_at(args[1].span, &args[1].node)?;
                                if is_sep(&a.ty, self)
                                    && self.types_compatible(&TypeId::Int, &mid.ty)
                                {
                                    // Split(sep, count, options)
                                } else if self.types_compatible(&TypeId::Char, &a.ty)
                                    && self.types_compatible(&TypeId::Char, &mid.ty)
                                {
                                    // Split(c0, c1, options)
                                } else {
                                    return Err(TypeError::Mismatch {
                                        expected: "(sep, count, options) or (char, char, options)"
                                            .into(),
                                        found: format!(
                                            "{}, {}, {}",
                                            a.ty.display(),
                                            mid.ty.display(),
                                            last.ty.display()
                                        ),
                                    });
                                }
                            } else {
                                for i in 0..n - 1 {
                                    self.require_char_arg(args, i)?;
                                }
                            }
                        } else {
                            // 全 char params
                            for i in 0..n {
                                self.require_char_arg(args, i)?;
                            }
                        }
                        string_arr
                    }
                    _ => unreachable!(),
                }
            }
            "Replace" => {
                self.require_arg_count(args, 2, method)?;
                self.require_string_arg(args, 0)?;
                self.require_string_arg(args, 1)?;
                TypeId::String
            }
            "Substring" => match args.len() {
                1 => {
                    self.require_int_arg(args, 0)?;
                    TypeId::String
                }
                2 => {
                    self.require_int_arg(args, 0)?;
                    self.require_int_arg(args, 1)?;
                    TypeId::String
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{n} arguments"),
                    });
                }
            },
            "Contains" => {
                self.require_arg_count(args, 1, method)?;
                self.require_string_arg(args, 0)?;
                TypeId::Bool
            }
            "StartsWith" | "EndsWith" => {
                self.require_arg_count(args, 1, method)?;
                // string 或 char（codegen 按 LLVM 类型分派）
                let a = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(&TypeId::String, &a.ty)
                    && !self.types_compatible(&TypeId::Char, &a.ty)
                {
                    return Err(TypeError::Mismatch {
                        expected: "string or char".into(),
                        found: a.ty.display(),
                    });
                }
                TypeId::Bool
            }
            "IndexOf" | "LastIndexOf" => match args.len() {
                1 => {
                    // Accept either string or char argument (代码生成根据参数类型分派)
                    TypeId::Int
                }
                2 => {
                    // Accept string+int or char+int (代码生成根据参数类型分派)
                    self.require_int_arg(args, 1)?;
                    TypeId::Int
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{n} arguments"),
                    });
                }
            },
            "Insert" => {
                self.require_arg_count(args, 2, method)?;
                self.require_int_arg(args, 0)?;
                self.require_string_arg(args, 1)?;
                TypeId::String
            }
            "Remove" => match args.len() {
                1 => {
                    self.require_int_arg(args, 0)?;
                    TypeId::String
                }
                2 => {
                    self.require_int_arg(args, 0)?;
                    self.require_int_arg(args, 1)?;
                    TypeId::String
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{n} arguments"),
                    });
                }
            },
            "Trim" | "TrimStart" | "TrimEnd" => {
                // Trim() | Trim(char) | Trim(params char...) | Trim(char[])
                if args.is_empty() {
                    TypeId::String
                } else if args.len() == 1 {
                    let a = self.check_expr_at(args[0].span, &args[0].node)?;
                    let ok = self.types_compatible(&TypeId::Char, &a.ty)
                        || matches!(&a.ty, TypeId::Array { elem } if self.types_compatible(&TypeId::Char, elem));
                    if !ok {
                        return Err(TypeError::Mismatch {
                            expected: "char or char[]".into(),
                            found: a.ty.display(),
                        });
                    }
                    TypeId::String
                } else {
                    for i in 0..args.len() {
                        self.require_char_arg(args, i)?;
                    }
                    TypeId::String
                }
            }
            "ToUpper" | "ToLower" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::String
            }
            // 实例 GetHashCode()——与 IHashable 静态 `string.GetHashCode(s)` 同契约；
            // codegen 走既有 string 哈希路径（当前为 length，见 emit_prim_get_hash_code）。
            "GetHashCode" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Int
            }
            // `Length` is a property in C#, but Arc code often calls it as a
            // method `s.Length()`. Treat the 0-arg call form identically to
            // the property access form (returns int).
            "Length" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Int
            }
            "ToCharArray" => {
                // ToCharArray() | ToCharArray(start, length) — UTF-8 码元；越界钳制同 Substring
                match args.len() {
                    0 => {}
                    2 => {
                        self.require_int_arg(args, 0)?;
                        self.require_int_arg(args, 1)?;
                    }
                    _ => {
                        return Err(TypeError::Mismatch {
                            expected: "0 or 2 arguments".into(),
                            found: format!("{} arguments", args.len()),
                        });
                    }
                }
                TypeId::Array {
                    elem: Box::new(TypeId::Char),
                }
            }
            // RFC 005 M2：UTF-8 诚实 —— 零拷贝视图为 `ReadOnlySpan<byte>`（码元=字节，非 UTF-16）。
            "AsSpan" => match args.len() {
                0 => TypeId::Span {
                    elem: Box::new(TypeId::Byte),
                    mutable: false,
                },
                2 => {
                    self.require_int_arg(args, 0)?;
                    self.require_int_arg(args, 1)?;
                    TypeId::Span {
                        elem: Box::new(TypeId::Byte),
                        mutable: false,
                    }
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "0 or 2 argument(s) for `string.AsSpan`".into(),
                        found: format!("{n} argument(s)"),
                    });
                }
            },
            "Compare" => {
                self.require_arg_count(args, 1, method)?;
                self.require_string_arg(args, 0)?;
                TypeId::Int
            }
            "PadLeft" | "PadRight" => match args.len() {
                1 => {
                    self.require_int_arg(args, 0)?;
                    TypeId::String
                }
                2 => {
                    self.require_int_arg(args, 0)?;
                    self.require_char_arg(args, 1)?;
                    TypeId::String
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "1 or 2 arguments".into(),
                        found: format!("{n} arguments"),
                    });
                }
            },
            _ => return Ok(None),
        }))
    }

    /// Type-check a builtin `Task<T>`/`Task` instance method (RFC 009 M1).
    ///
    /// Returns `Ok(Some(ret))` when `method` is a recognised Task method
    /// and the arguments type-check; `Ok(None)` when the method is not a
    /// Task builtin so the caller can fall through to registry resolution.
    /// M1 仅实现同步路径 API：Wait/Cancel/GetResult。GetAwaiter 依赖 M2 状态机
    /// lowering，M1 不支持。
    pub(crate) fn check_builtin_task_method(
        &mut self,
        recv_ty: &TypeId,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        let inner = match recv_ty {
            TypeId::Task { inner } => inner,
            _ => return Ok(None),
        };
        Ok(Some(match method.as_str() {
            "Wait" => {
                // M5.7: Wait() / Wait(int timeoutMs) / Wait(CancellationToken)
                match args.len() {
                    0 => TypeId::Void,
                    1 => {
                        let arg0 = self.check_expr_at(args[0].span, &args[0].node)?;
                        match &arg0.ty {
                            TypeId::Named(n) if n.as_str() == "CancellationToken" => TypeId::Bool,
                            TypeId::Int => TypeId::Bool,
                            other => {
                                return Err(TypeError::Mismatch {
                                    expected: "int or CancellationToken".into(),
                                    found: other.display(),
                                })
                            }
                        }
                    }
                    n => {
                        return Err(TypeError::Mismatch {
                            expected: "0 or 1 argument".into(),
                            found: format!("{n} arguments"),
                        })
                    }
                }
            }
            "Cancel" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Void
            }
            "GetResult" => {
                // 方法形式访问结果（与 Result 属性等价，便于链式调用）
                self.require_arg_count(args, 0, method)?;
                (**inner).clone()
            }
            "ConfigureAwait" => {
                // M5.7: task.ConfigureAwait(bool) → Task<T>（恒等映射）
                self.require_arg_count(args, 1, method)?;
                self.check_expr_at(args[0].span, &args[0].node)?; // validate arg exists
                TypeId::Task {
                    inner: inner.clone(),
                }
            }
            _ => return Ok(None),
        }))
    }

    /// CancellationTokenSource facade (RFC 009 M4): CTS 实例方法拦截。
    /// std/Arc/Tasks/CancellationTokenSource.as 为 stub，方法体不执行；typeck 在此校验
    /// 参数形状并返回类型，codegen 拦截后发射 rt_cts_* ABI。
    /// CT 与 CTS 共享同一 RtCts* 指针（D2 决策：CT 是 CTS 的只读别名）。
    pub(crate) fn check_builtin_cts_method(
        &mut self,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        Ok(Some(match method.as_str() {
            "Cancel" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Void
            }
            "CancelAfter" => {
                self.require_arg_count(args, 1, method)?;
                self.require_int_arg(args, 0)?;
                TypeId::Void
            }
            "get_Token" => {
                // CT 与 CTS 共享指针：返回 CancellationToken（Named 类型）
                self.require_arg_count(args, 0, method)?;
                TypeId::Named("CancellationToken".into())
            }
            "get_IsCancellationRequested" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Bool
            }
            _ => return Ok(None),
        }))
    }

    /// CancellationToken facade (RFC 005 M4): CT 实例方法拦截。
    /// CT 是 CTS 的只读视图（同一 RtCts* 指针）。
    pub(crate) fn check_builtin_ct_method(
        &mut self,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        Ok(Some(match method.as_str() {
            "ThrowIfCancellationRequested" => {
                // M4 用 rt_panic 兜底（D4 决策）；Exception 体系留独立 RFC
                self.require_arg_count(args, 0, method)?;
                TypeId::Void
            }
            "Register" => {
                // ct.Register(Action callback) → void
                self.require_arg_count(args, 1, method)?;
                let _ = self.check_expr_at(args[0].span, &args[0].node)?;
                TypeId::Void
            }
            "get_IsCancellationRequested" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Bool
            }
            "get_CanBeCanceled" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Bool
            }
            _ => return Ok(None),
        }))
    }

    /// RFC 005：`T[]` → `AsSpan` / `AsSpan(start,len)` / `AsReadOnlySpan`。
    pub(crate) fn check_builtin_array_span_method(
        &mut self,
        recv_ty: &TypeId,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        let TypeId::Array { elem } = recv_ty else {
            return Ok(None);
        };
        Ok(Some(match method.as_str() {
            "AsSpan" => match args.len() {
                0 => TypeId::Span {
                    elem: elem.clone(),
                    mutable: true,
                },
                2 => {
                    let start = self.check_expr_at(args[0].span, &args[0].node)?;
                    let len = self.check_expr_at(args[1].span, &args[1].node)?;
                    if !self.types_compatible(&TypeId::Int, &start.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: start.ty.display(),
                        });
                    }
                    if !self.types_compatible(&TypeId::Int, &len.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: len.ty.display(),
                        });
                    }
                    TypeId::Span {
                        elem: elem.clone(),
                        mutable: true,
                    }
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "0 or 2 argument(s) for `AsSpan`".into(),
                        found: format!("{n} argument(s)"),
                    });
                }
            },
            "AsReadOnlySpan" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Span {
                    elem: elem.clone(),
                    mutable: false,
                }
            }
            _ => return Ok(None),
        }))
    }

    /// RFC 005 M2：`List<T>` → `AsSpan` / `AsSpan(start,len)` / `AsReadOnlySpan`。
    ///
    /// 视图绑定当前 buffer；扩容（`Add` 触发 realloc）后持有的 Span **失效**（M2 不铺满
    /// 失效诊断；与 RFC「禁止持有过 Add」同残余）。
    pub(crate) fn check_builtin_list_span_method(
        &mut self,
        recv_ty: &TypeId,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        let TypeId::Named(name) = recv_ty else {
            return Ok(None);
        };
        if !name.starts_with("List_") {
            return Ok(None);
        }
        let Some(elem) = recv_ty.enumerable_elem() else {
            return Ok(None);
        };
        Ok(Some(match method.as_str() {
            "AsSpan" => match args.len() {
                0 => TypeId::Span {
                    elem: Box::new(elem),
                    mutable: true,
                },
                2 => {
                    self.require_int_arg(args, 0)?;
                    self.require_int_arg(args, 1)?;
                    TypeId::Span {
                        elem: Box::new(elem),
                        mutable: true,
                    }
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "0 or 2 argument(s) for `List.AsSpan`".into(),
                        found: format!("{n} argument(s)"),
                    });
                }
            },
            "AsReadOnlySpan" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Span {
                    elem: Box::new(elem),
                    mutable: false,
                }
            }
            _ => return Ok(None),
        }))
    }

    /// RFC 005：`Span<T>` / `ReadOnlySpan<T>` 实例方法（Slice / AsReadOnly）。
    pub(crate) fn check_builtin_span_method(
        &mut self,
        recv_ty: &TypeId,
        method: &Ident,
        args: &[Spanned<Expr>],
    ) -> Result<Option<TypeId>, TypeError> {
        let TypeId::Span { elem, mutable } = recv_ty else {
            return Ok(None);
        };
        Ok(Some(match method.as_str() {
            "Slice" => match args.len() {
                1 | 2 => {
                    let start = self.check_expr_at(args[0].span, &args[0].node)?;
                    if !self.types_compatible(&TypeId::Int, &start.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: start.ty.display(),
                        });
                    }
                    if args.len() == 2 {
                        let len = self.check_expr_at(args[1].span, &args[1].node)?;
                        if !self.types_compatible(&TypeId::Int, &len.ty) {
                            return Err(TypeError::Mismatch {
                                expected: "int".into(),
                                found: len.ty.display(),
                            });
                        }
                    }
                    TypeId::Span {
                        elem: elem.clone(),
                        mutable: *mutable,
                    }
                }
                n => {
                    return Err(TypeError::Mismatch {
                        expected: "1 or 2 argument(s) for Span.Slice".into(),
                        found: format!("{n} argument(s)"),
                    });
                }
            },
            "Fill" if *mutable => {
                self.require_arg_count(args, 1, method)?;
                let v = self.check_expr_at(args[0].span, &args[0].node)?;
                if !self.types_compatible(elem, &v.ty) {
                    return Err(TypeError::Mismatch {
                        expected: elem.display(),
                        found: v.ty.display(),
                    });
                }
                TypeId::Void
            }
            "Clear" if *mutable => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Void
            }
            "AsReadOnly" if *mutable => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Span {
                    elem: elem.clone(),
                    mutable: false,
                }
            }
            // RFC 005 std 面：`span.CopyTo(Span<T> dest)`（源可为只读或可变）。
            "CopyTo" => {
                self.require_arg_count(args, 1, method)?;
                let dest = self.check_expr_at(args[0].span, &args[0].node)?;
                match &dest.ty {
                    TypeId::Span {
                        elem: dest_elem,
                        mutable: true,
                    } if self.types_compatible(elem, dest_elem) => TypeId::Void,
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: format!("Span<{}>", elem.display()),
                            found: other.display(),
                        });
                    }
                }
            }
            // RFC 005：`TryCopyTo(Span<T>)` → bool；目标过短返回 false（不 panic）。
            "TryCopyTo" => {
                self.require_arg_count(args, 1, method)?;
                let dest = self.check_expr_at(args[0].span, &args[0].node)?;
                match &dest.ty {
                    TypeId::Span {
                        elem: dest_elem,
                        mutable: true,
                    } if self.types_compatible(elem, dest_elem) => TypeId::Bool,
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: format!("Span<{}>", elem.display()),
                            found: other.display(),
                        });
                    }
                }
            }
            // RFC 005：`ToArray()` → 新 `T[]`（堆拷贝；与视图解耦）。
            "ToArray" => {
                self.require_arg_count(args, 0, method)?;
                TypeId::Array { elem: elem.clone() }
            }
            _ => return Ok(None),
        }))
    }

    /// Check that `args.len() == expected`, returning a `TypeError::Mismatch` otherwise.
    fn require_arg_count(
        &self,
        args: &[Spanned<Expr>],
        expected: usize,
        method: &Ident,
    ) -> Result<(), TypeError> {
        if args.len() != expected {
            return Err(TypeError::Mismatch {
                expected: format!("{expected} argument(s) for `string.{method}`"),
                found: format!("{} argument(s)", args.len()),
            });
        }
        Ok(())
    }

    /// Check that `args[idx]` is compatible with `string`.
    pub(crate) fn require_string_arg(
        &mut self,
        args: &[Spanned<Expr>],
        idx: usize,
    ) -> Result<(), TypeError> {
        let a = self.check_expr_at(args[idx].span, &args[idx].node)?;
        if !self.types_compatible(&TypeId::String, &a.ty) {
            return Err(TypeError::Mismatch {
                expected: "string".into(),
                found: a.ty.display(),
            });
        }
        Ok(())
    }

    /// RFC 034 M5: 校验 `args[idx]` 是 IFormatProvider（或其实现类型）。
    /// 允许：实现了 IFormatProvider 的具名类、IFormatProvider 接口类型本身。
    pub(crate) fn require_format_provider_arg(
        &mut self,
        args: &[Spanned<Expr>],
        idx: usize,
    ) -> Result<(), TypeError> {
        let a = self.check_expr_at(args[idx].span, &args[idx].node)?;
        let ok = match &a.ty {
            TypeId::Named(class_name) => {
                if class_name.as_str() == "IFormatProvider" {
                    true
                } else {
                    let iface: Ident = "IFormatProvider".into();
                    self.registry.implements_interface(class_name, &iface)
                }
            }
            _ => false,
        };
        if !ok {
            return Err(TypeError::Mismatch {
                expected: "IFormatProvider".into(),
                found: a.ty.display(),
            });
        }
        Ok(())
    }

    /// Check that `args[idx]` is compatible with `int`.
    fn require_int_arg(&mut self, args: &[Spanned<Expr>], idx: usize) -> Result<(), TypeError> {
        let a = self.check_expr_at(args[idx].span, &args[idx].node)?;
        if !self.types_compatible(&TypeId::Int, &a.ty) {
            return Err(TypeError::Mismatch {
                expected: "int".into(),
                found: a.ty.display(),
            });
        }
        Ok(())
    }

    /// Check that `args[idx]` is compatible with `char`.
    fn require_char_arg(&mut self, args: &[Spanned<Expr>], idx: usize) -> Result<(), TypeError> {
        let a = self.check_expr_at(args[idx].span, &args[idx].node)?;
        if !self.types_compatible(&TypeId::Char, &a.ty) {
            return Err(TypeError::Mismatch {
                expected: "char".into(),
                found: a.ty.display(),
            });
        }
        Ok(())
    }

    /// RFC 005 dogfood：将 `params ReadOnlySpan<Task>` 调用点实参脱糖为单一
    /// [`Expr::StackSpanLit`]（或既有 ROS 直传）。就地改写 `args`。
    fn bind_params_task_span(&mut self, args: &mut Vec<Spanned<Expr>>) -> Result<(), TypeError> {
        let task_ty = TypeId::Task {
            inner: Box::new(TypeId::Void),
        };
        let ros_ty = TypeId::Span {
            elem: Box::new(task_ty.clone()),
            mutable: false,
        };
        if args.is_empty() {
            *args = vec![Spanned::new(
                Expr::StackSpanLit {
                    elements: vec![],
                    mutable: false,
                    elem: task_ty,
                },
                Span::DUMMY,
            )];
            return Ok(());
        }
        if args.len() == 1 {
            let te = self.check_expr_at(args[0].span, &args[0].node)?;
            if self.types_compatible(&ros_ty, &te.ty)
                || matches!(
                    &te.ty,
                    TypeId::Span { elem, .. } if matches!(elem.as_ref(), TypeId::Task { .. })
                )
            {
                args[0] = Spanned::new(te.expr, args[0].span);
                return Ok(());
            }
            if self.types_compatible(&task_ty, &te.ty) || matches!(&te.ty, TypeId::Task { .. }) {
                *args = vec![Spanned::new(
                    Expr::StackSpanLit {
                        elements: vec![Spanned::new(te.expr, args[0].span)],
                        mutable: false,
                        elem: task_ty,
                    },
                    args[0].span,
                )];
                return Ok(());
            }
            return Err(TypeError::Mismatch {
                expected: "ReadOnlySpan<Task> or Task (params)".into(),
                found: te.ty.display(),
            });
        }
        let mut elements = Vec::with_capacity(args.len());
        let mut span = Span::DUMMY;
        for a in args.iter() {
            let te = self.check_expr_at(a.span, &a.node)?;
            if !(self.types_compatible(&task_ty, &te.ty) || matches!(&te.ty, TypeId::Task { .. })) {
                return Err(TypeError::Mismatch {
                    expected: "Task".into(),
                    found: te.ty.display(),
                });
            }
            if span == Span::DUMMY {
                span = a.span;
            }
            elements.push(Spanned::new(te.expr, a.span));
        }
        *args = vec![Spanned::new(
            Expr::StackSpanLit {
                elements,
                mutable: false,
                elem: task_ty,
            },
            span,
        )];
        Ok(())
    }
}
