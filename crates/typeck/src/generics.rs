//! Generic monomorphization helpers.
//!
//! Name mangling convention (single type parameter): `{Def}_{arg}` where `arg` is the
//! concrete type suffix (`int`, `bool`, `string`, or a named type like `User`).
//! Examples: `Box<int>` 鈫?`Box_int`, `Identity<string>` 鈫?`Identity_string`.

use ast::TypeId;
use ast::*;
use indexmap::IndexMap;

/// Suffix for one concrete type argument in a mangled symbol.
///
/// 涓?`type_id_to_field_name` 淇濇寔涓€鑷达紝鎵€鏈夊鍚堢被鍨嬶紙Func/IEnumerable/
/// IQueryable/Expression/Nullable/Task/Vector/Array/Ref锛夐兘鏈夋樉寮?case锛?
/// 涓嶄娇鐢?`display().replace()` 浜х敓鍚嫭鍙?绠ご鐨勯潪娉?LLVM 鏍囪瘑绗︺€?
pub fn mangle_type_suffix(ty: &TypeId) -> String {
    match ty {
        TypeId::Void => "void".into(),
        TypeId::Int => "int".into(),
        TypeId::Long => "long".into(),
        TypeId::Short => "short".into(),
        TypeId::Byte => "byte".into(),
        TypeId::Char => "char".into(),
        TypeId::Float => "float".into(),
        TypeId::Double => "double".into(),
        TypeId::Bool => "bool".into(),
        TypeId::UInt => "uint".into(),
        TypeId::ULong => "ulong".into(),
        TypeId::UShort => "ushort".into(),
        TypeId::SByte => "sbyte".into(),
        TypeId::String => "string".into(),
        // RFC 006 M1: object 鏍圭被鍨嬬殑 mangle 鍚庣紑
        TypeId::Object => "object".into(),
        TypeId::Named(n) => n.to_string(),
        TypeId::Generic(n) => n.to_string(),
        TypeId::Array { elem } => format!("{}_arr", mangle_type_suffix(elem)),
        TypeId::Task { inner } => format!("Task_{}", mangle_type_suffix(inner)),
        TypeId::Ref { inner, .. } => mangle_type_suffix(inner),
        TypeId::Vector { elem, n } => format!("Vector_{}_{}", mangle_type_suffix(elem), n),
        TypeId::Span { elem, mutable } => {
            let prefix = if *mutable { "Span" } else { "ReadOnlySpan" };
            format!("{prefix}_{}", mangle_type_suffix(elem))
        }
        TypeId::Func { params, ret } => {
            let mut s = String::from("Func");
            for p in params {
                s.push('_');
                s.push_str(&mangle_type_suffix(p));
            }
            s.push('_');
            s.push_str(&mangle_type_suffix(ret));
            s
        }
        TypeId::IEnumerable { inner } => format!("IEnumerable_{}", mangle_type_suffix(inner)),
        TypeId::IQueryable { inner } => format!("IQueryable_{}", mangle_type_suffix(inner)),
        TypeId::Expression { inner } => format!("Expression_{}", mangle_type_suffix(inner)),
        TypeId::Nullable { inner } => {
            // 可空引用类型标注在签名 mangle 中**归约为基础类型**——与
            // `type_id_to_field_name` 行为一致（`object?` ≡ `object`，见该函数
            // 注释：两 stringifier 分叉曾致重载解析失败）。可空值类型
            // （`int?`）保持独立编码，维持值类型可空性区分。
            let is_ref = matches!(
                inner.as_ref(),
                TypeId::Object
                    | TypeId::String
                    | TypeId::Named(_)
                    | TypeId::Func { .. }
                    | TypeId::Task { .. }
                    | TypeId::IEnumerable { .. }
                    | TypeId::IQueryable { .. }
                    | TypeId::Expression { .. }
                    | TypeId::Nullable { .. }
            );
            if is_ref {
                mangle_type_suffix(inner)
            } else {
                format!("Nullable_{}", mangle_type_suffix(inner))
            }
        }
        TypeId::Infer => "Infer".into(),
        TypeId::Error => "Error".into(),
    }
}

/// Mangle a generic definition with concrete type arguments, e.g. `Box` + `[int]` 鈫?`Box_int`.
pub fn mangle_generic(def: &str, args: &[TypeId]) -> String {
    if args.is_empty() {
        return def.to_string();
    }
    let suffixes: Vec<_> = args.iter().map(mangle_type_suffix).collect();
    format!("{def}_{}", suffixes.join("_"))
}

