namespace Arc.Text.Yaml;

using Arc.Text;
using Arc.Collections;

// YAML 文档写出器（写路径）—— 将 YamlNode 文档树序列化为块风格 YAML 文本。
//
// 覆盖：
//   - 块映射 `key: value` / 块序列 `- item`，含任意缩进嵌套
//   - 空映射 `{}` / 空序列 `[]` 内联写出
//   - 标量按 ScalarKind 写出：null/bool/int/float/string
//   - 字符串引号策略：需要时双引号（含转义），允许明文时裸写
//   - 多行字符串 → 块字面量，按尾随换行选 chomping 指示符（无尾随`|-`/恰一`|`/多个`|+`），
//     与解析器 _applyChomp 对偶，保证解析-写出往返保真
//
// 实现细节，非用户面契约：开发者经 YamlSerializer.Serialize 消费，不直接触碰本类型。
// 诚实边界（文档化，非完备）：
//   - 不写锚点/别名/自定义 tag/文档标记/指令；复杂键（映射/序列键）降级为标量
//   - 浮点保留 Scalar 原始文本；int 以强类型 LongValue 写出（64 位保真）
//   - 折叠块标量 `>` 读写不对称（写出统一字面量 `|`，折叠语义不重放）
internal class YamlWriter
{
    private StringBuilder _sb;
    private string _indentString;

    public YamlWriter()
    {
        _sb = new StringBuilder();
        _indentString = "  ";
    }

    public YamlWriter(YamlWriterOptions options)
    {
        _sb = new StringBuilder();
        _indentString = options != null && options.IndentChars != null
            ? options.IndentChars
            : "  ";
    }

    /// <summary>将 YamlNode 文档树序列化为块风格 YAML 文本（末尾换行）。</summary>
    public string WriteString(YamlNode root)
    {
        _sb.Clear();
        if (root == null)
        {
            _sb.Append("null\n");
            return _sb.ToString();
        }
        _writeNode(root, 0);
        return _sb.ToString();
    }

    // ─────────────────────────── 三类节点 ───────────────────────────

    private void _writeNode(YamlNode n, int depth)
    {
        if (n.Kind == YamlNodeKind.Mapping)
        {
            _writeMapping(n, depth);
        }
        else if (n.Kind == YamlNodeKind.Sequence)
        {
            _writeSequence(n, depth);
        }
        else
        {
            _indent(depth);
            _appendScalar(n);
            _sb.Append("\n");
        }
    }

    private void _writeMapping(YamlNode map, int depth)
    {
        int n = map.Entries.Count;
        int i = 0;
        while (i < n)
        {
            YamlMapEntry e = map.Entries[i];
            _indent(depth);
            _sb.Append(_scalarKey(e.Key));
            _sb.Append(":");
            YamlNode v = e.Value;
            if (v == null)
            {
                _sb.Append(" null\n");
            }
            else if (v.Kind == YamlNodeKind.Scalar)
            {
                if (_isMultiline(v.Scalar))
                {
                    _appendBlockScalar(v, depth);
                }
                else
                {
                    _sb.Append(" ");
                    _appendScalar(v);
                    _sb.Append("\n");
                }
            }
            else if (v.Kind == YamlNodeKind.Mapping)
            {
                if (v.Entries.Count == 0)
                {
                    _sb.Append(" {}\n");
                }
                else
                {
                    _sb.Append("\n");
                    _writeMapping(v, depth + 1);
                }
            }
            else
            {
                if (v.Items.Count == 0)
                {
                    _sb.Append(" []\n");
                }
                else
                {
                    _sb.Append("\n");
                    _writeSequence(v, depth + 1);
                }
            }
            i = i + 1;
        }
    }

