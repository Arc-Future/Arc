//! `.xml` 文档注释产物生成器（RFC 017）。
//!
//! 从 AST `doc` 字段生成 C# `.xml` 兼容的 XML 文档产物（对标 `dotnet build`
//! 开启 `<GenerateDocumentationFile>` 时生成的 `<Assembly>.xml`）。产物路径
//! 默认 `bin/<config>/<package>.xml`（与二进制同目录），多语言本地化遵循
//! `<package>.<locale>.xml` 命名（如 `Arc.zh-CN.xml`）。
//!
//! typeck/codegen 不解析 XML 内容——doc comment 作为不透明字符串存储，本模块
//! 原样嵌入 XML（仅转义特殊字符）。arc-server 在 Quickinfo 时按需解析 XML 标签。

use ast::{FieldDef, FnDef, Item, MethodDef, MethodSig, Param, Program, PropertyDef, Type};

/// 生成 `.xml` 文档内容（C# DocComment 规范）。
///
/// `package_name` 是包名（如 "arc-std"），用于 `<assembly><name>`。
/// 遍历 program.items，递归 namespace，对每个有 doc 的符号生成 `<member>` 条目。
pub fn generate_doc_xml(program: &Program, package_name: &str) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str("<doc>\n");
    out.push_str(&format!(
        "  <assembly><name>{}</name></assembly>\n",
        escape_xml(package_name)
    ));
    out.push_str("  <members>\n");

    for item in &program.items {
        emit_item_members(&mut out, &item.node, &[]);
    }

    out.push_str("  </members>\n");
    out.push_str("</doc>\n");
    out
}

fn emit_item_members(out: &mut String, item: &Item, ns: &[String]) {
    match item {
        Item::Namespace(n) => {
            let mut path = ns.to_vec();
            path.extend(n.path.iter().map(|i| i.as_str().to_string()));
            for child in &n.items {
                emit_item_members(out, &child.node, &path);
            }
        }
        Item::Class(c) => {
            if let Some(doc) = &c.doc {
                let qual = qualified_name(ns, &c.name);
                out.push_str(&format!("    <member name=\"T:{}\">\n", qual));
                out.push_str(&format!("      {}\n", escape_xml(doc)));
                out.push_str("    </member>\n");
            }
            let type_qual = qualified_name(ns, &c.name);
            for f in &c.fields {
                emit_field_member(out, &type_qual, f);
            }
            for p in &c.properties {
                emit_property_member(out, &type_qual, p);
            }
            for m in &c.methods {
                emit_method_member(out, &type_qual, &m.node);
            }
        }
        Item::Struct(s) => {
            if let Some(doc) = &s.doc {
                let qual = qualified_name(ns, &s.name);
                out.push_str(&format!("    <member name=\"T:{}\">\n", qual));
                out.push_str(&format!("      {}\n", escape_xml(doc)));
                out.push_str("    </member>\n");
            }
            let type_qual = qualified_name(ns, &s.name);
            for f in &s.fields {
                emit_field_member(out, &type_qual, f);
            }
        }
        Item::Interface(i) => {
            if let Some(doc) = &i.doc {
                let qual = qualified_name(ns, &i.name);
                out.push_str(&format!("    <member name=\"T:{}\">\n", qual));
                out.push_str(&format!("      {}\n", escape_xml(doc)));
                out.push_str("    </member>\n");
            }
            let type_qual = qualified_name(ns, &i.name);
            for p in &i.properties {
                emit_property_member(out, &type_qual, p);
            }
            for m in &i.methods {
                emit_method_sig_member(out, &type_qual, m);
            }
        }
        Item::Enum(e) => {
            if let Some(doc) = &e.doc {
                let qual = qualified_name(ns, &e.name);
                out.push_str(&format!("    <member name=\"T:{}\">\n", qual));
                out.push_str(&format!("      {}\n", escape_xml(doc)));
                out.push_str("    </member>\n");
            }
            // EnumVariant 暂不生成条目（C# 枚举变体用 F:，P2 可选，先跳过简化）。
        }
        Item::Fn(f) => {
            emit_fn_member(out, ns, f);
        }
        Item::Use(_) | Item::Native(_) | Item::Variant(_) | Item::Delegate(_) => {}
    }
}

/// Emit a top-level function `<member name="M:...">` entry if it has a doc.
fn emit_fn_member(out: &mut String, ns: &[String], f: &FnDef) {
    if let Some(doc) = &f.doc {
        let qual = qualified_name(ns, &f.name);
        let params = format_params(&f.params);
        out.push_str(&format!("    <member name=\"M:{}{}\">\n", qual, params));
        out.push_str(&format!("      {}\n", escape_xml(doc)));
        out.push_str("    </member>\n");
    }
}

