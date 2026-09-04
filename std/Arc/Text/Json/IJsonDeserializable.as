namespace Arc.Text.Json;

// JSON 反序列化接口 —— 类型实现此接口以支持从 JSON 数据恢复自身。
// 消费方：JsonSerializer.Deserialize(string, IJsonDeserializable) 就地填充，
// 以及 JsonSerializer.Deserialize<T>(string) 泛型反序列化（RFC 004 语言修复后解锁）。
// 无属性注解；命名映射由 ReadJson 手写。
//
// 使用示例：
//   public class Person : IJsonDeserializable
//   {
//       public string Name;
//       public int Age;
//
//       public void ReadJson(JsonReader reader)
//       {
//           while (reader.Read())
//           {
//               if (reader.TokenType == JsonTokenType.EndObject) { return; }
//               if (reader.TokenType == JsonTokenType.PropertyName)
//               {
//                   string prop = reader.GetString();
//                   reader.Read();
//                   if (prop == "name") { Name = reader.GetString(); }
//                   else if (prop == "age") { Age = reader.GetInt32(); }
//                   else { reader.Skip(); }
//               }
//           }
//       }
//   }
//   Person p = new Person();
//   IJsonDeserializable boxed = p;
//   JsonSerializer.Deserialize(json, boxed);
public interface IJsonDeserializable
{
    void ReadJson(JsonReader reader);
}
