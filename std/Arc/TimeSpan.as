// RFC 027 M3: TimeSpan — aligns with C# System.TimeSpan.
//
// Internal storage: Ticks (100-nanosecond intervals).
// TicksPerMillisecond = 10000, TicksPerSecond = 10000000,
// TicksPerMinute = 600000000, TicksPerHour = 36000000000,
// TicksPerDay = 864000000000.
namespace Arc;

public struct TimeSpan {

    // ── Ticks ──
    public long Ticks { get; }

    // ── Constructors ──

    public TimeSpan(long ticks) {
        Ticks = ticks;
    }

    public TimeSpan(int hours, int minutes, int seconds) {
        Ticks = (long)hours * 36000000000 + (long)minutes * 600000000 + (long)seconds * 10000000;
    }

    public TimeSpan(int days, int hours, int minutes, int seconds) {
        Ticks = (long)days * 864000000000 + (long)hours * 36000000000
                    + (long)minutes * 600000000 + (long)seconds * 10000000;
    }

    public TimeSpan(int days, int hours, int minutes, int seconds, int ms) {
        Ticks = (long)days * 864000000000 + (long)hours * 36000000000
                    + (long)minutes * 600000000 + (long)seconds * 10000000
                    + (long)ms * 10000;
    }

    // ── Static factories ──

    public static TimeSpan FromDays(double value) {
        return new TimeSpan((long)(value * 864000000000));
    }

    public static TimeSpan FromHours(double value) {
        return new TimeSpan((long)(value * 36000000000));
    }

    public static TimeSpan FromMinutes(double value) {
        return new TimeSpan((long)(value * 600000000));
    }

    public static TimeSpan FromSeconds(double value) {
        return new TimeSpan((long)(value * 10000000));
    }

    public static TimeSpan FromMilliseconds(double value) {
        return new TimeSpan((long)(value * 10000));
    }

    public static TimeSpan FromTicks(long value) {
        return new TimeSpan(value);
    }

    // ── Constants ──

    public static TimeSpan Zero {
        get { return new TimeSpan(0); }
    }

    public static TimeSpan MinValue {
        get { long v = 0; v = v - 9223372036854775807; v = v - 1; return new TimeSpan(v); }
    }

    public static TimeSpan MaxValue {
        get { return new TimeSpan(9223372036854775807); }
    }

    // ── Component properties ──

    public int Days {
        get { return (int)(Ticks / 864000000000); }
    }

    public int Hours {
        get { long d = Ticks - (Ticks / 864000000000) * 864000000000; return (int)(d / 36000000000); }
    }

    public int Minutes {
        get { long d = Ticks - (Ticks / 36000000000) * 36000000000; return (int)(d / 600000000); }
    }

    public int Seconds {
        get { long d = Ticks - (Ticks / 600000000) * 600000000; return (int)(d / 10000000); }
    }

    public int Milliseconds {
        get { long d = Ticks - (Ticks / 10000000) * 10000000; return (int)(d / 10000); }
    }

    // ── Total properties ──

    public double TotalDays {
        get { return (double)Ticks / 864000000000; }
    }

    public double TotalHours {
        get { return (double)Ticks / 36000000000; }
    }

    public double TotalMinutes {
        get { return (double)Ticks / 600000000; }
    }

    public double TotalSeconds {
        get { return (double)Ticks / 10000000; }
    }

    public double TotalMilliseconds {
        get { return (double)Ticks / 10000; }
    }

    // ── Arithmetic ──

    public TimeSpan Add(TimeSpan ts) {
        return new TimeSpan(Ticks + ts.Ticks);
    }

    public TimeSpan Subtract(TimeSpan ts) {
        return new TimeSpan(Ticks - ts.Ticks);
    }

    public TimeSpan Duration() {
        if (Ticks >= 0) { return this; }
        return new TimeSpan(-Ticks);
    }

    public TimeSpan Negate() {
        return new TimeSpan(-Ticks);
    }

    // ── Comparison ──

    public static int Compare(TimeSpan a, TimeSpan b) {
        if (a.Ticks == b.Ticks) { return 0; }
        if (a.Ticks > b.Ticks) { return 1; }
        return -1;
    }

    public static bool Equals(TimeSpan a, TimeSpan b) {
        return a.Ticks == b.Ticks;
    }

    // ── Parse（诚实子集：与 ToString 往返；[-][d.]hh:mm:ss[.fff]；纯天数整数）──
    // 后置：自定义 format / culture / ParseExact / hh:mm（无秒）/ 七日格式。

    public static TimeSpan Parse(string s) {
        TimeSpan ts;
        if (!TimeSpan.TryParse(s, out ts)) {
            throw new FormatException("TimeSpan.Parse: invalid TimeSpan string");
        }
        return ts;
    }

