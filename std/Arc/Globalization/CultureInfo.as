// RFC 027 M5: 标准库本地化 — 区域信息 CultureInfo。
//
// 设计蓝本：C# System.Globalization.CultureInfo。
//   - 标识：Name / TwoLetterISOLanguageName / ThreeLetterISOLanguageName
//   - 显示：DisplayName / EnglishName / NativeName
//   - 层级：Parent / IsNeutralCulture
//   - 格式：NumberFormat / DateTimeFormat（RFC 027 M5 文化感知格式化）
//   - 静态：CurrentCulture / CurrentUICulture / InvariantCulture / InstalledUICulture
//     GetCultureInfo(name) —— 唯一工厂入口
//
// 实现策略（纯 Arc 路径）：
//   - 文化数据（display name / native name）：走 CultureData（60+ BCP 47 条目字典）
//   - BCP 47 字符串处理（normalize / parent / isNeutral）：走 CultureHelper
//   - 仅 OS 区域检测（rt_os_current_uilocale / rt_os_current_locale）走 C ABI
//
// Arc 剔除（糟粕）：
//   - RegionInfo              —— 非 CultureInfo 组成部分，无继承/组合关系
//   - CultureTypes            —— .NET 已 deprecated，枚举模糊
//   - LCID/KeyboardLayout/ThreeLetterWindowsName —— Windows 历史包袱
//   - CreateSpecificCulture   —— GetCultureInfo 统一处理
//   - CultureNotFoundException —— 返回 InvariantCulture 而非抛异常（对齐 Arc 哲学）

namespace Arc.Globalization;

using Arc.Runtime;

/// <summary>
/// 区域/文化信息，以 BCP 47 语言标签标识。
///
/// 文化层级示意图：
///   "zh-Hans-CN" (特定文化)
///        │ Parent ↓
///   "zh-Hans"     (脚本文化)
///        │ Parent ↓
///   "zh"          (中性文化)
///        │ Parent ↓
///   ""            (固定文化 / Invariant)
///
/// 使用 <see cref="GetCultureInfo"/> 工厂方法获取实例
/// （自动走缓存，避免重复构造）。
/// </summary>
public class CultureInfo : IFormatProvider {
    // ── 字段 ──

    private NumberFormatInfo? _numberFormat;
    private DateTimeFormatInfo? _dateTimeFormat;

    // ── 标识属性 ──

    /// <summary>BCP 47 文化名称（如 "zh-CN"、"en-US"）。</summary>
    public string Name { get; }

    /// <summary>两字母 ISO 639-1 语言代码（如 "zh"、"en"）。</summary>
    public string TwoLetterISOLanguageName { get; }

    /// <summary>三字母 ISO 639-2/B 语言代码（如 "zho"、"eng"）。</summary>
    public string ThreeLetterISOLanguageName { get; }

    // ── 格式模板属性（RFC 034 M5 文化感知格式化）──

    /// <summary>本文化的数值格式模板（NumberFormatInfo）。惰性初始化。</summary>
    public NumberFormatInfo NumberFormat {
        get {
            if (_numberFormat == null) {
                _numberFormat = NumberFormatInfo.GetInstance(Name);
            }
            return _numberFormat;
        }
        set {
            _numberFormat = value;
        }
    }

    /// <summary>本文化的日期时间格式模板（DateTimeFormatInfo）。惰性初始化。</summary>
    public DateTimeFormatInfo DateTimeFormat {
        get {
            if (_dateTimeFormat == null) {
                _dateTimeFormat = DateTimeFormatInfo.GetInstance(Name);
            }
            return _dateTimeFormat;
        }
        set {
            _dateTimeFormat = value;
        }
    }

    // ── IFormatProvider（RFC 034 M5）──

    /// <summary>按格式类型返回本文化的格式模板（NumberFormat / DateTimeFormat）。</summary>
    /// <param name="formatType">请求的格式类型（NumberFormatInfo 或 DateTimeFormatInfo 的类型）。</param>
    /// <returns>对应格式模板；formatType 不受支持时返回 null。</returns>
    public object GetFormat(Arc.Reflection.Type formatType) {
        if (formatType == null) {
            return null;
        }
        if (formatType.TypeId == typeof(NumberFormatInfo).TypeId) {
            return this.NumberFormat;
        }
        if (formatType.TypeId == typeof(DateTimeFormatInfo).TypeId) {
            return this.DateTimeFormat;
        }
        return null;
    }

