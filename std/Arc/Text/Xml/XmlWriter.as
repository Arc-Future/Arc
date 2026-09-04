namespace Arc.Text.Xml;

using Arc.Text;

// XML 流式写入器——L2 Text Xml Stable 最小面。
public class XmlWriter
{
    private StringBuilder _sb;
    private XmlWriterOptions _options;
    private int _currentDepth;
    private bool _pendingElementClose;
    private string _openElements;
    private int _openCount;

    public XmlWriter()
    {
        _sb = new StringBuilder(512);
        _options = new XmlWriterOptions();
        _currentDepth = 0;
        _pendingElementClose = false;
        _openElements = "";
        _openCount = 0;
    }

    public XmlWriter(XmlWriterOptions options)
    {
        _sb = new StringBuilder(512);
        _options = options;
        _currentDepth = 0;
        _pendingElementClose = false;
        _openElements = "";
        _openCount = 0;
    }

    public void WriteStartDocument()
    {
        if (!_options.OmitXmlDeclaration)
        {
            _sb.Append("<?xml version=\"1.0\"");
            _sb.Append(" encoding=\"");
            _sb.Append(_options.Encoding);
            _sb.Append("\"?>");
            if (_options.Indented)
            {
                _sb.Append(_options.NewLineChars);
            }
        }
    }

    public void WriteEndDocument()
    {
        while (_openCount > 0)
        {
            _popElement(true);
        }
    }

    public void WriteStartElement(string localName)
    {
        _closePendingStartTag();
        WriteNewLineAndIndent();
        _sb.Append("<");
        _sb.Append(localName);
        _pendingElementClose = true;
        _pushElement(localName);
    }

    public void WriteEndElement()
    {
        if (_pendingElementClose)
        {
            _sb.Append(" />");
            _pendingElementClose = false;
            _popElement(false);
        }
        else
        {
            _popElement(true);
        }
    }

    public void WriteAttributeString(string localName, string value)
    {
        _sb.Append(" ");
        _sb.Append(localName);
        _sb.Append("=\"");
        WriteAttributeValue(value);
        _sb.Append("\"");
    }

    public void WriteElementString(string localName, string value)
    {
        this.WriteStartElement(localName);
        this.WriteString(value);
        this.WriteEndElement();
    }

    public void WriteString(string text)
    {
        _closePendingStartTag();
        _writeTextEscaped(text);
    }

    public string ToString()
    {
        if (_openCount > 0)
        {
            this.WriteEndDocument();
        }
        return _sb.ToString();
    }

    public void Reset()
    {
        _sb.Clear();
        _currentDepth = 0;
        _pendingElementClose = false;
        _openElements = "";
        _openCount = 0;
    }

    private void _pushElement(string name)
    {
        _openCount = _openCount + 1;
        _currentDepth = _currentDepth + 1;
        if (_openElements.Length > 0)
        {
            _openElements = _openElements + "|" + name;
        }
        else
        {
            _openElements = name;
        }
    }

    private void _popElement(bool writeCloseTag)
    {
        if (_openCount <= 0)
        {
            return;
        }

        int lastSep = -1;
        int i = _openElements.Length - 1;
        // Arc：while 内 break 当前为空操作；用 cont 标志退出。
        bool cont = true;
        while (cont && i >= 0)
        {
            string c = _openElements.Substring(i, 1);
            if (c == "|")
            {
                lastSep = i;
                cont = false;
            }
            else
            {
                i = i - 1;
            }
        }

        string name = "";
        if (lastSep >= 0)
        {
            name = _openElements.Substring(lastSep + 1, _openElements.Length - lastSep - 1);
            _openElements = _openElements.Substring(0, lastSep);
        }
        else
        {
            name = _openElements;
            _openElements = "";
        }
        _openCount = _openCount - 1;
        _currentDepth = _currentDepth - 1;

        if (writeCloseTag)
        {
            WriteNewLineAndIndent();
            _sb.Append("</");
            _sb.Append(name);
            _sb.Append(">");
        }
    }

    private void _closePendingStartTag()
    {
        if (_pendingElementClose)
        {
            _sb.Append(">");
            _pendingElementClose = false;
        }
    }

    private void WriteNewLineAndIndent()
    {
        if (_options.Indented)
        {
            _sb.Append(_options.NewLineChars);
            int i = 0;
            while (i < _currentDepth)
            {
                _sb.Append(_options.IndentChars);
                i = i + 1;
            }
        }
    }

    private void _writeTextEscaped(string text)
    {
        int len = text.Length;
        int i = 0;
        while (i < len)
        {
            string ch = text.Substring(i, 1);
            if (ch == "<")
            {
                _sb.Append("&lt;");
            }
            else if (ch == "&")
            {
                _sb.Append("&amp;");
            }
            else if (ch == ">")
            {
                _sb.Append("&gt;");
            }
            else
            {
                _sb.Append(ch);
            }
            i = i + 1;
        }
    }

    private void WriteAttributeValue(string text)
    {
        int len = text.Length;
        int i = 0;
        while (i < len)
        {
            string ch = text.Substring(i, 1);
            if (ch == "\"")
            {
                _sb.Append("&quot;");
            }
            else if (ch == "&")
            {
                _sb.Append("&amp;");
            }
            else if (ch == "<")
            {
                _sb.Append("&lt;");
            }
            else
            {
                _sb.Append(ch);
            }
            i = i + 1;
        }
    }
}