pub fn type_id_to_field_name(ty: &TypeId) -> Ident {
    match ty {
        TypeId::Void => "void".into(),
        TypeId::Int => "int".into(),
        TypeId::Long => "long".into(),
        TypeId::Short => "short".into(),
        TypeId::Byte => "byte".into(),
        TypeId::Char => "char".into(),
        TypeId::Float => "float".into(),
        TypeId::Double => "double".into(),
        TypeId::Bool => "bool".into(),
        TypeId::UInt => "uint".into(),
        TypeId::ULong => "ulong".into(),
        TypeId::UShort => "ushort".into(),
        TypeId::SByte => "sbyte".into(),
        TypeId::String => "string".into(),
        // RFC 006 M1: object 鏍圭被鍨嬩綔涓哄瓧娈靛悕鍚庣紑
        TypeId::Object => "object".into(),
        TypeId::Named(n) => n.clone(),
        TypeId::Generic(n) => n.clone(),
        TypeId::Array { elem } => format!("{}_arr", type_id_to_field_name(elem)).into(),
        TypeId::Func { params, ret } => {
            let mut s = String::from("Func");
            for p in params {
                s.push('_');
                s.push_str(&type_id_to_field_name(p));
            }
            s.push('_');
            s.push_str(&type_id_to_field_name(ret));
            s.into()
        }
        TypeId::Task { inner } => format!("Task_{}", type_id_to_field_name(inner)).into(),
        TypeId::Ref { inner, .. } => type_id_to_field_name(inner),
        TypeId::IEnumerable { inner } => {
            format!("IEnumerable_{}", type_id_to_field_name(inner)).into()
        }
        TypeId::IQueryable { inner } => {
            format!("IQueryable_{}", type_id_to_field_name(inner)).into()
        }
        TypeId::Expression { inner } => {
            format!("Expression_{}", type_id_to_field_name(inner)).into()
        }
        TypeId::Nullable { inner } => {
            // 鍙┖寮曠敤绫诲瀷锛坄T?`锛夊綊绾︿负鍐呴儴绫诲瀷鍚嶏細`object?` 鈫?"object"銆乣ILogger?` 鈫?"ILogger"銆?
            // 涓?`registry::type_path_name` 琛屼负涓€鑷粹€斺€擟# 涓彲绌哄紩鐢ㄧ被鍨嬩笌鍩虹绫诲瀷鍦ㄧ鍚?
            // 鍏煎鎬т笂绛変环锛堜粎缂栬瘧鏈熸爣娉紝闈炵嫭绔嬬被鍨嬶級銆傚惁鍒?`sp.GetKeyedService(typeof(T), key)`
            // 涓?`key: object?` 浼氳 mangle 涓?"Nullable_object"锛屼笌鎺ュ彛鍙傛暟 "object" 涓嶅尮閰嶏紝
            // 瀵艰嚧閲嶈浇瑙ｆ瀽澶辫触銆佸洖閫€鍒版墿灞曟柟娉曡矾寰勮Е鍙戝弬鏁版暟閲忛敊璇€?
            type_id_to_field_name(inner)
        }
        TypeId::Vector { elem, n } => {
            format!("Vector_{}_{}", type_id_to_field_name(elem), n).into()
        }
        TypeId::Span { elem, mutable } => {
            let prefix = if *mutable { "Span" } else { "ReadOnlySpan" };
            format!("{prefix}_{}", type_id_to_field_name(elem)).into()
        }
        TypeId::Infer => "Infer".into(),
        TypeId::Error => "Error".into(),
    }
}

pub fn substitute_type(ty: &TypeId, map: &IndexMap<Ident, TypeId>) -> TypeId {
    match ty {
        TypeId::Generic(n) | TypeId::Named(n) if map.contains_key(n) => {
            map.get(n).cloned().unwrap()
        }
        TypeId::Ref {
            inner,
            mutable,
            kind,
        } => TypeId::Ref {
            inner: Box::new(substitute_type(inner, map)),
            mutable: *mutable,
            kind: *kind,
        },
        TypeId::Array { elem } => TypeId::Array {
            elem: Box::new(substitute_type(elem, map)),
        },
        TypeId::Task { inner } => TypeId::Task {
            inner: Box::new(substitute_type(inner, map)),
        },
        TypeId::IEnumerable { inner } => TypeId::IEnumerable {
            inner: Box::new(substitute_type(inner, map)),
        },
        TypeId::IQueryable { inner } => TypeId::IQueryable {
            inner: Box::new(substitute_type(inner, map)),
        },
        TypeId::Expression { inner } => TypeId::Expression {
            inner: Box::new(substitute_type(inner, map)),
        },
        TypeId::Func { params, ret } => TypeId::Func {
            params: params.iter().map(|p| substitute_type(p, map)).collect(),
            ret: Box::new(substitute_type(ret, map)),
        },
        TypeId::Nullable { inner } => TypeId::Nullable {
            inner: Box::new(substitute_type(inner, map)),
        },
        TypeId::Vector { elem, n } => TypeId::Vector {
            elem: Box::new(substitute_type(elem, map)),
            n: *n,
        },
        TypeId::Span { elem, mutable } => TypeId::Span {
            elem: Box::new(substitute_type(elem, map)),
            mutable: *mutable,
        },
        other => other.clone(),
    }
}

pub fn substitute_type_ast(ty: &Type, map: &IndexMap<Ident, TypeId>) -> Type {
    match ty {
        Type::Named { path, generics } if generics.is_empty() && path.len() == 1 => {
            let name = &path[0];
            if let Some(sub) = map.get(name) {
                return type_id_to_ast(sub);
            }
            ty.clone()
        }
        Type::Named { path, generics } => Type::Named {
            path: path.clone(),
            generics: generics
                .iter()
                .map(|g| Spanned::new(substitute_type_ast(&g.node, map), g.span))
                .collect(),
        },
        Type::Ref { inner, mutable } => Type::Ref {
            inner: Box::new(Spanned::new(
                substitute_type_ast(&inner.node, map),
                inner.span,
            )),
            mutable: *mutable,
        },
        Type::Array { inner } => Type::Array {
            inner: Box::new(Spanned::new(
                substitute_type_ast(&inner.node, map),
                inner.span,
            )),
        },
        // RFC 007/DI：泛型方法返回可空 `T?` 时，需递归替换 inner 中的泛型参数，
        // 否则单态化体保留未替换的 `T?`，body 校验报 `expected T?, found <concrete>`。
        Type::Nullable { inner } => Type::Nullable {
            inner: Box::new(Spanned::new(
                substitute_type_ast(&inner.node, map),
                inner.span,
            )),
        },
        Type::Func { params, ret } => Type::Func {
            params: params
                .iter()
                .map(|p| Spanned::new(substitute_type_ast(&p.node, map), p.span))
                .collect(),
            ret: Box::new(Spanned::new(substitute_type_ast(&ret.node, map), ret.span)),
        },
        other => other.clone(),
    }
}

