//! 布局 + 组件封装演示（RFC 040 §5）：博客首页 = 共享布局 + 组件多槽 + 三标记绑定。
//!
//! 展示「模板 → 类型安全渲染类 → HTML」三段：用 arc-ssr 库真实解析/生成，
//! 打印布局/组件/页面的渲染类源码，并给出版本集成后的预期 HTML。
//!
//! 设计要点（对齐 Web Components <slot> / Vue 插槽）：
//! - 布局 `<a-layout name>` + 单 `body` 槽：页面↔布局 1-N 包裹，Razor @RenderBody() 心智。
//! - 组件 `<a-component path source>`：多槽封装扩展（对标 Vue 多槽应对组件封装）。
//!   组件模板内 `<a-slot name="...">` 作扩展占位（可带 fallback）；
//!   调用方用 `slot="name"` 注入，无 slot 属性 → 默认槽；槽体在调用方作用域渲染；
//!   `source` 可选，缺省继承当前 DataContext。
//!
//! 运行：cargo run -p arc-ssr --example layout_demo

use arc_ssr::{
    component_slot_order, generate_component_render_source, generate_layout_render_source,
    generate_render_source, parse_template, RenderOptions,
};

fn main() {
    // ── 1) 共享布局模板（AppLayout）：外层骨架 + 唯一 body 注入点 ────
    let layout_tpl = parse_template(
        r#"<html>
  <body>
    <header class="site"><a href="/">Arc Blog</a></header>
    <nav><a href="/posts">Posts</a> | <a href="/about">About</a></nav>
    <a-slot name="body" />
    <footer>© 2026 Arc SSR</footer>
  </body>
</html>"#,
    )
    .expect("parse layout");

    let layout_code = generate_layout_render_source(
        &layout_tpl,
        &RenderOptions {
            class_name: "__SsrLayout_AppLayout".into(),
            model_type: "string".into(),
            model_param: "body".into(),
            ..RenderOptions::default()
        },
    );

    // ── 2) 组件模板（card）：固定外框 + 三个扩展槽（多槽封装） ────────
    //     <a-slot name="header"> 带 fallback：调用方未填时渲染组件级默认。
    let card_tpl = parse_template(
        r#"<section class="card">
  <header><a-slot name="header">默认标题</a-slot></header>
  <main><a-slot name="body" /></main>
  <footer><a-slot name="footer" /></footer>
</section>"#,
    )
    .expect("parse card");

    let card_code = generate_component_render_source(
        &card_tpl,
        &RenderOptions {
            class_name: "__SsrComponent_card".into(),
            model_type: "object".into(),
            model_param: "payload".into(),
            ..RenderOptions::default()
        },
    );

    // ── 3) 页面模板（HomePage）：声明布局 + 组件传参（source + 多槽） ──
    //     source={Card} 传数据（组件模板作用域）；槽注标记（调用方作用域）。
    let page_tpl = parse_template(
        r#"<a-layout name="AppLayout" />
<main>
  <h1>Welcome, {{Name}}</h1>
  <a-component path="card" source={Card}>
    <h2 slot="header">{{Card.Title}}</h2>
    <p slot="body">{{Card.Summary}}</p>
    <span slot="footer">#{{Card.Tag}}</span>
  </a-component>
</main>"#,
    )
    .expect("parse page");

    // 管线在编译期从 card 组件模板解析槽契约（card.html → ["header","body","footer"]）；
    // 独立演示时用 component_slot_order 同步该契约（对齐真实流水线行为）。
    let mut card_slots = std::collections::HashMap::new();
    card_slots.insert("card".to_string(), component_slot_order(&card_tpl.root));
    let page_code = generate_render_source(
        &page_tpl,
        &RenderOptions {
            class_name: "__SsrRender_HomePage".into(),
            model_type: "HomeModel".into(),
            model_param: "model".into(),
            component_slots: card_slots,
        },
    );

    println!("═══════════════ 模型（概念 · 已强类型，供绑定检查）═══════════════");
    println!("public class HomeModel {{");
    println!("    public string Name;");
    println!("    public CardModel Card;   // <a-component source={{Card}}>");
    println!("}}");
    println!("public class CardModel {{");
    println!("    public string Title;  public string Summary;  public string Tag;");
    println!("}}");

    println!("\n═══════════ 布局渲染类（编译一次 · 1-N 复用 · Render(string body)）═══════════");
    println!("{layout_code}");

    println!("\n═══════════ 组件渲染类（payload + 按声明序具名槽）═══════════");
    println!("{card_code}");

    println!("\n═══════════ 页面渲染类（外部包裹布局 + 组件多槽传参）═══════════");
    println!("{page_code}");

    println!("\n═══════════ 版本集成后 · 给定示例数据的最终 HTML（概念）═══════════");
    println!("数据: Name=Ada, Card=(Hello Arc, Server-rendered SSR, arc2026)");
    println!();
    println!("<html>");
    println!("  <body>");
    println!("    <header class=\"site\"><a href=\"/\">Arc Blog</a></header>");
    println!("    <nav><a href=\"/posts\">Posts</a> | <a href=\"/about\">About</a></nav>");
    println!("    <!-- ↓ 槽注入：页面渲染结果 sb.ToString() 填入 a-slot name=body -->");
    println!("  <main>");
    println!("    <h1>Welcome, Ada</h1>");
    println!("    <section class=\"card\">");
    println!("      <header><h2>Hello Arc</h2></header>");
    println!("      <main><p>Server-rendered SSR</p></main>");
    println!("      <footer><span>#arc2026</span></footer>");
    println!("    </section>");
    println!("  </main>");
    println!("    <footer>© 2026 Arc SSR</footer>");
    println!("  </body>");
    println!("</html>");
}
