namespace Arc.Text.Yaml;

// YAML 映射条目 —— 键/值对。YamlNode 的 Mapping 以有序 List<YamlMapEntry> 保存，
// 保持键的声明序（round-trip 稳定），与 Json 对象语义对齐。
public class YamlMapEntry
{
    public YamlNode Key;
    public YamlNode Value;

    public YamlMapEntry()
    {
        this.Key = null;
        this.Value = null;
    }

    public YamlMapEntry(YamlNode key, YamlNode value)
    {
        this.Key = key;
        this.Value = value;
    }
}