namespace Arc.Text.Xml;

// XmlReader 的配置选项——Stable 最小面接线 IgnoreWhitespace。
// IgnoreComments：Stable 恒跳过注释；Comment token 后置。DTD / 实体展开后置。
public class XmlReaderOptions
{
    // 预留：Comment token 后置前恒为跳过行为
    public bool IgnoreComments;

    // 是否忽略纯空白文本节点
    public bool IgnoreWhitespace;

    public XmlReaderOptions()
    {
        IgnoreComments = true;
        IgnoreWhitespace = true;
    }
}
