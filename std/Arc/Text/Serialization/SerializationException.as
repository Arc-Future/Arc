namespace Arc.Text.Serialization;

// 序列化/反序列化过程中抛出的异常。
public class SerializationException : Exception
{
    /// <summary>使用默认消息构造序列化异常。</summary>
    public SerializationException() : base() {}

    /// <summary>使用指定消息构造序列化异常。</summary>
    public SerializationException(string message) : base(message) {}

    /// <summary>使用指定消息和内部异常构造序列化异常。</summary>
    public SerializationException(string message, Exception innerException)
        : base(message, innerException) {}
}
