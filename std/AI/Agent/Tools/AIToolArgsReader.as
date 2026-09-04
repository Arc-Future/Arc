// RFC 038: AIToolArgsReader — 工具调用参数 JSON 的类型化读取（结构化参数支持）。
//
// 能力：
//  - 顶层/嵌套标量：GetString / GetInt / GetLong / GetDouble / GetBool（点路径，如 "user.name"）
//  - 字符串数组：GetStringArray（嵌套对象/数组内的字符串不会被误收为元素）
//  - 结构化原样：GetObjectJson 取对象/数组字段的原始 JSON（供业务模型反序列化/透传）
//  - 结构导航：GetChild 进入嵌套对象字段（返回子 reader）
//  - Has 探测字段存在性；缺失字段/类型不匹配 → 类型默认值（不抛异常）
//
// 解析为自包含深度感知扫描（不依赖 JsonReader 位置 API；字符串转义正确处理）。
namespace Arc.Agent;

using Arc;
using Arc.Collections;
using Arc.Text;

public class AIToolArgsReader {
    private List<AIToolArgEntry> _entries;

    public AIToolArgsReader(string json) {
        _entries = new List<AIToolArgEntry>();
        this.Parse(json != null ? json : "");
    }

    /// <summary>字段是否存在（点路径）。</summary>
    public bool Has(string path) {
        return this.Find(path) != null;
    }

