namespace Arc.Text.Protobuf;

/// <summary>
/// Protobuf 消息契约——类型实现此接口以支持自序列化/反序列化（gRPC codec 的载体）。
/// 对标 C# <c>Google.Protobuf.IMessage</c>；无运行时反射，由类型手写 <see cref="WriteTo"/>
/// 与 <see cref="MergeFrom"/>（对齐 <c>Arc.Text.Json.IJsonSerializable/IJsonDeserializable</c>
/// 就地填充先例）。泛型反序列化经 <c>MessageCodec.Deserialize&lt;T&gt;</c>（<c>where T : IMessage, new()</c>，
/// RFC 004 语言修复后解锁 <c>new T()</c>）。
/// </summary>
public interface IMessage {
    /// <summary>将本消息写入 <paramref name="output"/>（字段按 tag 升序或任意合法序写出）。</summary>
    void WriteTo(CodedOutputStream output);

    /// <summary>自 <paramref name="input"/> 就地合并字段（重复字段追加 · 未知字段跳过）。</summary>
    void MergeFrom(CodedInputStream input);
}
