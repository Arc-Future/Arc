namespace Arc.Text.Yaml;

// YAML 标量解析后的类型 —— 由解析器按 YAML 核心 schema 解析得出。
// 消费方（如 Agent Skills frontmatter）据此区分 name/description(字符串)、
// allowed-tools(列表)、metadata(映射) 等字段的语义。
public enum YamlScalarKind
{
    // 字符串（含引号包裹与需要引号的明文）
    String,

    // 布尔：true/false/yes/no/on/off（含大小写变体）
    Bool,

    // 整数：十进制（可选符号）
    Int,

    // 浮点：含小数点或指数
    Float,

    // 空：null/~（含大小写变体）
    Null
}