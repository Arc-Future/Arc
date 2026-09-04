# 附录 B 符号约定

## 语法元符号

| 符号 | 含义 |
|------|------|
| `*` | 零次或多次重复 |
| `+` | 一次或多次重复 |
| `?` | 可选 |
| `\|` | 择一 |
| `<...>` | 语法非终结符占位 |

示例：

```
Block ::= "{" Statement* "}"
Param   ::= Type Ident
```

## 类型书写

| 形式 | 含义 |
|------|------|
| `T` | 类型变量或具体类型 |
| `Task<T>` | 异步结果类型 |
| `IEnumerable<T>` | 可枚举序列 |
| `IQueryable<T>` | 可查询数据源 |
| `Expression<F>` | 表达式树包装的函数类型 F（C# 对齐） |
| `&T` / `&mut T` | borrowck / MIR 内部借用符号（**非**用户面语法） |

## 编译管线符号

| 符号 | 含义 |
|------|------|
| `Γ` | 类型环境（typeck） |
| `Δ` | 借用/所有权环境（borrowck） |
| `⊢` | 可导出/可类型检查 |
| `→` | 编译阶段转换 |

概念性：

```
Γ ⊢ e : T        （表达式 e 在 Γ 下具有类型 T）
Δ ⊢ stmt ok      （语句在借用环境下合法）
AST → HIR → MIR  （IR lowering 链）
```

## 代码字体

等宽字体表示：

- 源码与关键字
- CLI 命令与参数
- 文件路径与 crate 名
- 运行时符号（如 `rt_println`）

## 文件路径约定

| 路径 | 含义 |
|------|------|
| `crates/<name>/` | 编译器 crate |
| `std/` | 标准库 |
| `examples/` | 示例程序 |
| `docs/user-guide/` | 用户手册 |

## 文档链接

| 链接形式 | 示例 |
|----------|------|
| 章内 | `02-build-run.md` |
| 章节间 | `17-arc-toml-reference.md` |
| 附录 | `appendix-glossary.md` |
| 根文档 | `../SUMMARY.md` |

---

上一节：[18 Native 组件集成](18-native-integration-guide.md) · 相关：[附录 A 术语表](appendix-glossary.md) · [返回目录](index.md)