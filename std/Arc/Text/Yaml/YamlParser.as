namespace Arc.Text.Yaml;

using Arc.Text;
using Arc.Collections;

// YAML 1.2 准完整解析器（读路径）—— 行式递归下降，产出 YamlNode 文档树。
//
// 覆盖（近完整子集）：
//   - 块映射 `key: value` / 块序列 `- item`，含任意缩进嵌套
//   - `- key: value` 序列内联映射
//   - 流集合 `{a: 1, b: 2}` / `[1, 2, 3]`（单行内闭合）
//   - 明文/单引号/双引号标量；双引号转义
//   - 块标量：字面量 `|` 与折叠 `>`，含剪辑/去除/保留 chomping
//   - 注释 `#`（行内 + 整行）；文档标记 `---`/`...`；指令 `%...`
//   - 标量解析：null / ~；true|false|yes|no|on|off；int（十进制/0x/0o/0b，64 位
//     long，溢出/非法降级 string）；float 与 double（含 .inf/.nan）；string
//   - 锚点 `&name` 与别名 `*name`（标量/流节点）
//
// 实现细节，非用户面契约：开发者经 YamlSerializer.Parse 消费，不直接触碰本类型。
// 诚实边界（文档化，非完备）：
//   - 块标量显式缩进指示符（`|2`）部分支持；`*`/flow 内联注释不解析
//   - 块节点（映射/序列）上的锚点暂不登记；别名回退为 null
//   - 自定义 tag（`!!str` 等）忽略；多文档流仅取首个文档
//   - 制表符不可作缩进（YAML 规范禁止）；流集合不跨行
internal class YamlParser
{
    private string _text;
    private List<string> _lines;
    private int _size;
    private int _pos;
    private Dictionary<string, YamlNode> _anchors;

    public YamlParser()
    {
        _text = "";
        _size = 0;
        _pos = 0;
        _anchors = new Dictionary<string, YamlNode>();
    }

    /// <summary>解析 YAML 文本为文档根节点。空/全注释输入返回 null 标量。</summary>
    public YamlNode Parse(string text)
    {
        _text = text != null ? text : "";
        _lines = _splitLines(_text);
        _size = _lines.Count;
        _pos = 0;
        _anchors = new Dictionary<string, YamlNode>();
        return _parseBlockNode(-1);
    }

    /// <summary>按 `\n` 切分文本为行列表（保留尾随空行，与 Split("\n") 语义一致）。</summary>
    private List<string> _splitLines(string text)
    {
        List<string> result = new List<string>();
        int len = text.Length;
        int start = 0;
        int i = 0;
        while (i < len)
        {
            if (text.Substring(i, 1) == "\n")
            {
                result.Add(text.Substring(start, i - start));
                i = i + 1;
                start = i;
            }
            else
            {
                i = i + 1;
            }
        }
        result.Add(text.Substring(start, len - start));
        return result;
    }

    // ─────────────────────────── 块结构 ───────────────────────────

    private YamlNode _parseBlockNode(int parentIndent)
    {
        _skipIgnorable();
        if (_pos >= _size)
        {
            return _nullNode();
        }
        int ind = _indentOf(_lines[_pos]);
        string content = _lines[_pos].Trim();
        if (_isSeqIndicator(content))
        {
            return _parseSequence(ind);
        }
        int colon = _findMappingColon(content);
        if (colon >= 0)
        {
            return _parseMapping(ind);
        }
        YamlNode s = _parseScalarNode(content);
        _pos = _pos + 1;
        return s;
    }

    private YamlNode _parseMapping(int baseIndent)
    {
        YamlNode node = YamlNode.CreateMapping();
        bool cont = true;
        while (cont && _pos < _size)
        {
            string line = _lines[_pos];
            string tr = line.Trim();
            if (_isIgnorable(tr))
            {
                _pos = _pos + 1;
                continue;
            }
            int ind = _indentOf(line);
            if (ind < baseIndent)
            {
                cont = false;
                break;
            }
            if (ind > baseIndent)
            {
                cont = false;
                break;
            }
            int colon = _findMappingColon(tr);
            if (colon < 0)
            {
                cont = false;
                break;
            }
            string keyText = tr.Substring(0, colon).Trim();
            string valueText = tr.Substring(colon + 1, tr.Length - colon - 1).Trim();
            YamlNode key = _parseScalarNode(keyText);
            YamlNode value = _parseValueAt(valueText, baseIndent);
            node.AddMapEntry(new YamlMapEntry(key, value));
        }
        return node;
    }

