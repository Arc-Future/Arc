namespace Arc.Text.Xml;

// XML 序列化器——Stable 最小面：Serialize(IXmlSerializable)。
// Deserialize&lt;T&gt; 未立宪后置（同 Json）；勿在此加空壳 API。
public static class XmlSerializer
{
    public static string Serialize(IXmlSerializable value)
    {
        return Serialize(value, XmlSerializerOptions.Default);
    }

    public static string Serialize(IXmlSerializable value, XmlSerializerOptions options)
    {
        if (value == null)
        {
            return "";
        }

        XmlWriterOptions writerOpts = new XmlWriterOptions();
        writerOpts.Indented = options.WriteIndented;
        writerOpts.IndentChars = options.IndentChars;
        writerOpts.OmitXmlDeclaration = options.OmitXmlDeclaration;

        XmlWriter writer = new XmlWriter(writerOpts);
        writer.WriteStartDocument();
        value.WriteXml(writer);
        writer.WriteEndDocument();
        return writer.ToString();
    }
}
