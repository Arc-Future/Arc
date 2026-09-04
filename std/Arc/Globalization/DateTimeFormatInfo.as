// RFC 027 M5: 文化感知格式化 — 日期时间格式模板 DateTimeFormatInfo。
//
// 对标 C# System.Globalization.DateTimeFormatInfo。
//
// 定义日期时间格式化随文化而异的模式串（短/长日期、短/长时间、月份/星期名等），
// 为 `IFormattable.ToString(format, provider)` 与插值文化感知化提供数据基础
// （插值接入为后续独立事项）。
//
// 提供 Invariant 与常用文化模板（zh-CN / en-US / de-DE / fr-FR）的数据表，
// 其余文化回退 Invariant（诚实子集，后置扩展 CLDR 全量）。
//
// v2（2026-08-07，RFC 034 M5 补齐半成品）：名称表由「仅 1 月/星期日起始」扩展为
// 完整 12×7 月/星期名 + 缩写（MonthNames / AbbreviatedMonthNames 12 项、
// DayNames / AbbreviatedDayNames 7 项），并新增 GetMonthName / GetDayName 等
// 访问器供 `DateTime.ToString(format, provider)` 文化感知化消费。

namespace Arc.Globalization;

using Arc.Reflection;

/// <summary>日期时间格式模板——日期时间格式化随文化而异的模式集合。</summary>
public class DateTimeFormatInfo : IFormatProvider {
    // ── 日期/时间模式（对齐 .NET 标准模式）──

    /// <summary>短日期模式（如 "yyyy-MM-dd"）。</summary>
    public string ShortDatePattern;

    /// <summary>长日期模式（如 "dddd, MMMM d, yyyy"）。</summary>
    public string LongDatePattern;

    /// <summary>短时间模式（如 "HH:mm"）。</summary>
    public string ShortTimePattern;

    /// <summary>长时间模式（如 "HH:mm:ss"）。</summary>
    public string LongTimePattern;

    /// <summary>完整日期时间短时间模式（如 "yyyy-MM-dd HH:mm"）。</summary>
    public string FullDateTimePattern;

    /// <summary>年/月模式（如 "yyyy MMMM"）。</summary>
    public string YearMonthPattern;

    /// <summary>月/日模式（如 "MMMM d"）。</summary>
    public string MonthDayPattern;

    // ── 名称表（v2 完整 12×7：MonthNames[12] 一月起、DayNames[7] 星期日起）──

    /// <summary>完整月份名（12 项；下标 0 = 一月 … 11 = 十二月）。</summary>
    public string[] MonthNames;

    /// <summary>完整月份缩写（12 项；下标对齐 MonthNames）。</summary>
    public string[] AbbreviatedMonthNames;

    /// <summary>完整星期名（7 项；下标 0 = 星期日 … 6 = 星期六，对齐 DayOfWeek）。</summary>
    public string[] DayNames;

    /// <summary>完整星期缩写（7 项；下标对齐 DayNames）。</summary>
    public string[] AbbreviatedDayNames;

    /// <summary>AM 指示符（如 "AM" / "上午"）。</summary>
    public string AMDesignator;

    /// <summary>PM 指示符（如 "PM" / "下午"）。</summary>
    public string PMDesignator;

    // ── 构造器 ──

    /// <summary>创建日期时间格式模板。</summary>
    public DateTimeFormatInfo() {
        _setInvariant();
    }

    // ── 名称访问器 ──

    /// <summary>按月份（1=一月 … 12=十二月）返回完整月份名；越界返回空串。</summary>
    public string GetMonthName(int month) {
        if (month < 1 || month > 12) { return ""; }
        string[] names = this.MonthNames;
        return names[month - 1];
    }

    /// <summary>按月份返回月份缩写；越界返回空串。</summary>
    public string GetAbbreviatedMonthName(int month) {
        if (month < 1 || month > 12) { return ""; }
        string[] names = this.AbbreviatedMonthNames;
        return names[month - 1];
    }

    /// <summary>按 DayOfWeek（0=星期日 … 6=星期六）返回完整星期名；越界返回空串。</summary>
    public string GetDayName(int dayOfWeek) {
        if (dayOfWeek < 0 || dayOfWeek > 6) { return ""; }
        string[] names = this.DayNames;
        return names[dayOfWeek];
    }

    /// <summary>按 DayOfWeek 返回星期缩写；越界返回空串。</summary>
    public string GetAbbreviatedDayName(int dayOfWeek) {
        if (dayOfWeek < 0 || dayOfWeek > 6) { return ""; }
        string[] names = this.AbbreviatedDayNames;
        return names[dayOfWeek];
    }

    // ── IFormatProvider ──

    /// <summary>返回自身（DateTimeFormatInfo 即日期格式提供者）。</summary>
    public object GetFormat(Type formatType) {
        if (formatType == null) {
            return null;
        }
        if (formatType.TypeId == typeof(DateTimeFormatInfo).TypeId) {
            return this;
        }
        return null;
    }

    // ── 静态模板（static readonly 惰性：首触构造一次、线程安全）──

    private static readonly DateTimeFormatInfo _zhCN = _makeZhCN();
    private static readonly DateTimeFormatInfo _enUS = _makeEnUS();
    private static readonly DateTimeFormatInfo _deDE = _makeDeDE();
    private static readonly DateTimeFormatInfo _frFR = _makeFrFR();

    /// <summary>固定文化（Invariant）日期时间格式模板。</summary>
    public static readonly DateTimeFormatInfo InvariantInfo = new DateTimeFormatInfo();