/// Substitute a single type name (Ident) through a generic-parameter map.
///
/// Used when monomorphizing variant payloads: if the payload type name is a
/// generic parameter (e.g. `T`), replace it with the concrete type name (e.g.
/// `int`). Otherwise return the name unchanged.
pub fn substitute_type_name(name: &Ident, map: &IndexMap<Ident, TypeId>) -> Ident {
    match map.get(name) {
        Some(ty) => type_id_to_field_name(ty),
        None => name.clone(),
    }
}

/// Convert a lowered `TypeId` back to an AST `Type` (generic mono / target-typed `new`).
pub(crate) fn type_id_to_ast(ty: &TypeId) -> Type {
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
        TypeId::Void => Type::Named {
            path: vec!["void".into()],
            generics: vec![],
        },
        TypeId::Named(n) => Type::Named {
            path: vec![n.clone()],
            generics: vec![],
        },
        TypeId::Generic(n) => Type::Named {
            path: vec![n.clone()],
            generics: vec![],
        },
        TypeId::Array { elem } => Type::Array {
            inner: Box::new(Spanned::new(type_id_to_ast(elem), Span::DUMMY)),
        },
        TypeId::Ref { inner, mutable, .. } => Type::Ref {
            inner: Box::new(Spanned::new(type_id_to_ast(inner), Span::DUMMY)),
            mutable: *mutable,
        },
        TypeId::Func { params, ret } => Type::Func {
            params: params
                .iter()
                .map(|p| Spanned::new(type_id_to_ast(p), Span::DUMMY))
                .collect(),
            ret: Box::new(Spanned::new(type_id_to_ast(ret), Span::DUMMY)),
        },
        TypeId::Task { inner } => Type::Named {
            path: vec!["Task".into()],
            generics: vec![Spanned::new(type_id_to_ast(inner), Span::DUMMY)],
        },
        TypeId::IEnumerable { inner } => Type::Named {
            path: vec!["IEnumerable".into()],
            generics: vec![Spanned::new(type_id_to_ast(inner), Span::DUMMY)],
        },
        TypeId::IQueryable { inner } => Type::Named {
            path: vec!["IQueryable".into()],
            generics: vec![Spanned::new(type_id_to_ast(inner), Span::DUMMY)],
        },
        TypeId::Expression { inner } => Type::Named {
            path: vec!["Expression".into()],
            generics: vec![Spanned::new(type_id_to_ast(inner), Span::DUMMY)],
        },
        TypeId::Nullable { inner } => Type::Nullable {
            inner: Box::new(Spanned::new(type_id_to_ast(inner), Span::DUMMY)),
        },
        TypeId::Vector { elem, n } => Type::Named {
            path: vec!["Vector".into()],
            generics: vec![
                Spanned::new(type_id_to_ast(elem), Span::DUMMY),
                Spanned::new(Type::ConstInt(*n as i64), Span::DUMMY),
            ],
        },
        TypeId::Span { elem, mutable } => Type::Named {
            path: vec![if *mutable { "Span" } else { "ReadOnlySpan" }.into()],
            generics: vec![Spanned::new(type_id_to_ast(elem), Span::DUMMY)],
        },
        TypeId::Infer => Type::Infer,
        TypeId::Error => Type::Named {
            path: vec!["<error>".into()],
            generics: vec![],
        },
    }
}

pub fn substitution_map(params: &[GenericParam], args: &[TypeId]) -> IndexMap<Ident, TypeId> {
    params
        .iter()
        .zip(args.iter())
        .map(|(p, a)| (p.name.clone(), a.clone()))
        .collect()
}

