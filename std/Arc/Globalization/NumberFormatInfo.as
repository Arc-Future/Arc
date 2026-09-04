// RFC 034 M5: 文化感知格式化 — 数值格式模板 NumberFormatInfo。
//
// 对标 C# System.Globalization.NumberFormatInfo。
//
// 定义数值格式化（N/C/E/P 及自定义格式）随文化而异的模板字段：小数点、组分隔、
// 货币符号、货币正负模式、百分比符号等。为 `IFormattable.ToString(format, provider)`
// 与插值文化感知化提供数据基础（插值接入为后续独立事项）。
//
// 提供 Invariant 与常用文化模板（zh-CN / en-US / de-DE / fr-FR）的数据表，
// 其余文化回退 Invariant（诚实子集，后置扩展 CLDR 全量）。

namespace Arc.Globalization;

using Arc.Reflection;

/// <summary>数值格式模板——数值格式化随文化而异的字段集合。</summary>
public class NumberFormatInfo : IFormatProvider {
    // ── 数值模板字段（对齐 .NET 核心字段）──

    /// <summary>小数点分隔符（如 "." / ","）。</summary>
    public string NumberDecimalSeparator;

    /// <summary>整数组分隔符（如 "," / "." / " "）。</summary>
    public string NumberGroupSeparator;

    /// <summary>每组数字位数（如 3）。</summary>
    public int NumberGroupSizes;

    /// <summary>货币符号（如 "$" / "¤" / "¥" / "€"）。</summary>
    public string CurrencySymbol;

    /// <summary>货币小数点分隔符。</summary>
    public string CurrencyDecimalSeparator;

    /// <summary>货币整数组分隔符。</summary>
    public string CurrencyGroupSeparator;

    /// <summary>百分号符号（如 "%"）。</summary>
    public string PercentSymbol;

    /// <summary>正号（如 "+"）。</summary>
    public string PositiveSign;

    /// <summary>负号（如 "-"）。</summary>
    public string NegativeSign;

    /// <summary>默认小数位数（N/F 默认，如 2）。</summary>
    public int NumberDecimalDigits;

    /// <summary>默认货币小数位数（如 2）。</summary>
    public int CurrencyDecimalDigits;

    /// <summary>默认百分比小数位数（如 2）。</summary>
    public int PercentDecimalDigits;

    /// <summary>百分比小数点分隔符。</summary>
    public string PercentDecimalSeparator;

    /// <summary>百分比整数组分隔符。</summary>
    public string PercentGroupSeparator;

    // ── 正负模式（对齐 .NET 数值格式模式；v2 补齐半成品）──
    // C# 模式约定：CurrencyPositivePattern 0=¤n 1=n¤ 2=¤ n 3=n ¤；
    // CurrencyNegativePattern 0=(¤n) 1=-¤n 2=¤-n 3=¤n- 4=(n¤) 5=-n¤ 6=n-¤ 7=n¤-
    // 8=-n ¤ 9=-¤ n 10=n ¤- 11=¤ n- 12=¤ -n 13=n- ¤ 14=(¤ n) 15=(n ¤)；
    // NumberNegativePattern 0=(n) 1=-n 2=- n 3=n- 4=n -；
    // PercentPositivePattern 0=n % 1=n% 2=%n 3=% n；PercentNegativePattern 0=-n % 1=-n% 2=-%n 3=%-n 4=% -n 5=n %- 6=n%- 7=-% n 8=n- % 9=n-% 10=% n- 11=%n- 12=n %- 13=n%- 14=%-n 15=%n-。

    /// <summary>货币正数模式（0=¤n … 3=n ¤）。</summary>
    public int CurrencyPositivePattern;

    /// <summary>货币负数模式（0…15）。</summary>
    public int CurrencyNegativePattern;

    /// <summary>数值负数模式（0=(n) 1=-n …）。</summary>
    public int NumberNegativePattern;

    /// <summary>百分比正数模式（0=n % … 3=% n）。</summary>
    public int PercentPositivePattern;

    /// <summary>百分比负数模式（0=-n % … 15=%n-）。</summary>
    public int PercentNegativePattern;

    // ── 构造器 ──

    /// <summary>创建数值格式模板。</summary>
    public NumberFormatInfo() {
        _setInvariant();
    }

    // ── IFormatProvider ──