    private void _writeSequence(YamlNode seq, int depth)
    {
        int n = seq.Items.Count;
        int i = 0;
        while (i < n)
        {
            YamlNode item = seq.Items[i];
            _indent(depth);
            _sb.Append("-");
            if (item == null)
            {
                _sb.Append(" null\n");
            }
            else if (item.Kind == YamlNodeKind.Scalar)
            {
                if (_isMultiline(item.Scalar))
                {
                    _appendBlockScalar(item, depth);
                }
                else
                {
                    _sb.Append(" ");
                    _appendScalar(item);
                    _sb.Append("\n");
                }
            }
            else if (item.Kind == YamlNodeKind.Mapping)
            {
                if (item.Entries.Count == 0)
                {
                    _sb.Append(" {}\n");
                }
                else
                {
                    _sb.Append("\n");
                    _writeMapping(item, depth + 1);
                }
            }
            else
            {
                if (item.Items.Count == 0)
                {
                    _sb.Append(" []\n");
                }
                else
                {
                    _sb.Append("\n");
                    _writeSequence(item, depth + 1);
                }
            }
            i = i + 1;
        }
    }

    // ─────────────────────────── 标量写出 ───────────────────────────

    private void _appendScalar(YamlNode n)
    {
        if (n.ScalarKind == YamlScalarKind.Null)
        {
            _sb.Append("null");
            return;
        }
        if (n.ScalarKind == YamlScalarKind.Bool)
        {
            if (n.BoolValue)
            {
                _sb.Append("true");
            }
            else
            {
                _sb.Append("false");
            }
            return;
        }
        if (n.ScalarKind == YamlScalarKind.Int)
        {
            _sb.Append(n.LongValue.ToString());
            return;
        }
        if (n.ScalarKind == YamlScalarKind.Float)
        {
            _sb.Append(n.Scalar);
            return;
        }
        // String
        string s = n.Scalar;
        if (_needsQuote(s))
        {
            _sb.Append(_quoteDouble(s));
        }
        else
        {
            _sb.Append(s);
        }
    }

    /// <summary>映射键：键若是标量则按引号策略写出；否则降级为字符串。</summary>
    private string _scalarKey(YamlNode k)
    {
        if (k == null)
        {
            return "";
        }
        if (k.Kind == YamlNodeKind.Scalar)
        {
            if (k.ScalarKind == YamlScalarKind.Null)
            {
                return "null";
            }
            if (k.ScalarKind == YamlScalarKind.Bool)
            {
                return k.BoolValue ? "true" : "false";
            }
            if (k.ScalarKind == YamlScalarKind.Int)
            {
                return k.LongValue.ToString();
            }
            if (k.ScalarKind == YamlScalarKind.Float)
            {
                return k.Scalar;
            }
            string s = k.Scalar;
            if (_needsQuote(s))
            {
                return _quoteDouble(s);
            }
            return s;
        }
        return "";
    }

    /// <summary>块标量：按尾随换行选择 chomping 指示符（`|-`/`|`/`|+`）写出，保证往返保真。</summary>
    private void _appendBlockScalar(YamlNode v, int depth)
    {
        _sb.Append(" ");
        _sb.Append(_blockIndicator(v.Scalar));
        _sb.Append("\n");
        _writeBlockScalarContent(v.Scalar, depth + 1);
    }

    /// <summary>
    /// 依据尾随换行选择 chomping 指示符：
    /// 无尾随换行 → `|-`（strip）；恰好一个 → `|`（clip）；多个 → `|+`（keep）。
    /// 与解析器 _applyChomp 语义对偶，保证解析-写出往返保真。
    /// </summary>
    private string _blockIndicator(string value)
    {
        if (value == null || value.Length == 0)
        {
            return "|-";
        }
        if (value.Substring(value.Length - 1, 1) != "\n")
        {
            return "|-";
        }
        int n = value.Length;
        int count = 0;
        bool cont = true;
        while (cont && n > 0)
        {
            if (value.Substring(n - 1, 1) == "\n")
            {
                count = count + 1;
                n = n - 1;
            }
            else
            {
                cont = false;
            }
        }
        if (count == 1)
        {
            return "|";
        }
        return "|+";
    }