pub fn substitute_class_def(
    class: &ClassDef,
    mangled: &str,
    map: &IndexMap<Ident, TypeId>,
) -> ClassDef {
    let mut c = class.clone();
    c.name = mangled.into();
    c.generics = vec![];
    c.where_clause = vec![];
    for f in &mut c.fields {
        f.ty.node = substitute_type_ast(&f.ty.node, map);
        // 字段初始化器同样须替换类型参数（如 `Holder<T>.Cache =
        // new ConcurrentDictionary<string, T>()` → `...string, Thing>()`）。
        // 缺此替换时单态化类的静态字段 init 仍含 `T`，codegen 发射
        // `@__ctor_ConcurrentDictionary_string_T` / `@.vtable...._T`
        // undefined symbol（ORM 热路径「泛型字段 mono」）。
        if let Some(init) = &f.init {
            f.init = Some(Spanned::new(substitute_expr(&init.node, map), init.span));
        }
    }
    for p in &mut c.properties {
        p.ty.node = substitute_type_ast(&p.ty.node, map);
        for ip in &mut p.index_params {
            ip.ty.node = substitute_type_ast(&ip.ty.node, map);
        }
        if let Some(body) = &p.get_body {
            p.get_body = Some(substitute_block(body, map));
        }
        if let Some(body) = &p.set_body {
            p.set_body = Some(substitute_block(body, map));
        }
        if let Some(init) = &p.init {
            p.init = Some(Spanned::new(substitute_expr(&init.node, map), init.span));
        }
    }
    for m in &mut c.methods {
        for p in &mut m.node.sig.params {
            p.ty.node = substitute_type_ast(&p.ty.node, map);
        }
        if let Some(ret) = &mut m.node.sig.ret {
            ret.node = substitute_type_ast(&ret.node, map);
        }
        // 闄愬埗 2 淇锛氭浛鎹㈡柟娉曚綋琛ㄨ揪寮忎腑鐨勭被鍨嬪弬鏁帮紙濡?`new T()` 鈫?`new Person()`锛?
        if let Some(body) = &m.node.body {
            m.node.body = Some(substitute_block(body, map));
        }
    }
    for ctor in &mut c.constructors {
        for p in &mut ctor.node.params {
            p.ty.node = substitute_type_ast(&p.ty.node, map);
        }
        // 闄愬埗 2 淇锛氭浛鎹㈡瀯閫犲嚱鏁颁綋琛ㄨ揪寮忎腑鐨勭被鍨嬪弬鏁?
        ctor.node.body = substitute_block(&ctor.node.body, map);
    }
    // Substitute type params in base list (e.g., `IComparable<T>` 鈫?`IComparable<int>`)
    for b in &mut c.bases {
        *b = substitute_type_ast(b, map);
    }
    c
}

pub fn substitute_fn_def(f: &FnDef, mangled: &str, map: &IndexMap<Ident, TypeId>) -> FnDef {
    let mut fn_def = f.clone();
    fn_def.name = mangled.into();
    fn_def.generics = vec![];
    fn_def.where_clause = vec![];
    for p in &mut fn_def.params {
        p.ty.node = substitute_type_ast(&p.ty.node, map);
    }
    if let Some(ret) = &mut fn_def.ret {
        ret.node = substitute_type_ast(&ret.node, map);
    }
    // 闄愬埗 2 淇锛氭浛鎹㈠嚱鏁颁綋琛ㄨ揪寮忎腑鐨勭被鍨嬪弬鏁?
    if let Some(body) = &fn_def.body {
        fn_def.body = Some(substitute_block(body, map));
    }
    fn_def
}

pub fn resolve_instantiated_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Named { path, generics } if !generics.is_empty() => {
            let def = path.last()?.as_str();
            // Builtin `Vector<T, N>` uses const generics: N is an integer literal
            // (`Type::ConstInt`). `TypeId` has no const-int variant, so `mangle_generic`
            // cannot encode N. Produce the mangled name `Vector_{elem}_{n}` directly.
            if def == "Vector" && generics.len() == 2 {
                let elem_suffix = match &generics[0].node {
                    Type::Named { path, generics: gs } if gs.is_empty() => {
                        match path.last()?.as_str() {
                            "float" => "float",
                            "double" => "double",
                            _ => return None,
                        }
                    }
                    _ => return None,
                };
                let n = match &generics[1].node {
                    Type::ConstInt(n) => *n,
                    _ => return None,
                };
                return Some(format!("Vector_{elem_suffix}_{n}"));
            }
            // RFC 037 / RFC 009: Action<T1, ..., Tn> 鏄?Func<T1, ..., Tn, void> 鐨勮娉曠硸銆?
            // 涓?typeck `lower_type` 淇濇寔涓€鑷达細mangle 鏃剁粺涓€璧?Func 璺緞锛?
            // 浣?`Action<T, T>` 鈫?`Func_T_T_void`锛堣€岄潪 `Action_T_T`锛夛紝
            // 淇濊瘉 List<Action<T, T>> 涓?List<Func<T, T, void>> 鍏变韩鍚屼竴鍗曟€佸寲瀹炰緥銆?
            if def == "Action" {
                let params: Vec<TypeId> = generics
                    .iter()
                    .map(|g| lower_generic_arg_to_type_id(&g.node))
                    .collect();
                let func_ty = TypeId::Func {
                    params,
                    ret: Box::new(TypeId::Void),
                };
                return Some(mangle_type_suffix(&func_ty));
            }
            let args: Vec<TypeId> = generics
                .iter()
                .map(|g| lower_generic_arg_to_type_id(&g.node))
                .collect();
            Some(mangle_generic(def, &args))
        }
        _ => None,
    }
}

