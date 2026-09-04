namespace Arc.Text.Protobuf;

/// <summary>
/// Protobuf 消息层门面——<see cref="IMessage"/> ⇄ <c>byte[]</c> 编解码（gRPC codec 的载体面）。
/// 对标 C# <c>Google.Protobuf.MessageExtensions</c> 精简。无运行时反射：类型手写
/// <see cref="IMessage.WriteTo"/>/<see cref="IMessage.MergeFrom"/>；泛型反序列化经
/// <c>where T : IMessage, new()</c> + <c>new T()</c>（RFC 004 语言修复后解锁，对齐
/// <c>Arc.Text.Json.JsonSerializer.Deserialize&lt;T&gt;</c> 先例）。
/// 诚实边界：不做长度前缀分帧（gRPC 5 字节分帧属网络层 <c>Arc.Net.Grpc</c> 职责）；本面仅
/// 负责单条消息 ⇄ 字节。
/// </summary>
public static class MessageCodec {
    /// <summary>序列化消息为字节（不含长度前缀）。</summary>
    public static byte[] Serialize(IMessage message) {
        CodedOutputStream output = new CodedOutputStream();
        message.WriteTo(output);
        return output.ToArray();
    }

    /// <summary>反序列化字节为 <typeparamref name="T"/>（构造空消息后就地合并）。失败返回默认消息。</summary>
    public static T Deserialize<T>(byte[] data) where T : IMessage, new() {
        T value = new T();
        CodedInputStream input = new CodedInputStream(data);
        value.MergeFrom(input);
        return value;
    }

    /// <summary>就地反序列化进既有消息实例。返回是否成功（载荷格式错误为 false）。</summary>
    public static bool MergeInto(IMessage message, byte[] data) {
        if (message == null) { return false; }
        CodedInputStream input = new CodedInputStream(data);
        message.MergeFrom(input);
        return !input.Failed;
    }
}