    /// <summary>按文化标签获取日期时间格式模板；未知文化回退 Invariant。</summary>
    /// <param name="name">BCP 47 文化标签（如 "zh-CN"、"en-US"）。</param>
    public static DateTimeFormatInfo GetInstance(string name) {
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
                return DateTimeFormatInfo.InvariantInfo;
            }
        }
    }

    // ── 模板构造（诚实子集：zh-CN / en-US / de-DE / fr-FR）──

    private void _setInvariant() {
        this.ShortDatePattern = "yyyy-MM-dd";
        this.LongDatePattern = "dddd, MMMM d, yyyy";
        this.ShortTimePattern = "HH:mm";
        this.LongTimePattern = "HH:mm:ss";
        this.FullDateTimePattern = "yyyy-MM-dd HH:mm:ss";
        this.YearMonthPattern = "yyyy MMMM";
        this.MonthDayPattern = "MMMM d";
        this.MonthNames = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December"
        ];
        this.AbbreviatedMonthNames = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun",
            "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"
        ];
        this.DayNames = [
            "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"
        ];
        this.AbbreviatedDayNames = [
            "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"
        ];
        this.AMDesignator = "AM";
        this.PMDesignator = "PM";
    }

    private static DateTimeFormatInfo _makeEnUS() {
        DateTimeFormatInfo d = new DateTimeFormatInfo();
        // en-US：月/日/年 + 12 小时制 AM/PM（名称沿用 Invariant 英文）
        d.ShortDatePattern = "M/d/yyyy";
        d.LongDatePattern = "dddd, MMMM d, yyyy";
        d.ShortTimePattern = "h:mm tt";
        d.LongTimePattern = "h:mm:ss tt";
        d.FullDateTimePattern = "dddd, MMMM d, yyyy h:mm:ss tt";
        return d;
    }

    private static DateTimeFormatInfo _makeZhCN() {
        DateTimeFormatInfo d = new DateTimeFormatInfo();
        // zh-CN：年/月/日 + 24 小时制 + 中文名称
        d.ShortDatePattern = "yyyy/M/d";
        d.LongDatePattern = "yyyy\u5E74M\u6708d\u65E5"; // yyyy年M月d日
        d.ShortTimePattern = "H:mm";
        d.LongTimePattern = "H:mm:ss";
        d.MonthNames = [
            "\u4E00\u6708", "\u4E8C\u6708", "\u4E09\u6708", "\u56DB\u6708",
            "\u4E94\u6708", "\u516D\u6708", "\u4E03\u6708", "\u516B\u6708",
            "\u4E5D\u6708", "\u5341\u6708", "\u5341\u4E00\u6708", "\u5341\u4E8C\u6708"
        ];
        d.AbbreviatedMonthNames = d.MonthNames;
        d.DayNames = [
            "\u661F\u671F\u65E5", "\u661F\u671F\u4E00", "\u661F\u671F\u4E8C",
            "\u661F\u671F\u4E09", "\u661F\u671F\u56DB", "\u661F\u671F\u4E94",
            "\u661F\u671F\u516D"
        ];
        d.AbbreviatedDayNames = [
            "\u5468\u65E5", "\u5468\u4E00", "\u5468\u4E8C",
            "\u5468\u4E09", "\u5468\u56DB", "\u5468\u4E94", "\u5468\u516D"
        ];
        d.AMDesignator = "\u4E0A\u5348"; // 上午
        d.PMDesignator = "\u4E0B\u5348"; // 下午
        return d;
    }

    private static DateTimeFormatInfo _makeDeDE() {
        DateTimeFormatInfo d = new DateTimeFormatInfo();
        // de-DE：日.月.年 + 24 小时制 + 德语名称
        d.ShortDatePattern = "dd.MM.yyyy";
        d.LongDatePattern = "dddd, d. MMMM yyyy";
        d.ShortTimePattern = "HH:mm";
        d.LongTimePattern = "HH:mm:ss";
        d.MonthNames = [
            "Januar", "Februar", "M\u00E4rz", "April", "Mai", "Juni",
            "Juli", "August", "September", "Oktober", "November", "Dezember"
        ];
        d.AbbreviatedMonthNames = [
            "Jan.", "Feb.", "M\u00E4rz", "Apr.", "Mai", "Juni",
            "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."
        ];
        d.DayNames = [
            "Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"
        ];
        d.AbbreviatedDayNames = [
            "So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."
        ];
        return d;
    }

    private static DateTimeFormatInfo _makeFrFR() {
        DateTimeFormatInfo d = new DateTimeFormatInfo();
        // fr-FR：日/月/年 + 24 小时制 + 法语名称
        d.ShortDatePattern = "dd/MM/yyyy";
        d.LongDatePattern = "dddd d MMMM yyyy";
        d.ShortTimePattern = "HH:mm";
        d.LongTimePattern = "HH:mm:ss";
        d.MonthNames = [
            "janvier", "f\u00E9vrier", "mars", "avril", "mai", "juin",
            "juillet", "ao\u00FBt", "septembre", "octobre", "novembre", "d\u00E9cembre"
        ];
        d.AbbreviatedMonthNames = [
            "janv.", "f\u00E9vr.", "mars", "avr.", "mai", "juin",
            "juil.", "ao\u00FBt", "sept.", "oct.", "nov.", "d\u00E9c."
        ];
        d.DayNames = [
            "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"
        ];
        d.AbbreviatedDayNames = [
            "dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."
        ];
        return d;
    }
}
