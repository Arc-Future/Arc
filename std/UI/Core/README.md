# Arc.UI（L3 · 骨架 / 已解禁 · 有边界 Sprint · M2 hub 切片）

权威设计：[RFC 037](../../../docs/rfc/037-ui.md)。L3 政策：[RFC 036](../../../docs/rfc/036-maturity.md)。**视觉立宪**：[RFC 037 §4](../../../docs/rfc/037-ui.md) — 内置控件天生现代视觉，默认 Theme 即生产级观感；**禁止**灰框 MVP 终态与「丑默认 + 美可选」双轨。**虚拟化立宪**：[RFC 037 §4](../../../docs/rfc/037-ui.md) — 大数据/长列表/大文档控件须视口虚拟化；**标杆实现** [CodeEditor 虚拟化标杆](COMPONENTS.md)。控件面见 [COMPONENTS.md](COMPONENTS.md)。**灵活绘制**（流程图 / 画布 / 游戏子集）见 [RFC 037 §4](../../../docs/rfc/037-ui.md)（**不**取代 ARML，共用 DrawList IR）。

## M2 hub 切片（2026-07-30 · 非 Stable）

| 项 | 说明 |
|----|------|
| **验证** | cargo run -p arc -- build examples/ArmlDemo；cargo test -p arc-ui |
| **范围** | LayoutManager + 面板测量/排列；PlatformTreeSync；wgpu 唯一后端渲染（`WgpuRender` · `wgpu-native.ani`）+ ScrollView 滚轮/竖滚动条（Draft）；`crates/runtime-ui/platform/windows/rt_ui_scrollbar` |
| **仍后置** | 跨平台三桌面产物面、命中测试/指针事件产品面、Style/Resource 全链、Image 解码位图进 wgpu |
| **纪律** | 本目录 = UI 领域；语言/typeck/MIR 基础债不在此轨夹带 |

## 目录结构（hub）

| 子目录 | 职责 |
|--------|------|
| Components/ | Window、Button、Text 等控件（**公开面**）；Components/Layout/ 为布局面板控件 |
| Markup/ | Element / FrameworkElement / Control / Panel / Shape / DP 体系 / Content variant |
| Data/ | Binding / BindingOperations / DataContext（绑定域团聚，均归 `Arc.UI` 根命名空间） |
| Layout/ | 布局算法（Grid/Flex/Canvas/Dock）+ 布局值类型（Thickness/GridLength）+ 文本度量 |
| Internal/ | Router / Bridge（ImeBridge）/ Sync / FocusManager / FramePump / MotionEngine / UI 调度器（[RFC 037 §7](../../../docs/rfc/037-ui.md) · 调度 [M-AS1](../../../docs/rfc/037-ui.md)） |
| Rendering/ | IRender + wgpu/（WgpuRender 唯一后端 · 经 `wgpu-native.ani` 契约直连 wgpu-native）+ DrawList 契约（DrawContext / DrawList / DrawCommand / payload，M-draw1 ✅；SceneGraph M-draw2+） |
| Media/ | Brush / Brushes / Color / Elevation / FontManager（命名族字体注册） |
| Styling/ | Style / Setter / ResourceDictionary / ThemeDictionary / VisualStateManager |
| Themes/ | Light / Dark / Controls.arml——内置主题资源源（色值权威源，codegen 生成 Colors.g.as） |
| Adaptive/ | 自适应断点求值（Tier / Match / Token） |
| Animation/ | Storyboard / SequentialChain（用户面；引擎在 Internal/MotionEngine） |
| Editing/ | 文本编辑内核（TextBoxModel / PrefixWidthCache，服务 TextBox；CodeEditor 编辑内核 TextBuffer/LineIndex/EditorViewport 已迁 Arc.UI.Edit） |
| Enums/ | 对齐 / 方向 / 滚动 / 拉伸枚举 |

> L3 政策 **已宣布**（2026-07-30）→ 允许 **骨架内有边界 Sprint**。
> 本目录是 **声明式 GUI 骨架**（[RFC 037](../../../docs/rfc/037-ui.md)），**不是**产品级 UI 框架，**不是** Stable。

## L3 Sprint · 骨架诚实（本刀）

| 类别 | 内容 | 证据 / 状态 |
|------|------|-------------|
| **可证伪** | `Thickness` / `LayoutSize`；`DependencyPropertyRegistry` + `RegisterProperty<T>` 元数据 | `ui_skeleton_honesty_e2e` **非 Skip** |
| **骨架有缺口** | `Element.GetValue`/`SetValue`、Window 属性 wrapper、Content variant 集成 | Deferred / ignore；**不**计入本刀绿 |
| **仍 ignore / Deferred** | `window_e2e`、`content_variant_implicit_e2e`、`UnitTest.Deferred` UI 测 | **禁止**回迁顶绿 / 当绿证 |
| **禁止本 Sprint** | UI 框架扩张（ARML 深化、wgpu demo、控件面铺开）、Stable 空挂、碾压/业界领先宣称 | — |

详见本文件「L3 Sprint · 骨架诚实」节。

## 诚实边界（读前必看）

- **≠** 可显示窗口 / 可交互控件 / 完整数据绑定 / 主题与样式引擎「已完成」。
- **≠** 「超越 WPF」或任何无对照性能 / DX 宣称。
- **无**根命名空间 `std/Window.as`（旧空 stub **已删**）；唯一 Window 面 = `Arc.UI.Components.Window`（骨架，**非** Stable 显示链路）。
- 设计目标与里程碑以 [RFC 037](../../../docs/rfc/037-ui.md) 为准；RFC 为**目标架构**，本 README **不**复述长篇愿景冒充现状。
- **跨平台目标矩阵**（桌面 / OHOS / WASM）为后续目标（WASM 门禁见 [RFC 031](../../../docs/rfc/031-compiler-cli.md)）；**WASM 未开工**，`wasm32-unknown-unknown` 须硬错误。

## 目录职责（骨架地图）

| 路径 | 角色 | 本刀状态 |
|------|------|----------|
| `Layout/Thickness.as` · `LayoutSize.as` | 布局值类型 | **可证伪** |
| `Markup/DependencyProperty.as` | DP 元数据 + `RegisterProperty` | **可证伪（元数据）**；Element 存储后置 |
| `Markup/Element.as` 等 | 元素树 / DP 存储 | 骨架；GetValue 链另排 |
| `Components/*` | Window / Button / 布局控件 | 骨架；**禁**当 Stable |
| `Rendering/*` · `WgpuRender` | 渲染后端 | 唯一后端 · 已接入（wgpu-native） |
| `Markup/Content.as` | Content variant | 骨架；`string` 歧义等见 Deferred README |

## 验证

```text
cargo test -p arc-ui
```

> 注：原 `ui_skeleton_honesty_e2e` 已随 `arc-integration` 退场（a2627a0f），
> 骨架证据面由 `crates/arc-ui/tests/` 承接。

`examples/UnitTest.Deferred` 下 UI 测保持隔离；**禁止**迁回 `examples/UnitTest` 顶绿。

