//! `arc ui inspect` —— 输出 JSON 结构树 + ASCII 布局预览（RFC 037 M1 D11）。
//!
//! 为 AI 协作工具提供结构化输出，便于自动化分析 UI 结构。

use crate::ast::*;

/// 生成 `.arml` 文档的 JSON 结构树。
pub fn inspect_json(doc: &ArmlDocument) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"kind\": \"arml-document\",\n");
    if let Some(decl) = &doc.xml_decl {
        out.push_str(&format!(
            "  \"xmlDecl\": {{\"version\": \"{}\"}},\n",
            decl.version
        ));
    }
    out.push_str("  \"root\": ");
    inspect_element_json(&doc.root, &mut out, 1);
    out.push_str("\n}\n");
    out
}

fn inspect_element_json(element: &Element, out: &mut String, depth: usize) {
    let indent = "  ".repeat(depth);
    out.push_str("{\n");
    out.push_str(&format!("{}\"name\": \"{}\",\n", indent, element.name));
    if let Some(p) = &element.prefix {
        out.push_str(&format!("{}\"prefix\": \"{}\",\n", indent, p));
    }
    if !element.attributes.is_empty() {
        out.push_str(&format!("{}\"attributes\": [\n", indent));
        let attr_indent = format!("{indent}  ");
        for (i, attr) in element.attributes.iter().enumerate() {
            out.push_str(&format!("{attr_indent}{{"));
            out.push_str(&format!("\"name\": \"{}\"", attr.qualified_name()));
            out.push_str(&format!(", \"value\": {}", attr_value_json(&attr.value)));
            out.push('}');
            if i + 1 < element.attributes.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&format!("{}],\n", indent));
    }
    if !element.children.is_empty() {
        out.push_str(&format!("{}\"children\": [\n", indent));
        for (i, child) in element.children.iter().enumerate() {
            out.push_str(&indent);
            match child {
                ElementChild::Element(e) => {
                    out.push_str("  ");
                    inspect_element_json(e, out, depth + 2);
                }
                ElementChild::Text(t) => {
                    out.push_str(&format!(
                        "  {{\"kind\": \"text\", \"text\": \"{}\"}}",
                        escape_json(&t.text)
                    ));
                }
                ElementChild::Comment(c) => {
                    out.push_str(&format!(
                        "  {{\"kind\": \"comment\", \"text\": \"{}\"}}",
                        escape_json(&c.text)
                    ));
                }
            }
            if i + 1 < element.children.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&format!("{}]\n", indent));
    } else {
        out.push_str(&format!("{}\"children\": []\n", indent));
    }
    out.push_str(&format!("{}}}", "  ".repeat(depth - 1)));
}

fn attr_value_json(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Literal(s) => format!("\"{}\"", escape_json(s)),
        AttributeValue::MarkupExtension(ext) => {
            let mut s = String::from("{");
            s.push_str(&format!("\"markup\": \"{}\"", ext.kind.as_str()));
            if !ext.args.is_empty() {
                s.push_str(", \"args\": [");
                for (i, a) in ext.args.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&format!("\"{}\"", a));
                }
                s.push(']');
            }
            if !ext.properties.is_empty() {
                s.push_str(", \"properties\": {");
                for (i, (k, v)) in ext.properties.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&format!("\"{}\": \"{}\"", k, v));
                }
                s.push('}');
            }
            s.push('}');
            s
        }
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// 生成 ASCII 布局预览。
///
/// 输出树状缩进表示元素层次，标注组件名与关键属性。
pub fn ascii_preview(doc: &ArmlDocument) -> String {
    let mut out = String::new();
    ascii_element(&doc.root, 0, &mut out, true);
    out
}

fn ascii_element(element: &Element, depth: usize, out: &mut String, is_last: bool) {
    let _ = is_last;
    for _ in 0..depth {
        out.push_str("  ");
    }
    out.push_str(&format!("- <{}>", element.name));
    // 标注关键属性
    let mut tags = Vec::new();
    if let Some(t) = element.attr("Title") {
        if let Some(lit) = t.value.as_literal() {
            tags.push(format!("Title=\"{}\"", lit));
        }
    }
    if let Some(o) = element.attr("Orientation") {
        if let Some(lit) = o.value.as_literal() {
            tags.push(format!("Orientation={}", lit));
        }
    }
    if let Some(b) = element.attr_with_prefix("x", "Name") {
        if let Some(lit) = b.value.as_literal() {
            tags.push(format!("x:Name={}", lit));
        }
    }
    if let Some(c) = element.attr("Content") {
        if let Some(lit) = c.value.as_literal() {
            tags.push(format!("Content=\"{}\"", lit));
        }
    }
    if let Some(t) = element.attr("Text") {
        match &t.value {
            AttributeValue::Literal(l) => tags.push(format!("Text=\"{}\"", l)),
            AttributeValue::MarkupExtension(m) => {
                if !m.args.is_empty() {
                    tags.push(format!("Text={{{} {}}}", m.kind.as_str(), m.args[0]));
                }
            }
        }
    }
    if !tags.is_empty() {
        out.push_str(&format!("  [{}]", tags.join(", ")));
    }
    out.push('\n');
    for child in &element.children {
        if let ElementChild::Element(e) = child {
            ascii_element(e, depth + 1, out, false);
        }
    }
}
