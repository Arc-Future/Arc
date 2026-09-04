//! arc-ssr crate 单测（plan W-2 验收）：
//! 模板解析（三标记/自闭合/注释/插值）+ 渲染代码生成（含嵌套 a-for 索引链）。

use arc_ssr::{
    component_slot_order, generate_component_render_source, generate_layout_render_source,
    generate_render_source, parse_template, AttrKind, BindingPath, ComponentRef, Node,
    RenderOptions, Template,
};

// ─────────────────────────────────────────────────────────────────────────
// 模板解析
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn parse_three_markups_bindings() {
    let t = parse_template(
        r#"<main>
  <h1>{{Title}}</h1>
  <ul>
    <li a-for={post in Posts}>
      <a href={post.Slug} class="x">{{post.Title}}</a>
      <time>{{post.PublishedAt}}</time>
    </li>
  </ul>
  <p a-if={Empty}>暂无文章</p>
  <div a-html={IntroHtml}></div>
</main>"#,
    )
    .expect("parse");

    assert_eq!(t.root.len(), 1);
    let Node::Element(main) = &t.root[0] else {
        panic!("root should be element");
    };
    assert_eq!(main.tag, "main");
    // 过滤纯空白文本节点后为 4 个元素（h1/ul/p/div）
    let elem_children: Vec<&Node> = main
        .children
        .iter()
        .filter(|n| !matches!(n, Node::Text(t) if t.trim().is_empty()))
        .collect();
    assert_eq!(elem_children.len(), 4);
    let h1 = &elem_children[0];

    let Node::Element(h1) = h1 else { panic!("h1") };
    assert_eq!(h1.tag, "h1");
    assert!(matches!(&h1.children[0], Node::Interpolation(p) if p.parts == ["Title".to_string()]));

    let Node::Element(ul) = &elem_children[1] else {
        panic!("ul")
    };
    let li = ul
        .children
        .iter()
        .find(|n| !matches!(n, Node::Text(t) if t.trim().is_empty()))
        .expect("li element");
    let Node::Element(li) = li else { panic!("li") };
    let fl = li.for_loop.as_ref().expect("a-for");
    assert_eq!(fl.var, "post");
    assert_eq!(fl.collection.parts, ["Posts".to_string()]);
    let a_node = li
        .children
        .iter()
        .find(|n| !matches!(n, Node::Text(t) if t.trim().is_empty()))
        .expect("a element");
    let Node::Element(a) = a_node else {
        panic!("a")
    };
    assert_eq!(a.attrs.len(), 2);
    match &a.attrs[0].kind {
        AttrKind::Bound(raw) => {
            let p = BindingPath::parse(raw).expect("path");
            assert_eq!(p.parts, ["post".to_string(), "Slug".to_string()]);
        }
        _ => panic!("href should be bound"),
    }
    assert_eq!(a.attrs[1].kind, AttrKind::Static("x".into()));

    let Node::Element(p) = &elem_children[2] else {
        panic!("p")
    };
    let cond = p.if_cond.as_ref().expect("a-if");
    assert_eq!(cond.parts, ["Empty".to_string()]);

    let Node::Element(div) = &elem_children[3] else {
        panic!("div")
    };
    let raw = div.raw_html.as_ref().expect("a-html");
    assert_eq!(raw.parts, ["IntroHtml".to_string()]);
}