    private YamlNode _parseSequence(int baseIndent)
    {
        YamlNode node = YamlNode.CreateSequence();
        bool cont = true;
        while (cont && _pos < _size)
        {
            string line = _lines[_pos];
            string tr = line.Trim();
            if (_isIgnorable(tr))
            {
                _pos = _pos + 1;
                continue;
            }
            int ind = _indentOf(line);
            if (ind < baseIndent)
            {
                cont = false;
                break;
            }
            if (ind > baseIndent)
            {
                cont = false;
                break;
            }
            if (!_isSeqIndicator(tr))
            {
                cont = false;
                break;
            }
            string rest = tr.Substring(1, tr.Length - 1).Trim();
            YamlNode item = this.ParseSeqItem(rest, ind);
            node.AddItem(item);
        }
        return node;
    }

    private YamlNode ParseSeqItem(string rest, int itemIndent)
    {
        if (rest.Length == 0)
        {
            _pos = _pos + 1;
            return this.ParseNestedOrNull(itemIndent);
        }
        string c = rest.Substring(0, 1);
        if (c == "|" || c == ">")
        {
            return _parseBlockScalar(rest, itemIndent);
        }
        if (c == "{" || c == "[")
        {
            YamlNode v = _parseFlowValue(rest);
            _pos = _pos + 1;
            return v;
        }
        int colon = _findMappingColon(rest);
        if (colon >= 0)
        {
            return this.ParseSeqMapping(rest, itemIndent + 2);
        }
        YamlNode s = _parseScalarNode(rest);
        _pos = _pos + 1;
        return s;
    }

    // `- key: value` 的内联映射：首条目在当前行，后续条目顶到 mapIndent。
    private YamlNode ParseSeqMapping(string firstRest, int mapIndent)
    {
        YamlNode node = YamlNode.CreateMapping();
        int colon = _findMappingColon(firstRest);
        string keyText = firstRest.Substring(0, colon).Trim();
        string valueText = firstRest.Substring(colon + 1, firstRest.Length - colon - 1).Trim();
        YamlNode key = _parseScalarNode(keyText);
        YamlNode value = _parseValueAt(valueText, mapIndent);
        node.AddMapEntry(new YamlMapEntry(key, value));

        bool cont = true;
        while (cont && _pos < _size)
        {
            string line = _lines[_pos];
            string tr = line.Trim();
            if (_isIgnorable(tr))
            {
                _pos = _pos + 1;
                continue;
            }
            int ind = _indentOf(line);
            if (ind < mapIndent)
            {
                cont = false;
                break;
            }
            if (ind > mapIndent)
            {
                cont = false;
                break;
            }
            int c2 = _findMappingColon(tr);
            if (c2 < 0)
            {
                cont = false;
                break;
            }
            string kt = tr.Substring(0, c2).Trim();
            string vt = tr.Substring(c2 + 1, tr.Length - c2 - 1).Trim();
            YamlNode k2 = _parseScalarNode(kt);
            YamlNode v2 = _parseValueAt(vt, mapIndent);
            node.AddMapEntry(new YamlMapEntry(k2, v2));
        }
        return node;
    }

