namespace Arc.Text.Xml;

// 手写 XML 恢复钩子——类型自行从 XmlReader 填字段。
// 不是 XmlSerializer.Deserialize&lt;T&gt;：泛型工厂 / 注解 / 源生成未立宪，禁止伪实现。
public interface IXmlDeserializable
{
    void ReadXml(XmlReader reader);
}