    // ── 显示属性 ──

    /// <summary>本地化显示名称（英文），如 "Chinese (Simplified, China)"。</summary>
    public string DisplayName { get; }

    /// <summary>英文显示名称，同 DisplayName（当前 Arc 英文界面为主导语言）。</summary>
    public string EnglishName { get; }

    /// <summary>文化自身语言的显示名称，如 "中文(中国)" for zh-CN。</summary>
    public string NativeName { get; }

    // ── 层级属性 ──

    /// <summary>
    /// 父文化。中性文化指向 InvariantCulture；InvariantCulture 的 Parent 为 null。
    /// 例：zh-CN.Parent = zh (CultureInfo), zh.Parent = InvariantCulture
    /// </summary>
    public CultureInfo? Parent { get; }

    /// <summary>是否为中性文化（纯语言名称，不含区域后缀）。</summary>
    public bool IsNeutralCulture { get; }

    // ── 静态缓存 / 单例 ──
    // RFC 006 A3 S6a：静态字段初始化器已支持 `new` 表达式与静态方法调用。核心
    // 缓存/单例统一改用 `static readonly` 惰性字段（首触构造一次、线程安全），
    // 替代手写 `== null` 缓存。可变更属性（CurrentCulture/CurrentUICulture）保留
    // 惰性字段 + setter。
    private static readonly Dictionary<string, CultureInfo> _cache = new Dictionary<string, CultureInfo>();
    private static CultureInfo? _currentUICulture;
    private static CultureInfo? _currentCulture;
    // InstalledUICulture 初值依赖 OS 原生调用（rt_os_current_uilocale），若作
    // static readonly 惰性字段会使原生符号仅被 codegen 初始化器引用而被 tree-shake
    // 剪除 → undefined symbol。故保留惰性属性 + 字段（首触经正常调用图可达原生符号）。
    private static CultureInfo? _installedUICulture;

    /// <summary>固定文化 / 不区分文化（BCP 47 空字符串 ""）。只读惰性单例。</summary>
    public static readonly CultureInfo InvariantCulture = new CultureInfo("");

    // ── 静态属性 ──

    /// <summary>当前 UI 文化（影响资源查找与 UI 显示语言）。可设置。</summary>
    public static CultureInfo CurrentUICulture {
        get {
            if (_currentUICulture == null) {
                _currentUICulture = CultureInfo._fromOsLocale(rt_resources.rt_os_current_uilocale());
            }
            return _currentUICulture;
        }
        set {
            _currentUICulture = value;
        }
    }

    /// <summary>当前格式文化（影响日期/数值/货币格式化）。可设置。</summary>
    public static CultureInfo CurrentCulture {
        get {
            if (_currentCulture == null) {
                _currentCulture = CultureInfo._fromOsLocale(rt_resources.rt_os_current_locale());
            }
            return _currentCulture;
        }
        set {
            _currentCulture = value;
        }
    }

    // ── 工厂方法 ──

    /// <summary>
    /// 从 BCP 47 名称获取 CultureInfo 实例。
    /// 内部走静态缓存，相同名称返回同一实例。
    /// </summary>
    /// <param name="name">BCP 47 文化名称（如 "zh-CN"、"en"）。</param>
    /// <returns>CultureInfo 实例；name 为 null 或无效时返回 InvariantCulture。</returns>
    public static CultureInfo GetCultureInfo(string name) {
        if (name == null) {
            return CultureInfo.InvariantCulture;
        }

        string normalized = CultureHelper.Normalize(name);
        if (normalized == "") {
            return CultureInfo.InvariantCulture;
        }

        // `_cache` 为 static readonly 惰性字段（首触构造一次、线程安全），直接使用。
        if (_cache.ContainsKey(normalized)) {
            return _cache[normalized];
        }

        CultureInfo ci = new CultureInfo(normalized);
        _cache[normalized] = ci;
        return ci;
    }

    // ── 构造器（internal：仅工厂 GetCultureInfo / InvariantCulture 内部使用；
    //    对外统一走 GetCultureInfo 单一入口，避免绕过缓存重复构造）──