    // 解析「key: 」的值：消费当前行 + 可能的续行（嵌套块 / 块标量）。
    private YamlNode _parseValueAt(string valueText, int lineIndent)
    {
        if (valueText.Length == 0)
        {
            _pos = _pos + 1;
            return this.ParseNestedOrNull(lineIndent);
        }
        string c = valueText.Substring(0, 1);
        if (c == "|" || c == ">")
        {
            return _parseBlockScalar(valueText, lineIndent);
        }
        if (c == "{" || c == "[")
        {
            YamlNode v = _parseFlowValue(valueText);
            _pos = _pos + 1;
            return v;
        }
        if (c == "'" || c == "\"")
        {
            YamlNode q = _parseScalarNode(valueText);
            _pos = _pos + 1;
            return q;
        }
        // 明文标量：剥离行内注释后解析
        string clean = _stripInlineComment(valueText).Trim();
        YamlNode s = _parseScalarNode(clean);
        _pos = _pos + 1;
        return s;
    }

    // 当前行已消费；判断下一内容行是否构成更深一层嵌套块。
    private YamlNode ParseNestedOrNull(int parentIndent)
    {
        int save = _pos;
        while (save < _size)
        {
            string line = _lines[save];
            string tr = line.Trim();
            if (_isBlank(tr) || _isComment(tr))
            {
                save = save + 1;
                continue;
            }
            break;
        }
        if (save >= _size)
        {
            return _nullNode();
        }
        int ind = _indentOf(_lines[save]);
        if (ind <= parentIndent)
        {
            return _nullNode();
        }
        _pos = save;
        string content = _lines[_pos].Trim();
        if (_isSeqIndicator(content))
        {
            return _parseSequence(ind);
        }
        int colon = _findMappingColon(content);
        if (colon >= 0)
        {
            return _parseMapping(ind);
        }
        YamlNode s = _parseScalarNode(content);
        _pos = _pos + 1;
        return s;
    }

    // ─────────────────────────── 标量 ───────────────────────────

    private YamlNode _parseScalarNode(string text)
    {
        text = text.Trim();
        if (text.Length == 0)
        {
            return _nullNode();
        }
        string c0 = text.Substring(0, 1);
        if (c0 == "\"")
        {
            YamlNode n = new YamlNode();
            n.Kind = YamlNodeKind.Scalar;
            n.ScalarKind = YamlScalarKind.String;
            n.Scalar = _unescapeDouble(text);
            return n;
        }
        if (c0 == "'")
        {
            YamlNode n = new YamlNode();
            n.Kind = YamlNodeKind.Scalar;
            n.ScalarKind = YamlScalarKind.String;
            n.Scalar = _unescapeSingle(text);
            return n;
        }
        if (c0 == "*")
        {
            string name = text.Substring(1, text.Length - 1).Trim();
            YamlNode refNode = null;
            if (_anchors.TryGetValue(name, out refNode))
            {
                return refNode;
            }
            return _nullNode();
        }
        if (c0 == "&")
        {
            int sp = _indexOfWhitespace(text, 0);
            string name = "";
            string rest = "";
            if (sp < 0)
            {
                name = text.Substring(1, text.Length - 1);
                rest = "";
            }
            else
            {
                name = text.Substring(1, sp - 1);
                rest = text.Substring(sp, text.Length - sp).Trim();
            }
            YamlNode anchored = _parseScalarNode(rest);
            _anchors[name] = anchored;
            return anchored;
        }
        YamlNode p = new YamlNode();
        p.Kind = YamlNodeKind.Scalar;
        _resolvePlain(p, text);
        return p;
    }

    private void _resolvePlain(YamlNode n, string text)
    {
        string lower = text.ToLower();
        if (lower == "null" || lower == "~")
        {
            n.ScalarKind = YamlScalarKind.Null;
            n.Scalar = "";
            return;
        }
        if (_isBoolLower(lower))
        {
            n.ScalarKind = YamlScalarKind.Bool;
            n.Scalar = text;
            n.BoolValue = lower == "true" || lower == "yes" || lower == "on";
            return;
        }
        if (_parseIntInto(n, text))
        {
            n.ScalarKind = YamlScalarKind.Int;
            n.Scalar = text;
            return;
        }
        if (this.IsFloat(text))
        {
            n.ScalarKind = YamlScalarKind.Float;
            n.Scalar = text;
            n.DoubleValue = _parseDouble(text);
            return;
        }
        n.ScalarKind = YamlScalarKind.String;
        n.Scalar = text;
    }