    public static bool TryParse(string s, out TimeSpan result) {
        result = TimeSpan.Zero;
        if (s == null || s == "") { return false; }
        string t = s;
        bool neg = false;
        if (t.Substring(0, 1) == "-") {
            neg = true;
            t = t.Substring(1, t.Length - 1);
            if (t.Length == 0) { return false; }
        }
        int days = 0;
        int hours = 0;
        int minutes = 0;
        int seconds = 0;
        int ms = 0;
        // Optional days prefix: "d."
        int dot = TimeSpan.IndexOf(t, ".");
        int colon1 = TimeSpan.IndexOf(t, ":");
        // Pure day integer (C# TimeSpan.Parse("3") → 3.00:00:00)；无 ':' 且无 '.'。
        if (colon1 < 0 && dot < 0) {
            int dayOnly = 0;
            if (!TimeSpan.TryParseInt(t, out dayOnly)) { return false; }
            long dayTicks = (long)dayOnly * 864000000000;
            if (neg) { dayTicks = -dayTicks; }
            result = new TimeSpan(dayTicks);
            return true;
        }
        if (colon1 < 0) { return false; }
        string timePart = t;
        if (dot >= 0 && (colon1 < 0 || dot < colon1)) {
            // days.hh:mm:ss[.fff] — first '.' before first ':' is day separator
            string dayStr = t.Substring(0, dot);
            if (!TimeSpan.TryParseInt(dayStr, out days)) { return false; }
            timePart = t.Substring(dot + 1, t.Length - (dot + 1));
        }
        // timePart = hh:mm:ss[.fff]
        int c1 = TimeSpan.IndexOf(timePart, ":");
        if (c1 < 0) { return false; }
        string rest = timePart.Substring(c1 + 1, timePart.Length - (c1 + 1));
        int c2 = TimeSpan.IndexOf(rest, ":");
        if (c2 < 0) { return false; }
        string hStr = timePart.Substring(0, c1);
        string mStr = rest.Substring(0, c2);
        string secPart = rest.Substring(c2 + 1, rest.Length - (c2 + 1));
        if (!TimeSpan.TryParseInt(hStr, out hours)) { return false; }
        if (!TimeSpan.TryParseInt(mStr, out minutes)) { return false; }
        int secDot = TimeSpan.IndexOf(secPart, ".");
        if (secDot < 0) {
            if (!TimeSpan.TryParseInt(secPart, out seconds)) { return false; }
        } else {
            string sStr = secPart.Substring(0, secDot);
            string frac = secPart.Substring(secDot + 1, secPart.Length - (secDot + 1));
            if (!TimeSpan.TryParseInt(sStr, out seconds)) { return false; }
            // Up to 3 digit ms (ToString emits ms); pad/truncate to 3
            if (frac.Length == 0) { return false; }
            if (frac.Length == 1) { frac = frac + "00"; }
            else if (frac.Length == 2) { frac = frac + "0"; }
            else if (frac.Length > 3) { frac = frac.Substring(0, 3); }
            if (!TimeSpan.TryParseInt(frac, out ms)) { return false; }
        }
        long ticks = (long)days * 864000000000 + (long)hours * 36000000000
                   + (long)minutes * 600000000 + (long)seconds * 10000000
                   + (long)ms * 10000;
        if (neg) { ticks = -ticks; }
        result = new TimeSpan(ticks);
        return true;
    }

    private static int IndexOf(string s, string ch) {
        int i = 0;
        bool go = true;
        while (go) {
            if (i >= s.Length) { return -1; }
            if (s.Substring(i, 1) == ch) { return i; }
            i = i + 1;
        }
        return -1;
    }

    private static bool TryParseInt(string s, out int value) {
        value = 0;
        if (s == null || s == "") { return false; }
        int r = 0;
        int i = 0;
        bool go = true;
        while (go) {
            if (i >= s.Length) { go = false; }
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

    // ── ToString ──

    public string ToString() {
        string s = "";
        if (Ticks < 0) { s = "-"; }
        long t = Ticks;
        if (t < 0) { t = -t; }
        long d = t / 864000000000;
        long h = (t - d * 864000000000) / 36000000000;
        long m = (t - (t / 36000000000) * 36000000000) / 600000000;
        long sec = (t - (t / 600000000) * 600000000) / 10000000;
        long ms = (t - (t / 10000000) * 10000000) / 10000;
        string num = "";
        if (d > 0) { num = num + d + "."; }
        if (h < 10) { num = num + "0"; }
        num = num + h + ":";
        if (m < 10) { num = num + "0"; }
        num = num + m + ":";
        if (sec < 10) { num = num + "0"; }
        num = num + sec;
        if (ms > 0) {
            num = num + ".";
            if (ms < 100) { num = num + "0"; }
            if (ms < 10) { num = num + "0"; }
            num = num + ms;
        }
        return s + num;
    }
}