/// Emit a field `<member name="F:...">` entry if it has a doc.
fn emit_field_member(out: &mut String, type_qual: &str, f: &FieldDef) {
    if let Some(doc) = &f.doc {
        out.push_str(&format!(
            "    <member name=\"F:{}.{}\">\n",
            type_qual,
            escape_xml(&f.name)
        ));
        out.push_str(&format!("      {}\n", escape_xml(doc)));
        out.push_str("    </member>\n");
    }
}

/// Emit a property `<member name="P:...">` entry if it has a doc.
fn emit_property_member(out: &mut String, type_qual: &str, p: &PropertyDef) {
    if let Some(doc) = &p.doc {
        out.push_str(&format!(
            "    <member name=\"P:{}.{}\">\n",
            type_qual,
            escape_xml(&p.name)
        ));
        out.push_str(&format!("      {}\n", escape_xml(doc)));
        out.push_str("    </member>\n");
    }
}

/// Emit a method `<member name="M:...">` entry from a MethodDef if it has a doc.
fn emit_method_member(out: &mut String, type_qual: &str, m: &MethodDef) {
    let doc = m.doc.as_ref().or(m.sig.doc.as_ref());
    if let Some(doc) = doc {
        let params = format_params(&m.sig.params);
        out.push_str(&format!(
            "    <member name=\"M:{}.{}{}\">\n",
            type_qual,
            escape_xml(&m.sig.name),
            params
        ));
        out.push_str(&format!("      {}\n", escape_xml(doc)));
        out.push_str("    </member>\n");
    }
}

/// Emit a method `<member name="M:...">` entry from a MethodSig (interface method).
fn emit_method_sig_member(out: &mut String, type_qual: &str, m: &MethodSig) {
    if let Some(doc) = &m.doc {
        let params = format_params(&m.params);
        out.push_str(&format!(
            "    <member name=\"M:{}.{}{}\">\n",
            type_qual,
            escape_xml(&m.name),
            params
        ));
        out.push_str(&format!("      {}\n", escape_xml(doc)));
        out.push_str("    </member>\n");
    }
}

/// Build a fully-qualified name from namespace segments and a type/member name.
fn qualified_name(ns: &[String], name: &str) -> String {
    if ns.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", ns.join("."), name)
    }
}

/// Format a parameter list as C# DocComment parameter type spec:
/// `(System.Int32,System.String)` or `()` for no params.
fn format_params(params: &[Param]) -> String {
    let parts: Vec<String> = params
        .iter()
        .map(|p| format_type_name(&p.ty.node))
        .collect();
    format!("({})", parts.join(","))
}

/// Map an Arc Type to a C# DocComment type name string.
///
/// Primitives map to System.* names; other Named types use their dotted path.
fn format_type_name(t: &Type) -> String {
    match t {
        Type::Named { path, .. } => {
            if let Some(first) = path.first() {
                let mapped = match first.as_str() {
                    "int" => Some("System.Int32"),
                    "string" => Some("System.String"),
                    "double" => Some("System.Double"),
                    "float" => Some("System.Single"),
                    "bool" => Some("System.Boolean"),
                    "void" => Some("System.Void"),
                    "long" => Some("System.Int64"),
                    _ => None,
                };
                if let Some(m) = mapped {
                    return m.to_string();
                }
            }
            path.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(".")
        }
        Type::Ref { inner, .. } => format_type_name(&inner.node),
        Type::Array { inner } => format!("{}[]", format_type_name(&inner.node)),
        Type::Nullable { inner } => format!("{}?", format_type_name(&inner.node)),
        Type::Func { .. } => "System.Func".to_string(),
        Type::ConstInt(n) => n.to_string(),
        Type::Infer => "System.Object".to_string(),
    }
}