/// RFC 037 M1: 灏嗗崟涓硾鍨嬪疄鍙?AST 鑺傜偣闄嶇骇涓?TypeId锛屾敮鎸佸祵濂楁硾鍨嬪疄渚嬪寲銆?
///
/// 渚嬶細`Func<T, T, bool>` 涓瘡涓疄鍙傦紙`T`銆乣T`銆乣bool`锛夐€氳繃姝ゅ嚱鏁拌В鏋愩€?
/// 宓屽娉涘瀷锛堝 `List<Func<T, T, bool>>` 鐨勫唴灞?`Func<T, T, bool>`锛夐€氳繃
/// 閫掑綊璋冪敤 `resolve_instantiated_type_name` mangle 涓?`Func_T_T_bool`锛?
/// 鍐嶅寘鎴?`Named("Func_T_T_bool")` 浣滃灞?`List` 鐨勫疄鍙傦紝鏈€缁?mangle 涓?`List_Func_T_T_bool`銆?
fn lower_generic_arg_to_type_id(ty: &Type) -> TypeId {
    match ty {
        Type::Named { path, generics } => {
            // 绌烘硾鍨嬪疄鍙傦細鍩哄厓绫诲瀷鎴栫被鍨嬪弬鏁版爣璇嗙锛圱/U/V 绛夛級
            if generics.is_empty() {
                let n = path.last().cloned().unwrap_or_else(|| "unknown".into());
                return match n.as_str() {
                    "int" => TypeId::Int,
                    "bool" => TypeId::Bool,
                    "string" => TypeId::String,
                    // RFC 006 M1: 娉涘瀷瀹炲弬鏀寔 object 鏍圭被鍨?
                    "object" => TypeId::Object,
                    "void" => TypeId::Void,
                    "long" => TypeId::Long,
                    "short" => TypeId::Short,
                    "byte" => TypeId::Byte,
                    "char" => TypeId::Char,
                    "float" => TypeId::Float,
                    "double" => TypeId::Double,
                    "uint" => TypeId::UInt,
                    "ulong" => TypeId::ULong,
                    "ushort" => TypeId::UShort,
                    "sbyte" => TypeId::SByte,
                    // 裸 `Action`（无类型实参）≡ `Func<void>`。与 `lower_type`
                    // （check_type.rs 裸 `Action` → TypeId::Func{[],Void}）及
                    // `type_path_name`（registry.rs 裸 `Action` → "Func_void"）
                    // 保持一致；否则 `List<Action>` 在「声明类型记录」
                    // （`type_path_name` → `resolve_instantiated_type_name`）
                    // 与「`new List<Action>()` 表达式」（`lower_type` →
                    // `instantiate_generic_class`）两条路径分别 mangle 为
                    // `List_Action` 与 `List_Func_void`，导致字段初始化
                    // 类型失配、`_detachActions.Count` 等内建 List 成员
                    // 在 `List_Action` 上查找失败。
                    "Action" => TypeId::Func {
                        params: Vec::new(),
                        ret: Box::new(TypeId::Void),
                    },
                    _ => TypeId::Named(n),
                };
            }
            // 宓屽娉涘瀷锛氶€掑綊 mangle 涓哄畬鏁村悕锛堝 Func_T_T_bool锛夛紝鍐嶅寘鎴?Named
            resolve_instantiated_type_name(ty)
                .map(|mangled| TypeId::Named(mangled.into()))
                .unwrap_or(TypeId::Infer)
        }
        // 澶嶅悎绫诲瀷瀹炲弬锛氳浆 TypeId 鍚庣敱 mangle_type_suffix 澶勭悊
        Type::Array { inner } => TypeId::Array {
            elem: Box::new(lower_generic_arg_to_type_id(&inner.node)),
        },
        Type::Nullable { inner } => TypeId::Nullable {
            inner: Box::new(lower_generic_arg_to_type_id(&inner.node)),
        },
        _ => TypeId::Infer,
    }
}

// === 闄愬埗 2 淇锛氭柟娉曚綋绫诲瀷鍙傛暟鏇挎崲 ===
// 鏀寔 `class Box<T> { T Make() { return new T(); } }` 瀹炰緥鍖栨椂
// 鏂规硶浣撹〃杈惧紡涓殑绫诲瀷鍙傛暟琚纭浛鎹负鍏蜂綋绫诲瀷鍙傛暟銆?

fn sub_span_expr(e: &Spanned<Expr>, map: &IndexMap<Ident, TypeId>) -> Spanned<Expr> {
    Spanned::new(substitute_expr(&e.node, map), e.span)
}