    private bool _isBoolLower(string lower)
    {
        return lower == "true" || lower == "false" || lower == "yes" || lower == "no"
            || lower == "on" || lower == "off";
    }

    // 解析 int（十进制 / 0x / 0o / 0b 前缀）。成功返回 true 并填充 LongValue/IntValue；
    // 非法数字或超出 64 位长整型域返回 false，由调用方降级为 string（宽容解析）。
    // 十进制经非 panic 的 long.TryParse（含负号与 INT64_MIN 边界）；非十进制手动按
    // 进制累积（带溢出检测，无异常）再套符号。不可用 long.Parse/异常：其失败是
    // rt_panic（不可捕获），会中止整个程序。
    private bool _parseIntInto(YamlNode n, string t)
    {
        int len = t.Length;
        if (len == 0)
        {
            return false;
        }
        int i = 0;
        string c0 = t.Substring(0, 1);
        if (c0 == "-" || c0 == "+")
        {
            i = 1;
        }
        if (i >= len)
        {
            return false;
        }
        int baseN = 10;
        string p = t.Substring(i, 1);
        if (p == "0" && i + 1 < len)
        {
            string p1 = t.Substring(i + 1, 1);
            if (p1 == "x" || p1 == "X")
            {
                baseN = 16;
                i = i + 2;
            }
            else if (p1 == "o" || p1 == "O")
            {
                baseN = 8;
                i = i + 2;
            }
            else if (p1 == "b" || p1 == "B")
            {
                baseN = 2;
                i = i + 2;
            }
        }
        if (i >= len)
        {
            return false;
        }

        long value = 0;
        if (baseN == 10)
        {
            // 整串交 long.TryParse（非 panic；自行处理符号与 INT64_MIN）。
            if (!long.TryParse(t, ref value))
            {
                return false;
            }
        }
        else
        {
            bool neg = c0 == "-";
            bool ok = false;
            value = this.ParseNonDecimalValue(t.Substring(i, len - i), baseN, ref ok);
            if (!ok)
            {
                return false;
            }
            if (neg)
            {
                value = 0 - value;
            }
        }
        n.LongValue = value;
        n.IntValue = (int)value;
        return true;
    }

    // 按进制解析非负数字串为 long 幅度；非法数字或溢出返回 false（经 ok 指示，不抛异常）。
    // 幅度用局部变量累积并返回（而非 ref 参数循环累加）：`ref long` 在紧循环内累加至恰为
    // long.MaxValue 边界时会被编译器误编译为 -1（缺陷规避），局部累积 + 返回值结果正确。
    // 溢出阈值先算入局部变量：`9223372036854775807 / baseN` 若内联进比较表达式，常量折叠
    // 会把长整型除法误算，导致小值误判为溢出。两处均为编译器缺陷规避。
    private long ParseNonDecimalValue(string digits, int baseN, ref bool ok)
    {
        long mag = 0;
        int len = digits.Length;
        int i = 0;
        long limit = 9223372036854775807 / baseN;
        bool cont = true;
        while (cont && i < len)
        {
            int d = _digitBase(digits.Substring(i, 1), baseN);
            if (d < 0)
            {
                ok = false;
                return mag;
            }
            if (mag > limit)
            {
                ok = false;
                return mag;
            }
            mag = mag * baseN + d;
            i = i + 1;
        }
        ok = true;
        return mag;
    }

