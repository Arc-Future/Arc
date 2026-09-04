# 14 结构化诊断

Arc 编译器输出带源位置的结构化诊断，供终端人类阅读与工具机器解析。

## 诊断结构

每条诊断包含：

| 字段 | 含义 |
|------|------|
| 级别 | error / warning / note |
| 消息 | 人类可读摘要 |
| 标签 | 一个或多个 `(file, span, label text)` |
| 相关说明 | 可选 secondary span |

示例（概念输出）：

```
error: use of moved value `a`
  --> examples/demo.as:12:5
   |
12 |     Console.WriteLine(a);
   |     ^^^^^^^^^ value moved here on line 11
```

## 与 borrowck / typeck 的集成

| 来源 | 典型错误 |
|------|----------|
| `borrowck` | `UseAfterMove`, `AlreadyBorrowed`, `MutablyBorrowed` |
| `typeck` | 类型不匹配、Queryable 路径缺少 `expression` |
| `parser` | 语法期望不符 |

错误类型在各自 crate 定义，CLI 层映射为 `Diagnostic`。

## 机器可读输出

除人类可读输出外，规划中的 `--message-format json` 将输出机器可读诊断：

```json
{
  "level": "error",
  "code": "E0308",
  "message": "expected IEnumerable<User>, found IQueryable<User>",
  "spans": [{ "file": "main.as", "line_start": 5, "line_end": 5 }]
}
```

智能体工具应优先解析 JSON 通道（可用时），fallback 至文本。

## 修复建议

诊断消息应包含**可操作建议**，而非仅陈述失败：

- 「Queryable 路径 Lambda 须使用 `expression` 关键字」
- 「值已于上一行移动，请复制句柄或使用新的绑定」

## CLI 行为

- `arc check` — 仅诊断，不 codegen
- 非零退出码表示存在 error 级诊断
- warning 默认不阻断 build（策略可配置）

## 协作工作流

结构化诊断是「人机协作」工作流的关键环节：

1. 人类或智能体编写 `.as` 源码
2. `arc check` 获取结构化诊断
3. 根据 span 与建议局部修补
4. `arc build` 确认通过后再集成

避免「运行时才发现错误」的长反馈环。

---

上一节：[13 标准库架构](13-standard-library.md) · 下一节：[15 能力系统](15-capability-system.md)