// RFC 027 M4: Version -- immutable version number type.
//
// Aligns with C# System.Version.
// Stored as Major.Minor.Build.Revision (four ints, -1 = unspecified).
// Immutable: all fields private.
// Supports parsing from "1.2.3.4" strings and comparison.
// Pure Arc implementation.

namespace Arc;

public struct Version {

    public int Major    { get; }
    public int Minor    { get; }
    public int Build    { get; }
    public int Revision { get; }

    public Version(int major, int minor, int build, int revision) {
        Major = major;
        Minor = minor;
        Build = build;
        Revision = revision;
    }

    public Version(int major, int minor) {
        Major = major;
        Minor = minor;
        Build = -1;
        Revision = -1;
    }

    public static Version Parse(string v) {
        if (v == null) { throw new FormatException("Version.Parse: null"); }
        if (v == "") { throw new FormatException("Version.Parse: empty"); }
        string[] parts = v.Split(".");
        int count = parts.Length;
        if (count < 1 || count > 4) { throw new FormatException("Version.Parse: invalid segment count"); }
        int ma = 0; int mi = 0; int bu = -1; int re = -1;
        if (count >= 1) { ma = Version.ParsePart(parts[0]); }
        if (count >= 2) { mi = Version.ParsePart(parts[1]); }
        if (count >= 3) { bu = Version.ParsePart(parts[2]); }
        if (count >= 4) { re = Version.ParsePart(parts[3]); }
        return new Version(ma, mi, bu, re);
    }

    /// <summary>宽松解析版本号；失败返回 false 且 result 为 (0,0)。</summary>
    public static bool TryParse(string v, out Version result) {
        result = new Version(0, 0);
        if (v == null || v == "") { return false; }
        string[] parts = v.Split(".");
        int count = parts.Length;
        if (count < 1 || count > 4) { return false; }
        int ma = 0; int mi = 0; int bu = -1; int re = -1;
        if (!Version.TryParsePart(parts[0], out ma)) { return false; }
        if (count >= 2 && !Version.TryParsePart(parts[1], out mi)) { return false; }
        if (count >= 3 && !Version.TryParsePart(parts[2], out bu)) { return false; }
        if (count >= 4 && !Version.TryParsePart(parts[3], out re)) { return false; }
        result = new Version(ma, mi, bu, re);
        return true;
    }

    public string ToString() {
        string s = Major + "." + Minor;
        if (Build >= 0) { s = s + "." + Build; }
        if (Revision >= 0) { s = s + "." + Revision; }
        return s;
    }

    public static int Compare(Version v1, Version v2) {
        if (v1.Major != v2.Major) {
            if (v1.Major > v2.Major) { return 1; } else { return -1; }
        }
        if (v1.Minor != v2.Minor) {
            if (v1.Minor > v2.Minor) { return 1; } else { return -1; }
        }
        int b1 = v1.Build < 0 ? 0 : v1.Build;
        int b2 = v2.Build < 0 ? 0 : v2.Build;
        if (b1 != b2) {
            if (b1 > b2) { return 1; } else { return -1; }
        }
        int r1 = v1.Revision < 0 ? 0 : v1.Revision;
        int r2 = v2.Revision < 0 ? 0 : v2.Revision;
        if (r1 != r2) {
            if (r1 > r2) { return 1; } else { return -1; }
        }
        return 0;
    }

    public static bool Equals(Version v1, Version v2) {
        return v1.Major == v2.Major && v1.Minor == v2.Minor
            && v1.Build == v2.Build && v1.Revision == v2.Revision;
    }

    private static int ParsePart(string s) {
        int value = 0;
        if (!Version.TryParsePart(s, out value)) {
            throw new FormatException("Version.Parse: invalid segment");
        }
        return value;
    }

    private static bool TryParsePart(string s, out int value) {
        value = 0;
        if (s == null || s == "") { return false; }
        int r = 0; int i = 0;
        while (i < s.Length) {
            string ch = s.Substring(i, 1); int d = -1;
            if (ch == "0") { d = 0; } else if (ch == "1") { d = 1; } else if (ch == "2") { d = 2; }
            else if (ch == "3") { d = 3; } else if (ch == "4") { d = 4; } else if (ch == "5") { d = 5; }
            else if (ch == "6") { d = 6; } else if (ch == "7") { d = 7; } else if (ch == "8") { d = 8; }
            else if (ch == "9") { d = 9; }
            if (d < 0) { return false; }
            r = r * 10 + d; i = i + 1;
        }
        value = r;
        return true;
    }
}
