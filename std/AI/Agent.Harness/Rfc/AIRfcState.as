// RFC 043 H-2c / S0：AIRfc 持久化载体 — 全部聚合根快照的 JSON 外壳（AIRfc 跨会话恢复）。
// 与 AIRfcRuntime.Serialize / Restore 配套；不参与业务 API 面。
namespace Arc.Agent.Harness;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// AIRfc 持久化载体：全部聚合根快照的 JSON 外壳（<c>{"rfcs":[...]}</c>）。与
/// <see cref="AIRfcRuntime.Serialize"/> / <see cref="AIRfcRuntime.Restore"/> 配套；
/// 不参与业务 API 面。
/// </summary>
public class AIRfcState : IJsonSerializable, IJsonDeserializable {
    public List<AIRfc> Rfcs;

    public AIRfcState() {
        this.Rfcs = new List<AIRfc>();
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WritePropertyName("rfcs");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.Rfcs.Count) {
            this.Rfcs[i].WriteJson(writer);
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
            if (prop != "rfcs") {
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
                AIRfc rfc = new AIRfc();
                rfc.ReadJson(reader);
                this.Rfcs.Add(rfc);
            }
        }
    }
}