/// 閫掑綊鏇挎崲琛ㄨ揪寮忎腑鐨勭被鍨嬪弬鏁般€?
pub(crate) fn substitute_expr(expr: &Expr, map: &IndexMap<Ident, TypeId>) -> Expr {
    use Expr as E;
    match expr {
        E::New { ty, args, obj_init } => E::New {
            ty: Spanned::new(substitute_type_ast(&ty.node, map), ty.span),
            args: args.iter().map(|a| sub_span_expr(a, map)).collect(),
            obj_init: obj_init.as_ref().map(|inits| {
                inits
                    .iter()
                    .map(|(name, e)| (name.clone(), sub_span_expr(e, map)))
                    .collect()
            }),
        },
        E::Call {
            func,
            args,
            type_args,
            params_span,
        } => E::Call {
            func: Box::new(sub_span_expr(func, map)),
            args: args.iter().map(|a| sub_span_expr(a, map)).collect(),
            type_args: type_args
                .iter()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span))
                .collect(),
            params_span: params_span.clone(),
        },
        E::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span,
        } => {
            // RFC 004 M1锛歚T.Method(...)` 褰㈠紡鐨?static abstract 璋冪敤鈥斺€?
            // 鑻?receiver 鏄硾鍨嬪弬鏁版爣璇嗙锛屾浛鎹负鍏蜂綋绫诲瀷鍚嶏紙濡?`int`锛夛紝
            // 璁╁崟鎬佸寲鍚庣殑 typeck/codegen 鑳借瘑鍒负鍩哄厓绫诲瀷 static abstract 璋冪敤銆?
            let new_receiver = match &receiver.node {
                E::Ident(name) if map.contains_key(name) => {
                    let ty = &map[name];
                    let concrete_name = type_id_to_field_name(ty);
                    Spanned::new(E::Ident(concrete_name), receiver.span)
                }
                _ => sub_span_expr(receiver, map),
            };
            E::MethodCall {
                receiver: Box::new(new_receiver),
                method: method.clone(),
                args: args.iter().map(|a| sub_span_expr(a, map)).collect(),
                type_args: type_args
                    .iter()
                    .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span))
                    .collect(),
                params_span: params_span.clone(),
            }
        }
        E::Field { receiver, field } => {
            // RFC 004 M1锛歚T.Prop` 褰㈠紡鐨?static abstract 灞炴€ц闂€斺€斿悓涓婃浛鎹€?
            let new_receiver = match &receiver.node {
                E::Ident(name) if map.contains_key(name) => {
                    let ty = &map[name];
                    let concrete_name = type_id_to_field_name(ty);
                    Spanned::new(E::Ident(concrete_name), receiver.span)
                }
                _ => sub_span_expr(receiver, map),
            };
            E::Field {
                receiver: Box::new(new_receiver),
                field: field.clone(),
            }
        }
        E::Index { receiver, index } => E::Index {
            receiver: Box::new(sub_span_expr(receiver, map)),
            index: Box::new(sub_span_expr(index, map)),
        },
        E::Binary { op, left, right } => E::Binary {
            op: *op,
            left: Box::new(sub_span_expr(left, map)),
            right: Box::new(sub_span_expr(right, map)),
        },
        // 赋值表达式：目标与值均递归替换（方法体内泛型参数可作赋值目标）。
        E::Assign { target, value } => E::Assign {
            target: Box::new(sub_span_expr(target, map)),
            value: Box::new(sub_span_expr(value, map)),
        },
        E::Unary { op, expr } => E::Unary {
            op: *op,
            expr: Box::new(sub_span_expr(expr, map)),
        },
        E::Comptime(inner) => E::Comptime(Box::new(sub_span_expr(inner, map))),
        E::Cast { expr, ty } => E::Cast {
            expr: Box::new(sub_span_expr(expr, map)),
            ty: Spanned::new(substitute_type_ast(&ty.node, map), ty.span),
        },
        E::Default { ty } => E::Default {
            ty: Spanned::new(substitute_type_ast(&ty.node, map), ty.span),
        },
        E::RefArg { is_out, expr } => E::RefArg {
            is_out: *is_out,
            expr: Box::new(sub_span_expr(expr, map)),
        },
        E::NamedArg { name, expr } => E::NamedArg {
            name: name.clone(),
            expr: Box::new(sub_span_expr(expr, map)),
        },
        E::StackSpanLit {
            elements,
            mutable,
            elem,
        } => E::StackSpanLit {
            elements: elements.iter().map(|e| sub_span_expr(e, map)).collect(),
            mutable: *mutable,
            elem: substitute_type(elem, map),
        },
        E::Await(e) => E::Await(Box::new(sub_span_expr(e, map))),
        E::Coalesce { left, right } => E::Coalesce {
            left: Box::new(sub_span_expr(left, map)),
            right: Box::new(sub_span_expr(right, map)),
        },
        E::Ternary {
            cond,
            then_branch,
            else_branch,
        } => E::Ternary {
            cond: Box::new(sub_span_expr(cond, map)),
            then_branch: Box::new(sub_span_expr(then_branch, map)),
            else_branch: Box::new(sub_span_expr(else_branch, map)),
        },
        E::NullCond { access } => E::NullCond {
            access: Box::new(sub_span_expr(access, map)),
        },
        E::ForceDeref { access } => E::ForceDeref {
            access: Box::new(sub_span_expr(access, map)),
        },
        E::Block(b) => E::Block(substitute_block(b, map)),
        E::If {
            cond,
            then_branch,
            else_branch,
        } => E::If {
            cond: Box::new(sub_span_expr(cond, map)),
            then_branch: substitute_block(then_branch, map),
            else_branch: else_branch.as_ref().map(|b| substitute_block(b, map)),
        },
        E::CollectionExpr { elements } => E::CollectionExpr {
            elements: elements
                .iter()
                .map(|el| match el {
                    CollectionElement::Element(e) => {
                        CollectionElement::Element(sub_span_expr(e, map))
                    }
                    CollectionElement::Spread(e) => {
                        CollectionElement::Spread(sub_span_expr(e, map))
                    }
                })
                .collect(),
        },
        E::Lambda(l) => E::Lambda(substitute_lambda(l, map)),
        E::ExpressionLit(el) => E::ExpressionLit(ExpressionLit {
            lambda: substitute_lambda(&el.lambda, map),
        }),
        E::Switch(s) => E::Switch(SwitchExpr {
            scrutinee: Box::new(sub_span_expr(&s.scrutinee, map)),
            cases: s
                .cases
                .iter()
                .map(|c| SwitchCase {
                    pattern: c.pattern.as_ref().map(|p| substitute_pattern(p, map)),
                    when: c.when.as_ref().map(|w| sub_span_expr(w, map)),
                    body: substitute_block(&c.body, map),
                })
                .collect(),
        }),
        E::SwitchForm(s) => E::SwitchForm(SwitchExprForm {
            scrutinee: Box::new(sub_span_expr(&s.scrutinee, map)),
            arms: s
                .arms
                .iter()
                .map(|a| SwitchExprArm {
                    pattern: substitute_pattern(&a.pattern, map),
                    when: a.when.as_ref().map(|w| sub_span_expr(w, map)),
                    body: sub_span_expr(&a.body, map),
                })
                .collect(),
        }),
        E::Query(q) => E::Query(QueryExpr {
            clauses: q
                .clauses
                .iter()
                .map(|cl| substitute_query_clause(cl, map))
                .collect(),
            select: Box::new(sub_span_expr(&q.select, map)),
        }),
        E::Box { expr, value_ty } => E::Box {
            expr: Box::new(sub_span_expr(expr, map)),
            value_ty: Spanned::new(substitute_type_ast(&value_ty.node, map), value_ty.span),
        },
        E::Unbox { expr, value_ty } => E::Unbox {
            expr: Box::new(sub_span_expr(expr, map)),
            value_ty: Spanned::new(substitute_type_ast(&value_ty.node, map), value_ty.span),
        },
        // `new T[n]`：元素类型可能含泛型类型参数，长度表达式可能含类型参数。
        E::NewArray { elem_type, length } => E::NewArray {
            elem_type: Spanned::new(substitute_type_ast(&elem_type.node, map), elem_type.span),
            length: Box::new(sub_span_expr(length, map)),
        },
        // 鍙跺瓙鑺傜偣锛氭棤绫诲瀷鍙傛暟闇€鏇挎崲
        E::IntLit(_)
        | E::FloatLit(_)
        | E::BoolLit(_)
        | E::StringLit(_)
        | E::CharLit(_)
        | E::Ident(_)
        | E::Path(_)
        | E::This
        | E::Base
        | E::Null => expr.clone(),
        E::InterpolatedString { parts } => E::InterpolatedString {
            parts: parts
                .iter()
                .map(|p| match p {
                    InterpPart::Lit(s) => InterpPart::Lit(s.clone()),
                    InterpPart::Expr(hole) => InterpPart::Expr(InterpHole {
                        expr: sub_span_expr(&hole.expr, map),
                        alignment: hole.alignment,
                        format: hole.format.clone(),
                    }),
                })
                .collect(),
        },
        // `typeof(T)` 鈥?閫掑綊鏇挎崲绫诲瀷鍙傛暟锛圧FC 035 M1锛夈€?
        E::TypeOf(ty) => E::TypeOf(Spanned::new(substitute_type_ast(&ty.node, map), ty.span)),
        // `expr is pattern` 鈥?閫掑綊鏇挎崲瀛愯〃杈惧紡锛圧FC 036 M1锛夈€?
        E::Is { expr, pattern } => E::Is {
            expr: Box::new(sub_span_expr(expr, map)),
            pattern: pattern.clone(),
        },
        // RFC 006 M2锛歚with` 鈥?閫掑綊鏇挎崲鎺ユ敹鑰呬笌鍒濆鍖栧櫒銆?
        E::With { receiver, inits } => E::With {
            receiver: Box::new(sub_span_expr(receiver, map)),
            inits: inits
                .iter()
                .map(|(n, e)| (n.clone(), sub_span_expr(e, map)))
                .collect(),
        },
    }
}

