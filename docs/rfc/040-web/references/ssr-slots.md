# SSR 多槽能力 · 实现细节与 Vue 对标

> 关联主文档：[040 Web 框架与 SSR §5](../../040-web.md)。本子项承载 `<a-layout>` / `<a-slot>` / `<a-component>` 的**实现细节**与**与 Vue 插槽的对比**；040 主文档只保留架构级表述，二者不重复。

## 1. 设计心智：两级原语、职责分离

SSR 片段复用走**两个职责清晰的标记级原语**，命名统一为显式 `name` 属性（废除"首个属性当槽名"的位置式写法）：

| 原语 | 角色 | 槽语义 | 对标 |
|------|------|--------|------|
| `<a-layout>` | 页面↔布局的**外壳包裹**（1-N） | **单 `body` 注入** | Razor `@RenderBody()` |
| `<a-component>` | 可复用**组件封装**（1-N） | **默认槽 + 具名槽** | Vue 具名插槽 / Web Components `<slot>` |

为什么"多槽只属于组件、不属于布局"——诚实边界：

- **布局**：页面端点每次产**一整块内容**、经 `body` 唯一注入点包裹。页面↔布局是 1 对 1 包裹，天然单注入点，多槽无意义、布局不承担多内容区职责。
- **组件**：外框固定、内部内容却要随用法变化——这才需要多槽，正是 Vue 多槽应对"组件封装扩展"的场景。

## 2. 数据结构