    private bool IsFloat(string t)
    {
        int len = t != null ? t.Length : 0;
        if (len == 0)
        {
            return false;
        }
        string lower = t.ToLower();
        if (lower == ".inf" || lower == "-.inf" || lower == "+.inf" || lower == ".nan")
        {
            return true;
        }
        bool hasDot = false;
        bool hasExp = false;
        bool anyDigit = false;
        bool hasExpDigit = false;
        int i = 0;
        string c0 = t.Substring(0, 1);
        if (c0 == "-" || c0 == "+")
        {
            i = 1;
        }
        if (i >= len)
        {
            return false;
        }
        bool cont = true;
        while (cont && i < len)
        {
            string ch = t.Substring(i, 1);
            if (ch >= "0" && ch <= "9")
            {
                anyDigit = true;
                if (hasExp)
                {
                    hasExpDigit = true;
                }
                i = i + 1;
            }
            else if (ch == "." && !hasDot)
            {
                hasDot = true;
                i = i + 1;
            }
            else if ((ch == "e" || ch == "E") && !hasExp)
            {
                hasExp = true;
                i = i + 1;
            }
            else if (hasExp && (ch == "-" || ch == "+"))
            {
                i = i + 1;
            }
            else
            {
                cont = false;
            }
        }
        return anyDigit && i == len && (hasDot || hasExp) && (hasDot || hasExpDigit);
    }

    // 浮点值解析：.inf/-.inf/+.inf/.nan 按 IEEE 特判；其余走非 panic 的 double.TryParse
    // （对超大指数等非法/溢出输入返回 false，降级 0.0）。不可用 double.Parse：其失败是
    // rt_panic（不可捕获），会中止整个程序。
    private double _parseDouble(string t)
    {
        string lower = t.ToLower();
        if (lower == ".inf" || lower == "+.inf")
        {
            return 1.0 / 0.0;
        }
        if (lower == "-.inf")
        {
            return 0.0 - (1.0 / 0.0);
        }
        if (lower == ".nan")
        {
            return 0.0 / 0.0;
        }
        double value;
        if (double.TryParse(t, out value))
        {
            return value;
        }
        return 0.0;
    }

    private int _digitValue(string ch)
    {
        if (ch == "0") { return 0; }
        if (ch == "1") { return 1; }
        if (ch == "2") { return 2; }
        if (ch == "3") { return 3; }
        if (ch == "4") { return 4; }
        if (ch == "5") { return 5; }
        if (ch == "6") { return 6; }
        if (ch == "7") { return 7; }
        if (ch == "8") { return 8; }
        if (ch == "9") { return 9; }
        if (ch == "a" || ch == "A") { return 10; }
        if (ch == "b" || ch == "B") { return 11; }
        if (ch == "c" || ch == "C") { return 12; }
        if (ch == "d" || ch == "D") { return 13; }
        if (ch == "e" || ch == "E") { return 14; }
        if (ch == "f" || ch == "F") { return 15; }
        return -1;
    }

    // 按进制取数字值；非法或超出该进制返回 -1。
    private int _digitBase(string ch, int baseN)
    {
        int d = _digitValue(ch);
        if (d >= 0 && d < baseN)
        {
            return d;
        }
        return -1;
    }

    // ─────────────────────────── 流集合 ───────────────────────────

    private YamlNode _parseFlowValue(string text)
    {
        text = text.Trim();
        if (text.Length == 0)
        {
            return _nullNode();
        }
        string c = text.Substring(0, 1);
        if (c == "{")
        {
            return _parseFlowMapping(text);
        }
        if (c == "[")
        {
            return _parseFlowSequence(text);
        }
        return _parseScalarNode(text);
    }

    private YamlNode _parseFlowMapping(string text)
    {
        YamlNode node = YamlNode.CreateMapping();
        string inner = text.Length >= 2 ? text.Substring(1, text.Length - 2) : "";
        List<string> entries = this.SplitTopLevel(inner, ",");
        int n = entries.Count;
        int i = 0;
        while (i < n)
        {
            string entry = entries[i].Trim();
            if (entry.Length == 0)
            {
                i = i + 1;
                continue;
            }
            int colon = _findFlowColon(entry);
            if (colon < 0)
            {
                YamlNode key = _parseScalarNode(entry);
                node.AddMapEntry(new YamlMapEntry(key, _nullNode()));
            }
            else
            {
                string kt = entry.Substring(0, colon).Trim();
                string vt = entry.Substring(colon + 1, entry.Length - colon - 1).Trim();
                YamlNode key = _parseScalarNode(kt);
                YamlNode value = _parseFlowValue(vt);
                node.AddMapEntry(new YamlMapEntry(key, value));
            }
            i = i + 1;
        }
        return node;
    }

