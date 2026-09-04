namespace Arc.Text.Yaml;

// YamlWriter 的配置选项 —— 内部实现细节，非用户面契约（YAML 为 DOM 优先，
// 开发者经 YamlSerializerOptions 配置缩进，不直接触碰本类型）。
// 诚实差异：YAML 本为块风格（无紧凑单行模式），故不含 Json/Xml 的 Indented 字段；
// 仅保留对 YAML 有意义的 IndentChars。
internal class YamlWriterOptions
{
    // 缩进字符（默认 2 个空格）
    public string IndentChars;

    public YamlWriterOptions()
    {
        IndentChars = "  ";
    }
}