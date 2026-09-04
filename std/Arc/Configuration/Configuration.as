// Configuration —— 强类型配置实现（RFC 039 §1.4 / RFC 040 §2）：层叠解析 appSettings 族文件。
//
// 对齐 ASP.NET Core 配置源：
//   - 基础源 appSettings.json；
//   - 环境覆盖 appSettings.{env}.json（ARC_ENV 指定环境，默认 Production）。
//   层叠语义：环境文件后加载覆盖先加载——环境文件的顶层 key 整段替换基础文件同名 key
//   （含对象 section），新增 key 追加，其余基础 key 保留。
// 单一 IConfiguration 契约（Arc.Configuration），无 IOptions 间接层——
// 配置即服务，经 DI 注入处理器直接读取（Get<T>() 整体强类型反序列化，对标 C# Binder）。
namespace Arc.Configuration;
using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Text.Json;

/// <summary>
/// 文件配置实现：按优先级层叠多个 JSON 源并合并为单一 JSON，Get&lt;T&gt;() 整体反序列化。
/// - Load()：自动发现 appSettings.json + appSettings.{env}.json（环境层叠覆盖，无文件则空配置）。
/// - Load(path)：显式加载单文件（兼容测试与定制）。
/// 合并为顶层 key 级（值含嵌套对象整段替换）；诚实边界：数字仅 int（Arc.Text.Json 现状）。
/// </summary>
public class Configuration : IConfiguration {
    private string _json;

    private Configuration(string json) {
        _json = json;
    }

    /// <summary>自动发现：appSettings.json + appSettings.{ARC_ENV}.json 层叠合并（无文件则空配置）。</summary>
    public static Configuration Load() {
        string env = Environment.GetEnvironmentVariable("ARC_ENV");
        if (env == null || env == "") { env = "Production"; }
        string merged = "{}";
        if (File.Exists("appSettings.json")) {
            merged = File.ReadAllText("appSettings.json");
        }
        string envPath = "appSettings." + env + ".json";
        if (File.Exists(envPath)) {
            merged = Merge(merged, File.ReadAllText(envPath));
        }
        return new Configuration(merged);
    }

    /// <summary>从单个文件加载（显式路径；兼容测试与定制）。</summary>
    public static Configuration Load(string path) {
        return new Configuration(File.ReadAllText(path));
    }

    /// <summary>Get&lt;T&gt;()：把合并后的整个配置 JSON 反序列化为强类型 T（对标 C# IConfiguration.Get&lt;T&gt;()）。</summary>
    public T Get<T>() where T : IJsonDeserializable, new() {
        T value = new T();
        JsonReader reader = new JsonReader(_json);
        value.ReadJson(reader);
        return value;
    }

    /// <summary>GetSection(key)：按 ':' 路径取子树配置片段；未命中返回空片段（对标 C# IConfiguration.GetSection）。</summary>
    public IConfiguration GetSection(string key) {
        string? sectionJson = ExtractSection(_json, key);
        if (sectionJson == null) { return new Configuration("{}"); }
        return new Configuration(sectionJson);
    }

    /// <summary>GetValue&lt;T&gt;(key)：读取 key 处标量并转换为 T；key 缺失/值为对象数组/null 返回 default(T)。</summary>
    public T GetValue<T>(string key) {
        string? sectionJson = ExtractSection(_json, key);
        if (sectionJson == null) { return default(T); }
        JsonReader r = new JsonReader(sectionJson);
        if (!r.Read()) { return default(T); }
        if (r.TokenType == JsonTokenType.String) {
            object boxed = r.GetString();
            return (T)boxed;
        }
        if (r.TokenType == JsonTokenType.Number) {
            object boxed = r.GetInt32();
            return (T)boxed;
        }
        if (r.TokenType == JsonTokenType.True || r.TokenType == JsonTokenType.False) {
            object boxed = r.GetBoolean();
            return (T)boxed;
        }
        return default(T);
    }

    // ── 按 key 路径提取：':' 分隔嵌套路径，命中则返回子树 JSON（恒为合法 JSON 文本）──

    /// <summary>按 ':' 路径提取 key 处子树的 JSON 文本；未命中返回 null。key 为空返回整棵配置。</summary>
    private static string? ExtractSection(string json, string key) {
        if (key == null || key == "") { return json; }
        string[] segments = key.Split(":");
        JsonReader r = new JsonReader(json);
        if (!r.Read()) { return null; }
        int i = 0;
        while (i < segments.Length) {
            if (r.TokenType != JsonTokenType.StartObject) { return null; }
            string target = segments[i];
            bool last = (i == segments.Length - 1);
            bool matched = false;
            while (r.Read()) {
                if (r.TokenType == JsonTokenType.EndObject) { break; }
                if (r.TokenType == JsonTokenType.PropertyName) {
                    string name = r.GetString();
                    r.Read();
                    if (name == target) {
                        matched = true;
                        if (last) { return SerializeValue(r); }
                        if (r.TokenType != JsonTokenType.StartObject) { return null; }
                        break;
                    }
                    if (r.TokenType == JsonTokenType.StartObject || r.TokenType == JsonTokenType.StartArray) {
                        r.Skip();
                    }
                }
            }
            if (!matched) { return null; }
            i = i + 1;
        }
        return null;
    }

