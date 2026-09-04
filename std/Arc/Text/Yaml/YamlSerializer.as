namespace Arc.Text.Yaml;

// YAML 序列化器 —— 家族统一门面（对齐 JsonSerializer / XmlSerializer）。
//
// 家族统一约定（Json / Xml / Yaml）：
//   - 门面类名：XxxSerializer（JsonSerializer / XmlSerializer / YamlSerializer）
//   - 方法形状：Serialize(value) / Serialize(value, options)；Parse/Deserialize 读路径
//   - 选项类：XxxSerializerOptions（统一 Default 静态属性）
//   - 低层读写：XxxReader / XxxWriter + XxxWriterOptions
//
// 诚实差异（文档化，非伪装一致）：Json/Xml 为「契约优先」（IXxxSerializable
// 流式接口 WriteXxx(XxxWriter)），故其低层 XxxReader/XxxWriter/XxxWriterOptions 是
// 公开用户面 API；Yaml 为「DOM 优先」（YamlNode 文档树），developer 只经本门面
// Parse/Serialize 消费，低层 YamlParser/YamlWriter 为 internal 实现细节——
// YAML 的缩进结构天然适合整树读取与操作，是 Arc.AI Agent Skills frontmatter
// 提取的底座。门面类名、方法形状、序列化选项类命名仍与家族统一。
public static class YamlSerializer
{
    /// <summary>解析 YAML 文本为 YamlNode 文档树。</summary>
    public static YamlNode Parse(string text)
    {
        YamlParser parser = new YamlParser();
        return parser.Parse(text);
    }

    /// <summary>将 YamlNode 文档树序列化为块风格 YAML 文本。</summary>
    public static string Serialize(YamlNode node)
    {
        return Serialize(node, YamlSerializerOptions.Default);
    }

    /// <summary>将 YamlNode 文档树序列化为块风格 YAML 文本，自定义选项。</summary>
    public static string Serialize(YamlNode node, YamlSerializerOptions options)
    {
        YamlWriterOptions writerOpts = new YamlWriterOptions();
        writerOpts.IndentChars = options != null && options.IndentChars != null
            ? options.IndentChars
            : "  ";
        YamlWriter writer = new YamlWriter(writerOpts);
        return writer.WriteString(node);
    }
}