namespace Arc.Text.Serialization;

// 格式无关的序列化写入器抽象。
// 定义了序列化所需的原子写入操作，由各格式处理器（Json / Xml / Yaml / Toml）实现。
// 对标 C# System.Text.Json.Utf8JsonWriter 的核心写入语义，但去除了 UTF-8 编码偏见。
public abstract class SerializationWriter
{
    // 写入一个命名属性的开始
    public abstract void WritePropertyName(string name);

    // 写入字符串值
    public abstract void WriteString(string value);

    // 写入整数
    public abstract void WriteInt32(int value);
    public abstract void WriteInt64(long value);

    // 写入浮点数
    public abstract void WriteFloat(float value);
    public abstract void WriteDouble(double value);

    // 写入布尔值
    public abstract void WriteBoolean(bool value);

    // 写入 null
    public abstract void WriteNull();

    // 写入对象的开始/结束
    public abstract void WriteStartObject();
    public abstract void WriteEndObject();

    // 写入数组的开始/结束
    public abstract void WriteStartArray();
    public abstract void WriteEndArray();

    // 将缓冲内容转为字符串输出
    public abstract string ToString();
}
