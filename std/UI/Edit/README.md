# Arc.UI.Edit

代码编辑组件库：**CodeEditor**（大文档视口虚拟化 · mmap piece-table）及其专属编辑内核。

**来源**：自 `Arc.UI`（`std/UI/Core`）迁移——`CodeEditor`、`TextBuffer`、`LineIndex`、`EditorViewport`、`EditorInputRouter`。命名空间保持不变（`Arc.UI.Components` / `Arc.UI.Editing` / `Arc.UI.Internal`，目录可解耦于命名空间，RFC 037 §2）。TextBox 共用的 `PrefixWidthCache` / `TextBoxModel` 留 Core。

**设计权威**：RFC 037 §4 · 实现规划（CodeEditor 视口虚拟化 M-CE1）。

**内容**：

| 文件 | 命名空间 | 职责 |
|------|---------|------|
| `Components/CodeEditor.as` | `Arc.UI.Components` | 大文档编辑器组件（视口虚拟化 + 算术 ExtentHeight） |
| `Editing/TextBuffer.as` | `Arc.UI.Editing` | Piece-table 文档缓冲（mmap OpenPath，禁 ReadAllText） |
| `Editing/LineIndex.as` | `Arc.UI.Editing` | 行索引（TextBuffer C 侧索引门面） |
| `Editing/EditorViewport.as` | `Arc.UI.Editing` | 可见行视口算术（First/Last/overscan） |
| `Internal/EditorInputRouter.as` | `Arc.UI.Internal` | CodeEditor 键盘/IME 焦点注册表 |

**状态**：迁移完成（文件就位）；组件功能演进与测试随 实现规划 排期。
