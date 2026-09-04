namespace Arc.Text.Xml;

// XmlSerializer 的配置选项——Stable 最小面仅接线已实现字段。
// RootElementName / DefaultNamespace / UseNamespaces 随注解·命名空间后置，不在此伪暴露。
public class XmlSerializerOptions
{
    // 是否缩进格式化输出
    public bool WriteIndented;

    // 缩进字符串
    public string IndentChars;

    // 是否省略 XML 声明
    public bool OmitXmlDeclaration;

    /// <summary>默认选项（static readonly 惰性单例：首触构造一次、线程安全）。</summary>
    public static readonly XmlSerializerOptions Default = new XmlSerializerOptions();

    public XmlSerializerOptions()
    {
        WriteIndented = true;
        IndentChars = "  ";
        OmitXmlDeclaration = false;
    }
}