    /// <summary>返回自身（NumberFormatInfo 即数值格式提供者）。</summary>
    public object GetFormat(Type formatType) {
        if (formatType == null) {
            return null;
        }
        // 仅当请求类型是 NumberFormatInfo 时返回自身；否则 null。
        if (formatType.TypeId == typeof(NumberFormatInfo).TypeId) {
            return this;
        }
        return null;
    }

    // ── 静态模板（static readonly 惰性：首触构造一次、线程安全）──

    private static readonly NumberFormatInfo _zhCN = _makeZhCN();
    private static readonly NumberFormatInfo _enUS = _makeEnUS();
    private static readonly NumberFormatInfo _deDE = _makeDeDE();
    private static readonly NumberFormatInfo _frFR = _makeFrFR();

    /// <summary>固定文化（Invariant）数值格式模板。</summary>
    public static readonly NumberFormatInfo InvariantInfo = new NumberFormatInfo();

    /// <summary>按文化标签获取数值格式模板；未知文化回退 Invariant。</summary>
    /// <param name="name">BCP 47 文化标签（如 "zh-CN"、"en-US"）。</param>
    public static NumberFormatInfo GetInstance(string name) {
        if (name == null) {
            name = "";
        }
        switch (name)
        {
            case "zh-CN":
            {
                return _zhCN;
            }
            case "en-US":
            {
                return _enUS;
            }
            case "de-DE":
            {
                return _deDE;
            }
            case "fr-FR":
            {
                return _frFR;
            }
            default:
            {
                return NumberFormatInfo.InvariantInfo;
            }
        }
    }

    // ── 模板构造（诚实子集：zh-CN / en-US / de-DE / fr-FR）──

    private void _setInvariant() {
        this.NumberDecimalSeparator = ".";
        this.NumberGroupSeparator = ",";
        this.NumberGroupSizes = 3;
        this.CurrencySymbol = "\u00A4"; // ¤
        this.CurrencyDecimalSeparator = ".";
        this.CurrencyGroupSeparator = ",";
        this.PercentSymbol = "%";
        this.PositiveSign = "+";
        this.NegativeSign = "-";
        this.NumberDecimalDigits = 2;
        this.CurrencyDecimalDigits = 2;
        this.PercentDecimalDigits = 2;
        this.PercentDecimalSeparator = ".";
        this.PercentGroupSeparator = ",";
        this.CurrencyPositivePattern = 0;
        this.CurrencyNegativePattern = 0;
        this.NumberNegativePattern = 1;
        this.PercentPositivePattern = 0;
        this.PercentNegativePattern = 0;
    }

    private static NumberFormatInfo _makeEnUS() {
        NumberFormatInfo n = new NumberFormatInfo();
        n._setInvariant();
        n.CurrencySymbol = "$";
        return n;
    }

    private static NumberFormatInfo _makeZhCN() {
        NumberFormatInfo n = new NumberFormatInfo();
        n._setInvariant();
        n.CurrencySymbol = "\u00A5"; // ¥
        n.CurrencyNegativePattern = 1; // -¥n
        return n;
    }

    private static NumberFormatInfo _makeDeDE() {
        NumberFormatInfo n = new NumberFormatInfo();
        n._setInvariant();
        // de-DE：小数点 `,`、组分 `.`、货币 €
        n.NumberDecimalSeparator = ",";
        n.NumberGroupSeparator = ".";
        n.CurrencyDecimalSeparator = ",";
        n.CurrencyGroupSeparator = ".";
        n.PercentDecimalSeparator = ",";
        n.PercentGroupSeparator = ".";
        n.CurrencySymbol = "\u20AC"; // €
        n.CurrencyPositivePattern = 3; // n €
        n.CurrencyNegativePattern = 8; // -n €
        return n;
    }

    private static NumberFormatInfo _makeFrFR() {
        NumberFormatInfo n = new NumberFormatInfo();
        n._setInvariant();
        // fr-FR：小数点 `,`、组分（窄不换行空格 U+202F）、货币 €
        n.NumberDecimalSeparator = ",";
        n.NumberGroupSeparator = "\u202F";
        n.CurrencyDecimalSeparator = ",";
        n.CurrencyGroupSeparator = "\u202F";
        n.PercentDecimalSeparator = ",";
        n.PercentGroupSeparator = "\u202F";
        n.CurrencySymbol = "\u20AC"; // €
        n.CurrencyPositivePattern = 3; // n €
        n.CurrencyNegativePattern = 8; // -n €
        return n;
    }
}