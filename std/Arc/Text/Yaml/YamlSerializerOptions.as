namespace Arc.Text.Yaml;

// YamlSerializer 的配置选项 —— 家族统一（对齐 JsonSerializerOptions / XmlSerializerOptions）。
// 诚实差异：YAML 无紧凑模式（块风格即格式），故不含 Json/Xml 的 WriteIndented；
// 仅保留对 YAML 有意义的 IndentChars（映射到 YamlWriterOptions.IndentChars）。
public class YamlSerializerOptions
{
    // 缩进字符串（映射到 YamlWriterOptions.IndentChars）
    public string IndentChars;

    /// <summary>默认选项（static readonly 惰性单例：首触构造一次、线程安全）。</summary>
    public static readonly YamlSerializerOptions Default = new YamlSerializerOptions();

    public YamlSerializerOptions()
    {
        IndentChars = "  ";
    }
}