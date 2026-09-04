namespace Arc.Text.Json;

// JSON 序列化器——Stable 最小面：
//   Serialize(IJsonSerializable)
//   Deserialize(string, IJsonDeserializable)  —— 就地填充，与 Serialize 同构（接口契约）
//   Deserialize<T>(string)  —— 泛型反序列化（RFC 004 语言修复后解锁；2026-08-02）
//
// 诚实边界：
//   - 无运行时反射；类型须手写 ReadJson（IJsonDeserializable）
//   - 无属性注解 / 源生成（[JsonPropertyName] 等未立项）
//   - 与 Serialize 相同：concrete→接口形参须显式装箱（见 json_xml_e2e）
//   - 本切片不提供 Options 重载（同名 void 重载 tip 会 LLVM 符号碰撞；Options 后置）
//   - JsonSerializerOptions 在 Deserialize 路径暂未消费
public static class JsonSerializer
{
    public static string Serialize(IJsonSerializable value)
    {
        return Serialize(value, JsonSerializerOptions.Default);
    }

    public static string Serialize(IJsonSerializable value, JsonSerializerOptions options)
    {
        if (value == null)
        {
            return "null";
        }

        JsonWriterOptions writerOpts = new JsonWriterOptions();
        writerOpts.Indented = options.WriteIndented;

        JsonWriter writer = new JsonWriter(writerOpts);
        value.WriteJson(writer);
        return writer.ToString();
    }

    public static void Serialize(IJsonSerializable value, JsonWriter writer)
    {
        if (value == null)
        {
            writer.WriteNull();
            return;
        }
        value.WriteJson(writer);
    }

    public static void Deserialize(string json, IJsonDeserializable value)
    {
        if (value == null)
        {
            return;
        }
        if (json == null)
        {
            return;
        }
        JsonReader reader = new JsonReader(json);
        value.ReadJson(reader);
    }

    /// <summary>泛型反序列化：构造 T 并就地填充 JSON（RFC 004 语言修复后解锁）。</summary>
    public static T Deserialize<T>(string json) where T : IJsonDeserializable, new()
    {
        T value = new T();
        if (json == null)
        {
            return value;
        }
        JsonReader reader = new JsonReader(json);
        value.ReadJson(reader);
        return value;
    }
}