    private YamlNode _parseFlowSequence(string text)
    {
        YamlNode node = YamlNode.CreateSequence();
        string inner = text.Length >= 2 ? text.Substring(1, text.Length - 2) : "";
        List<string> items = this.SplitTopLevel(inner, ",");
        int n = items.Count;
        int i = 0;
        while (i < n)
        {
            string item = items[i].Trim();
            if (item.Length == 0)
            {
                i = i + 1;
                continue;
            }
            if (_startsWithFlow(item))
            {
                node.AddItem(_parseFlowValue(item));
            }
            else
            {
                node.AddItem(_parseScalarNode(item));
            }
            i = i + 1;
        }
        return node;
    }

    private bool _startsWithFlow(string text)
    {
        string c = text.Substring(0, 1);
        return c == "{" || c == "[";
    }

    // 顶层分隔拆分：跳过引号与嵌套流括号。
    private List<string> SplitTopLevel(string s, string sepChar)
    {
        List<string> parts = new List<string>();
        StringBuilder cur = new StringBuilder();
        int depth = 0;
        string quote = "";
        int i = 0;
        int len = s.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = s.Substring(i, 1);
            if (quote != "")
            {
                cur.Append(ch);
                if (quote == "\"")
                {
                    if (ch == "\\")
                    {
                        if (i + 1 < len)
                        {
                            cur.Append(s.Substring(i + 1, 1));
                            i = i + 1;
                        }
                    }
                    else if (ch == "\"")
                    {
                        quote = "";
                    }
                }
                else
                {
                    if (ch == "'")
                    {
                        if (i + 1 < len && s.Substring(i + 1, 1) == "'")
                        {
                            cur.Append("'");
                            i = i + 1;
                        }
                        else
                        {
                            quote = "";
                        }
                    }
                }
            }
            else if (ch == "'" || ch == "\"")
            {
                quote = ch;
                cur.Append(ch);
            }
            else if (ch == "{" || ch == "[")
            {
                depth = depth + 1;
                cur.Append(ch);
            }
            else if (ch == "}" || ch == "]")
            {
                depth = depth - 1;
                cur.Append(ch);
            }
            else if (ch == sepChar && depth == 0)
            {
                parts.Add(cur.ToString());
                cur.Clear();
            }
            else
            {
                cur.Append(ch);
            }
            i = i + 1;
        }
        parts.Add(cur.ToString());
        return parts;
    }

    // 顶层冒号（flow 映射分隔符）：跳过引号与嵌套流括号。
    private int _findFlowColon(string s)
    {
        int depth = 0;
        string quote = "";
        int i = 0;
        int len = s.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = s.Substring(i, 1);
            if (quote != "")
            {
                if (quote == "\"")
                {
                    if (ch == "\\")
                    {
                        i = i + 2;
                        continue;
                    }
                    if (ch == "\"")
                    {
                        quote = "";
                    }
                }
                else
                {
                    if (ch == "'")
                    {
                        if (i + 1 < len && s.Substring(i + 1, 1) == "'")
                        {
                            i = i + 2;
                            continue;
                        }
                        quote = "";
                    }
                }
            }
            else if (ch == "'" || ch == "\"")
            {
                quote = ch;
            }
            else if (ch == "{" || ch == "[")
            {
                depth = depth + 1;
            }
            else if (ch == "}" || ch == "]")
            {
                depth = depth - 1;
            }
            else if (ch == ":" && depth == 0)
            {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }

    // ─────────────────────────── 块标量 ───────────────────────────

    private YamlNode _parseBlockScalar(string markers, int lineIndent)
    {
        bool folded = markers.Substring(0, 1) == ">";
        int chomp = 0;
        int explicitIndent = 0;
        int j = 1;
        int mlen = markers.Length;
        bool cont = true;
        while (cont && j < mlen)
        {
            string ch = markers.Substring(j, 1);
            if (ch == "-")
            {
                chomp = -1;
            }
            else if (ch == "+")
            {
                chomp = 1;
            }
            else if (ch >= "0" && ch <= "9")
            {
                explicitIndent = _digitValue(ch);
            }
            j = j + 1;
        }
        _pos = _pos + 1; // 消费标记行
        int contentIndent = explicitIndent != 0 ? lineIndent + explicitIndent : -1;
        StringBuilder sb = new StringBuilder();
        bool anyLine = false;
        bool cont2 = true;
        while (cont2 && _pos < _size)
        {
            string line = _lines[_pos];
            string tr = line.Trim();
            if (contentIndent < 0)
            {
                if (_isBlank(tr))
                {
                    _pos = _pos + 1;
                    continue;
                }
                contentIndent = _indentOf(line);
            }
            int ind = _indentOf(line);
            if (!_isBlank(tr) && ind < contentIndent)
            {
                cont2 = false;
                break;
            }
            string content = "";
            if (!_isBlank(tr))
            {
                if (line.Length >= contentIndent)
                {
                    content = line.Substring(contentIndent, line.Length - contentIndent);
                }
            }
            if (anyLine)
            {
                sb.Append("\n");
            }
            anyLine = true;
            sb.Append(content);
            _pos = _pos + 1;
        }
        string value = sb.ToString();
        if (folded)
        {
            value = _fold(value);
        }
        value = _applyChomp(value, chomp);
        YamlNode n = new YamlNode();
        n.Kind = YamlNodeKind.Scalar;
        n.ScalarKind = YamlScalarKind.String;
        n.Scalar = value;
        return n;
    }

    private string _fold(string text)
    {
        string[] lines = text.Split("\n");
        StringBuilder sb = new StringBuilder();
        bool haveAny = false;
        bool prevBlank = false;
        int i = 0;
        int n = lines.Length;
        while (i < n)
        {
            string line = lines[i];
            if (line.Length == 0)
            {
                if (haveAny)
                {
                    sb.Append("\n");
                    prevBlank = true;
                }
            }
            else
            {
                if (haveAny)
                {
                    if (!prevBlank)
                    {
                        sb.Append(" ");
                    }
                }
                sb.Append(line);
                haveAny = true;
                prevBlank = false;
            }
            i = i + 1;
        }
        return sb.ToString();
    }

    private string _applyChomp(string v, int chomp)
    {
        if (chomp == -1)
        {
            return this.TrimTrailingNewlines(v);
        }
        if (chomp == 0)
        {
            string t = this.TrimTrailingNewlines(v);
            return t + "\n";
        }
        return v;
    }

    private string TrimTrailingNewlines(string v)
    {
        int end = v.Length;
        bool cont = true;
        while (cont && end > 0)
        {
            if (v.Substring(end - 1, 1) == "\n")
            {
                end = end - 1;
            }
            else
            {
                cont = false;
            }
        }
        return v.Substring(0, end);
    }

    // ─────────────────────────── 引号转义 ───────────────────────────

    private string _unescapeDouble(string text)
    {
        StringBuilder sb = new StringBuilder();
        int i = 1;
        int len = text.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = text.Substring(i, 1);
            if (ch == "\"")
            {
                cont = false;
            }
            else if (ch == "\\")
            {
                if (i + 1 < len)
                {
                    string e = text.Substring(i + 1, 1);
                    if (e == "n")
                    {
                        sb.Append("\n");
                    }
                    else if (e == "t")
                    {
                        sb.Append("\t");
                    }
                    else if (e == "r")
                    {
                        sb.Append("\r");
                    }
                    else if (e == "\"")
                    {
                        sb.Append("\"");
                    }
                    else if (e == "\\")
                    {
                        sb.Append("\\");
                    }
                    else if (e == "0")
                    {
                        sb.Append("\0");
                    }
                    else
                    {
                        sb.Append(e);
                    }
                    i = i + 2;
                    continue;
                }
                else
                {
                    cont = false;
                }
            }
            else
            {
                sb.Append(ch);
                i = i + 1;
            }
        }
        return sb.ToString();
    }

    private string _unescapeSingle(string text)
    {
        StringBuilder sb = new StringBuilder();
        int i = 1;
        int len = text.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = text.Substring(i, 1);
            if (ch == "'")
            {
                if (i + 1 < len && text.Substring(i + 1, 1) == "'")
                {
                    sb.Append("'");
                    i = i + 2;
                    continue;
                }
                cont = false;
            }
            else
            {
                sb.Append(ch);
                i = i + 1;
            }
        }
        return sb.ToString();
    }

    // ─────────────────────────── 行工具 ───────────────────────────

    private void _skipIgnorable()
    {
        bool cont = true;
        while (cont && _pos < _size)
        {
            string tr = _lines[_pos].Trim();
            if (_isIgnorable(tr))
            {
                _pos = _pos + 1;
            }
            else
            {
                cont = false;
            }
        }
    }

    private bool _isIgnorable(string tr)
    {
        return _isBlank(tr) || _isComment(tr) || _isDocMarker(tr) || _isDirective(tr);
    }

    private bool _isBlank(string tr)
    {
        return tr == "" || tr == "\r";
    }

    private bool _isComment(string tr)
    {
        return tr.StartsWith("#");
    }

    private bool _isDocMarker(string tr)
    {
        return tr == "---" || tr == "...";
    }

    private bool _isDirective(string tr)
    {
        return tr.StartsWith("%");
    }

    private bool _isSeqIndicator(string tr)
    {
        return tr == "-" || tr.StartsWith("- ");
    }

    private int _indentOf(string line)
    {
        int i = 0;
        int len = line != null ? line.Length : 0;
        bool cont = true;
        while (cont && i < len)
        {
            if (line.Substring(i, 1) == " ")
            {
                i = i + 1;
            }
            else
            {
                cont = false;
            }
        }
        return i;
    }

    private int _indexOfWhitespace(string s, int start)
    {
        int i = start;
        int len = s.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = s.Substring(i, 1);
            if (ch == " " || ch == "\t")
            {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }

    // 块映射分隔冒号：跳过引号/流括号，且冒号后须接空白或行尾。
    private int _findMappingColon(string content)
    {
        int depth = 0;
        string quote = "";
        int i = 0;
        int len = content.Length;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = content.Substring(i, 1);
            if (quote != "")
            {
                if (quote == "\"")
                {
                    if (ch == "\\")
                    {
                        i = i + 2;
                        continue;
                    }
                    if (ch == "\"")
                    {
                        quote = "";
                    }
                }
                else
                {
                    if (ch == "'")
                    {
                        if (i + 1 < len && content.Substring(i + 1, 1) == "'")
                        {
                            i = i + 2;
                            continue;
                        }
                        quote = "";
                    }
                }
            }
            else if (ch == "'" || ch == "\"")
            {
                quote = ch;
            }
            else if (ch == "{" || ch == "[")
            {
                depth = depth + 1;
            }
            else if (ch == "}" || ch == "]")
            {
                depth = depth - 1;
            }
            else if (ch == ":" && depth == 0)
            {
                if (i + 1 >= len)
                {
                    return i;
                }
                string next = content.Substring(i + 1, 1);
                if (next == " " || next == "\t")
                {
                    return i;
                }
            }
            i = i + 1;
        }
        return -1;
    }

    private string _stripInlineComment(string s)
    {
        int idx = _indexOfHashAfterSpace(s);
        if (idx < 0)
        {
            return s;
        }
        return s.Substring(0, idx);
    }

    private int _indexOfHashAfterSpace(string s)
    {
        int i = 0;
        int len = s.Length;
        while (i < len)
        {
            string ch = s.Substring(i, 1);
            if (ch == "#")
            {
                if (i == 0)
                {
                    return i;
                }
                string prev = s.Substring(i - 1, 1);
                if (prev == " " || prev == "\t")
                {
                    return i;
                }
            }
            i = i + 1;
        }
        return -1;
    }

    private YamlNode _nullNode()
    {
        YamlNode n = new YamlNode();
        n.Kind = YamlNodeKind.Scalar;
        n.ScalarKind = YamlScalarKind.Null;
        n.Scalar = "";
        return n;
    }
}