    /// <summary>把「当前已读 token」对应的完整值序列化为 JSON 文本。</summary>
    /// 注意：ExtractSection 命中时 r 已指向 value 的起始 token（StartObject/
    /// StartArray/标量），故直接走 HandleToken（当前 token 递归写出），
    /// 而非 CopyValue——后者先 `r.Read()` 会把起始 token 消费掉，
    /// 对象子树序列化为空。
    private static string SerializeValue(JsonReader r) {
        JsonWriter w = new JsonWriter();
        HandleToken(r, w);
        return w.ToString();
    }

    // ── 层叠合并：环境 JSON 顶层 key 整段覆盖基础 JSON 同名 key ──

    private static string Merge(string baseJson, string envJson) {
        List<string> envKeys = CollectKeys(envJson);
        JsonWriter w = new JsonWriter();
        w.WriteStartObject();
        CopyObject(baseJson, w, envKeys, true); // 跳过 env 覆盖的 key
        CopyObject(envJson, w, envKeys, false); // env 全部写出（覆盖 + 新增）
        w.WriteEndObject();
        return w.ToString();
    }

    /// <summary>收集 JSON 对象的所有顶层 key（用于跳过被覆盖的基础成员）。</summary>
    private static List<string> CollectKeys(string json) {
        List<string> keys = new List<string>();
        JsonReader r = new JsonReader(json);
        if (!r.Read()) { return keys; }
        while (r.Read()) {
            if (r.TokenType == JsonTokenType.EndObject) { break; }
            if (r.TokenType == JsonTokenType.PropertyName) {
                keys.Add(r.GetString());
                r.Read();
                if (r.TokenType == JsonTokenType.StartObject || r.TokenType == JsonTokenType.StartArray) {
                    r.Skip();
                }
            }
        }
        return keys;
    }

    /// <summary>把 JSON 对象的顶层成员逐个写出；skip=true 时跳过 envKeys 命中的 key。</summary>
    private static void CopyObject(string json, JsonWriter w, List<string> envKeys, bool skip) {
        JsonReader r = new JsonReader(json);
        if (!r.Read()) { return; }
        while (r.Read()) {
            if (r.TokenType == JsonTokenType.EndObject) { break; }
            if (r.TokenType == JsonTokenType.PropertyName) {
                string key = r.GetString();
                if (skip && Contains(envKeys, key)) {
                    r.Read();
                    if (r.TokenType == JsonTokenType.StartObject || r.TokenType == JsonTokenType.StartArray) {
                        r.Skip();
                    }
                    continue;
                }
                w.WritePropertyName(key);
                CopyValue(r, w);
            }
        }
    }

    /// <summary>读取并写出一个完整 JSON 值（对象/数组递归；标量直写）。</summary>
    private static void CopyValue(JsonReader r, JsonWriter w) {
        if (!r.Read()) { return; }
        HandleToken(r, w);
    }

    /// <summary>处理「当前已读 token」对应的值：对象/数组递归遍历，标量按类型写出。</summary>
    private static void HandleToken(JsonReader r, JsonWriter w) {
        if (r.TokenType == JsonTokenType.StartObject) {
            w.WriteStartObject();
            while (r.Read()) {
                if (r.TokenType == JsonTokenType.EndObject) { break; }
                if (r.TokenType == JsonTokenType.PropertyName) {
                    w.WritePropertyName(r.GetString());
                    CopyValue(r, w);
                }
            }
            w.WriteEndObject();
        } else if (r.TokenType == JsonTokenType.StartArray) {
            w.WriteStartArray();
            bool cont = true;
            while (cont) {
                if (!r.Read()) {
                    cont = false;
                } else if (r.TokenType == JsonTokenType.EndArray) {
                    cont = false;
                } else {
                    HandleToken(r, w);
                }
            }
            w.WriteEndArray();
        } else if (r.TokenType == JsonTokenType.String) {
            w.WriteString(r.GetString());
        } else if (r.TokenType == JsonTokenType.Number) {
            w.WriteNumber(r.GetInt32());
        } else if (r.TokenType == JsonTokenType.True) {
            w.WriteBoolean(true);
        } else if (r.TokenType == JsonTokenType.False) {
            w.WriteBoolean(false);
        } else if (r.TokenType == JsonTokenType.Null) {
            w.WriteNull();
        }
    }

    private static bool Contains(List<string> keys, string key) {
        int i = 0;
        while (i < keys.Count) {
            if (keys[i] == key) { return true; }
            i = i + 1;
        }
        return false;
    }
}
