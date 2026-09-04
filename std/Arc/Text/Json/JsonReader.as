namespace Arc.Text.Json;

using Arc.Text;

// JSON 流式读取器——L2 Text Json Stable 最小面。
// Substring 逐字符；实例方法调用一律 this.；无按位运算。
public class JsonReader
{
    private string _json;
    private int _pos;
    private int _length;
    private JsonTokenType _tokenType;
    private string _tempString;

    public JsonTokenType TokenType
    {
        get { return _tokenType; }
    }

    public JsonReader(string json)
    {
        _json = json;
        _pos = 0;
        _length = json.Length;
        _tokenType = JsonTokenType.None;
        _tempString = "";
    }

    public bool Read()
    {
        _skipWhitespace();
        if (_pos >= _length)
        {
            _tokenType = JsonTokenType.None;
            return false;
        }

        string c = _json.Substring(_pos, 1);

        if (c == "{")
        {
            _tokenType = JsonTokenType.StartObject;
            _pos = _pos + 1;
        }
        else if (c == "}")
        {
            _tokenType = JsonTokenType.EndObject;
            _pos = _pos + 1;
        }
        else if (c == "[")
        {
            _tokenType = JsonTokenType.StartArray;
            _pos = _pos + 1;
        }
        else if (c == "]")
        {
            _tokenType = JsonTokenType.EndArray;
            _pos = _pos + 1;
        }
        else if (c == ",")
        {
            _pos = _pos + 1;
            return this.Read();
        }
        else if (c == ":")
        {
            _pos = _pos + 1;
            return this.Read();
        }
        else if (c == "\"")
        {
            string str = _readString();
            _pos = _pos + 1;
            int savedPos = _pos;
            _skipWhitespace();
            if (_pos < _length && _json.Substring(_pos, 1) == ":")
            {
                _tokenType = JsonTokenType.PropertyName;
                _pos = savedPos;
            }
            else
            {
                _tokenType = JsonTokenType.String;
                _pos = savedPos;
            }
            _tempString = str;
        }
        else if (c == "t")
        {
            if (!_expectLiteral("true"))
            {
                _tokenType = JsonTokenType.None;
                return false;
            }
            _tokenType = JsonTokenType.True;
        }
        else if (c == "f")
        {
            if (!_expectLiteral("false"))
            {
                _tokenType = JsonTokenType.None;
                return false;
            }
            _tokenType = JsonTokenType.False;
        }
        else if (c == "n")
        {
            if (!_expectLiteral("null"))
            {
                _tokenType = JsonTokenType.None;
                return false;
            }
            _tokenType = JsonTokenType.Null;
        }
        else if (c == "-" || _isDigit(c))
        {
            _readNumber();
            _tokenType = JsonTokenType.Number;
        }
        else
        {
            _tokenType = JsonTokenType.None;
            return false;
        }

        return true;
    }

    public string GetString()
    {
        if (_tokenType == JsonTokenType.String || _tokenType == JsonTokenType.PropertyName)
        {
            return _tempString;
        }
        return "";
    }

    public int GetInt32()
    {
        if (_tokenType == JsonTokenType.Number)
        {
            return _parseInt(_tempString);
        }
        return 0;
    }

    /// <summary>当前 token 的原始文本：String / PropertyName / Number 返回 _tempString；其余返回空串。
    /// （AIToolArgsReader 依赖：按字段名索引后需以原文做 long/double 等二次解析。）</summary>
    public string GetRawText()
    {
        if (_tokenType == JsonTokenType.String || _tokenType == JsonTokenType.PropertyName || _tokenType == JsonTokenType.Number)
        {
            return _tempString;
        }
        return "";
    }

    public bool GetBoolean()
    {
        return _tokenType == JsonTokenType.True;
    }

