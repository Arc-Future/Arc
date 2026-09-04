namespace Arc.Text.Xml;

using Arc.Text;

// XML 流式读取器——L2 Text Xml Stable 最小面。
// 契约：Read / TokenType / Name / Value / GetAttribute；
// StartElement / EndElement / EmptyElement / Text；属性在起止标签上可查询。
// Arc：while 内 break 当前为空操作——循环退出一律用 cont 标志或 return。
public class XmlReader
{
    private string _xml;
    private int _pos;
    private int _length;
    private XmlTokenType _tokenType;
    private string _name;
    private string _value;
    private string _attrBlob;
    private bool _ignoreComments;
    private bool _ignoreWhitespace;

    public XmlTokenType TokenType
    {
        get { return _tokenType; }
    }

    public string Name
    {
        get { return _name; }
    }

    public string Value
    {
        get { return _value; }
    }

    public XmlReader(string xml)
    {
        _xml = xml;
        _pos = 0;
        _length = xml.Length;
        _tokenType = XmlTokenType.None;
        _name = "";
        _value = "";
        _attrBlob = "";
        _ignoreComments = true;
        _ignoreWhitespace = true;
    }

    public XmlReader(string xml, XmlReaderOptions options)
    {
        _xml = xml;
        _pos = 0;
        _length = xml.Length;
        _tokenType = XmlTokenType.None;
        _name = "";
        _value = "";
        _attrBlob = "";
        _ignoreComments = options.IgnoreComments;
        _ignoreWhitespace = options.IgnoreWhitespace;
    }

    public bool Read()
    {
        SkipWhitespace();
        if (_pos >= _length)
        {
            _tokenType = XmlTokenType.EndDocument;
            return false;
        }

        string c = _xml.Substring(_pos, 1);
        if (c == "<")
        {
            if (_pos + 1 < _length && _xml.Substring(_pos + 1, 1) == "/")
            {
                _readEndElement();
                return true;
            }
            if (_pos + 3 < _length && _xml.Substring(_pos, 4) == "<!--")
            {
                // Stable：恒跳过注释；IgnoreComments=false → Comment token 后置
                _skipUntil("-->");
                return this.Read();
            }
            if (_pos + 4 < _length && _xml.Substring(_pos, 5) == "<?xml")
            {
                _skipUntil("?>");
                return this.Read();
            }
            _readStartElement();
            return true;
        }

        ReadText();
        if (_ignoreWhitespace)
        {
            if (_isWhitespaceText(_value))
            {
                return this.Read();
            }
        }
        return true;
    }

    // 查询当前 StartElement / EmptyElement 上的属性值；缺失返回 ""。
    public string GetAttribute(string name)
    {
        if (name == null || name.Length == 0 || _attrBlob.Length == 0)
        {
            return "";
        }
        int i = 0;
        int len = _attrBlob.Length;
        bool cont = true;
        while (cont && i < len)
        {
            int nameStart = i;
            bool nameCont = true;
            while (nameCont && i < len)
            {
                if (_attrBlob.Substring(i, 1) == "\n")
                {
                    nameCont = false;
                }
                else
                {
                    i = i + 1;
                }
            }
            string an = _attrBlob.Substring(nameStart, i - nameStart);
            if (i < len)
            {
                i = i + 1;
            }
            int valStart = i;
            bool valCont = true;
            while (valCont && i < len)
            {
                if (_attrBlob.Substring(i, 1) == "\n")
                {
                    valCont = false;
                }
                else
                {
                    i = i + 1;
                }
            }
            string av = _attrBlob.Substring(valStart, i - valStart);
            if (i < len)
            {
                i = i + 1;
            }
            if (an == name)
            {
                return av;
            }
        }
        return "";
    }

    private void _readStartElement()
    {
        _pos = _pos + 1;
        int start = _pos;
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _xml.Substring(_pos, 1);
            if (c == " " || c == ">" || c == "/" || c == "\t" || c == "\n" || c == "\r")
            {
                cont = false;
            }
            else
            {
                _pos = _pos + 1;
            }
        }
        _name = _xml.Substring(start, _pos - start);
        _value = "";
        _attrBlob = "";

        _parseAttributes();

        if (_pos < _length && _xml.Substring(_pos, 1) == "/")
        {
            _pos = _pos + 1;
            if (_pos < _length && _xml.Substring(_pos, 1) == ">")
            {
                _pos = _pos + 1;
            }
            _tokenType = XmlTokenType.EmptyElement;
            return;
        }

