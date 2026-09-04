// RFC 027 M3: DateTime — aligns with C# System.DateTime.
//
// Internal storage: Ticks (100-nanosecond intervals since 0001-01-01T00:00:00)
// and _kind (0=Unspecified, 1=Utc, 2=Local).
namespace Arc;

using Arc.Globalization;

public struct DateTime {
    private int  _kind;

    // ── Ticks ──
    public long Ticks { get; }

    // ── Kind ──
    public int Kind { get { return _kind; } }

    // ── Constructor ──
    // Only one constructor: DateTime(long ticks). Use static From* methods for
    // other constructions — Arc structs don't support ctor overloading (LLVM
    // emit generates mismatched signatures for different ctor parameter counts).
    public DateTime(long ticks) {
        Ticks = ticks;
        _kind = 0;
    }

    // ── Static factory methods ──

    public static DateTime FromYMD(int year, int month, int day) {
        return new DateTime(DateTime._dateToTicks(year, month, day));
    }

    public static DateTime FromYMDHMS(int year, int month, int day, int hour, int minute, int second) {
        long dateTicks = DateTime._dateToTicks(year, month, day);
        double dH = hour;
        double dM = minute;
        double dS = second;
        double tH = dH * 36000000000.0;
        double tM = dM * 600000000.0;
        double tS = dS * 10000000.0;
        double sum = tH;
        sum = sum + tM;
        sum = sum + tS;
        long ticks = (long)sum;
        return new DateTime(dateTicks + ticks);
    }

    // ── Static properties / methods ──

    /// <summary>0001-01-01T00:00:00（ticks = 0）。</summary>
    public static DateTime MinValue {
        get { return new DateTime(0); }
    }

    /// <summary>9999-12-31T23:59:59.9999999（对齐 C# DateTime.MaxValue.Ticks）。</summary>
    public static DateTime MaxValue {
        get { return new DateTime(3155378975999999999); }
    }

    public static DateTime Now {
        get { long t = rt_resources.rt_os_now_ticks(); return new DateTime(t); }
    }

    public static DateTime UtcNow {
        get { long t = rt_resources.rt_os_now_utc_ticks(); return new DateTime(t); }
    }

    public static DateTime Today {
        get {
            long t = rt_resources.rt_os_now_ticks();
            long tod = t - (t / 864000000000) * 864000000000;
            return new DateTime(t - tod);
        }
    }

    /// <summary>设置 Kind（0=Unspecified, 1=Utc, 2=Local）；不改 ticks。无时区换算。</summary>
    public static DateTime SpecifyKind(DateTime value, int kind) {
        return DateTime.WithKind(value, kind);
    }

    // ── Date / TimeOfDay ──

    public DateTime Date {
        get {
            long tod = Ticks - (Ticks / 864000000000) * 864000000000;
            return new DateTime(Ticks - tod);
        }
    }

    public TimeSpan TimeOfDay {
        get { long d = Ticks - (Ticks / 864000000000) * 864000000000; return new TimeSpan(d); }
    }

    // ── Date parts (Gregorian calendar) ──

    public int Year {
        get { int y; int m; int d; DateTime.TicksToDate(Ticks, out y, out m, out d); return y; }
    }

    public int Month {
        get { int y; int m; int d; DateTime.TicksToDate(Ticks, out y, out m, out d); return m; }
    }

    public int Day {
        get { int y; int m; int d; DateTime.TicksToDate(Ticks, out y, out m, out d); return d; }
    }

    public int DayOfWeek {
        get { long days = Ticks / 864000000000; return (int)((days + 1) - ((days + 1) / 7) * 7); }
    }

    public int DayOfYear {
        get {
            int y; int m; int d;
            DateTime.TicksToDate(Ticks, out y, out m, out d);
            long jan1 = DateTime._dateToTicks(y, 1, 1);
            long diff = (Ticks - jan1) / 864000000000;
            return (int)diff + 1;
        }
    }

    // ── Time parts ──

    public int Hour {
        get {
            long ticksPerDay = 864000000000;
            long ticksPerHour = 36000000000;
            long tod = Ticks - (Ticks / ticksPerDay) * ticksPerDay;
            return (int)(tod / ticksPerHour);
        }
    }