    public void Skip()
    {
        if (_tokenType == JsonTokenType.StartObject || _tokenType == JsonTokenType.StartArray)
        {
            int depth = 1;
            while (depth > 0 && this.Read())
            {
                if (_tokenType == JsonTokenType.StartObject || _tokenType == JsonTokenType.StartArray)
                {
                    depth = depth + 1;
                }
                else if (_tokenType == JsonTokenType.EndObject || _tokenType == JsonTokenType.EndArray)
                {
                    depth = depth - 1;
                }
            }
        }
    }

    private void _skipWhitespace()
    {
        // Arc：while 内 break 当前为空操作；用 cont 标志退出。
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _json.Substring(_pos, 1);
            if (c == " " || c == "\t" || c == "\n" || c == "\r")
            {
                _pos = _pos + 1;
            }
            else
            {
                cont = false;
            }
        }
    }

    private bool _isDigit(string c)
    {
        return c == "0" || c == "1" || c == "2" || c == "3" || c == "4"
            || c == "5" || c == "6" || c == "7" || c == "8" || c == "9";
    }

    private bool _expectLiteral(string expected)
    {
        int len = expected.Length;
        if (_pos + len > _length)
        {
            return false;
        }
        if (_json.Substring(_pos, len) != expected)
        {
            return false;
        }
        _pos = _pos + len;
        return true;
    }

    private string _readString()
    {
        _pos = _pos + 1;
        StringBuilder sb = new StringBuilder();
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _json.Substring(_pos, 1);
            if (c == "\"")
            {
                cont = false;
            }
            else if (c == "\\")
            {
                _pos = _pos + 1;
                if (_pos >= _length)
                {
                    cont = false;
                }
                else
                {
                    string ec = _json.Substring(_pos, 1);
                    if (ec == "\"")
                    {
                        sb.Append("\"");
                    }
                    else if (ec == "\\")
                    {
                        sb.Append("\\");
                    }
                    else if (ec == "/")
                    {
                        sb.Append("/");
                    }
                    else if (ec == "n")
                    {
                        sb.Append("\n");
                    }
                    else if (ec == "r")
                    {
                        sb.Append("\r");
                    }
                    else if (ec == "t")
                    {
                        sb.Append("\t");
                    }
                    else if (ec == "b")
                    {
                        sb.Append((char)8);
                    }
                    else if (ec == "f")
                    {
                        sb.Append((char)12);
                    }
                    else if (ec == "u")
                    {
                        // \uXXXX：4 位 hex → codepoint → UTF-8 字节序列（Append(char) 为单字节写入）。
                        // p 为局部游标：分支结束时 _pos = p - 1，由外层统一 +1 抵消。
                        int p = _pos + 1;
                        int cp = 0;
                        int n = 0;
                        while (n < 4 && p < _length)
                        {
                            int hv = this._hexDigit(_json.Substring(p, 1));
                            if (hv < 0)
                            {
                                break;
                            }
                            cp = cp * 16 + hv;
                            p = p + 1;
                            n = n + 1;
                        }
                        if (n == 4 && cp >= 55296 && cp <= 56319
                            && p + 1 < _length
                            && _json.Substring(p, 1) == "\\"
                            && _json.Substring(p + 1, 1) == "u")
                        {
                            // 高位代理（0xD800-0xDBFF）后随 \uXXXX：解析低半，成对则合并为增补平面 codepoint。
                            int p2 = p + 2;
                            int lo = 0;
                            int m = 0;
                            while (m < 4 && p2 < _length)
                            {
                                int lv = this._hexDigit(_json.Substring(p2, 1));
                                if (lv < 0)
                                {
                                    break;
                                }
                                lo = lo * 16 + lv;
                                p2 = p2 + 1;
                                m = m + 1;
                            }
                            if (m == 4 && lo >= 56320 && lo <= 57343)
                            {
                                cp = 65536 + (cp - 55296) * 1024 + (lo - 56320);
                                p = p2;
                            }
                        }
                        this._appendUtf8(sb, cp);
                        _pos = p - 1;
                    }
                    else
                    {
                        sb.Append(ec);
                    }
                    _pos = _pos + 1;
                }
            }
            else
            {
                sb.Append(c);
                _pos = _pos + 1;
            }
        }
        return sb.ToString();
    }

    private void _readNumber()
    {
        int start = _pos;
        if (_pos < _length && _json.Substring(_pos, 1) == "-")
        {
            _pos = _pos + 1;
        }
        while (_pos < _length && _isDigit(_json.Substring(_pos, 1)))
        {
            _pos = _pos + 1;
        }
        if (_pos < _length && _json.Substring(_pos, 1) == ".")
        {
            _pos = _pos + 1;
            while (_pos < _length && _isDigit(_json.Substring(_pos, 1)))
            {
                _pos = _pos + 1;
            }
        }
        if (_pos < _length && (_json.Substring(_pos, 1) == "e" || _json.Substring(_pos, 1) == "E"))
        {
            _pos = _pos + 1;
            if (_pos < _length && (_json.Substring(_pos, 1) == "+" || _json.Substring(_pos, 1) == "-"))
            {
                _pos = _pos + 1;
            }
            while (_pos < _length && _isDigit(_json.Substring(_pos, 1)))
            {
                _pos = _pos + 1;
            }
        }
        _tempString = _json.Substring(start, _pos - start);
    }

    private int _digit(string c)
    {
        if (c == "0") { return 0; }
        if (c == "1") { return 1; }
        if (c == "2") { return 2; }
        if (c == "3") { return 3; }
        if (c == "4") { return 4; }
        if (c == "5") { return 5; }
        if (c == "6") { return 6; }
        if (c == "7") { return 7; }
        if (c == "8") { return 8; }
        if (c == "9") { return 9; }
        return 0;
    }

    /// <summary>单字符 hex 值（0-9/a-f/A-F）；非法返回 -1。供 \uXXXX 解析。</summary>
    private int _hexDigit(string c)
    {
        if (this._isDigit(c)) { return this._digit(c); }
        if (c == "a" || c == "A") { return 10; }
        if (c == "b" || c == "B") { return 11; }
        if (c == "c" || c == "C") { return 12; }
        if (c == "d" || c == "D") { return 13; }
        if (c == "e" || c == "E") { return 14; }
        if (c == "f" || c == "F") { return 15; }
        return -1;
    }

    /// <summary>codepoint → UTF-8 字节序列逐字节追加（string 为 UTF-8 字节串；除法/取模拆位，
    /// 规避按位运算；Append(char) 走 rt_text_sb_append_char 单字节写入）。</summary>
    private void _appendUtf8(StringBuilder sb, int cp)
    {
        if (cp < 128)
        {
            sb.Append((char)cp);
        }
        else if (cp < 2048)
        {
            sb.Append((char)(192 + (cp / 64)));
            sb.Append((char)(128 + (cp % 64)));
        }
        else if (cp < 65536)
        {
            sb.Append((char)(224 + (cp / 4096)));
            sb.Append((char)(128 + ((cp / 64) % 64)));
            sb.Append((char)(128 + (cp % 64)));
        }
        else
        {
            sb.Append((char)(240 + (cp / 262144)));
            sb.Append((char)(128 + ((cp / 4096) % 64)));
            sb.Append((char)(128 + ((cp / 64) % 64)));
            sb.Append((char)(128 + (cp % 64)));
        }
    }

    private int _parseInt(string s)
    {
        if (s.Length == 0)
        {
            return 0;
        }
        int result = 0;
        int sign = 1;
        int i = 0;
        if (s.Substring(0, 1) == "-")
        {
            sign = -1;
            i = 1;
        }
        bool cont = true;
        while (cont && i < s.Length)
        {
            string c = s.Substring(i, 1);
            if (c == "." || c == "e" || c == "E")
            {
                cont = false;
            }
            else
            {
                if (_isDigit(c))
                {
                    result = result * 10 + _digit(c);
                }
                i = i + 1;
            }
        }
        return result * sign;
    }
}