/// 閫掑綊鏇挎崲璇彞涓殑绫诲瀷鍙傛暟銆?
pub(crate) fn substitute_stmt(stmt: &Stmt, map: &IndexMap<Ident, TypeId>) -> Stmt {
    use Stmt as S;
    match stmt {
        S::Let {
            mutable,
            name,
            ty,
            init,
        } => S::Let {
            mutable: *mutable,
            name: name.clone(),
            ty: ty
                .as_ref()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
            init: init.as_ref().map(|e| sub_span_expr(e, map)),
        },
        S::Expr(e) => S::Expr(sub_span_expr(e, map)),
        S::Return(e) => S::Return(e.as_ref().map(|x| sub_span_expr(x, map))),
        S::While { cond, body } => S::While {
            cond: sub_span_expr(cond, map),
            body: substitute_block(body, map),
        },
        S::For { var, iter, body } => S::For {
            var: var.clone(),
            iter: sub_span_expr(iter, map),
            body: substitute_block(body, map),
        },
        S::Assign { target, value } => S::Assign {
            target: sub_span_expr(target, map),
            value: sub_span_expr(value, map),
        },
        S::Throw { expr } => S::Throw {
            expr: sub_span_expr(expr, map),
        },
        S::TryCatch {
            try_body,
            catch_ty,
            catch_name,
            when_cond,
            catch_body,
            finally,
        } => S::TryCatch {
            try_body: substitute_block(try_body, map),
            catch_ty: Spanned::new(substitute_type_ast(&catch_ty.node, map), catch_ty.span),
            catch_name: catch_name.clone(),
            when_cond: when_cond.as_ref().map(|w| sub_span_expr(w, map)),
            catch_body: substitute_block(catch_body, map),
            finally: finally.as_ref().map(|f| substitute_block(f, map)),
        },
        S::TryFinally { body, finally } => S::TryFinally {
            body: substitute_block(body, map),
            finally: substitute_block(finally, map),
        },
        S::Using {
            name,
            ty,
            init,
            body,
        } => S::Using {
            name: name.clone(),
            ty: ty
                .as_ref()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
            init: sub_span_expr(init, map),
            body: substitute_block(body, map),
        },
        S::UsingVar { name, ty, init } => S::UsingVar {
            name: name.clone(),
            ty: ty
                .as_ref()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
            init: sub_span_expr(init, map),
        },
        S::AwaitUsing {
            name,
            ty,
            init,
            body,
        } => S::AwaitUsing {
            name: name.clone(),
            ty: ty
                .as_ref()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
            init: sub_span_expr(init, map),
            body: substitute_block(body, map),
        },
        S::AwaitUsingVar { name, ty, init } => S::AwaitUsingVar {
            name: name.clone(),
            ty: ty
                .as_ref()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
            init: sub_span_expr(init, map),
        },
        S::YieldReturn { value } => S::YieldReturn {
            value: sub_span_expr(value, map),
        },
        S::YieldBreak => S::YieldBreak,
        S::Lock { expr, body } => S::Lock {
            expr: sub_span_expr(expr, map),
            body: substitute_block(body, map),
        },
        S::ForC {
            init,
            cond,
            inc,
            body,
        } => S::ForC {
            init: init.clone(),
            cond: cond.as_ref().map(|e| sub_span_expr(e, map)),
            inc: inc.clone(),
            body: substitute_block(body, map),
        },
        S::Break => S::Break,
        S::Continue => S::Continue,
        S::DeconstructAssign {
            declare,
            targets,
            value,
        } => S::DeconstructAssign {
            declare: *declare,
            targets: targets.clone(),
            value: sub_span_expr(value, map),
        },
    }
}

