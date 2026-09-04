namespace Arc.Agent;
using Arc.Collections;
using Arc.Text.Json;

public class AIWikiMeta : IJsonSerializable, IJsonDeserializable {
    public string Title;
    /// <summary>标签（检索/分类辅助；可为空数组）。</summary>
    public string[] Tags;
    /// <summary>页面版本号（Upsert 递增语义由调用方决定；0 = 未声明）。</summary>
    public int Version;
    /// <summary>来源/引用（如 "user" / "session-xxx" / 外部文件路径）。</summary>
    public string Source;

    public AIWikiMeta() {
        this.Title = "";
        string[] empty = [];
        this.Tags = empty;
        this.Version = 0;
        this.Source = "";
    }

    public AIWikiMeta(string title) {
        this.Title = title != null ? title : "";
        string[] empty = [];
        this.Tags = empty;
        this.Version = 0;
        this.Source = "";
    }

    public AIWikiMeta(string title, string[] tags, int version, string source) {
        this.Title = title != null ? title : "";
        string[] t = tags;
        if (t == null) {
            string[] empty = [];
            t = empty;
        }
        this.Tags = t;
        this.Version = version;
        this.Source = source != null ? source : "";
    }

    /// <summary>JSON 序列化（落盘用）：title / version / source / tags。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("title", this.Title);
        writer.WriteNumber("version", this.Version);
        writer.WriteString("source", this.Source);
        writer.WritePropertyName("tags");
        writer.WriteStartArray();
        string[] tags = this.Tags;
        int n = tags != null ? tags.Length : 0;
        int i = 0;
        while (i < n) {
            writer.WriteString(tags[i]);
            i = i + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>JSON 反序列化（落盘加载）。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "title") {
                    this.Title = reader.GetString();
                } else if (prop == "version") {
                    this.Version = reader.GetInt32();
                } else if (prop == "source") {
                    this.Source = reader.GetString();
                } else if (prop == "tags") {
                    this.Tags = this.ReadStringArray(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private string[] ReadStringArray(JsonReader reader) {
        List<string> list = new List<string>();
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                break;
            }
            if (reader.TokenType == JsonTokenType.String) {
                list.Add(reader.GetString());
            }
        }
        return list.ToArray();
    }
}