    public int Minute {
        get {
            long ticksPerDay = 864000000000;
            long ticksPerHour = 36000000000;
            long ticksPerMinute = 600000000;
            long tod = Ticks - (Ticks / ticksPerDay) * ticksPerDay;
            long hh = tod / ticksPerHour;
            return (int)((tod / ticksPerMinute) - hh * 60);
        }
    }

    public int Second {
        get {
            long ticksPerDay = 864000000000;
            long ticksPerMinute = 600000000;
            long ticksPerSecond = 10000000;
            long tod = Ticks - (Ticks / ticksPerDay) * ticksPerDay;
            long mm = tod / ticksPerMinute;
            return (int)((tod / ticksPerSecond) - mm * 60);
        }
    }

    public int Millisecond {
        get {
            long ticksPerDay = 864000000000;
            long ticksPerSecond = 10000000;
            long ticksPerMs = 10000;
            long tod = Ticks - (Ticks / ticksPerDay) * ticksPerDay;
            long ss = tod / ticksPerSecond;
            return (int)((tod / ticksPerMs) - ss * 1000);
        }
    }

    // ── Add methods ──

    public DateTime Add(TimeSpan ts) {
        return new DateTime(Ticks + ts.Ticks);
    }

    public DateTime AddDays(double value) {
        long t = (long)(value * 864000000000);
        return new DateTime(Ticks + t);
    }

    public DateTime AddHours(double value) {
        long t = (long)(value * 36000000000);
        return new DateTime(Ticks + t);
    }

    public DateTime AddMinutes(double value) {
        long t = (long)(value * 600000000);
        return new DateTime(Ticks + t);
    }

    public DateTime AddSeconds(double value) {
        long t = (long)(value * 10000000);
        return new DateTime(Ticks + t);
    }

    public DateTime AddMilliseconds(double value) {
        long t = (long)(value * 10000);
        return new DateTime(Ticks + t);
    }

    public DateTime AddTicks(long value) {
        return new DateTime(Ticks + value);
    }

    public DateTime AddMonths(int months) {
        int y; int m; int d;
        DateTime.TicksToDate(Ticks, out y, out m, out d);
        int totalM = y * 12 + (m - 1) + months;
        int newY = totalM / 12;
        int newM = totalM - newY * 12 + 1;
        int maxD = DateTime._daysInMonth(newY, newM);
        if (d > maxD) { d = maxD; }
        long dateTicks = DateTime._dateToTicks(newY, newM, d);
        long tod = Ticks - (Ticks / 864000000000) * 864000000000;
        return new DateTime(dateTicks + tod);
    }

    public DateTime AddYears(int years) {
        int y; int m; int d;
        DateTime.TicksToDate(Ticks, out y, out m, out d);
        int newY = y + years;
        int maxD = DateTime._daysInMonth(newY, m);
        if (d > maxD) { d = maxD; }
        long dateTicks = DateTime._dateToTicks(newY, m, d);
        long tod = Ticks - (Ticks / 864000000000) * 864000000000;
        return new DateTime(dateTicks + tod);
    }

    // ── Subtract ──

    public TimeSpan Subtract(DateTime value) {
        return new TimeSpan(Ticks - value.Ticks);
    }

    public DateTime Subtract(TimeSpan value) {
        return new DateTime(Ticks - value.Ticks);
    }

    // ── ToString ──

    public string ToString() {
        int y; int m; int d;
        DateTime.TicksToDate(Ticks, out y, out m, out d);
        int h = this.Hour;
        int min = this.Minute;
        int s = this.Second;
        return DateTime._pad4(y) + "-" + DateTime._pad2(m) + "-" + DateTime._pad2(d)
             + " " + DateTime._pad2(h) + ":" + DateTime._pad2(min) + ":" + DateTime._pad2(s);
    }

    /// RFC 007 日期格式诚实子集：`yyyy`/`yy`/`MMMM`/`MMM`/`MM`/`M`/`dddd`/`ddd`/`dd`/
    /// `HH`/`hh`/`mm`/`ss`/`fff`/`tt`/`zzz` + 字面分隔符（`-`/`:`/`/`/` `/`.`/`T`/`,`）。
    /// 无参重载按 Invariant 文化（英文名称 + "AM"/"PM"）；`zzz` 固定 `+00:00`（无 TZ 偏移 API）。
    /// 未列举 token → `FormatException`。
    public string ToString(string format) {
        return this.ToStringCore(format, CultureInfo.InvariantCulture.DateTimeFormat);
    }

