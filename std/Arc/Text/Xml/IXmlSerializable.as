namespace Arc.Text.Xml;

// XML 序列化接口 —— 类型实现此接口以支持将自身序列化为 XML。
public interface IXmlSerializable
{
    void WriteXml(XmlWriter writer);
}
