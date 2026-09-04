// RFC 043 P2：绿点快照模型 — 工作区关键状态 + 文件清单（JSON 落盘，/resume 可恢复）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 一次绿点快照：git HEAD + stash 列表 + 文件清单 + 绿点标签 + AIRfc Revision + 可选
/// AIPlan 状态摘要。多绿点按 `checkpoint-&lt;seq&gt;.json` 落盘，`index.json` 维护索引；
/// 回滚可指定任意绿点（默认最近）。
/// </summary>
public class AICheckpointSnapshot : IJsonSerializable, IJsonDeserializable {
    /// <summary>绿点标识（如 "cp-000001"，与 index.json 条目对齐）。</summary>
    public string Id;
    /// <summary>绿点序号（单调递增，1-based）。</summary>
    public int Seq;
    public string Label;
    /// <summary>绿点时点的 AIRfc Revision（回滚联动升版/恢复依据）。</summary>
    public int Revision;
    /// <summary>绿点时点的 AIPlan 状态摘要（Pending/Approved/Executing/Verifying/Completed/Rejected/空）。</summary>
    public string PlanStatus;
    public string GitHead;
    public string StashList;
    public string CreatedAt;
    public bool Truncated;
    public List<AICheckpointFileEntry> Files;

    public AICheckpointSnapshot() {
        this.Id = "";
        this.Seq = 0;
        this.Label = "";
        this.Revision = 0;
        this.PlanStatus = "";
        this.GitHead = "";
        this.StashList = "";
        this.CreatedAt = "";
        this.Truncated = false;
        this.Files = new List<AICheckpointFileEntry>();
    }

    /// <summary>是否有条目具备可回滚内容（无 git 环境也可恢复的最小判定）。</summary>
    public bool HasRestorableFiles {
        get {
            int i = 0;
            while (i < this.Files.Count) {
                if (this.Files[i].HasContent || this.Files[i].ObjectRef != "") {
                    return true;
                }
                i = i + 1;
            }
            return false;
        }
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("id", this.Id);
        writer.WriteNumber("seq", this.Seq);
        writer.WriteString("label", this.Label);
        writer.WriteNumber("revision", this.Revision);
        writer.WriteString("planStatus", this.PlanStatus);
        writer.WriteString("gitHead", this.GitHead);
        writer.WriteString("stashList", this.StashList);
        writer.WriteString("createdAt", this.CreatedAt);
        writer.WriteBoolean("truncated", this.Truncated);
        writer.WritePropertyName("files");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.Files.Count) {
            this.Files[i].WriteJson(writer);
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
            if (prop == "files") {
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
                    AICheckpointFileEntry entry = new AICheckpointFileEntry();
                    entry.ReadJson(reader);
                    this.Files.Add(entry);
                }
            } else {
                if (!reader.Read()) {
                    return;
                }
                if (prop == "id") {
                    this.Id = reader.GetString();
                } else if (prop == "seq") {
                    this.Seq = reader.GetInt32();
                } else if (prop == "label") {
                    this.Label = reader.GetString();
                } else if (prop == "revision") {
                    this.Revision = reader.GetInt32();
                } else if (prop == "planStatus") {
                    this.PlanStatus = reader.GetString();
                } else if (prop == "gitHead") {
                    this.GitHead = reader.GetString();
                } else if (prop == "stashList") {
                    this.StashList = reader.GetString();
                } else if (prop == "createdAt") {
                    this.CreatedAt = reader.GetString();
                } else if (prop == "truncated") {
                    this.Truncated = reader.GetBoolean();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