    /// <summary>按 BCP 47 名称创建文化信息实例。</summary>
    /// <param name="name">BCP 47 文化名称（如 "zh-CN"）。空字符串表示 Invariant。</param>
    internal CultureInfo(string name) {
        if (name == null) {
            name = "";
        }

        // 1. 规范化（纯 Arc 路径，走 CultureHelper）
        string normalized = CultureHelper.Normalize(name);
        Name = normalized;

        // 2. 显示名（纯 Arc 路径，走 CultureData 60+ 条目字典）
        CultureData.CultureEntry entry = CultureData.Find(normalized);
        DisplayName = entry.DisplayName;
        EnglishName = entry.EnglishName;
        NativeName = entry.NativeName;

        // 3. 中性判断（纯 Arc 路径，走 CultureHelper）
        IsNeutralCulture = CultureHelper.IsNeutral(normalized);

        // 4. 语言代码（从 BCP 47 名称提取）
        TwoLetterISOLanguageName = CultureInfo._extractTwoLetter(normalized);
        ThreeLetterISOLanguageName = CultureInfo._mapToThreeLetter(TwoLetterISOLanguageName);

        // 5. 父文化链（纯 Arc 路径，走 CultureHelper）
        if (normalized == "") {
            Parent = null; // Invariant 无父文化
        } else {
            string parentName = CultureHelper.GetParent(normalized);
            if (parentName == "") {
                Parent = CultureInfo.InvariantCulture;
            } else {
                Parent = CultureInfo.GetCultureInfo(parentName);
            }
        }

        // 6. 格式模板即刻就位（RFC 027 M5）：运行时文化感知格式化直接按字段偏移
        //    读取 `_numberFormat` / `_dateTimeFormat`，故构造即填充，不依赖惰性访问。
        _numberFormat = NumberFormatInfo.GetInstance(normalized);
        _dateTimeFormat = DateTimeFormatInfo.GetInstance(normalized);
    }

    // ── 私有帮助方法 ──

    /// <summary>
    /// 从 OS locale 字符串构造 CultureInfo，null/空时返回 InvariantCulture。
    /// 抽取 CurrentUICulture / CurrentCulture / InstalledUICulture 共用逻辑。
    /// </summary>
    private static CultureInfo _fromOsLocale(string? localeName) {
        string name = "";
        if (localeName != null) {
            name = localeName;
        }
        if (name == "") {
            return CultureInfo.InvariantCulture;
        }
        return CultureInfo.GetCultureInfo(name);
    }

    /// <summary>从 BCP 47 名称提取两字母 ISO 639-1 语言代码。</summary>
    private static string _extractTwoLetter(string name) {
        if (name == null || name == "") {
            return "iv"; // invariant
        }
        int hyphen = name.IndexOf("-");
        if (hyphen < 0) {
            return name.Length >= 2 ? name.Substring(0, 2) : name;
        }
        return name.Substring(0, hyphen > 2 ? 2 : hyphen);
    }

    /// <summary>ISO 639-1 → ISO 639-2/B 映射表。</summary>
    private static string _mapToThreeLetter(string two) {
        // 覆盖 ISO 639-1 最常见语言（按使用频次排序）
        switch (two) {
            case "zh": return "zho";
            case "en": return "eng";
            case "ja": return "jpn";
            case "ko": return "kor";
            case "de": return "deu";
            case "fr": return "fra";
            case "ru": return "rus";
            case "es": return "spa";
            case "pt": return "por";
            case "it": return "ita";
            case "nl": return "nld";
            case "sv": return "swe";
            case "pl": return "pol";
            case "ar": return "ara";
            case "tr": return "tur";
            case "cs": return "ces";
            case "th": return "tha";
            case "vi": return "vie";
            case "id": return "ind";
            case "hi": return "hin";
            case "el": return "ell";
            case "he": return "heb";
            case "da": return "dan";
            case "fi": return "fin";
            case "nb": return "nob";
            case "ro": return "ron";
            case "hu": return "hun";
            case "uk": return "ukr";
            case "ms": return "msa";
            case "ta": return "tam";
            case "bg": return "bul";
            case "sr": return "srp";
            case "hr": return "hrv";
            case "sk": return "slk";
            case "sl": return "slv";
            case "lt": return "lit";
            case "lv": return "lav";
            case "et": return "est";
            case "iw": return "heb"; // iw 是 he 的旧代码
            case "in": return "ind"; // in 是 id 的旧代码
            default:  return two;    // 未知代码直接返回原文
        }
    }
}