/// Escape XML special characters: `&` first, then `<` and `>`.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{
        ClassDef, EnumDef, FnDef, Ident, InterfaceDef, MethodDef, MethodModifier, MethodSig, Param,
        PropertyDef, Spanned, StructDef, Type, Visibility,
    };

    fn named(name: &str) -> Spanned<Type> {
        Spanned::new(
            Type::Named {
                path: vec![Ident::from(name)],
                generics: vec![],
            },
            ast::Span::DUMMY,
        )
    }

    fn param(name: &str, ty: &str) -> Param {
        Param {
            name: Ident::from(name),
            ty: named(ty),
            attributes: vec![],
            is_extension_receiver: false,
            is_ref: false,
            is_out: false,
            is_in: false,
            is_params: false,
            default: None,
        }
    }

    fn spanned(item: Item) -> Spanned<Item> {
        Spanned::new(item, ast::Span::DUMMY)
    }

    #[test]
    fn generates_type_entry_for_documented_class() {
        let class = ClassDef {
            vis: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_partial: false,
            is_record: false,
            name: Ident::from("Foo"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            fields: vec![],
            properties: vec![],
            methods: vec![],
            constructors: vec![],
            attributes: vec![],
            doc: Some("A documented class.".into()),
            synthesized_host: None,
        };
        let program = Program {
            items: vec![spanned(Item::Class(class))],
        };
        let xml = generate_doc_xml(&program, "test-pkg");
        assert!(xml.contains("<assembly><name>test-pkg</name></assembly>"));
        assert!(xml.contains("<member name=\"T:Foo\">"));
        assert!(xml.contains("A documented class."));
    }

    #[test]
    fn generates_method_entry_with_param_types() {
        let method = MethodDef {
            sig: MethodSig {
                vis: Visibility::Public,
                name: Ident::from("Add"),
                generics: vec![],
                where_clause: vec![],
                params: vec![param("a", "int"), param("b", "int")],
                ret: Some(named("int")),
                is_async: false,
                modifier: MethodModifier::None,
                is_static_abstract: false,
                attributes: vec![],
                doc: None,
            },
            body: None,
            doc: Some("Adds two ints.".into()),
        };
        let class = ClassDef {
            vis: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_partial: false,
            is_record: false,
            name: Ident::from("Calc"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            fields: vec![],
            properties: vec![],
            methods: vec![Spanned::new(method, ast::Span::DUMMY)],
            constructors: vec![],
            attributes: vec![],
            doc: None,
            synthesized_host: None,
        };
        let program = Program {
            items: vec![spanned(Item::Class(class))],
        };
        let xml = generate_doc_xml(&program, "test-pkg");
        assert!(
            xml.contains("<member name=\"M:Calc.Add(System.Int32,System.Int32)\">"),
            "expected method member with System.Int32 params, got: {xml}"
        );
        assert!(xml.contains("Adds two ints."));
    }

    #[test]
    fn generates_top_level_fn_entry() {
        let f = FnDef {
            vis: Visibility::Public,
            name: Ident::from("Main"),
            generics: vec![],
            where_clause: vec![],
            params: vec![],
            ret: None,
            body: None,
            is_async: false,
            attributes: vec![],
            doc: Some("Entry point.".into()),
        };
        let program = Program {
            items: vec![spanned(Item::Fn(f))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains("<member name=\"M:Main()\">"));
        assert!(xml.contains("Entry point."));
    }

    #[test]
    fn generates_field_and_property_entries() {
        let field = FieldDef {
            vis: Visibility::Public,
            name: Ident::from("age"),
            ty: named("int"),
            is_readonly: false,
            is_const: false,
            is_static: false,
            init: None,
            attributes: vec![],
            doc: Some("User age.".into()),
        };
        let prop = PropertyDef {
            vis: Visibility::Public,
            name: Ident::from("Name"),
            ty: named("string"),
            has_get: true,
            has_set: true,
            has_init: false,
            is_required: false,
            get_body: None,
            set_body: None,
            get_vis: None,
            set_vis: None,
            modifier: MethodModifier::None,
            is_static_abstract: false,
            attributes: vec![],
            index_params: vec![],
            init: None,
            doc: Some("User name.".into()),
        };
        let class = ClassDef {
            vis: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_partial: false,
            is_record: false,
            name: Ident::from("User"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            fields: vec![field],
            properties: vec![prop],
            methods: vec![],
            constructors: vec![],
            attributes: vec![],
            doc: None,
            synthesized_host: None,
        };
        let program = Program {
            items: vec![spanned(Item::Class(class))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains("<member name=\"F:User.age\">"));
        assert!(xml.contains("User age."));
        assert!(xml.contains("<member name=\"P:User.Name\">"));
        assert!(xml.contains("User name."));
    }

    #[test]
    fn skips_symbols_without_doc() {
        let class = ClassDef {
            vis: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_partial: false,
            is_record: false,
            name: Ident::from("NoDoc"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            fields: vec![FieldDef {
                vis: Visibility::Public,
                name: Ident::from("x"),
                ty: named("int"),
                is_readonly: false,
                is_const: false,
                is_static: false,
                init: None,
                attributes: vec![],
                doc: None,
            }],
            properties: vec![],
            methods: vec![],
            constructors: vec![],
            attributes: vec![],
            doc: None,
            synthesized_host: None,
        };
        let program = Program {
            items: vec![spanned(Item::Class(class))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(!xml.contains("T:NoDoc"));
        assert!(!xml.contains("F:NoDoc.x"));
        // members section still present but empty
        assert!(xml.contains("<members>"));
    }

    #[test]
    fn escapes_xml_special_chars() {
        let f = FnDef {
            vis: Visibility::Public,
            name: Ident::from("Compare"),
            generics: vec![],
            where_clause: vec![],
            params: vec![],
            ret: None,
            body: None,
            is_async: false,
            attributes: vec![],
            doc: Some("Returns true if a < b && c > d.".into()),
        };
        let program = Program {
            items: vec![spanned(Item::Fn(f))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains("a &lt; b &amp;&amp; c &gt; d"));
        assert!(!xml.contains("a < b"));
    }

    #[test]
    fn recurses_into_namespace() {
        let class = ClassDef {
            vis: Visibility::Public,
            is_static: false,
            is_abstract: false,
            is_partial: false,
            is_record: false,
            name: Ident::from("Math"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            fields: vec![],
            properties: vec![],
            methods: vec![],
            constructors: vec![],
            attributes: vec![],
            doc: Some("Math lib.".into()),
            synthesized_host: None,
        };
        let ns = ast::NamespaceItem {
            path: vec![Ident::from("Arc")],
            items: vec![spanned(Item::Class(class))],
            capabilities: vec![],
        };
        let program = Program {
            items: vec![spanned(Item::Namespace(ns))],
        };
        let xml = generate_doc_xml(&program, "arc-std");
        assert!(xml.contains("<member name=\"T:Arc.Math\">"));
        assert!(xml.contains("Math lib."));
    }

    #[test]
    fn generates_struct_and_interface_and_enum_entries() {
        let s = StructDef {
            vis: Visibility::Public,
            is_readonly: false,
            is_record: false,
            name: Ident::from("Point"),
            generics: vec![],
            where_clause: vec![],
            fields: vec![],
            bases: vec![],
            properties: vec![],
            methods: vec![],
            constructors: vec![],
            attributes: vec![],
            doc: Some("A point.".into()),
        };
        let i = InterfaceDef {
            vis: Visibility::Public,
            name: Ident::from("IDrawable"),
            generics: vec![],
            where_clause: vec![],
            bases: vec![],
            methods: vec![],
            properties: vec![],
            attributes: vec![],
            doc: Some("Drawable contract.".into()),
        };
        let e = EnumDef {
            vis: Visibility::Public,
            name: Ident::from("Color"),
            variants: vec![],
            attributes: vec![],
            doc: Some("A color.".into()),
        };
        let program = Program {
            items: vec![
                spanned(Item::Struct(s)),
                spanned(Item::Interface(i)),
                spanned(Item::Enum(e)),
            ],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains("<member name=\"T:Point\">"));
        assert!(xml.contains("<member name=\"T:IDrawable\">"));
        assert!(xml.contains("<member name=\"T:Color\">"));
    }

    #[test]
    fn maps_primitive_param_types() {
        let f = FnDef {
            vis: Visibility::Public,
            name: Ident::from("Mix"),
            generics: vec![],
            where_clause: vec![],
            params: vec![
                param("a", "int"),
                param("b", "string"),
                param("c", "double"),
                param("d", "bool"),
                param("e", "long"),
                param("f", "float"),
            ],
            ret: None,
            body: None,
            is_async: false,
            attributes: vec![],
            doc: Some("mix.".into()),
        };
        let program = Program {
            items: vec![spanned(Item::Fn(f))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains(
            "M:Mix(System.Int32,System.String,System.Double,System.Boolean,System.Int64,System.Single)"
        ));
    }

    #[test]
    fn uses_type_name_for_non_primitive_named() {
        let f = FnDef {
            vis: Visibility::Public,
            name: Ident::from("Process"),
            generics: vec![],
            where_clause: vec![],
            params: vec![Param {
                name: Ident::from("u"),
                ty: Spanned::new(
                    Type::Named {
                        path: vec![Ident::from("Arc"), Ident::from("User")],
                        generics: vec![],
                    },
                    ast::Span::DUMMY,
                ),
                attributes: vec![],
                is_extension_receiver: false,
                is_ref: false,
                is_out: false,
                is_in: false,
                is_params: false,
                default: None,
            }],
            ret: None,
            body: None,
            is_async: false,
            attributes: vec![],
            doc: Some("proc.".into()),
        };
        let program = Program {
            items: vec![spanned(Item::Fn(f))],
        };
        let xml = generate_doc_xml(&program, "app");
        assert!(xml.contains("M:Process(Arc.User)"));
    }

    #[test]
    fn xml_header_and_structure() {
        let program = Program { items: vec![] };
        let xml = generate_doc_xml(&program, "empty");
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"));
        assert!(xml.contains("<doc>\n"));
        assert!(xml.contains("<assembly><name>empty</name></assembly>"));
        assert!(xml.contains("<members>\n"));
        assert!(xml.contains("</members>\n"));
        assert!(xml.ends_with("</doc>\n"));
    }
}