    /// <summary>块字面量内容行：在既有 `|` 标记后按 depth 缩进逐行写出。</summary>
    private void _writeBlockScalarContent(string value, int depth)
    {
        string[] lines = value.Split("\n");
        int n = lines.Length;
        int i = 0;
        while (i < n)
        {
            _indent(depth);
            _sb.Append(lines[i]);
            _sb.Append("\n");
            i = i + 1;
        }
    }

    private bool _isMultiline(string s)
    {
        return s != null && s.IndexOf("\n") >= 0;
    }

    // ─────────────────────────── 引号策略 ───────────────────────────

    /// <summary>判断字符串是否需要双引号（含转义）以保持语义。</summary>
    private bool _needsQuote(string s)
    {
        if (s == null)
        {
            return true;
        }
        int len = s.Length;
        if (len == 0)
        {
            return true;
        }
        string c0 = s.Substring(0, 1);
        if (_isIndicator(c0))
        {
            return true;
        }
        if (_isAmbiguousScalar(s))
        {
            return true;
        }
        if (_isNumericLike(s))
        {
            return true;
        }
        if (s.IndexOf(": ") >= 0 || s.IndexOf(" #") >= 0)
        {
            return true;
        }
        if (s.IndexOf("\n") >= 0 || s.IndexOf("\t") >= 0 || s.IndexOf("\r") >= 0)
        {
            return true;
        }
        string last = s.Substring(len - 1, 1);
        if (last == " " || last == "\t")
        {
            return true;
        }
        return false;
    }

    private bool _isIndicator(string c)
    {
        return c == "-" || c == "?" || c == ":" || c == "," || c == "["
            || c == "]" || c == "{" || c == "}" || c == "#" || c == "&"
            || c == "*" || c == "!" || c == "|" || c == ">" || c == "'"
            || c == "\"" || c == "%" || c == "@" || c == "`";
    }

    private bool _isAmbiguousScalar(string s)
    {
        string lower = s.ToLower();
        return lower == "null" || lower == "~" || lower == "true" || lower == "false"
            || lower == "yes" || lower == "no" || lower == "on" || lower == "off";
    }

    private bool _isNumericLike(string s)
    {
        int len = s.Length;
        if (len == 0)
        {
            return false;
        }
        string c0 = s.Substring(0, 1);
        if (!(_isDigit(c0)) && c0 != "-" && c0 != "+" && c0 != ".")
        {
            return false;
        }
        // 含数字 → 数字状；纯符号（如 "-"）不算
        bool anyDigit = false;
        int i = 0;
        bool cont = true;
        while (cont && i < len)
        {
            string ch = s.Substring(i, 1);
            if (_isDigit(ch))
            {
                anyDigit = true;
            }
            else if (!(ch == "-" || ch == "+" || ch == "." || ch == "e" || ch == "E"))
            {
                cont = false;
            }
            i = i + 1;
        }
        return anyDigit;
    }

    private bool _isDigit(string c)
    {
        return c >= "0" && c <= "9";
    }

    private string _quoteDouble(string s)
    {
        StringBuilder q = new StringBuilder();
        q.Append("\"");
        int len = s.Length;
        int i = 0;
        while (i < len)
        {
            string ch = s.Substring(i, 1);
            if (ch == "\\")
            {
                q.Append("\\\\");
            }
            else if (ch == "\"")
            {
                q.Append("\\\"");
            }
            else if (ch == "\n")
            {
                q.Append("\\n");
            }
            else if (ch == "\t")
            {
                q.Append("\\t");
            }
            else if (ch == "\r")
            {
                q.Append("\\r");
            }
            else
            {
                q.Append(ch);
            }
            i = i + 1;
        }
        q.Append("\"");
        return q.ToString();
    }

    // ─────────────────────────── 缩进 ───────────────────────────

    private void _indent(int depth)
    {
        int i = 0;
        while (i < depth)
        {
            _sb.Append(_indentString);
            i = i + 1;
        }
    }
}