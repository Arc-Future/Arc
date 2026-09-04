# AI 原生 · VisualHost 演进（生成容器 · 评审单元 · 预览宿主）

> 本子项承载 [037 §10(../../037-ui.md) §11 的 VisualHost 三角色正式化。VisualHost 现有语义（iframe 式隔离：
> 独立 Element 子树 + 帧内 ResourceDictionary 根 + 默认 Light 主题 + DataContext 继承边界 + 生命周期事件 +
> Rebuild 预览入口）是 AI 原生能力的天然载体。配套：[live-preview](ai-native-live-preview.md) · [render-capture](ai-native-render-capture.md) · [layout-snapshot](ai-native-layout-snapshot.md)。

## 1. 三角色正式化

| 角色 | 语义 | 承载 API |
|------|------|---------|
| AI 生成容器 | 生成物永远在一个 VisualHost 内实例化：资源/主题/DataContext 隔离，动态绑定受限通道在内层 | Rebuild（已有）/ ApplyPatch / Reset |
| 评审单元 | 渲染回读、布局快照、golden 对比都以 VisualHost 为边界 | CapturePng / GetLayoutSnapshot |
| 预览宿主 | 无窗口离屏渲染，设计时/生成时即时预览 | LivePreviewHost（派生自 VisualHost） |

- 一石二鸟：VisualHost 同时是 A2UI 动态 UI 容器（协议面）与保真评审单元（质量面）——两个方向共享同一容器语义。

## 2. API 演进

    namespace Arc.UI.Components;

    public class VisualHost : ContentControl {
        // ===== 既有（RFC 037 §4.3）=====
        public void SetContent(Element root);          // 同步替换内层根
        public void Rebuild(Element root, ResourceDictionary resources);  // 预览管线入口
        public void Clear();
        public Signal<bool> ContentChanged / InnerLoaded / InnerUnloaded;
        public override bool IsDataContextBoundary();  // true

        // ===== AI 原生新增 =====
        /// <summary>属性补丁：按元素路径改属性并单帧重渲染（改即见）。</summary>
        public void ApplyPatch(string elementPath, string propertyName, object value);

        /// <summary>重置为初始状态（卸载内层树）。</summary>
        public void Reset();

        /// <summary>评审单元：渲染当前内层树到离屏 target 并回读为 PNG。</summary>
        public bool CapturePng(string filePath, double width, double height);

        /// <summary>评审单元：当前内层树的结构化布局快照。</summary>
        public LayoutSnapshot GetLayoutSnapshot();

        /// <summary>人审固化：动态树 → ARML 静态资产（spec → ARML 反向 codegen，带来源元数据）。</summary>
        public string ExportArml();
    }

## 3. 隔离语义（AI 生成容器的安全边界）

| 面 | 语义 | 对生成物的意义 |
|----|------|---------------|
| 资源隔离 | 帧内 ResourceDictionary 根，宿主隐式样式不穿透 | 生成物不污染产品 UI 样式 |
| DataContext 边界 | IsDataContextBoundary()=true，宿主 DataContext 不流入 | 动态绑定在受限通道内解析，不触碰宿主模型 |
| 主题 | 默认合并 Light 主题（Rebuild 可换资源） | 生成物主题自洽，SwitchTheme 不影响预览容器 |
| 生命周期 | InnerLoaded/InnerUnloaded/ContentChanged | 生成/替换/销毁可观察、可审计 |

## 4. 动态绑定受限通道

- 动态 UI 的 DataContext 运行时路径（A2UI binding 语义）**不走反射扫描**：
  显式命名槽（Name → 对象表）+ 属性表（白名单 DP）双约束，路径解析失败即显式错误。
- 这是「动态是数据不是代码」的执行面：渲染面零业务逻辑，绑定值只经受限通道进入。

## 5. 交互预览（后续能力 · 诚实边界）

- 诚实缺口：独立 HWND、焦点域/输入路由域/IME 隔离不在当前能力面。
- 决策：预览**无交互**（G2 明确「无交互体验」）；交互预览（点击/键入反馈）依赖独立的
  焦点域/输入路由/IME（后续能力），与 037 §8 输入栈重构衔接，不阻塞本系列渲染回读能力。

## 6. 边界（本子项）

- 不把 VisualHost 当通用窗口替代（窗口语义属 Window/WindowHost）。
- 不引入第二套资源解析（VisualHost 资源链沿用既有合并语义）。
- ExportArml 产物必须重新通过编译期闸门方可合入（对齐 fidelity-loop §5）。
