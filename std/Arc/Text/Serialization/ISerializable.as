namespace Arc.Text.Serialization;

// 格式无关的序列化核心接口。
// 实现者负责将自身状态写入格式无关的 SerializationWriter，
// 由具体的格式处理器（JsonWriter / XmlWriter / YamlWriter 等）承载写入。
public interface ISerializable
{
    void Serialize(SerializationWriter writer);
}
