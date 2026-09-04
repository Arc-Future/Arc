//! 渲染代码生成（RFC 040 §5）：模板 AST -> 类型安全 .g.as 渲染代码文本。
//!
//! 生成代码形态（注入编译单元后由 typeck 对照模型类型天然检查绑定路径）：
//!
//! ```as
//!     public static class __SsrRender_HomePage {
//!         public static string Render(HomeModel model) {
//!             StringBuilder sb = new StringBuilder();
//!             sb.Append("<main>");
//!             sb.Append(HtmlEncoder.Encode(model.Title));   // {{Title}}
//!             int __i0 = 0;                                        // a-for
//!             while (__i0 < model.Posts.Count) {
//!                 sb.Append("<a href=\"");
//!                 sb.Append(HtmlEncoder.EncodeAttribute(model.Posts[__i0].Slug));
//!                 sb.Append("\">");
//!                 __i0++;
//!             }
//!             if (model.Empty) { ... }                             // a-if
//!             sb.Append(model.IntroHtml);                          // a-html（原始，不转义）
//!             return sb.ToString();
//!         }
//!     }
//! ```
//! 静态文本经 escape_arc_string 写入字符串字面量；绑定路径按作用域解析：
//! 顶层相对 model，a-for 循环变量映射为索引链（post -> model.Posts[__i0]）。
//!
//! 布局与组件的槽能力（RFC 040 §5，对齐 Web Components `<slot>` / Vue / kilnx）：
//! - 布局模板：单 `body` 注入，`Render(string body)` 复用单元，`<a-slot name="body" />` 注入 body。
//! - 组件模板：`<a-slot name>` 具名槽 + fallback，`Render(payload, slot_a, ...)`。
//! - 调用方：`<a-component path source>` 子内容按 `slot="name"` 分发，槽体在调用方作用域
//!   渲染为字符串入参；`source` 缺省继承当前上下文（DataContext）。

use std::collections::HashMap;

use crate::escape_arc_string;
use crate::template::{AttrKind, BindingPath, ComponentRef, Element, Node, SlotRef, Template};

/// 单模型渲染描述（一个模型类型一个 Render 重载）。
#[derive(Debug, Clone)]
pub struct RenderModel {
    pub model_type: String,
    pub model_param: String,
}

/// 渲染代码生成选项。
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// 生成的渲染类名（如 __SsrRender_HomePage）。
    pub class_name: String,
    /// 模型类型简单名（如 HomeModel；注入命名空间内可见）。
    pub model_type: String,
    /// 渲染函数模型参数名（默认 model）。
    pub model_param: String,
    /// 组件槽契约：path -> 组件模板按声明序的具名槽名（`""` 表示默认槽）。
    /// 调用方编译期由管线从组件模板解析注入；arc-ssr 独立使用为空时组件调用退化为 `Render(payload)`。
    pub component_slots: HashMap<String, Vec<String>>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            class_name: "__SsrRender_Page".into(),
            model_type: "object".into(),
            model_param: "model".into(),
            component_slots: HashMap::new(),
        }
    }
}

/// 生成页面/片段渲染类源码（.g.as 文本）。
pub fn generate_render_source(template: &Template, opts: &RenderOptions) -> String {
    let models = [RenderModel {
        model_type: opts.model_type.clone(),
        model_param: opts.model_param.clone(),
    }];
    generate_render_class_opts(template, &opts.class_name, &models, &opts.component_slots)
}

/// 生成渲染类源码：一个模型类型一个 `Render` 重载（同一模板，多 DataContext 形态）。
///
/// 无组件槽契约（空）——供 arc pipeline 的基础路径使用；需组件槽的分发请走
/// `generate_render_source`（经 RenderOptions.component_slots 携带契约）。
pub fn generate_render_class(
    template: &Template,
    class_name: &str,
    models: &[RenderModel],
) -> String {
    generate_render_class_opts(template, class_name, models, &HashMap::new())
}

