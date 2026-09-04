// RFC 043 P2：绿点索引条目 — 绿点摘要（index.json 的一条）。
namespace Arc.Agent.Harness;
using Arc.Text.Json;

/// <summary>
/// 绿点索引条目：与 <see cref="AICheckpointSnapshot"/> 同源摘要（不含文件清单，供历史列出
/// 与「回滚到指定绿点」定位）。Id 形如 "cp-000001"，Seq 单调递增。
/// </summary>
public class AICheckpointIndexEntry : IJsonSerializable, IJsonDeserializable {
    public string Id;
    public int Seq;
    public string Label;
    /// <summary>绿点时点的 AIRfc Revision（回滚联动恢复目标版本）。</summary>
    public int Revision;
    public string PlanStatus;
    public string CreatedAt;
    public string GitHead;

    public AICheckpointIndexEntry() {
        this.Id = "";
        this.Seq = 0;
        this.Label = "";
        this.Revision = 0;
        this.PlanStatus = "";
        this.CreatedAt = "";
        this.GitHead = "";
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("id", this.Id);
        writer.WriteNumber("seq", this.Seq);
        writer.WriteString("label", this.Label);
        writer.WriteNumber("revision", this.Revision);
        writer.WriteString("planStatus", this.PlanStatus);
        writer.WriteString("createdAt", this.CreatedAt);
        writer.WriteString("gitHead", this.GitHead);
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
            } else if (prop == "createdAt") {
                this.CreatedAt = reader.GetString();
            } else if (prop == "gitHead") {
                this.GitHead = reader.GetString();
            } else {
                reader.Skip();
            }
        }
    }
}
