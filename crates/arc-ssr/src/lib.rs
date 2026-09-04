//! arc-ssr —— Arc.Web SSR 模板编译（RFC 040 §5）。
//!
//! 把 SSR 页面模板（标准 HTML 子集 + 三标记绑定模型）编译为类型安全渲染代码：
//!
//! - {{Path}} 文本插值 -> Arc.Text.HtmlEncoder.Encode(model.Path)（默认转义）
//! - attr={Path} 属性绑定 -> Arc.Text.HtmlEncoder.EncodeAttribute(...)（上下文感知）
//! - a-for={x in Xs} 循环 -> while + 索引链（model.Xs[__i0].Y）
//! - a-if={B} 条件 -> if (model.B)
//! - a-html={Path} 原始 HTML -> 直接 sb.Append(model.Path)（显式退出转义）
//!
//! 生成代码为普通 .g.as 文本，注入编译单元后由 typeck 对照强类型模型天然检查
//! 绑定路径（绑错即编译期报错），无需专门绑定检查逻辑（RFC 040 §5 / plan W-2）。
//!
//! 本 crate 为纯文本处理（不依赖编译器核心 crate），供 arc pipeline（W-3）与
//! crate 单测（W-2）独立使用。

mod escape;
mod render;
mod template;

pub use render::{
    component_slot_order, generate_component_render_source, generate_layout_render_source,
    generate_render_class, generate_render_class_with_slots, generate_render_source, RenderModel,
    RenderOptions,
};
pub use template::{
    distribute_slots, parse_template, Attr, AttrKind, BindingPath, ComponentRef, Element, ForLoop,
    Node, SlotRef, Template, TemplateError,
};

/// 转义 Arc 字符串字面量中的特殊字符（\\、"、换行等）。
///
/// 生成渲染代码时所有静态文本（HTML 片段）经本函数写入字面量，
/// 与 arc-ui 生成 .g.as 的转义先例一致。
pub fn escape_arc_string(s: &str) -> String {
    escape::escape_arc_string(s)
}
