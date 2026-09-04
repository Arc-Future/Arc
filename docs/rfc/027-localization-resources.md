# RFC 027 本地化与资源

## 背景

应用需要文化感知的文本格式化与资源管理。设计目标：`.resx` 兼容本地化（ResX CodeGen 强类型访问器 + 编译期回退链展开）、文化类型纯 Arc、文化感知格式化对齐 C# `System.Globalization`。文化数据纯 Arc 实现，C 仅承担 OS 边界 ABI。

## 设计决策

### 命名空间对齐

对标 C# 目录/命名空间分层：根接口归根命名空间，文化类型归子命名空间。

| C# 命名空间 | Arc 命名空间 | 目录 |
|------------|-------------|------|
| `System`（根） | `Arc`（根） | `std/Arc/` |
| `System.IFormatProvider` | `Arc.IFormatProvider` | `std/Arc/IFormatProvider.as` |
| `System.IFormattable` | `Arc.IFormattable` | `std/Arc/IFormattable.as` |
| `System.Globalization` | `Arc.Globalization` | `std/Arc/Globalization/` |

### 文化感知格式化（`Arc.Globalization`）

| 组件 | C# 对标 | 说明 |
|------|---------|------|
| `IFormatProvider` | `System.IFormatProvider` | 按格式类型返回格式模板的抽象；`CultureInfo`/`NumberFormatInfo`/`DateTimeFormatInfo` 实现 |
| `NumberFormatInfo` | `System.Globalization.NumberFormatInfo` | 数值格式模板：小数点/组分分隔/货币符号/百分比/正负模式 |
| `DateTimeFormatInfo` | `System.Globalization.DateTimeFormatInfo` | 日期/时间格式模式（`yyyy`/`MMMM`/`dddd`/`ShortDatePattern`） |
| `CultureInfo` | `System.Globalization.CultureInfo` | `NumberFormat`/`DateTimeFormat` 属性接入；`GetFormat(Type)` 返回对应模板；未知文化回退 Invariant |

**设计决策**：

- 根接口归根命名空间（`IFormatProvider`/`IFormattable` 在 `Arc`），与文化类型解耦；子命名空间穿透父命名空间（`Arc.Globalization` 内类型自然引用根 `Arc` 的接口）。
- 文化数据纯 Arc：`CultureData`/`CultureHelper`/`NumberFormatInfo`/`DateTimeFormatInfo` 为 `internal` / 抽象面，`CultureInfo` 为 public 用户面。
- 文化感知格式化经 `rt_*_to_string_fmt_p` ABI 覆盖全部数值基元（int/long/short/byte/float/double/uint/ulong/ushort/sbyte）；`DateTime.ToString(format, provider)` 按文化输出。
- 数值基元有参 `ToString(format, provider)` 与 `IFormattable` 有参重载为文化格式化门禁面。

### 资源管理（ResX CodeGen 强类型访问器）

| 面 | 类型 | 说明 |
|----|------|------|
| 资源源 | `.resx`（neutral + `<Base>.<Culture>.resx`） | 与 C# 完全同构的 XML 资源文件 |
| 代码生成 | 编译期注入 `resx_<Class>.g.as` | `.resx` → 强类型访问器类（静态属性） |
| 回退链 | BCP-47 前缀匹配 | `zh-Hans-CN` → `zh-Hans` → `zh` → neutral，编译期展开为常量分支 |
| 运行时 | 零 | 无 `.resources` 二进制、无哈希查找、无 `rt_*` 资源 ABI |

**设计决策**：

- 编译期由管线扫描源目录 `.resx`，生成顶层强类型访问器类并注入编译单元；资源读取
  编译为「读 `CultureInfo.CurrentUICulture.Name` → 前缀链常量分支」——运行时零解析、
  零哈希、零 ABI 调用，直达字面量（极致性能，天然 AOT）。
- neutral `.resx` 必须存在且 key 集完整（文化文件 key 缺失于 neutral → 硬错误）；
  无文化变体的 key 直接内联 neutral 常量，不发射文化读取代码。
- 诊断码：R054020（文化 key 缺失于 neutral）、R054021（控制字符）、R054022（key 无法
  净化为合法 PascalCase 标识符/碰撞）、R054023（`byte[]` 延后至数组字面量语法落地）、
  R054024（同文件重复 key）、R054025（文化文件无对应 neutral）。
- `[NeutralResourcesLanguage]` 保留：标记 neutral 资源文化，CodeGen 回退链到达即终止。
- `ResourceManager`/`ResourceSet`/`IResourceWriter`/`ResXResourceWriter` 不在设计面内：无二进制 `.resources` 资源链，`rt_resources.c` 仅承担 OS locale 检测。

```as
// Messages.resx + Messages.zh-CN.resx → 编译期生成：
public class Messages {
    public static string Greeting {
        get {
            string c = CultureInfo.CurrentUICulture.Name;
            if (c == "zh-CN" || (c.Length > 6 && c.Substring(0, 6) == "zh-CN-")) {
                return "你好，Arc！";
            }
            return "Hello, Arc!";
        }
    }
}

// 用户代码：Console.WriteLine(Messages.Greeting);
```

`arc resx generate` 离线工具、DI + `IResourceReader` 完整集成不在本设计面内。

**Attribute 本地化约定**（`[DisplayName]`/`[Description]`/`[Category]`）：

- 字面量构造 `[DisplayName("Full Name")]` 为非本地化主路径。
- 本地化引用构造 `[DisplayName(typeof(Res), nameof(Res.Key))]` 只携带
  `(ResourceType, ResourceKey)` **数据对**：元数据不含行为（RFC 018 物理边界，
  `MethodInfo` 无 `Invoke`），不做编译期内联替换（编译期无当前文化）。
- 解析归消费框架：ResX CodeGen 生成的访问器属性即文化前缀分支（内联字面量），
  是推荐的解析目标。

```as
using Arc.Globalization;

var provider = CultureInfo.GetCultureInfo("de-DE");
double v = 1234.5;
string s = v.ToString("N2", provider);   // "1.234,50"
```

## 边界

- 本文档讲本地化与资源、文化感知格式化；计时/环境（`Stopwatch`/`Environment`）见 [023 数学、张量与依赖注入](023-math-tensor-di.md)。
- `IFormattable`/`IFormatProvider` 的根命名空间归属见 [020 标准库架构与拆分](020-std-architecture.md)。

---

上一节：[026 加密与安全](026-cryptography-security.md) · 下一节：[028 类型反射面](028-type-reflection.md)