/// 同 [`generate_render_class`]，额外携带组件槽契约（`path -> 组件模板按声明序具名槽`）。
///
/// 页面模板含 `<a-component>` 时，管线须把各组件槽契约注入调用方渲染展开
/// （`__SsrComponent_{path}.Render(payload, slot_a, ...)`）；无契约则组件退化为
/// `Render(payload)`（槽内容丢失）。arc pipeline 的 SSR 注入走本入口。
pub fn generate_render_class_with_slots(
    template: &Template,
    class_name: &str,
    models: &[RenderModel],
    component_slots: &HashMap<String, Vec<String>>,
) -> String {
    generate_render_class_opts(template, class_name, models, component_slots)
}

fn generate_render_class_opts(
    template: &Template,
    class_name: &str,
    models: &[RenderModel],
    component_slots: &HashMap<String, Vec<String>>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("public static class {} {{\n", class_name));
    for m in models {
        let opts = RenderOptions {
            class_name: class_name.to_string(),
            model_type: m.model_type.clone(),
            model_param: m.model_param.clone(),
            component_slots: component_slots.clone(),
        };
        let mut g = Generator::new(&opts);
        let body = g.render_nodes(&template.root);
        out.push_str(&format!(
            "    public static string Render({} {}) {{\n",
            m.model_type, m.model_param
        ));
        out.push_str("        StringBuilder sb = new StringBuilder();\n");
        out.push_str(&body);
        // 声明了布局：外包共享布局（1-N 复用），把本页已渲染内容注入布局 body 槽。
        if let Some(layout_name) = &template.layout {
            out.push_str(&format!(
                "        return __SsrLayout_{}.Render(sb.ToString());\n",
                layout_name
            ));
        } else {
            out.push_str("        return sb.ToString();\n");
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    out
}

/// 生成布局渲染类源码（1-N 复用单元）：
/// `Render(string body)` —— 外层骨架 + body 槽注入页面内容（对标 Razor `_Layout` + `@RenderBody()`）。
/// 生成类名规则 `__SsrLayout_{模板名}`，与页面渲染的 `__SsrLayout_{layout}.Render(...)` 调用对应。
pub fn generate_layout_render_source(template: &Template, opts: &RenderOptions) -> String {
    let mut g = Generator {
        opts,
        bindings: Vec::new(),
        counter: 0,
        render_body: true,
        slot_params: None,
        sb_var: "sb".into(),
    };
    let body = g.render_nodes(&template.root);
    let mut out = String::new();
    out.push_str(&format!("public static class {} {{\n", opts.class_name));
    out.push_str(&format!(
        "    public static string Render(string {}) {{\n",
        opts.model_param
    ));
    out.push_str("        StringBuilder sb = new StringBuilder();\n");
    out.push_str(&body);
    out.push_str("        return sb.ToString();\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

/// 收集组件模板按声明序的具名槽名（含默认槽 `""`，去重保首见序）。
///
/// 供管线解析"同步"组件槽契约：调用方编译期据此展开 `__SsrComponent_{path}.Render(payload, ...)` 的槽实参。
pub fn component_slot_order(nodes: &[Node]) -> Vec<String> {
    fn walk(nodes: &[Node], order: &mut Vec<String>) {
        for n in nodes {
            match n {
                Node::Slot(s) => {
                    if !order.contains(&s.name) {
                        order.push(s.name.clone());
                    }
                }
                Node::Element(el) => walk(&el.children, order),
                _ => {}
            }
        }
    }
    let mut order = Vec::new();
    walk(nodes, &mut order);
    order
}

/// 生成组件模板渲染类源码：`Render(payload, slot_a, slot_b, ...)`（payload + 按声明序具名槽）。
///
/// 槽实参非 `null`（调用方已提供、渲染于调用方作用域）时直接注入字符串；
/// `null`（调用方未提供）或未入契约则渲染组件模板 `<a-slot>` 的 fallback 内容（组件作用域）。
pub fn generate_component_render_source(template: &Template, opts: &RenderOptions) -> String {
    let order = component_slot_order(&template.root);
    let mut params: Vec<String> = vec![format!("{} {}", opts.model_type, opts.model_param)];
    let mut slot_params: Vec<(String, String)> = Vec::new();
    for name in &order {
        let var = slot_var(name);
        params.push(format!("string {var}"));
        slot_params.push((name.clone(), var));
    }
    let mut g = Generator {
        opts,
        bindings: Vec::new(),
        counter: 0,
        render_body: false,
        slot_params: Some(slot_params),
        sb_var: "sb".into(),
    };
    let body = g.render_nodes(&template.root);
    let mut out = String::new();
    out.push_str(&format!("public static class {} {{\n", opts.class_name));
    out.push_str(&format!(
        "    public static string Render({}) {{\n",
        params.join(", ")
    ));
    out.push_str("        StringBuilder sb = new StringBuilder();\n");
    out.push_str(&body);
    out.push_str("        return sb.ToString();\n    }\n");
    out.push_str("}\n");
    out
}

struct Generator<'a> {
    opts: &'a RenderOptions,
    /// 循环变量作用域栈：(变量名 -> 索引链访问前缀)
    bindings: Vec<(String, String)>,
    /// 索引临时变量计数
    counter: usize,
    /// 布局渲染模式：Slot 以 body 参数填充（页面模板中渲染为传值占位，不内联注入）。
    render_body: bool,
    /// 组件渲染模式：槽契约 (槽名 -> Render 参数名)。Some 时以非 null 实参注入，否则渲染 fallback。
    slot_params: Option<Vec<(String, String)>>,
    /// 当前 StringBuilder 接收变量名（默认 sb；组件槽内容渲染时切换为独立局部）。
    sb_var: String,
}

impl<'a> Generator<'a> {
    fn new(opts: &'a RenderOptions) -> Self {
        Generator {
            opts,
            bindings: Vec::new(),
            counter: 0,
            render_body: false,
            slot_params: None,
            sb_var: "sb".into(),
        }
    }

    /// 解析绑定路径为模型访问链（作用域感知）。
    fn resolve(&self, path: &BindingPath) -> String {
        if path.parts.is_empty() {
            return self.opts.model_param.clone();
        }
        let head = &path.parts[0];
        if let Some((_, prefix)) = self.bindings.iter().rev().find(|(v, _)| v == head) {
            if path.parts.len() == 1 {
                return prefix.clone();
            }
            return format!("{}.{}", prefix, path.parts[1..].join("."));
        }
        format!("{}.{}", self.opts.model_param, path.display())
    }

    fn fresh_index(&mut self) -> String {
        let i = self.counter;
        self.counter += 1;
        format!("__i{i}")
    }

    /// 渲染组件引用（调用方作用域）：source 解析 payload（缺省继承 DataContext），
    /// 子内容已解析进 ComponentRef 的具名/默认槽，按组件槽契约渲染为槽实参字符串后调用。
    fn render_component(&mut self, c: &ComponentRef) -> String {
        let payload = match &c.source {
            Some(p) => self.resolve(p),
            None => self.opts.model_param.clone(),
        };
        let order = self
            .opts
            .component_slots
            .get(&c.path)
            .cloned()
            .unwrap_or_default();
        if order.is_empty() {
            // 无槽契约（管线未注入）：退化为 Render(payload)。
            return format!(
                "        {}.Append(__SsrComponent_{}.Render({}));\n",
                self.sb_var, c.path, payload
            );
        }
        let mut out = String::new();
        let mut args: Vec<String> = vec![payload];
        for name in &order {
            let var = slot_var(name);
            let content = if name.is_empty() {
                Some(&c.default)
            } else {
                c.slots.iter().find(|(n, _)| n == name).map(|(_, v)| v)
            };
            out.push_str(&self.render_slot_arg(&var, content));
            args.push(var);
        }
        out.push_str(&format!(
            "        {}.Append(__SsrComponent_{}.Render({}));\n",
            self.sb_var,
            c.path,
            args.join(", ")
        ));
        out
    }

    /// 渲染单个槽实参：调用方提供了内容则渲染到独立 StringBuilder 求字符串，否则置 `null`
    /// （组件侧据此回退其 fallback）。内容在调用方作用域渲染（对齐 Vue/kilnx）。
    fn render_slot_arg(&mut self, var: &str, content: Option<&Vec<Node>>) -> String {
        match content {
            None => format!("        string {var} = null;\n"),
            Some(nodes) => {
                let saved = self.sb_var.clone();
                let buf = format!("__sb_{var}");
                self.sb_var = buf.clone();
                let mut out = format!("        StringBuilder {buf} = new StringBuilder();\n");
                out.push_str(&self.render_nodes(nodes));
                self.sb_var = saved;
                out.push_str(&format!("        string {var} = {buf}.ToString();\n"));
                out
            }
        }
    }

    /// 渲染槽点。
    fn render_slot(&mut self, slot: &SlotRef) -> String {
        // 布局：单 body 注入页面内容参数。
        if self.render_body {
            if slot.name == "body" || slot.name.is_empty() {
                return format!(
                    "        {}.Append({});\n",
                    self.sb_var, self.opts.model_param
                );
            }
            return String::new();
        }
        // 组件：契约内槽以非 null 实参注入，否则渲染 fallback。
        if let Some(params) = &self.slot_params {
            if let Some((_, var)) = params.iter().find(|(n, _)| *n == slot.name) {
                let mut out = format!("        if ({} != null) {{\n", var);
                out.push_str(&format!("            {}.Append({});\n", self.sb_var, var));
                out.push_str("        } else {\n");
                out.push_str(&self.render_nodes(&slot.fallback));
                out.push_str("        }\n");
                return out;
            }
        }
        // 页面模板含槽无意义（诚实边界）：渲染 fallback（通常为空）不产出。
        self.render_nodes(&slot.fallback)
    }

    /// 渲染节点列表（含作用域绑定）。
    fn render_nodes(&mut self, nodes: &[Node]) -> String {
        let mut out = String::new();
        for n in nodes {
            match n {
                Node::Text(t) => {
                    if !t.is_empty() {
                        out.push_str(&format!(
                            "        {}.Append(\"{}\");\n",
                            self.sb_var,
                            escape_arc_string(t)
                        ));
                    }
                }
                Node::Interpolation(p) => {
                    out.push_str(&format!(
                        "        {}.Append(HtmlEncoder.Encode({}));\n",
                        self.sb_var,
                        self.resolve(p)
                    ));
                }
                Node::Element(el) => out.push_str(&self.render_element(el)),
                Node::Component(c) => out.push_str(&self.render_component(c)),
                Node::Slot(s) => out.push_str(&self.render_slot(s)),
            }
        }
        out
    }

    fn render_element(&mut self, el: &Element) -> String {
        let mut out = String::new();
        if let Some(fl) = &el.for_loop {
            let idx = self.fresh_index();
            let coll = self.resolve(&fl.collection);
            out.push_str(&format!("        int {idx} = 0;\n"));
            out.push_str(&format!("        while ({idx} < {coll}.Count) {{\n"));
            self.bindings
                .push((fl.var.clone(), format!("{coll}[{idx}]")));
            let inner = self.render_element_guarded(el);
            self.bindings.pop();
            out.push_str(&inner);
            out.push_str(&format!("        {idx}++;\n"));
            out.push_str("        }\n");
            return out;
        }
        out.push_str(&self.render_element_guarded(el));
        out
    }

    /// 渲染元素本体（外层 a-for 已剥离；此处处理 a-if / a-html / 常规）。
    fn render_element_guarded(&mut self, el: &Element) -> String {
        let mut out = String::new();
        if let Some(cond) = &el.if_cond {
            out.push_str(&format!("        if ({}) {{\n", self.resolve(cond)));
            let inner = self.render_element_payload(el);
            for line in inner.lines() {
                out.push_str(&format!("    {line}\n"));
            }
            out.push_str("        }\n");
            return out;
        }
        out.push_str(&self.render_element_payload(el));
        out
    }

    /// 渲染元素载荷：开始标签 + 内容 + 结束标签。
    fn render_element_payload(&mut self, el: &Element) -> String {
        let mut out = String::new();
        for part in build_open_tag_parts(el) {
            match part {
                OpenTagPart::Static(s) => {
                    out.push_str(&format!(
                        "        {}.Append(\"{}\");\n",
                        self.sb_var,
                        escape_arc_string(&s)
                    ));
                }
                OpenTagPart::Bound(path) => {
                    out.push_str(&format!(
                        "        {}.Append(HtmlEncoder.EncodeAttribute({}));\n",
                        self.sb_var,
                        self.resolve(&path)
                    ));
                }
            }
        }
        if el.self_closing {
            return out;
        }
        if el.children.is_empty() && is_void_tag(&el.tag) {
            // void 元素（img/br/input 等）：无闭合标签（HTML 语义）。
            return out;
        }
        if let Some(raw) = &el.raw_html {
            // a-html：内容 = 原始 HTML 绑定（显式退出转义）；忽略子节点。
            out.push_str(&format!(
                "        {}.Append({});\n",
                self.sb_var,
                self.resolve(raw)
            ));
        } else {
            out.push_str(&self.render_nodes(&el.children));
        }
        out.push_str(&format!(
            "        {}.Append(\"</{}>\");\n",
            self.sb_var, el.tag
        ));
        out
    }
}

/// 开标签分段：静态文本段与绑定段交错，`>` 恒位于最后一段静态段（绑定值须在 `>` 前）。
enum OpenTagPart {
    Static(String),
    Bound(BindingPath),
}

fn build_open_tag_parts(el: &Element) -> Vec<OpenTagPart> {
    let mut parts: Vec<OpenTagPart> = Vec::new();
    let mut cur = format!("<{}", el.tag);
    for attr in &el.attrs {
        match &attr.kind {
            AttrKind::Static(v) => {
                cur.push(' ');
                cur.push_str(&attr.name);
                cur.push_str("=\"");
                cur.push_str(v);
                cur.push('"');
            }
            AttrKind::Bound(raw) => {
                let path = BindingPath::parse(raw).unwrap_or_else(|_| BindingPath {
                    parts: vec![raw.clone()],
                });
                cur.push(' ');
                cur.push_str(&attr.name);
                cur.push_str("=\"");
                parts.push(OpenTagPart::Static(std::mem::take(&mut cur)));
                parts.push(OpenTagPart::Bound(path));
                // 保留闭合引号，随后的静态段以 `"` 开头（如 `">`），确保属性值闭合。
                cur.push('"');
            }
            AttrKind::Bare => {
                cur.push(' ');
                cur.push_str(&attr.name);
            }
        }
    }
    if el.self_closing {
        cur.push_str(" />");
    } else {
        cur.push('>');
    }
    parts.push(OpenTagPart::Static(cur));
    parts
}

/// 槽参数名：默认槽 `""` -> `slot_default`；具名槽 -> `slot_{name}`（过滤非法标识符字符）。
fn slot_var(name: &str) -> String {
    if name.is_empty() {
        "slot_default".to_string()
    } else {
        let mut s = "slot_".to_string();
        s.extend(name.chars().filter(|c| c.is_alphanumeric() || *c == '_'));
        s
    }
}

/// HTML void 元素（无闭合标签）。
fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}
