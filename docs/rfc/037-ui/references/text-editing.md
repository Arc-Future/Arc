# 文本编辑契约（TextBoxModel · TextBlock/TextBox 命名修订）

> 关联主文档：[037 §8(../../037-ui.md)。焦点/键盘三原则（P1 单一焦点权威 / P2 单一键盘通道 / P3 无静默丢弃）以主文档 §8 为权威，本篇不重复。本篇承载：**界面表现问题定案、命名修订与改名清单、TextBoxModel 内核契约、分层职责**。

## 1. 背景：界面表现问题定案

ArmlDemo「点击 Input 无 caret、键入无内容」经全链路源码推演闭环（结构性缺陷登记 D1–D10），根因是**结构性缺陷类，非单点 bug**：

1. `FocusManager.RegisterTabStop` 与 `InputFocusRouter.RegisterInput` 双固定 8 槽注册表：demo 第 9 个可聚焦控件（分区 7 `NameInput`）注册被**静默丢弃**（P3 违约）；
2. 点击路由**双 miss**（tab registry × input slot 均查无 handle）→ `IsFocused` 恒 0 → 渲染端 caret 恒不画；
3. C 侧 `g_ime.focus` 恒 NULL → `WM_CHAR`/编辑键全被 `rt_ui_ime_get_focus()` gate 吞掉 → 无输入内容；
4. `ActivateDefaultFocus` 补丁只救启动场景（首个 Input 默认焦点），救不了槽满点击路径。

**教训**：对此类问题做补丁式修复（默认焦点兜底、双通道拼装）耗费大量时间无效——修复必须落在契约层（动态注册表、单一焦点出口、单一键盘通道），编辑语义收敛进可 headless 测试的内核。

### 缺陷类 → 契约消除映射

| 缺陷 | 旧链路表现 | 消除条款 |
|------|-----------|---------|
| D1 8 槽 ×2 静默容量上限 | 注册丢弃 → 点击双 miss | §8 P1 动态注册表 + P3 溢出告警 |
| D2 键盘三重前置静默吞键 | C 层 gate 全吞 | §8 P2 单一键盘通道（平台只做机械转换） |
| D6 编辑语义 4 层散布 | Shift/编辑键分支在 C 层 | 本文 §3 TextBoxModel 唯一编辑真相 |
| D7 点击双通道 | focus/click 拼装 | §8 P2 `rt_ui_dispatch_input_activated` 单通道 |
| D8 选区 4 字段 3 冗余 | start/length/anchor/caret 并存 | 本文 §3.1 anchor+caret 双真值，余者派生 |
| D9 几何常量跨层硬编码 | 命中/渲染各写一套 | 本文 §4 `InputMetrics` 单点 |

（D3/D4/D5/D10 焦点清理与事件路径缺陷由 §8 P1/P3 消除，见主文档。）

## 2. 命名修订（单一惯用法 · 硬改名）

| 旧 | 新 | 依据 |
|----|----|------|
| `Text` | `TextBlock` | 与属性名 `Text` 解撞（`<Text Text="…">` 可读性差）；domain/ui.md 与本 RFC §3 示例**已经**使用 TextBlock，实现向文档对齐 |
| `Input` | `TextBox` | `Input` 是类别名非控件名（CheckBox/Slider 同属 input）；XAML 系（WPF/WinUI/MAUI）统一 TextBox；Block=只读 / Box=可编辑对仗工整；与 `CodeEditor` 分工清晰 |

**不留别名、不做兼容 shim**（单一惯用法）。mirror 属性名（`"Text"`/`"CaretIndex"` 等经 `ElementSet*` 传递的字符串）是属性面，不受元素改名影响；`rt_*` ABI 无元素名字符串。

### 2.1 改名清单（实现面盘点）

| 层 | 位置 | 动作 |
|----|------|------|
| std | `std/UI/Core/Components/Text.as` | 类名 `Text` → `TextBlock`，文件同名 |
| std | `std/UI/Core/Components/Input.as` | 类名 `Input` → `TextBox`，文件同名；内部按 §3–§4 重构 |
| std | `FocusManager.ApplyFocused`/`SetFocusIndex` 的 `TypeName == "Input"` 分支、`ImeBridge.RegisterInput`、`InputFocusRouter`（整类并入 FocusManager） | 同一变更集内随重构消除 |
| arc-ui | `typeck.rs` 组件注册（`"Text"`/`"Input"`）、`codegen.rs` 属性映射两处（`("Text","Text")`/`("Input","Text")`）、`verify.rs` interactive 列表、`adaptive.rs` 注释 | 名称表同步 |
| arml | `std/UI/Core/Themes/Controls.arml` 选择器、`examples/ArmlDemo/MainWindow.arml`（Text ×~30 / Input ×2） | 标记同步 |
| tests | `input_hit_test_focus_e2e`、`xbind_twoway_e2e`、`data_driven_twoway_e2e`、`ui_container_layout_e2e` 等 | 引用同步 |
| docs | RFC 037 §8、`builtin-theme-resources.md` 控件行、`std/UI/Core/COMPONENTS.md` | 文档同步；COMPONENTS.md 随实现同步 |

## 3. TextBoxModel 内核契约

**定位**：编辑语义唯一真相（D6 的解）。纯逻辑、零渲染/平台依赖、`internal`；可**无窗口 headless 全量测试**——这是「快速且完整」的结构保证。

归属：`std/UI/Core/Editing/TextBoxModel.as`（`namespace Arc.UI.Editing;`，与 `PrefixWidthCache` 同域，`internal class`；同命名空间的 `TextBuffer`/`LineIndex` 已随 CodeEditor 迁 `Arc.UI.Edit` 包）。

### 3.1 状态（唯一真值集）

| 字段 | 语义 |
|------|------|
| `text: string` | 已提交文本 |
| `caret: int` | 选区活动端（= CaretIndex） |
| `anchor: int` | 选区不动端（Shift 扩选收缩原点） |
| undo/redo 栈 | 快照 `(text, caret, anchor)`，容量上限 |
| `composition: string` | IME 组字 overlay，**不进 text** |
| `version: int` | 每次突变 +1；前缀宽度缓存失效依据 |

`SelectionStart`/`SelectionLength` 为**派生只读**计算属性（归一化区间，D8 消除）。

### 3.2 操作（全部同步、可单测）

| 组 | 操作 | 语义 |
|----|------|------|
| 编辑 | `Insert(chunk)` | 有选区先整体替换再插入（选区消费收敛为一点） |
| | `DeleteBackward` / `DeleteForward` | 有选区整体删除；否则按 caret 方向删一字符 |
| 移动 | `MoveCaret(granularity, extend)` | granularity ∈ `Char`/`Word`/`Home`/`End`（多行追加 `Line`）；`extend` 对应 Shift 扩选。取代 8 个 `MoveCaretXxx`/`ExtendSelectionXxx` 散方法 |
| 选区 | `SelectAll` / `ClearSelection` / `SetSelection(anchor, active)` | 归一化并同步 caret |
| 撤销 | `Undo` / `Redo` | 快照式；连续 `Insert` 合并一个单元；IME commit 独立单元 |
| 程序化 | `SetText(text)` | 绕过撤销合并，独立快照 |
| 组字 | `SetComposition(text)` / `CommitComposition(chunk)` / `CancelComposition()` | commit 是一个撤销单元，边界干净 |

### 3.3 策略内建

`MaxLength` / `IsReadOnly` / 字符过滤在内核统一裁决——不再散落各编辑入口与平台层（C 侧 `IsReadOnly` 判断删除，见 IN-R2）。

### 3.4 不变量

- `0 ≤ caret, anchor ≤ text.Length`，任何操作出口保证；
- `composition` 非空时，编辑命令仅作用于组字（Backspace 交 IME 的判断在控制器，内核不感知平台）；
- 状态突变必经操作方法；镜像同步单向（Model → TextBox DP → mirror → 渲染）。

## 4. 分层职责

| 层 | 归属 | 职责 |
|----|------|------|
| `TextBoxModel` | `std/UI/Core/Editing/`（internal） | §3 全部编辑语义 + 撤销 + 选区不变量 |
| `TextBox` | `std/UI/Core/Components/TextBox.as`（public） | DP 壳：`Text`/`Placeholder`/`IsReadOnly`/`MaxLength`；事件 `TextChanged`/`SelectionChanged`；`MeasureOverride` 走 `TextMeasuring`；持有内核实例并转发 |
| `TextBoxController` | `std/UI/Core/Input/`（internal） | `KeyboardRouter` 键命令 → 内核操作映射（方向键+Ctrl/Shift、Home/End、Delete/Backspace、Ctrl+A/Z/Y、可打印字符）；指针（click 定位 / drag 拖选 / 双击词选 / 三击行选）经 `rt_ui_dispatch_input_activated` 单通道；IME 事件桥接内核组字操作 |
| 渲染 | `WgpuRender.RenderTree` | 复用现有文本管线；选区高亮、caret 闪烁（`FramePump.CaretBlinkOn`）、组字下划线；命中测试用**前缀宽度缓存**（按 `version` 失效），替代逐前缀 `MeasureText` O(n) 重测 |
| 几何 | `InputMetrics` 单点 | 文本原点内边距、caret 宽高等常量，命中端与渲染端同源引用（D9 消除） |

## 5. 非目标（本契约边界）

- 多行编辑 UI（`AcceptsReturn` DP 与 `Line` 粒度契约预留，布局/滚动实现后续）；
- 剪贴板（Ctrl+C/V/X）与覆盖模式；
- `PasswordChar` 口令遮蔽、SpellCheck/ICT 自动更正；
- CodeEditor 迁移至 TextBoxModel（`TextBuffer`/`LineIndex` 独立体系，视后续成熟度决定是否收敛）。

---

[返回 037 主题入口(../../037-ui.md) · [返回 references 索引](index.md)
