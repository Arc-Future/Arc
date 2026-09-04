namespace Arc.Text.Json;

// JSON token 类型，对应 RFC 8259 中定义的 JSON 值类型。
// JsonReader 使用此枚举标识当前读取位置的 token 类型。
// 对标 C# System.Text.Json.JsonTokenType。
public enum JsonTokenType
{
    // 初始状态，尚未读取任何 token
    None,

    // { 对象开始
    StartObject,

    // } 对象结束
    EndObject,

    // [ 数组开始
    StartArray,

    // ] 数组结束
    EndArray,

    // "key": 属性名
    PropertyName,

    // "value" 字符串值
    String,

    // 数字值（整数或浮点）
    Number,

    // true 字面量
    True,

    // false 字面量
    False,

    // null 字面量
    Null
}