    /// RFC 027 M5 文化感知：按 provider 的 DateTimeFormatInfo 渲染（月/星期名、AM/PM 随文化）。
    /// provider 为 null、或 GetFormat 不支持日期模板时回退 CultureInfo.CurrentCulture。
    public string ToString(string format, IFormatProvider provider) {
        DateTimeFormatInfo d = DateTime._resolveDtfi(provider);
        return this.ToStringCore(format, d);
    }

    /// RFC 007 日期格式核心（文化感知名称表）。
    private string ToStringCore(string format, DateTimeFormatInfo d) {
        if (format == null || format == "") {
            return this.ToString();
        }
        int y; int m; int dd;
        DateTime.TicksToDate(Ticks, out y, out m, out dd);
        int h = this.Hour;
        int min = this.Minute;
        int s = this.Second;
        int ms = this.Millisecond;
        int dow = this.DayOfWeek;
        string result = "";
        int i = 0;
        bool go = true;
        while (go) {
            if (i >= format.Length) { go = false; }
            else {
                if (DateTime._fmtStarts(format, i, "yyyy")) {
                    result = result + DateTime._pad4(y);
                    i = i + 4;
                } else if (DateTime._fmtStarts(format, i, "MMMM")) {
                    result = result + d.GetMonthName(m);
                    i = i + 4;
                } else if (DateTime._fmtStarts(format, i, "dddd")) {
                    result = result + d.GetDayName(dow);
                    i = i + 4;
                } else if (DateTime._fmtStarts(format, i, "MMM")) {
                    result = result + d.GetAbbreviatedMonthName(m);
                    i = i + 3;
                } else if (DateTime._fmtStarts(format, i, "ddd")) {
                    result = result + d.GetAbbreviatedDayName(dow);
                    i = i + 3;
                } else if (DateTime._fmtStarts(format, i, "fff")) {
                    result = result + DateTime._pad3(ms);
                    i = i + 3;
                } else if (DateTime._fmtStarts(format, i, "zzz")) {
                    result = result + "+00:00";
                    i = i + 3;
                } else if (DateTime._fmtStarts(format, i, "yy")) {
                    result = result + DateTime._pad2(y - (y / 100) * 100);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "MM")) {
                    result = result + DateTime._pad2(m);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "dd")) {
                    result = result + DateTime._pad2(dd);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "HH")) {
                    result = result + DateTime._pad2(h);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "hh")) {
                    result = result + DateTime._pad2(DateTime._hour12(h));
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "mm")) {
                    result = result + DateTime._pad2(min);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "ss")) {
                    result = result + DateTime._pad2(s);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "tt")) {
                    result = result + DateTime._amPm(h, d);
                    i = i + 2;
                } else if (DateTime._fmtStarts(format, i, "M")) {
                    result = result + DateTime._pad(m, 1);
                    i = i + 1;
                } else if (DateTime._fmtIsSep(format, i)) {
                    result = result + format.Substring(i, 1);
                    i = i + 1;
                } else {
                    throw new FormatException(
                        "Unsupported DateTime format token (RFC 007 honest subset): "
                        + format.Substring(i, 1));
                }
            }
        }
        return result;
    }

    // ── Static helpers ──

    // ── Parse（C# 常用子集；失败抛 FormatException；无效不再静默回 0）──
    //
    // Supported formats:
    //   yyyy-MM-dd                     (ISO date)
    //   yyyy-MM-ddTHH:mm:ss           (ISO datetime)
    //   yyyy-MM-ddTHH:mm:ss.fff       (ISO with ms)
    //   yyyy-MM-dd HH:mm:ss           (space-separated)
    //   yyyy/MM/dd                     (slash date)
    //   yyyy/MM/dd HH:mm:ss           (slash datetime)
    // 后置：ParseExact / 文化 / 时区偏移 / 宽松自由格式。

    public static DateTime Parse(string s) {
        DateTime dt;
        if (!DateTime.TryParse(s, out dt)) {
            throw new FormatException("DateTime.Parse: invalid DateTime string");
        }
        return dt;
    }

    public static bool TryParse(string s, out DateTime result) {
        result = new DateTime(0);
        if (s == null || s == "") { return false; }
        if (s.Length < 10) { return false; }

        int y = 0; int m = 0; int d = 0;
        int h = 0; int min = 0; int sec = 0; int ms = 0;

        string sep0 = s.Substring(4, 1);
        string sep1 = s.Substring(7, 1);
        if (!(sep0 == "-" || sep0 == "/")) { return false; }
        if (sep1 != sep0) { return false; }
        if (!DateTime.TryParseDigits(s, 0, 4, out y)) { return false; }
        if (!DateTime.TryParseDigits(s, 5, 2, out m)) { return false; }
        if (!DateTime.TryParseDigits(s, 8, 2, out d)) { return false; }
        if (y < 1 || m < 1 || m > 12 || d < 1) { return false; }
        int dim = DateTime._daysInMonth(y, m);
        if (d > dim) { return false; }

        if (s.Length >= 19) {
            string sep = s.Substring(10, 1);
            if (sep == "T" || sep == " ") {
                if (!DateTime.TryParseDigits(s, 11, 2, out h)) { return false; }
                if (s.Substring(13, 1) != ":") { return false; }
                if (!DateTime.TryParseDigits(s, 14, 2, out min)) { return false; }
                if (s.Substring(16, 1) != ":") { return false; }
                if (!DateTime.TryParseDigits(s, 17, 2, out sec)) { return false; }
                if (h > 23 || min > 59 || sec > 59) { return false; }
                if (s.Length >= 23 && s.Substring(19, 1) == ".") {
                    if (!DateTime.TryParseDigits(s, 20, 3, out ms)) { return false; }
                }
            } else if (s.Length > 10) {
                return false;
            }
        } else if (s.Length > 10) {
            return false;
        }

        if (h == 0 && min == 0 && sec == 0 && ms == 0 && s.Length <= 10) {
            result = DateTime.FromYMD(y, m, d);
            return true;
        }
        long dateTicks = DateTime._dateToTicks(y, m, d);
        double dH = h;
        double dM = min;
        double dS = sec;
        double dMs = ms;
        double sum = dH * 36000000000.0;
        sum = sum + dM * 600000000.0;
        sum = sum + dS * 10000000.0;
        sum = sum + dMs * 10000.0;
        long ticks = (long)sum;
        result = new DateTime(dateTicks + ticks);
        return true;
    }

    public static int Compare(DateTime a, DateTime b) {
        if (a.Ticks == b.Ticks) { return 0; }
        if (a.Ticks > b.Ticks) { return 1; }
        return -1;
    }

    public static bool Equals(DateTime a, DateTime b) {
        return a.Ticks == b.Ticks;
    }

    public static int DaysInMonth(int year, int month) {
        return DateTime._daysInMonth(year, month);
    }

    public static bool IsLeapYear(int year) {
        if (year - (year / 4) * 4 != 0) { return false; }
        if (year - (year / 100) * 100 != 0) { return true; }
        return year - (year / 400) * 400 == 0;
    }

    // ── Private helpers ──

    private static DateTime WithKind(DateTime dt, int kind) {
        dt._kind = kind;
        return dt;
    }

    private static bool TryParseDigits(string s, int start, int len, out int value) {
        value = 0;
        if (start + len > s.Length) { return false; }
        int r = 0;
        int i = start;
        int end = start + len;
        bool go = true;
        while (go) {
            if (i >= end) { go = false; }
            else {
                int d = -1;
                string ch = s.Substring(i, 1);
                if (ch == "0") { d = 0; } else if (ch == "1") { d = 1; } else if (ch == "2") { d = 2; }
                else if (ch == "3") { d = 3; } else if (ch == "4") { d = 4; } else if (ch == "5") { d = 5; }
                else if (ch == "6") { d = 6; } else if (ch == "7") { d = 7; } else if (ch == "8") { d = 8; }
                else if (ch == "9") { d = 9; }
                if (d < 0) { return false; }
                r = r * 10 + d;
                i = i + 1;
            }
        }
        value = r;
        return true;
    }

    private static void TicksToDate(long ticks, out int year, out int month, out int day) {
        // Days since 0001-01-01
        long totalDays = ticks / 864000000000;

        // 400-year cycle = 146097 days
        int n400 = (int)(totalDays / 146097);
        int rem = (int)(totalDays - n400 * 146097);

        // 100-year cycle = 36524 days
        int n100 = rem / 36524;
        if (n100 == 4) { n100 = 3; }
        rem = rem - n100 * 36524;

        // 4-year cycle = 1461 days
        int n4 = rem / 1461;
        rem = rem - n4 * 1461;

        // 1-year = 365 days
        int n1 = rem / 365;
        if (n1 == 4) { n1 = 3; }
        rem = rem - n1 * 365;

        year = n400 * 400 + n100 * 100 + n4 * 4 + n1 + 1;

        // rem = 0-based day of year
        bool leap = DateTime.IsLeapYear(year);
        // 不用 md[1]=29 就地改写（数组元素赋值当前不可靠）；二月天数按 leap 分支。
        int[] md = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

        month = 1;
        int i2 = 0;
        bool go = true;
        while (go) {
            if (i2 >= 12) { go = false; }
            else {
                int dim = md[i2];
                if (i2 == 1) {
                    if (leap) { dim = 29; }
                }
                if (rem < dim) { go = false; }
                else {
                    rem = rem - dim;
                    month = month + 1;
                    i2 = i2 + 1;
                }
            }
        }
        day = rem + 1;
    }

    private static long _dateToTicks(int year, int month, int day) {
        int y = year - 1;
        long days = (long)y * 365 + y / 4 - y / 100 + y / 400;
        int[] md = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        days = days + md[month - 1];
        if (month > 2) {
            if (DateTime.IsLeapYear(year)) {
                days = days + 1;
            }
        }
        days = days + day - 1;
        return days * 864000000000;
    }

    private static int _daysInMonth(int year, int month) {
        int[] md = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        if (month == 2) {
            if (DateTime.IsLeapYear(year)) { return 29; }
        }
        return md[month - 1];
    }

    private static string _pad4(int v) { return DateTime._pad(v, 4); }
    private static string _pad3(int v) { return DateTime._pad(v, 3); }
    private static string _pad2(int v) { return DateTime._pad(v, 2); }

    private static bool _fmtStarts(string format, int i, string token) {
        int n = token.Length;
        if (i + n > format.Length) { return false; }
        return format.Substring(i, n) == token;
    }

    private static bool _fmtIsSep(string format, int i) {
        string ch = format.Substring(i, 1);
        return ch == "-" || ch == ":" || ch == "/" || ch == " " || ch == "." || ch == "T" || ch == ",";
    }

    private static int _hour12(int h) {
        int h12 = h - (h / 12) * 12;
        if (h12 == 0) { return 12; }
        return h12;
    }

    private static string _amPm(int h, DateTimeFormatInfo d) {
        if (h < 12) { return d.AMDesignator; }
        return d.PMDesignator;
    }

    /// RFC 027 M5：从 provider 解析 DateTimeFormatInfo；null / 不支持时回退 CurrentCulture。
    private static DateTimeFormatInfo _resolveDtfi(IFormatProvider provider) {
        if (provider != null) {
            object o = provider.GetFormat(typeof(DateTimeFormatInfo));
            if (o != null) {
                return (DateTimeFormatInfo)o;
            }
        }
        return CultureInfo.CurrentCulture.DateTimeFormat;
    }

    private static string _pad(int v, int w) {
        string s = "";
        int n = v;
        if (n == 0) { s = "0"; }
        while (n > 0) {
            int d = n - (n / 10) * 10;
            if (d == 0) { s = "0" + s; }
            else if (d == 1) { s = "1" + s; }
            else if (d == 2) { s = "2" + s; }
            else if (d == 3) { s = "3" + s; }
            else if (d == 4) { s = "4" + s; }
            else if (d == 5) { s = "5" + s; }
            else if (d == 6) { s = "6" + s; }
            else if (d == 7) { s = "7" + s; }
            else if (d == 8) { s = "8" + s; }
            else if (d == 9) { s = "9" + s; }
            n = n / 10;
        }
        while (s.Length < w) { s = "0" + s; }
        return s;
    }
}

