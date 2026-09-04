namespace Arc.Text.Json;

// JSON 序列化接口 —— 类型实现此接口以支持将自身序列化为 JSON。
// 对标 C# System.Text.Json.Serialization.IJsonOnSerializing 的自定义序列化逻辑。
//
// 使用示例：
//   public class Person : IJsonSerializable
//   {
//       public string Name;
//       public int Age;
//
//       public void WriteJson(JsonWriter writer)
//       {
//           writer.WriteStartObject();
//           writer.WriteString("name", Name);
//           writer.WriteNumber("age", Age);
//           writer.WriteEndObject();
//       }
//   }
public interface IJsonSerializable
{
    void WriteJson(JsonWriter writer);
}