/// 閫掑綊鏇挎崲鍧椾腑鐨勭被鍨嬪弬鏁帮紙璇彞 + 灏捐〃杈惧紡锛夈€?
pub(crate) fn substitute_block(block: &Block, map: &IndexMap<Ident, TypeId>) -> Block {
    Block {
        stmts: block
            .stmts
            .iter()
            .map(|s| Spanned::new(substitute_stmt(&s.node, map), s.span))
            .collect(),
        tail: block.tail.as_ref().map(|e| Box::new(sub_span_expr(e, map))),
    }
}

fn substitute_lambda(l: &LambdaExpr, map: &IndexMap<Ident, TypeId>) -> LambdaExpr {
    LambdaExpr {
        params: l
            .params
            .iter()
            .map(|p| LambdaParam {
                name: p.name.clone(),
                ty: p
                    .ty
                    .as_ref()
                    .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span)),
                default: p
                    .default
                    .as_ref()
                    .map(|e| Spanned::new(substitute_expr(&e.node, map), e.span)),
            })
            .collect(),
        body: match &l.body {
            LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(sub_span_expr(e, map))),
            LambdaBody::Block(b) => LambdaBody::Block(substitute_block(b, map)),
        },
        is_expression_tree: l.is_expression_tree,
        is_async: l.is_async,
        captures: l
            .captures
            .iter()
            .map(|c| LambdaCapture {
                name: c.name.clone(),
                ty: substitute_type(&c.ty, map),
                mode: c.mode.clone(),
            })
            .collect(),
    }
}

fn substitute_pattern(p: &Pattern, map: &IndexMap<Ident, TypeId>) -> Pattern {
    match p {
        Pattern::Wildcard => Pattern::Wildcard,
        Pattern::Ident(n) => Pattern::Ident(n.clone()),
        Pattern::Literal(e) => Pattern::Literal(sub_span_expr(e, map)),
        Pattern::Type { ty, binding } => Pattern::Type {
            ty: Spanned::new(substitute_type_ast(&ty.node, map), ty.span),
            binding: binding.clone(),
        },
        Pattern::Null => Pattern::Null,
        Pattern::Var(n) => Pattern::Var(n.clone()),
        // Pattern::Variant锛氫繚鐣?path/case/binding锛泃ype_args 涓殑绫诲瀷鍚嶆寜 map 鏇挎崲
        Pattern::Variant {
            path,
            type_args,
            case,
            binding,
        } => Pattern::Variant {
            path: path.clone(),
            type_args: type_args
                .iter()
                .map(|t| Spanned::new(substitute_type_ast(&t.node, map), t.span))
                .collect(),
            case: case.clone(),
            binding: binding.clone(),
        },
        Pattern::Positional(elems) => Pattern::Positional(elems.clone()),
    }
}

fn substitute_query_clause(cl: &QueryClause, map: &IndexMap<Ident, TypeId>) -> QueryClause {
    match cl {
        QueryClause::From { ident, source } => QueryClause::From {
            ident: ident.clone(),
            source: sub_span_expr(source, map),
        },
        QueryClause::Let { ident, value } => QueryClause::Let {
            ident: ident.clone(),
            value: sub_span_expr(value, map),
        },
        QueryClause::Where(e) => QueryClause::Where(sub_span_expr(e, map)),
        QueryClause::OrderBy { key, descending } => QueryClause::OrderBy {
            key: sub_span_expr(key, map),
            descending: *descending,
        },
        QueryClause::Join {
            ident,
            source,
            on_left,
            on_right,
        } => QueryClause::Join {
            ident: ident.clone(),
            source: sub_span_expr(source, map),
            on_left: sub_span_expr(on_left, map),
            on_right: sub_span_expr(on_right, map),
        },
        QueryClause::GroupBy {
            key,
            element,
            into_ident,
        } => QueryClause::GroupBy {
            key: sub_span_expr(key, map),
            element: element.as_ref().map(|e| sub_span_expr(e, map)),
            into_ident: into_ident.clone(),
        },
    }
}
