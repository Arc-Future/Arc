// RFC 043 P2：绿点索引模型 — 多绿点历史索引（JSON 落盘 index.json）。
//
// 绿点体系从「单份 latest.json」升级为「多绿点历史」（RFC 043 场景 3.4）：每次
// CheckpointGreenAsync 追加一条索引条目 + 一份 `checkpoint-<seq>.json` 完整快照；
// /rollback 按条目 id 或默认最近回滚。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>绿点历史索引：条目按捕获顺序追加（Seq 单调递增），尾部为最近绿点。</summary>
public class AICheckpointIndex : IJsonSerializable, IJsonDeserializable {
    public List<AICheckpointIndexEntry> Entries;

    public AICheckpointIndex() {
        this.Entries = new List<AICheckpointIndexEntry>();
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WritePropertyName("entries");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.Entries.Count) {
            this.Entries[i].WriteJson(writer);
            i = i + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop != "entries") {
                if (!reader.Read()) {
                    return;
                }
                reader.Skip();
                continue;
            }
            if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
                return;
            }
            bool cont = true;
            while (cont && reader.Read()) {
                if (reader.TokenType == JsonTokenType.EndArray) {
                    cont = false;
                    break;
                }
                if (reader.TokenType != JsonTokenType.StartObject) {
                    continue;
                }
                AICheckpointIndexEntry entry = new AICheckpointIndexEntry();
                entry.ReadJson(reader);
                this.Entries.Add(entry);
            }
        }
    }
}
