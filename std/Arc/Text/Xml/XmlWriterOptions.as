namespace Arc.Text.Xml;

// XmlWriter 的配置选项——Stable 最小面仅接线已实现字段。
public class XmlWriterOptions
{
    // 是否启用缩进格式化输出
    public bool Indented;

    // 缩进字符（默认 2 个空格）
    public string IndentChars;

    // 换行符（默认 \n）
    public string NewLineChars;

    // 是否省略 XML 声明 <?xml version="1.0" encoding="utf-8"?>
    public bool OmitXmlDeclaration;

    // XML 声明中的编码名称
    public string Encoding;

    public XmlWriterOptions()
    {
        Indented = false;
        IndentChars = "  ";
        NewLineChars = "\n";
        OmitXmlDeclaration = false;
        Encoding = "utf-8";
    }
}
