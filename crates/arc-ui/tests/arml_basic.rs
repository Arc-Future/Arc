//! RFC 026 M1 基础单元测试：lexer + parser + AST。
//!
//! 测试 `.arml` 文档的解析能力，覆盖 D1/D2 元素与语法特性。

use arc_ui::*;
use arc_ui::{Lexer, MarkupKind, Parser};

// ============================================================
// Lexer
// ============================================================

#[test]
fn lexer_peek_and_advance() {
    let mut lex = Lexer::new("abc");
    assert_eq!(lex.peek(), Some(b'a'));
    lex.advance();
    assert_eq!(lex.peek(), Some(b'b'));
    lex.advance();
    assert_eq!(lex.peek(), Some(b'c'));
    lex.advance();
    assert_eq!(lex.peek(), None);
    assert!(lex.is_at_end());
}

#[test]
fn lexer_skip_whitespace() {
    let mut lex = Lexer::new("   \n\t  hello");
    lex.skip_whitespace();
    assert_eq!(lex.peek(), Some(b'h'));
}

#[test]
fn lexer_qname_simple() {
    let mut lex = Lexer::new("Window");
    let name = lex.lex_qname().unwrap();
    assert_eq!(name.as_str(), "Window");
}

#[test]
fn lexer_qname_qualified() {
    let mut lex = Lexer::new("x:Class");
    let name = lex.lex_qname().unwrap();
    assert_eq!(name.as_str(), "x:Class");
}

#[test]
fn lexer_qname_dotted() {
    let mut lex = Lexer::new("Button.Background");
    let name = lex.lex_qname().unwrap();
    assert_eq!(name.as_str(), "Button.Background");
}

#[test]
fn lexer_string_lit() {
    let mut lex = Lexer::new("\"Hello, World!\"");
    let s = lex.lex_string_lit().unwrap();
    assert_eq!(s.as_str(), "Hello, World!");
}

#[test]
fn lexer_string_lit_unterminated() {
    let mut lex = Lexer::new("\"unterminated");
    assert!(lex.lex_string_lit().is_err());
}

#[test]
fn lexer_decode_entities() {
    let s = Lexer::decode_entities("a&amp;b&lt;c&gt;d&quot;e&apos;f");
    assert_eq!(s, "a&b<c>d\"e'f");
}

#[test]
fn lexer_bump_and_starts_with() {
    let mut lex = Lexer::new("<!-- comment -->");
    assert!(lex.starts_with("<!--"));
    lex.bump(4);
    assert!(!lex.starts_with("<!--"));
    assert_eq!(lex.peek(), Some(b' '));
}

#[test]
fn markup_kind_roundtrip() {
    for kind in [
        MarkupKind::XBind,
        MarkupKind::Binding,
        MarkupKind::StaticResource,
        MarkupKind::Token,
    ] {
        let s = kind.as_str();
        let back = MarkupKind::parse(s).unwrap();
        assert_eq!(kind, back);
    }
}

#[test]
fn markup_kind_unknown() {
    assert!(MarkupKind::parse("Unknown").is_none());
}

// ============================================================
// Parser
// ============================================================

#[test]
fn parse_minimal_element() {
    let doc = Parser::parse(r#"<Window/>"#).unwrap();
    assert_eq!(doc.root.name.as_str(), "Window");
    assert!(doc.root.children.is_empty());
}

#[test]
fn parse_element_with_attributes() {
    let doc = Parser::parse(r#"<Window Title="Hello" Width="800"/>"#).unwrap();
    assert_eq!(doc.root.name.as_str(), "Window");
    assert_eq!(doc.root.attributes.len(), 2);
    assert_eq!(
        doc.root.attr("Title").unwrap().value.as_literal().unwrap(),
        "Hello"
    );
    assert_eq!(
        doc.root.attr("Width").unwrap().value.as_literal().unwrap(),
        "800"
    );
}

#[test]
fn parse_xml_declaration() {
    let doc = Parser::parse(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Window/>"#,
    )
    .unwrap();
    let decl = doc.xml_decl.as_ref().unwrap();
    assert_eq!(decl.version.as_str(), "1.0");
    assert_eq!(decl.encoding.as_deref(), Some("UTF-8"));
}

#[test]
fn parse_qualified_attribute() {
    let doc = Parser::parse(r#"<Window x:Class="MyApp.Main"/>"#).unwrap();
    let attr = doc.root.attr_with_prefix("x", "Class").unwrap();
    assert_eq!(attr.value.as_literal().unwrap(), "MyApp.Main");
}

#[test]
fn parse_nested_elements() {
    let src = r#"<Window>
        <StackPanel>
            <TextBlock Text="Hello"/>
            <Button Content="Click"/>
        </StackPanel>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    assert_eq!(doc.root.name.as_str(), "Window");
    assert_eq!(doc.root.children.len(), 1);
    let stack = doc.root.children[0].as_element().unwrap();
    assert_eq!(stack.name.as_str(), "StackPanel");
    assert_eq!(stack.children.len(), 2);
}

#[test]
fn parse_self_closing_with_whitespace() {
    let src = r#"<Window Title = "Test" />"#;
    let doc = Parser::parse(src).unwrap();
    assert_eq!(
        doc.root.attr("Title").unwrap().value.as_literal().unwrap(),
        "Test"
    );
}

#[test]
fn parse_text_content() {
    let doc = Parser::parse(r#"<TextBlock>Hello, World!</TextBlock>"#).unwrap();
    assert_eq!(doc.root.children.len(), 1);
    match &doc.root.children[0] {
        ElementChild::Text(t) => assert_eq!(t.text.as_str(), "Hello, World!"),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn parse_comment() {
    let src = r#"<Window>
        <!-- this is a comment -->
        <TextBlock Text="Hi"/>
    </Window>"#;
    let doc = Parser::parse(src).unwrap();
    assert!(doc
        .root
        .children
        .iter()
        .any(|c| matches!(c, ElementChild::Comment(_))));
}

#[test]
fn parse_xbind_markup_extension() {
    let src = r#"<TextBlock Text="{x:Bind Count, Mode=OneWay}"/>"#;
    let doc = Parser::parse(src).unwrap();
    let attr = doc.root.attr("Text").unwrap();
    match &attr.value {
        AttributeValue::MarkupExtension(ext) => {
            assert_eq!(ext.kind, MarkupKind::XBind);
            assert!(!ext.args.is_empty());
            assert_eq!(ext.args[0].as_str(), "Count");
        }
        _ => panic!("expected markup extension"),
    }
}

#[test]
fn parse_static_resource_extension() {
    let src = r#"<TextBlock Foreground="{StaticResource AccentBrush}"/>"#;
    let doc = Parser::parse(src).unwrap();
    let attr = doc.root.attr("Foreground").unwrap();
    match &attr.value {
        AttributeValue::MarkupExtension(ext) => {
            assert_eq!(ext.kind, MarkupKind::StaticResource);
            assert!(!ext.args.is_empty());
        }
        _ => panic!("expected markup extension"),
    }
}

#[test]
fn parse_error_unclosed_element() {
    let r = Parser::parse(r#"<Window><TextBlock Text="hi"></Window>"#);
    assert!(r.is_err(), "expected mismatched closing tag error");
}

#[test]
fn parse_error_unterminated_attribute() {
    let r = Parser::parse(r#"<Window Title="hi"#);
    assert!(r.is_err());
}

#[test]
fn parse_error_expected_root() {
    let r = Parser::parse("not xml");
    assert!(r.is_err());
}
