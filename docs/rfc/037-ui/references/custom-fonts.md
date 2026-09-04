# Arc.UI 自定义字体（WPF 对齐最小面）

> 本文是 [037 UI 声明式框架(../../037-ui.md) 的**渐进式披露子项**。037 为主主题入口，本文承载「命名字体族注册 + markup 消费」的确定性设计。
>
> 成像领域的 `Arc.Drawing.Font`（离屏量字/栅格）见 [029](../../029-imaging-graphics.md)；**UI 帧内文本不得绑该类型**（禁双轨）。

## 1. 定位与最小条

对标 **WPF 心智**：应用在启动期按**族名**注册字体文件，控件经 `FontFamily` / `FontSize` / `FontWeight` 消费；布局度量与绘制必须同源。

| 最小面 | 说明 |
|--------|------|
| 命名族可注册 | `Application` / `FontManager` 注册 API（见 §2） |
| markup `FontFamily` | ARML / DP 字符串族名（见 §3） |
| 项目相对路径 | 字体路径相对 app/project base 解析（见 §4） |
| 同源度量 | `Measure*` 与 `DrawText` 使用同一 atlas / 同一已解析族 |
| `FontWeight` 换面 | 至少 `Normal` / `Bold` 可对应不同字面文件 |

现有半成品：`WgpuRender.RegisterFontFamily(name, chain)`（绝对路径 + `|`/`,` chain）为**后端内部**能力；用户面正道收敛到 `FontManager`，后端实现可适配既有 atlas API，**不得**把后端 chain 字符串暴露为第二套公开惯用法。

## 2. 注册 API 语义

### 2.1 归属

| 类型 | 命名空间 | 角色 |
|------|----------|------|
| `FontManager` | `Arc.UI` | 应用级字体注册表（单一实例，由 `Application` 持有） |
| `Application.Fonts` | `Arc.UI` | 正道入口：`Application.Current.Fonts`（或等价只读属性） |

注册须在**首次布局/绘制依赖该族之前**完成（典型：`Application` 启动 / 主窗体 `InitializeComponent` 之前）。迟注册不保证已测控件自动重测；实现可选择脏标记刷新，但**不**把「热替换字体」立为交付项（后续能力）。

### 2.2 表面（最小）

```as
namespace Arc.UI;

/// <summary>应用级字体注册表——命名族 → 字面文件。</summary>
public class FontManager
{
    /// <summary>
    /// 注册命名族的 Normal 面。path 为相对 app/project base 的相对路径（.ttf/.otf）。
    /// 成功返回 true；失败返回 false（见 §5），禁止静默当成功。
    /// </summary>
    public bool RegisterFamily(string familyName, string relativePath);

    /// <summary>
    /// 注册命名族，并分别指定 Normal / Bold 字面。
    /// Bold 路径在仅使用 FontWeight=Normal 时仍须可解析（文件须存在）；失败同 RegisterFamily。
    /// </summary>
    public bool RegisterFamily(string familyName, string normalRelativePath, string boldRelativePath);
}
```

| 约定 | 语义 |
|------|------|
| `familyName` | 非空；与 markup `FontFamily` **精确字符串匹配**（大小写敏感，与 WPF 族名常见用法一致：作者自洽即可） |
| 重复注册同名 | **失败**（返回 false）；不覆盖、不静默替换（避免「以为换了字体其实还在用旧面」） |
| 空名 / 空路径 | **失败** |
| 返回值 | `true` = 族名已可被 `FontFamily` 解析且对应文件已纳入度量/绘制；`false` = 未注册成功 |

后端可将成功注册映射到既有 `RegisterFontFamily` / atlas；**用户代码只调用 `FontManager`**。

## 3. Markup / 依赖属性

`Control`（及派生）既有 DP 为本面消费点（不另开属性轨）：

| 属性 | 类型 | 语义 |
|------|------|------|
| `FontFamily` | `string` | 已注册族名，或平台默认族名（如 `"Segoe UI"`）；**不是**文件路径，**不是** pack URI |
| `FontSize` | `double` | 字号（px / DIP，与现有 Control 默认一致） |
| `FontWeight` | `string` | `"Normal"`（默认）/ `"Bold"` → 选用对应字面；其它字面量**不承诺换面**（见下） |
| `FontStyle` | — | 斜体/Oblique 等不在能力面；markup 若出现不保证生效，**不得**文档暗示已支持 |

```arml
<TextBlock FontFamily="AppSans" FontSize="16" FontWeight="Bold"
      Text="自定义字体" />
```

`FontWeight` 非 `Bold` 时走 Normal 面；未知字重字符串**不得**假装加载了第三套字面——回退 Normal 面，并须可诊断（见 §5）。

## 4. 路径解析与 bin 约定

| 规则 | 说明 |
|------|------|
| 相对路径基准 | **app/project base**：含 `arc.toml` 的项目根；运行时以进程可定位的应用基目录为准（与可执行文件所在目录 / 项目根的对应关系由实现固定一条，**禁止**多套隐式 cwd 猜测） |
| 禁止默认绝对路径正道 | 绝对路径可作为实现调试旁路，**不是**用户面正道；文档与示例一律相对路径 |
| 扩展名 | `.ttf` / `.otf`；其它格式失败 |
| 构建复制 | 字体文件须在运行时可按相对路径打开。约定：作者将字体放在项目树内（推荐 `Assets/Fonts/...`），`arc build` **须**把注册所用相对路径下的文件复制到 `bin/<config>/` 下保持相对路径（或等价保证运行时基目录可见）。若构建复制未落地，实现须**诚实失败**（注册返回 false），禁止「注册成功但运行时找不到文件」 |

**不做** `pack://application:,,,/` 及 WPF pack URI 全集（见 §6）。

## 5. 失败与回退（禁静默骗成功）

| 场景 | 行为 |
|------|------|
| 注册：文件不存在 / 不可读 / 非支持格式 / atlas 拒绝 | `RegisterFamily` → **`false`**；族名**不**进入已注册表 |
| 注册：同名已存在 | **`false`**；保留先注册者 |
| markup / DP：`FontFamily` 为空或未注册名 | **回退默认族**（与 `FontFamilyProperty` 元数据默认一致，当前为 `"Segoe UI"` / 后端默认族索引 0）；控件仍可布局与绘制，**不得**把未注册名记为「已加载自定义字体」 |
| `FontWeight=Bold` 但仅注册了 Normal 面 | 使用 Normal 面绘制（可诊断）；**不得**用错误字形冒充粗体成功 |
| 度量 vs 绘制 | 同一 `FontFamily`+`FontSize`+`FontWeight` 解析结果必须一致；禁止布局用 A、绘制用 B |

诊断：失败注册与未注册名回退应可经现有诊断/日志通道观察（具体 API 不另立）；**禁止**返回 true 却实际未加载。

## 6. 非目标（能力边界）

| 项 | 说明 |
|----|------|
| `pack://application:,,,/` | 完整 pack URI / 资源嵌入 URI **不做** |
| HarfBuzz / 复杂文种整形 | 复杂脚本 shaping **不做** |
| 彩色 emoji | 彩色 emoji 字体管线 **不做** |
| UI ↔ `Arc.Drawing.Font` | **禁双轨**；离屏成像继续用 Drawing；UI 只用族名 + FontManager |
| `FontStyle` / 多字重轴（Light/Medium/…） | **未交付**；仅 Normal/Bold 换面为最小条 |
| 字体子集/CDN/系统字体枚举 API | 不做 |
| 热替换 / 运行时卸载族 | 不做 |

## 7. 与渲染后端关系

- 度量与绘制均经 wgpu 文本 atlas（或其后继），`FontManager` 为唯一用户面注册入口。
- `IRender` **不必**新增用户可见字体方法；内部桥接即可。
- 单一惯用法：一意图一条正道——注册走 `Fonts.RegisterFamily`，消费走 `FontFamily` 字符串。

---

[返回 037 主题入口(../../037-ui.md) · [references 索引](index.md) · [返回 RFC 索引](../../index.md)