    /// <summary>取字符串字段；缺失返回 ""（点路径支持嵌套，如 "user.name"）。</summary>
    public string GetString(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null) { return ""; }
        return e.Text;
    }

    /// <summary>取整数字段；缺失/非数字返回 0。</summary>
    public int GetInt(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.Number) { return 0; }
        int v = 0;
        bool ok = int.TryParse(e.Text, ref v);
        if (ok) { return v; }
        return 0;
    }

    /// <summary>取长整数字段；缺失/非数字返回 0。</summary>
    public long GetLong(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.Number) { return 0; }
        long v = 0;
        bool ok = long.TryParse(e.Text, ref v);
        if (ok) { return v; }
        return 0;
    }

    /// <summary>取浮点字段；缺失/非数字返回 0。</summary>
    public double GetDouble(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.Number) { return 0; }
        double v = 0;
        bool ok = double.TryParse(e.Text, ref v);
        if (ok) { return v; }
        return 0;
    }

    /// <summary>取布尔字段；缺失/非布尔返回 false。</summary>
    public bool GetBool(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.Bool) { return false; }
        return e.Text == "true";
    }

    /// <summary>取字符串数组字段；缺失/非字符串数组返回空数组。</summary>
    public string[] GetStringArray(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.StringArray || e.Array == null) {
            string[] empty = [];
            return empty;
        }
        return e.Array.ToArray();
    }

    /// <summary>取对象/数组字段的原始 JSON 文本；缺失/非结构化返回 ""（供业务模型反序列化/透传）。</summary>
    public string GetObjectJson(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null) { return ""; }
        if (e.Kind != AIToolArgKind.Object && e.Kind != AIToolArgKind.JsonArray) { return ""; }
        return e.RawJson;
    }

    /// <summary>导航到嵌套对象字段（结构化子 reader）；缺失/非对象返回 null。</summary>
    public AIToolArgsReader GetChild(string path) {
        AIToolArgEntry e = this.Find(path);
        if (e == null || e.Kind != AIToolArgKind.Object) { return null; }
        return new AIToolArgsReader(e.RawJson);
    }

    private AIToolArgEntry Find(string path) {
        if (path == null) { return null; }
        // 1) 精确匹配（含嵌套点路径，如 "user.name"）。
        AIToolArgEntry exact = this.FindExact(path);
        if (exact != null) {
            return exact;
        }
        // 2) Tool-Call Repair（schema-aware 模糊匹配兜底）：模型工具调用参数名偏离
        //    schema 时（大小写不同 / snake_case vs camelCase / 连字符分隔），按规范化
        //    key（小写 + 去分隔符）匹配最近 schema 参数名——内联修复而非丢弃调用，
        //    保住缓存前缀稳定（对齐 Reasonix tool-call repair）。仅精确缺失时启用。
        return this.FindFuzzy(path);
    }

    private AIToolArgEntry FindExact(string path) {
        int i = 0;
        int n = _entries.Count;
        while (i < n) {
            AIToolArgEntry e = _entries[i];
            if (e != null && e.Name == path) {
                return e;
            }
            i = i + 1;
        }
        return null;
    }

    private AIToolArgEntry FindFuzzy(string path) {
        string target = AIToolArgsReader.NormalizeKey(path);
        if (target == "") {
            return null;
        }
        int i = 0;
        int n = _entries.Count;
        while (i < n) {
            AIToolArgEntry e = _entries[i];
            if (e != null && AIToolArgsReader.NormalizeKey(e.Name) == target) {
                return e;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>规范化参数名（Tool-Call Repair 修复键）：小写 + 去下划线/连字符/空格/点。</summary>
    private static string NormalizeKey(string s) {
        if (s == null) {
            return "";
        }
        string lower = s.ToLower();
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = lower.Length;
        while (i < n) {
            string c = lower.Substring(i, 1);
            if (c != "_" && c != "-" && c != " " && c != ".") {
                sb.Append(c);
            }
            i = i + 1;
        }
        return sb.ToString();
    }

    // ── 自包含解析：深度 0 遍历顶层属性；对象值递归（子字段以点路径扁平）──

    private void Parse(string json) {
        int n = json.Length;
        int pos = 0;
        while (pos < n) {
            int keyStart = this.FindQuote(json, pos, n);
            if (keyStart < 0) { return; }
            int keyEnd = this.ReadStringEnd(json, keyStart, n);
            if (keyEnd < 0) { return; }
            string key = this.Unescape(json.Substring(keyStart + 1, keyEnd - keyStart - 1));
            int valStart = this.SkipToValue(json, keyEnd + 1, n);
            if (valStart < 0) { return; }
            AIToolArgKind kind = AIToolArgKind.Null;
            string text = "";
            string raw = "";
            List<string> items = null;
            int valEnd = 0;
            this.ReadValue(json, valStart, n, ref kind, ref text, ref raw, ref items, ref valEnd);
            if (valEnd < valStart) { return; }
            AIToolArgEntry entry = new AIToolArgEntry(key, kind, text);
            entry.RawJson = raw;
            entry.Array = items;
            if (kind == AIToolArgKind.Object) {
                // 子字段以点路径扁平进入 entries（支持 GetString("user.name") 等）。
                AIToolArgsReader child = new AIToolArgsReader(raw);
                int ci = 0;
                int cn = child._entries.Count;
                while (ci < cn) {
                    AIToolArgEntry ce = child._entries[ci];
                    AIToolArgEntry dotted = new AIToolArgEntry(key + "." + ce.Name, ce.Kind, ce.Text);
                    dotted.Array = ce.Array;
                    dotted.RawJson = ce.RawJson;
                    _entries.Add(dotted);
                    ci = ci + 1;
                }
            }
            _entries.Add(entry);
            pos = valEnd + 1;
        }
    }

    /// <summary>从 fromPos 起找下一个未转义引号（属性名/字符串值起始）。</summary>
    private int FindQuote(string json, int fromPos, int n) {
        int i = fromPos;
        while (i < n) {
            if (json.Substring(i, 1) == "\"") {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }

    /// <summary>读字符串到结束引号（转义跳过）；返回结束引号位置。</summary>
    private int ReadStringEnd(string json, int quotePos, int n) {
        int i = quotePos + 1;
        while (i < n) {
            string c = json.Substring(i, 1);
            if (c == "\\") {
                i = i + 2;
            } else if (c == "\"") {
                return i;
            } else {
                i = i + 1;
            }
        }
        return -1;
    }

    /// <summary>跳过 ':' 与空白，返回值起始位置。</summary>
    private int SkipToValue(string json, int fromPos, int n) {
        int i = fromPos;
        while (i < n) {
            string c = json.Substring(i, 1);
            if (c == ":") {
                i = i + 1;
                while (i < n) {
                    string w = json.Substring(i, 1);
                    if (w == " " || w == "\t" || w == "\n" || w == "\r") {
                        i = i + 1;
                    } else {
                        break;
                    }
                }
                return i;
            }
            if (c == "," || c == "}") { return -1; }
            i = i + 1;
        }
        return -1;
    }

    /// <summary>读取一个值；输出 kind/text/raw/arrayItems 与值末字符位置（valEnd，含最后字符）。</summary>
    private void ReadValue(string json, int start, int n, ref AIToolArgKind kind, ref string text, ref string raw, ref List<string> arrayItems, ref int valEnd) {
        string c = json.Substring(start, 1);
        if (c == "\"") {
            int end = this.ReadStringEnd(json, start, n);
            if (end < 0) { valEnd = start; return; }
            text = this.Unescape(json.Substring(start + 1, end - start - 1));
            kind = AIToolArgKind.String;
            valEnd = end;
            return;
        }
        if (c == "{") {
            int end = this.FindMatchingBracket(json, start, n);
            if (end < 0) { valEnd = start; return; }
            raw = json.Substring(start, end - start + 1);
            kind = AIToolArgKind.Object;
            valEnd = end;
            return;
        }
        if (c == "[") {
            int end = this.FindMatchingBracket(json, start, n);
            if (end < 0) { valEnd = start; return; }
            raw = json.Substring(start, end - start + 1);
            kind = AIToolArgKind.JsonArray;
            this.ClassifyArray(raw, ref kind, ref arrayItems);
            valEnd = end;
            return;
        }
        // 标量：读到分隔符（, 或 } 或数组 ]）
        int i = start;
        while (i < n) {
            string ch = json.Substring(i, 1);
            if (ch == "," || ch == "}" || ch == "]") {
                break;
            }
            i = i + 1;
        }
        string token = "";
        if (i > start) {
            token = json.Substring(start, i - start);
        }
        string t = token;
        // 去掉首尾空白
        string trimmed = "";
        int ts = 0;
        int te = t.Length;
        while (ts < te) {
            string w = t.Substring(ts, 1);
            if (w == " " || w == "\t" || w == "\n" || w == "\r") { ts = ts + 1; } else { break; }
        }
        while (te > ts) {
            string w = t.Substring(te - 1, 1);
            if (w == " " || w == "\t" || w == "\n" || w == "\r") { te = te - 1; } else { break; }
        }
        if (te > ts) { trimmed = t.Substring(ts, te - ts); }
        if (trimmed == "true" || trimmed == "false") {
            text = trimmed;
            kind = AIToolArgKind.Bool;
        } else if (trimmed == "null") {
            text = "";
            kind = AIToolArgKind.Null;
        } else {
            text = trimmed;
            kind = AIToolArgKind.Number;
        }
        valEnd = i - 1;
    }

    /// <summary>判定数组种类：全部元素为字符串 → StringArray（items 收集）；含对象/数组/标量 → JsonArray。</summary>
    private void ClassifyArray(string raw, ref AIToolArgKind kind, ref List<string> items) {
        int n = raw.Length - 1;
        int i = 1;
        bool allStrings = true;
        List<string> list = new List<string>();
        while (i < n) {
            string c = raw.Substring(i, 1);
            if (c == " " || c == "\t" || c == "\n" || c == "\r" || c == ",") {
                i = i + 1;
            } else if (c == "\"") {
                int end = this.ReadStringEnd(raw, i, n + 1);
                if (end < 0) { allStrings = false; i = i + 1; } else {
                    list.Add(this.Unescape(raw.Substring(i + 1, end - i - 1)));
                    i = end + 1;
                }
            } else if (c == "{" || c == "[") {
                allStrings = false;
                int end = this.FindMatchingBracket(raw, i, n + 1);
                i = end > 0 ? end + 1 : i + 1;
            } else {
                allStrings = false;
                i = i + 1;
            }
        }
        if (allStrings) {
            kind = AIToolArgKind.StringArray;
            items = list;
        }
    }

    /// <summary>找与 start 处 {/[ 匹配的结束括号位置（字符串内括号忽略；转义跳过）。</summary>
    private int FindMatchingBracket(string json, int start, int n) {
        string open = json.Substring(start, 1);
        string close = open == "{" ? "}" : "]";
        int depth = 1;
        int i = start + 1;
        bool inString = false;
        while (i < n) {
            string c = json.Substring(i, 1);
            if (c == "\\" && inString) {
                i = i + 2;
            } else if (c == "\"") {
                inString = !inString;
                i = i + 1;
            } else if (!inString) {
                if (c == open) { depth = depth + 1; }
                else if (c == close) {
                    depth = depth - 1;
                    if (depth == 0) { return i; }
                }
                i = i + 1;
            } else {
                i = i + 1;
            }
        }
        return -1;
    }

    /// <summary>JSON 字符串反转义（\" \\ \n \r \t \/ \u 不做全量处理——工具参数为模型产物，转义面收敛）。</summary>
    private string Unescape(string s) {
        if (s == null) { return ""; }
        int n = s.Length;
        if (n == 0) { return ""; }
        bool hasEscape = false;
        int i = 0;
        while (i < n) {
            if (s.Substring(i, 1) == "\\") { hasEscape = true; i = i + 1; }
            i = i + 1;
        }
        if (!hasEscape) { return s; }
        StringBuilder sb = new StringBuilder();
        i = 0;
        while (i < n) {
            string c = s.Substring(i, 1);
            if (c == "\\" && i + 1 < n) {
                string e = s.Substring(i + 1, 1);
                if (e == "\"") { sb.Append("\""); }
                else if (e == "\\") { sb.Append("\\"); }
                else if (e == "/") { sb.Append("/"); }
                else if (e == "n") { sb.Append("\n"); }
                else if (e == "r") { sb.Append("\r"); }
                else if (e == "t") { sb.Append("\t"); }
                else { sb.Append(e); }
                i = i + 2;
            } else {
                sb.Append(c);
                i = i + 1;
            }
        }
        return sb.ToString();
    }
}
