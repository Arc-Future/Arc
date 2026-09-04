namespace Arc.Text.Json;

using Arc.Text;

// JSON 流式写入器——L2 Text Json Stable 最小面。
// 契约：对象/数组起止、属性名、string/int/bool/null、ToString。
// 不用按位运算、`: this()`、复合赋值、无花括号赋值 if；私有方法调用一律 this. 前缀。
public class JsonWriter
{
    private StringBuilder _sb;
    private bool _indented;
    private bool _escapeForwardSlash;
    private int _currentDepth;
    private bool _pendingComma;
    private string _commaStack;

    public JsonWriter()
    {
        _sb = new StringBuilder(256);
        _indented = false;
        _escapeForwardSlash = false;
        _currentDepth = 0;
        _pendingComma = false;
        _commaStack = "";
    }

    public JsonWriter(bool indented)
    {
        _sb = new StringBuilder(256);
        _indented = indented;
        _escapeForwardSlash = false;
        _currentDepth = 0;
        _pendingComma = false;
        _commaStack = "";
    }

    public JsonWriter(JsonWriterOptions options)
    {
        _sb = new StringBuilder(256);
        _indented = options.Indented;
        _escapeForwardSlash = options.EscapeForwardSlash;
        _currentDepth = 0;
        _pendingComma = false;
        _commaStack = "";
    }

    public void WriteStartObject()
    {
        _writePendingComma();
        _writeIndent();
        _sb.Append("{");
        this.PushContainer();
    }

    public void WriteEndObject()
    {
        _popContainer();
        _writeNewLine();
        _writeIndent();
        _sb.Append("}");
    }

    public void WriteStartArray()
    {
        _writePendingComma();
        _writeIndent();
        _sb.Append("[");
        this.PushContainer();
    }

    public void WriteEndArray()
    {
        _popContainer();
        _writeNewLine();
        _writeIndent();
        _sb.Append("]");
    }

    public void WritePropertyName(string name)
    {
        _writePendingComma();
        _writeNewLine();
        _writeIndent();
        this.WriteStringValue(name);
        _sb.Append(":");
        if (_indented)
        {
            _sb.Append(" ");
        }
        _pendingComma = false;
    }

    public void WriteString(string value)
    {
        _writePendingComma();
        _writeIndent();
        if (value == null)
        {
            _sb.Append("null");
        }
        else
        {
            this.WriteStringValue(value);
        }
    }

    public void WriteNumber(int value)
    {
        _writePendingComma();
        _writeIndent();
        _sb.Append(value);
    }

    public void WriteBoolean(bool value)
    {
        _writePendingComma();
        _writeIndent();
        if (value)
        {
            _sb.Append("true");
        }
        else
        {
            _sb.Append("false");
        }
    }

    public void WriteNull()
    {
        _writePendingComma();
        _writeIndent();
        _sb.Append("null");
    }

    public void WriteString(string propertyName, string value)
    {
        this.WritePropertyName(propertyName);
        this.WriteString(value);
    }

    public void WriteNumber(string propertyName, int value)
    {
        this.WritePropertyName(propertyName);
        this.WriteNumber(value);
    }

    public void WriteBoolean(string propertyName, bool value)
    {
        this.WritePropertyName(propertyName);
        this.WriteBoolean(value);
    }

    public void WriteNull(string propertyName)
    {
        this.WritePropertyName(propertyName);
        this.WriteNull();
    }

    public string ToString()
    {
        return _sb.ToString();
    }

    public void Reset()
    {
        _sb.Clear();
        _currentDepth = 0;
        _pendingComma = false;
        _commaStack = "";
    }

    private void _writePendingComma()
    {
        if (_pendingComma)
        {
            _sb.Append(",");
        }
        _pendingComma = true;
    }

    private void PushContainer()
    {
        _currentDepth = _currentDepth + 1;
        if (_pendingComma)
        {
            _commaStack = _commaStack + "1";
        }
        else
        {
            _commaStack = _commaStack + "0";
        }
        _pendingComma = false;
    }

    private void _popContainer()
    {
        _currentDepth = _currentDepth - 1;
        if (_commaStack.Length == 0)
        {
            _pendingComma = false;
            return;
        }
        string bit = _commaStack.Substring(_commaStack.Length - 1, 1);
        _commaStack = _commaStack.Substring(0, _commaStack.Length - 1);
        _pendingComma = bit == "1";
    }

    private void _writeNewLine()
    {
        if (_indented)
        {
            _sb.Append("\n");
        }
    }

    private void _writeIndent()
    {
        if (_indented)
        {
            int i = 0;
            while (i < _currentDepth)
            {
                _sb.Append("  ");
                i = i + 1;
            }
        }
    }

    private void WriteStringValue(string value)
    {
        _sb.Append("\"");
        int len = value.Length;
        int i = 0;
        while (i < len)
        {
            string ch = value.Substring(i, 1);
            if (ch == "\"")
            {
                _sb.Append("\\\"");
            }
            else if (ch == "\\")
            {
                _sb.Append("\\\\");
            }
            else if (ch == "/")
            {
                if (_escapeForwardSlash)
                {
                    _sb.Append("\\/");
                }
                else
                {
                    _sb.Append("/");
                }
            }
            else if (ch == "\n")
            {
                _sb.Append("\\n");
            }
            else if (ch == "\r")
            {
                _sb.Append("\\r");
            }
            else if (ch == "\t")
            {
                _sb.Append("\\t");
            }
            else
            {
                _sb.Append(ch);
            }
            i = i + 1;
        }
        _sb.Append("\"");
    }
}