#[test]
fn parse_self_closing_and_comment() {
    let t = parse_template(r#"<!-- header --><br/><img src="a.png" alt={Alt}>"#).expect("parse");
    let Node::Text(comment) = &t.root[0] else {
        panic!("comment")
    };
    assert_eq!(comment, "<!-- header -->");
    let Node::Element(br) = &t.root[1] else {
        panic!("br")
    };
    assert!(br.self_closing);
    let Node::Element(img) = &t.root[2] else {
        panic!("img")
    };
    assert!(img.children.is_empty()); // void 元素隐式自闭合（源码无 />）
    assert_eq!(img.attrs.len(), 2);
}

#[test]
fn parse_rejects_mismatched_close() {
    let err = parse_template("<div></span>").expect_err("mismatch should error");
    assert!(err.message.contains("mismatched closing tag"));
}

#[test]
fn parse_interpolation_in_mixed_text() {
    let t = parse_template("Hello {{Name}}! <b>{{Title}}</b>").expect("parse");
    assert_eq!(t.root.len(), 4);
    let Node::Text(first) = &t.root[0] else {
        panic!()
    };
    assert_eq!(first, "Hello ");
    let Node::Text(second) = &t.root[2] else {
        panic!()
    };
    assert_eq!(second, "! ");
    let Node::Interpolation(p) = &t.root[1] else {
        panic!()
    };
    assert_eq!(p.parts, ["Name".to_string()]);
}

// ─────────────────────────────────────────────────────────────────────────
// 渲染代码生成
// ─────────────────────────────────────────────────────────────────────────

fn opts(class: &str, model: &str) -> RenderOptions {
    RenderOptions {
        class_name: class.into(),
        model_type: model.into(),
        model_param: "model".into(),
        ..RenderOptions::default()
    }
}

#[test]
fn render_static_text_and_escape() {
    let t: Template = parse_template("<main>a\"b\\c</main>").expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_P", "PM"));
    assert!(code.contains("public static class __SsrRender_P {"));
    assert!(code.contains("public static string Render(PM model) {"));
    assert!(code.contains(r#"sb.Append("<main>");"#));
    assert!(code.contains(r#"sb.Append("a\"b\\c");"#));
    assert!(code.contains(r#"sb.Append("</main>");"#));
}

#[test]
fn render_interpolation_escapes_and_for_index_chain() {
    let t: Template = parse_template(
        "<ul><li a-for={post in Posts}><a href={post.Slug}>{{post.Title}}</a></li></ul>",
    )
    .expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_Home", "HomeModel"));
    assert!(
        code.contains("while (__i0 < model.Posts.Count) {"),
        "{code}"
    );
    assert!(code.contains("HtmlEncoder.EncodeAttribute(model.Posts[__i0].Slug)"));
    assert!(code.contains("HtmlEncoder.Encode(model.Posts[__i0].Title)"));
    assert!(code.contains("__i0++;"));
    assert!(code.contains(r#"sb.Append("<a href=\"");"#));
    assert!(
        code.contains(r#"sb.Append("\">");"#),
        "bound attr must close quote: {code}"
    );
}

#[test]
fn render_nested_for_index_chain() {
    let t: Template = parse_template(
        "<div a-for={book in Books}><p a-for={tag in book.Tags}>{{tag.Name}}</p></div>",
    )
    .expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_Home", "HomeModel"));
    assert!(code.contains("while (__i0 < model.Books.Count) {"));
    assert!(code.contains("while (__i1 < model.Books[__i0].Tags.Count) {"));
    assert!(code.contains("HtmlEncoder.Encode(model.Books[__i0].Tags[__i1].Name)"));
}

#[test]
fn render_if_and_raw_html() {
    let t: Template =
        parse_template("<p a-if={Empty}>暂无</p><div a-html={Intro}></div>").expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_P", "PM"));
    assert!(code.contains("if (model.Empty) {"), "{code}");
    assert!(code.contains("sb.Append(model.Intro);"));
    assert!(!code.contains("HtmlEncoder.Encode(model.Intro)"));
}

#[test]
fn render_static_attr_quoted() {
    let t: Template = parse_template("<a href=\"/x\" data-k=\"v\">L</a>").expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_P", "PM"));
    assert!(code.contains(r#"sb.Append("<a href=\"/x\" data-k=\"v\">");"#));
    assert!(code.contains(r#"sb.Append("</a>");"#));
    assert!(code.contains(r#"sb.Append("L");"#));
}

// ─────────────────────────────────────────────────────────────────────────
// 布局渲染骨架：<a-layout>/<a-slot> 伪元素 + 1-N 复用
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn slot_parse_explicit_name_and_default() {
    // 具名槽：<a-slot name="body" /> -> SlotRef{ name, fallback }；自身不声明布局。
    let lt: Template = parse_template(r#"<html><body><a-slot name="body" /></body></html>"#)
        .expect("parse layout");
    assert_eq!(lt.layout, None);

    let Node::Element(html) = &lt.root[0] else {
        panic!("html")
    };
    let Node::Element(body) = &html.children[0] else {
        panic!("body")
    };
    let Node::Slot(s) = &body.children[0] else {
        panic!("slot")
    };
    assert_eq!(s.name, "body");
    assert!(s.fallback.is_empty());

    // 未命名 <a-slot> = 默认槽（name 空）。
    let d: Template = parse_template("<main><a-slot>fallback</a-slot></main>").expect("parse");
    let Node::Element(main) = &d.root[0] else {
        panic!("main")
    };
    let Node::Slot(ds) = &main.children[0] else {
        panic!("default slot")
    };
    assert_eq!(ds.name, "");
    assert!(!ds.fallback.is_empty(), "fallback children kept");
}

#[test]
fn slot_fallback_children_kept() {
    // <a-slot name="header">Default</a-slot>：子内容作为槽未填时的 fallback。
    let t: Template =
        parse_template("<header><a-slot name=\"header\">Default Title</a-slot></header>")
            .expect("parse");
    let Node::Element(header) = &t.root[0] else {
        panic!("header")
    };
    let Node::Slot(s) = &header.children[0] else {
        panic!("slot")
    };
    assert_eq!(s.name, "header");
    let Node::Text(fb) = s
        .fallback
        .iter()
        .find(|n| !matches!(n, Node::Text(x) if x.trim().is_empty()))
        .unwrap()
    else {
        panic!("fallback text")
    };
    assert_eq!(fb, "Default Title");
}

#[test]
fn layout_parse_recognizes_declaration() {
    // 页面模板：<a-layout name="AppLayout" /> 归位 layout 声明，不产出节点。
    let pt: Template = parse_template(r#"<a-layout name="AppLayout" /><main>{{Title}}</main>"#)
        .expect("parse page");
    assert_eq!(pt.layout.as_deref(), Some("AppLayout"));
    assert_eq!(pt.root.len(), 1);
    let Node::Element(main) = &pt.root[0] else {
        panic!("main")
    };
    assert_eq!(main.tag, "main");
}

#[test]
fn layout_generates_reuse_render_class() {
    // 布局编译为独立复用渲染类：Render(string body)，槽注入 body。
    let lt: Template = parse_template(
        r#"<html><body><header>Site</header><a-slot name="body" /><footer>©</footer></body></html>"#,
    )
    .expect("parse");
    let lcode = generate_layout_render_source(
        &lt,
        &RenderOptions {
            class_name: "__SsrLayout_AppLayout".into(),
            model_type: "string".into(),
            model_param: "body".into(),
            ..RenderOptions::default()
        },
    );
    assert!(lcode.contains("public static class __SsrLayout_AppLayout {"));
    assert!(lcode.contains("public static string Render(string body) {"));
    assert!(lcode.contains(r#"sb.Append("<header>");"#));
    assert!(lcode.contains(r#"sb.Append("Site");"#));
    assert!(lcode.contains(r#"sb.Append("</header>");"#));
    assert!(lcode.contains("sb.Append(body);"));
    assert!(lcode.contains(r#"sb.Append("<footer>");"#));
    assert!(lcode.contains(r#"sb.Append("©");"#));
    assert!(lcode.contains(r#"sb.Append("</footer>");"#));
}

#[test]
fn page_with_layout_wraps_reuse_render() {
    // 1-N 复用：两个页面均声明同一布局，渲染均调用共享 __SsrLayout_AppLayout.Render(body)。
    let page = parse_template(r#"<a-layout name="AppLayout" /><main><h1>{{Title}}</h1></main>"#)
        .expect("parse");
    let code = generate_render_source(&page, &opts("__SsrRender_Home", "HomeModel"));

    assert!(
        !code.contains("a-layout"),
        "layout declaration must not be output"
    );
    assert!(code.contains(r#"sb.Append("<main>");"#));
    assert!(code.contains("HtmlEncoder.Encode(model.Title)"));
    assert!(
        code.contains("return __SsrLayout_AppLayout.Render(sb.ToString());"),
        "{code}"
    );
    // 页面渲染不含内联骨架（布局如声明则外包），无重复 <header>。
    assert!(
        !code.contains("Site"),
        "layout skeleton must not be inlined per page"
    );
}

#[test]
fn page_without_layout_still_plain_return() {
    let t: Template = parse_template("<main></main>").expect("parse");
    let code = generate_render_source(&t, &opts("__SsrRender_Home", "HomeModel"));
    assert!(code.contains("return sb.ToString();"), "{code}");
    assert!(!code.contains("__SsrLayout_"), "{code}");
}

// ─────────────────────────────────────────────────────────────────────────
// 组件封装与多槽：<a-component> + 具名槽/默认槽（Vue 多槽对标 · RFC 040 §5）
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn component_parse_and_distribute_slots() {
    // 调用方：具名槽 slot="header" 归 ordered slots，无 slot 属性的子内容进默认槽。
    let t: Template = parse_template(
        r#"<a-component path="card" source={Card}>
  <h1 slot="header">{{Card.Title}}</h1>
  <p>no slot → default</p>
</a-component>"#,
    )
    .expect("parse");
    assert_eq!(t.root.len(), 1);
    let Node::Component(ComponentRef {
        path,
        source,
        slots,
        default,
    }) = &t.root[0]
    else {
        panic!("a-component");
    };
    assert_eq!(path, "card");
    let src = source.as_ref().expect("source");
    assert_eq!(src.parts, ["Card".to_string()]);
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].0, "header");
    assert!(!slots[0].1.is_empty());
    // 默认槽：<p> 元素（slot 属性已被剥离）。
    let default_has_p = default
        .iter()
        .any(|n| matches!(n, Node::Element(e) if e.tag == "p" && !e.attrs.iter().any(|a| a.name == "slot")));
    assert!(default_has_p, "{default:?}");
}

#[test]
fn component_source_optional_inherits_context() {
    // source 缺省：payload 继承当前上下文数据（组件模板级 DataContext，对齐 WPF）。
    let t: Template = parse_template("<a-component path=\"menu\" />").expect("parse");
    let Node::Component(ComponentRef { source, .. }) = &t.root[0] else {
        panic!("a-component");
    };
    assert!(source.is_none());
}

#[test]
fn component_slot_order_collects_declared_order() {
    // 组件模板声明序：默认槽 + header + footer（按首见序去重）。
    let comp: Template = parse_template(
        r#"<div><a-slot>main</a-slot><header><a-slot name="header">T</a-slot></header><footer><a-slot name="footer" /></footer></div>"#,
    )
    .expect("parse");
    let order = component_slot_order(&comp.root);
    assert_eq!(order, vec!["", "header", "footer"]);
}

#[test]
fn component_render_source_payload_and_named_slots() {
    // 组件模板编译为 Render(payload, slot_header, slot_footer, ...)：
    // 槽非 null 注入；null 渲染组件级 fallback。
    let comp: Template = parse_template(
        r#"<div class="card"><header><a-slot name="header">Default Title</a-slot></header><main><a-slot name="body" /></main></div>"#,
    )
    .expect("parse");
    let code = generate_component_render_source(
        &comp,
        &RenderOptions {
            class_name: "__SsrComponent_card".into(),
            model_type: "CardModel".into(),
            model_param: "payload".into(),
            ..RenderOptions::default()
        },
    );
    assert!(
        code.contains("public static class __SsrComponent_card {"),
        "{code}"
    );
    assert!(
        code.contains("public static string Render(CardModel payload, string slot_header, string slot_body) {"),
        "{code}"
    );
    // 非 null 注入；null 回退组件级 fallback 文本。
    assert!(code.contains("if (slot_header != null) {"));
    assert!(code.contains(r#"sb.Append("Default Title");"#));
    assert!(
        !code.contains("sb.Append(payload);"),
        "payload not printed bare"
    );
}

#[test]
fn component_call_renders_slot_args_at_caller_scope() {
    // 调用方 codegen：source 解析 payload；具名槽渲染到独立 StringBuilder 为字符串实参；
    // 组件 Render(payload, header) 非 null 实参注入。缺省 source 走 DataContext（model）。
    let page: Template = parse_template(
        r#"<a-component path="card" source={Card}>
  <h1 slot="header">{{Card.Title}}</h1>
</a-component>"#,
    )
    .expect("parse");
    let mut opts = opts("__SsrRender_Home", "HomeModel");
    opts.component_slots
        .insert("card".into(), vec!["header".into()]);
    let code = generate_render_source(&page, &opts);
    assert!(
        code.contains("sb.Append(__SsrComponent_card.Render(model.Card, slot_header));"),
        "{code}"
    );
    assert!(code.contains("StringBuilder __sb_slot_header"), "{code}");
    assert!(
        code.contains("HtmlEncoder.Encode(model.Card.Title)"),
        "slot body rendered at caller scope"
    );
}

#[test]
fn component_without_source_passes_context_model() {
    // source 缺省：payload = 当前 DataContext（页面 model），槽契约给足后同样展开。
    let page: Template = parse_template(
        r#"<a-component path="menu"><h1 slot="header">{{MenuTitle}}</h1></a-component>"#,
    )
    .expect("parse");
    let mut opts = opts("__SsrRender_P", "PM");
    opts.component_slots
        .insert("menu".into(), vec!["header".into()]);
    let code = generate_render_source(&page, &opts);
    assert!(!code.contains("source"), "no source passthrough");
    assert!(
        code.contains("sb.Append(__SsrComponent_menu.Render(model, slot_header));"),
        "{code}"
    );
}