        if (_pos < _length && _xml.Substring(_pos, 1) == ">")
        {
            _pos = _pos + 1;
        }
        _tokenType = XmlTokenType.StartElement;
    }

    private void _parseAttributes()
    {
        bool cont = true;
        while (cont && _pos < _length)
        {
            SkipWhitespace();
            if (_pos >= _length)
            {
                cont = false;
            }
            else
            {
                string c = _xml.Substring(_pos, 1);
                if (c == "/" || c == ">")
                {
                    cont = false;
                }
                else
                {
                    int nameStart = _pos;
                    bool nameCont = true;
                    while (nameCont && _pos < _length)
                    {
                        string nc = _xml.Substring(_pos, 1);
                        if (nc == "=" || nc == " " || nc == "\t" || nc == "\n" || nc == "\r" || nc == "/" || nc == ">")
                        {
                            nameCont = false;
                        }
                        else
                        {
                            _pos = _pos + 1;
                        }
                    }
                    string attrName = _xml.Substring(nameStart, _pos - nameStart);
                    SkipWhitespace();
                    if (_pos < _length && _xml.Substring(_pos, 1) == "=")
                    {
                        _pos = _pos + 1;
                    }
                    SkipWhitespace();
                    string attrVal = "";
                    if (_pos < _length)
                    {
                        string q = _xml.Substring(_pos, 1);
                        if (q == "\"" || q == "'")
                        {
                            _pos = _pos + 1;
                            int valStart = _pos;
                            bool valCont = true;
                            while (valCont && _pos < _length)
                            {
                                if (_xml.Substring(_pos, 1) == q)
                                {
                                    valCont = false;
                                }
                                else
                                {
                                    _pos = _pos + 1;
                                }
                            }
                            attrVal = _unescapeAttr(_xml.Substring(valStart, _pos - valStart));
                            if (_pos < _length)
                            {
                                _pos = _pos + 1;
                            }
                        }
                    }
                    if (attrName.Length > 0)
                    {
                        _attrBlob = _attrBlob + attrName + "\n" + attrVal + "\n";
                    }
                }
            }
        }
    }

    private string _unescapeAttr(string text)
    {
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int len = text.Length;
        while (i < len)
        {
            string ch = text.Substring(i, 1);
            if (ch == "&")
            {
                if (i + 5 <= len && text.Substring(i, 5) == "&amp;")
                {
                    sb.Append("&");
                    i = i + 5;
                }
                else if (i + 4 <= len && text.Substring(i, 4) == "&lt;")
                {
                    sb.Append("<");
                    i = i + 4;
                }
                else if (i + 4 <= len && text.Substring(i, 4) == "&gt;")
                {
                    sb.Append(">");
                    i = i + 4;
                }
                else if (i + 6 <= len && text.Substring(i, 6) == "&quot;")
                {
                    sb.Append("\"");
                    i = i + 6;
                }
                else if (i + 6 <= len && text.Substring(i, 6) == "&apos;")
                {
                    sb.Append("'");
                    i = i + 6;
                }
                else
                {
                    sb.Append(ch);
                    i = i + 1;
                }
            }
            else
            {
                // XML 属性值归一化（2.11 行尾归一 + 3.3.3）：字面 \r\n、\r、\n、\t
                // 解析期归一为空格——同时消除 _attrBlob 以 "\n" 作分隔符的歧义；
                // 实体解码（&amp; 等）在其后，解码产物不受影响。
                if (ch == "\r")
                {
                    if (i + 1 < len && text.Substring(i + 1, 1) == "\n")
                    {
                        i = i + 1;
                    }
                    sb.Append(" ");
                }
                else if (ch == "\n" || ch == "\t")
                {
                    sb.Append(" ");
                }
                else
                {
                    sb.Append(ch);
                }
                i = i + 1;
            }
        }
        return sb.ToString();
    }

    private void _readEndElement()
    {
        _pos = _pos + 2;
        int start = _pos;
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _xml.Substring(_pos, 1);
            if (c == ">")
            {
                cont = false;
            }
            else
            {
                _pos = _pos + 1;
            }
        }
        _name = _xml.Substring(start, _pos - start);
        _value = "";
        _attrBlob = "";
        if (_pos < _length && _xml.Substring(_pos, 1) == ">")
        {
            _pos = _pos + 1;
        }
        _tokenType = XmlTokenType.EndElement;
    }

    private void ReadText()
    {
        int start = _pos;
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _xml.Substring(_pos, 1);
            if (c == "<")
            {
                cont = false;
            }
            else
            {
                _pos = _pos + 1;
            }
        }
        _name = "";
        _value = _unescapeAttr(_xml.Substring(start, _pos - start));
        _attrBlob = "";
        _tokenType = XmlTokenType.Text;
    }

    private bool _isWhitespaceText(string text)
    {
        int i = 0;
        while (i < text.Length)
        {
            string c = text.Substring(i, 1);
            if (c != " " && c != "\t" && c != "\n" && c != "\r")
            {
                return false;
            }
            i = i + 1;
        }
        return true;
    }

    private void _skipUntil(string marker)
    {
        int mlen = marker.Length;
        bool cont = true;
        while (cont && _pos + mlen <= _length)
        {
            if (_xml.Substring(_pos, mlen) == marker)
            {
                _pos = _pos + mlen;
                cont = false;
            }
            else
            {
                _pos = _pos + 1;
            }
        }
    }

    private void SkipWhitespace()
    {
        bool cont = true;
        while (cont && _pos < _length)
        {
            string c = _xml.Substring(_pos, 1);
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
}