`arc-ssr`（[crates/arc-ssr/src/template.rs(../../../../crates/arc-ssr/src/template.rs)）以 AST 节点承载槽语义：

```rust
// 节点族：既有的 Text / Interpolation / Element 之外，新增两个子面。
enum Node {
    // ...
    Slot(SlotRef),          // <a-slot>：可复用模板内的扩展占位
    Component(ComponentRef), // <a-component>：模块化片段引用（独立渲染类复用）
}

// 槽点：name 即槽名（"" = 默认槽，对齐 Web Components 未命名 <slot>）；
// fallback 为槽未填充时、在本模板作用域渲染的后备内容。
struct SlotRef {
    name: String,
    fallback: Vec<Node>,
}

// 模块化组件引用：独立模板文件编译为渲染类 __SsrComponent_{path}。
struct ComponentRef {
    path: String,                        // 组件模板名 → 渲染类类名后缀
    source: Option<BindingPath>,          // 数据绑定 → 组件 payload（可选，见 §5）
    slots: Vec<(String, Vec<Node>)>,      // 调用方按 slot="name" 提供的具名槽（首见序去重）
    default: Vec<Node>,                   // 无 slot 属性的子内容 → 默认槽
}
```

## 3. 解析与分发

### 3.1 元素归纳 `element_to_node`

`a-slot` / `a-layout` / `a-component` 按普通元素解析后，在元素归纳阶段归位：

- `a-slot` → 取 `name` 属性（缺省 `""`），子内容作 `fallback`，产出 `Node::Slot`。
- `a-layout` → 取 `name` 属性写入 `Template.layout`，**不产出渲染节点**（纯元数据）。
- `a-component` → 读 `path` / `source` 属性，子内容经 `distribute_slots` 分发，产出 `Node::Component`。

### 3.2 槽内容分发 `distribute_slots`

把组件子内容按 `slot` 属性分发到具名槽 / 默认槽（对齐 Web Components named slot assignment）：

- 带 `slot="name"` 的元素 → 落入对应具名槽（同名去重保首见序作契约顺序）；
- 无 `slot` 属性（含文本/注释）→ 落入默认槽；
- `slot` 属性在分发后被**剥离**，不进入最终 HTML。

### 3.3 槽契约推导 `component_slot_order`

从**组件模板**按声明序收集所声明的所有槽名（含默认槽 `""`，去重保首见序）。这是编译期"同步"组件槽契约的手段：调用方据此展开
`__SsrComponent_{path}.Render(payload, ...)` 的槽实参序列。管线在编译期把该契约注入 `RenderOptions.component_slots`（`path -> Vec<槽名>`）。

## 4. 渲染代码生成

生成逻辑见 [crates/arc-ssr/src/render.rs(../../../../crates/arc-ssr/src/render.rs)。

### 4.1 布局：单 `body` 注入

```as
// _layout.html 编译为独立复用单元：Render(string body)
public static class __SsrLayout_AppLayout {
    public static string Render(string body) {
        StringBuilder sb = new StringBuilder();
        sb.Append("<nav>…</nav>");
        // <a-slot name="body" /> → 注入页面内容参数
        sb.Append(body);
        sb.Append("<footer>…</footer>");
        return sb.ToString();
    }
}
```

布局渲染模式（`render_body`）下，`Node::Slot` 命中 `name == "body"` 或默认槽时注入 `body` 参数；页面模板声明 `a-layout` 后，其渲染类以
`return __SsrLayout_{Name}.Render(sb.ToString());` 外包共享布局——一次编译、1-N 复用。

### 4.2 组件：`Render(payload, slot_a, ...)` 带后备

```as
// card.html（<a-slot name="header">Default</a-slot>）编译为：
public static class __SsrComponent_card {
    public static string Render(object payload, string slot_header, string slot_body, string slot_footer) {
        // …
        if (slot_header != null) { sb.Append(slot_header); }   // 调用方已注入 → 原样出
        else { sb.Append("默认标题"); }                          // 未注入 → 组件级 fallback
        // …
        return sb.ToString();
    }
}
```

组件渲染模式（`slot_params`）下，契约内槽以**非 `null` 实参**注入，`null`（调用方未提供）回退渲染 `<a-slot>` 的 fallback。

### 4.3 调用方：槽体在调用方作用域渲染

```as
sb.Append(__SsrComponent_card.Render(model.Card, slot_header, slot_body, slot_footer));
```

调用方侧 `render_component` + `render_slot_arg`：

1. **payload**：`source` 有绑定 → 按路径解析（`model.Card`）；缺省 → 继承当前 DataContext（见 §5）。
2. **每个槽实参**：调用方提供了内容 → 渲染到**独立 StringBuilder**（槽体在调用方作用域求值，访问父数据）取字符串；未提供 → `string slot_xxx = null;` 触发组件侧 fallback。
3. 按槽契约顺序拼参调用组件渲染类。

## 5. `source` 可选 → 组件级 DataContext

`source` 绑定是可选的（[template.rs(../../../../crates/arc-ssr/src/template.rs)：
`source: Option<BindingPath>`）。缺省时 payload 以**当前上下文数据**为值——即组件模板内的绑定相对调用方传来的 DataContext 解析，对齐 WPF
DataContext 继承语义。便于数据驱动组件（如 `<a-component path="menu" />` 直接消费页面 model）无冗余传参。

## 6. 与 Vue 插槽对标

| 维度 | Vue 插槽 | Arc SSR（本实现） | 映射 |
|------|---------|------------------|------|
| 具名槽 | `<slot name="header"/>` + 调用方 `<template #header>` | `<a-slot name="header"/>` + 调用方 `slot="header"` 属性 | 1:1 |
| 默认槽 | 未命名 `<slot/>` | 无 `name` 的 `<a-slot/>`（`name=""`） | 1:1 |
| 后备内容 | `<slot>fallback</slot>` | `<a-slot>fallback</a-slot>`（`fallback` 字段） | 1:1 |
| 槽体作用域 | 插槽内容在**父作用域**编译渲染 | 槽体在**调用方作用域**渲染（独立 StringBuilder 求值） | 1:1 |
| 数据从子回流父 | **scoped slots**：`<template #x="props">` 父作用域访问子数据 | `source` payload 在**组件模板作用域**解析；要回流数据给槽内容 → 用 `source` 绑定传入 | 形态对等、机制易位 |
| 封装复用 | 组件 + 具名插槽应对封装扩展 | `<a-component>` 多槽封装扩展 | 1:1 |
| 多个根 / 虚拟节点 | 支持 `<template>` 包裹、多根 | `slot` 属性直接标记单个元素子树 | 简化为单标记 |
| 动态槽名 | `v-slot:[name]` | 不支持 | 诚实边界（编译期静态槽契约） |
| 客户端水合 | 客户端可复用渲染 | **非同构、无 JS 水合** | 诚实边界 |

**诚实边界**：Arc SSR 无反刷/水合、无运行时；`slot="name"` 采用确定性静态分发（编译期展开为命名参数），不引入动态槽名与任意表达式。scoped
slots 的"父读子数据"诉求，由数据驱动路径 `source` 对等承接，不重复 DSL（单一惯用法）。

---

[返回 040 主题入口](../../040-web.md) · [返回 RFC 索引](../../index.md)