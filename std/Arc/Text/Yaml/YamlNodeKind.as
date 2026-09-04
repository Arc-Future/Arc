namespace Arc.Text.Yaml;

// YAML 节点种类 —— 对标 JSON 的对象/数组/值三分，对应 YAML 的映射/序列/标量。
public enum YamlNodeKind
{
    // 标量：字符串/数字/布尔/空
    Scalar,

    // 序列：`- item`（block）或 `[a, b]`（flow）
    Sequence,

    // 映射：`key: value`（block）或 `{a: 1}`（flow）
    Mapping
}