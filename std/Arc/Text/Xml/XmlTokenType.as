namespace Arc.Text.Xml;

// XML token 类型，描述 XML 读取器当前位置的节点类型。
// 对标 C# System.Xml.XmlNodeType。
public enum XmlTokenType
{
    // 初始状态
    None,

    // XML 声明 <?xml ...?>
    XmlDeclaration,

    // 元素开始标签 <element>
    StartElement,

    // 元素结束标签 </element>
    EndElement,

    // 自闭合元素 <element/>
    EmptyElement,

    // 文本内容
    Text,

    // CDATA 段 <![CDATA[...]]>
    CData,

    // 注释 <!-- ... -->
    Comment,

    // 属性（在 StartElement 内部读取属性列表时）
    Attribute,

    // 文档结束
    EndDocument
}
