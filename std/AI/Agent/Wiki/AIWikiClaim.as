// RFC 038：AIWikiClaim —— 持久断言（知识图原子单位）。
//
// 每个断言必须锚定至少一个不可变源（source_ids 必填；G1 fail-closed 拒绝无源断言）。
// 断言按 Fidelity 分层（Evidence/Synthesis/Inference），并携带捕获/核验时间与可信度，
// 供 Query 合成与 Lint 体检（weak_citations / orphan_claims）。
namespace Arc.Agent;
using Arc;
using Arc.Text.Json;

/// <summary>断言保真度分层：直接证据 → 综合 → 推断。</summary>
public enum AIWikiFidelity {
    /// <summary>直接证据（源原文可证）。</summary>
    Evidence,
    /// <summary>多源综合。</summary>
    Synthesis,
    /// <summary>推断（非直接出处）。</summary>
    Inference,
}

/// <summary>断言可信度。</summary>
public enum AIWikiConfidence {
    /// <summary>低可信（Lint 记为 weak_citation）。</summary>
    Low,
    /// <summary>中可信。</summary>
    Medium,
    /// <summary>高可信。</summary>
    High,
}

/// <summary>断言生命周期状态。</summary>
public enum AIWikiClaimStatus {
    /// <summary>草稿（未核验）。</summary>
    Draft,
    /// <summary>已核验。</summary>
    Verified,
    /// <summary>已驳回。</summary>
    Rejected,
    /// <summary>已弃用。</summary>
    Deprecated,
}

/// <summary>持久断言：一段可溯源、可核验的知识陈述。</summary>
public class AIWikiClaim : IJsonSerializable, IJsonDeserializable {
    /// <summary>断言唯一标识。</summary>
    public string Id;
    /// <summary>断言正文。</summary>
    public string Text;
    /// <summary>锚定的不可变源标识（必填；空 → G1 拒绝）。</summary>
    public string[] SourceIds;
    /// <summary>保真度分层。</summary>
    public AIWikiFidelity Fidelity;
    /// <summary>可信度。</summary>
    public AIWikiConfidence Confidence;
    /// <summary>捕获时间。</summary>
    public DateTime CapturedAt;
    /// <summary>核验时间（未核验 = DateTime.MinValue）。</summary>
    public DateTime VerifiedAt;
    /// <summary>状态。</summary>
    public AIWikiClaimStatus Status;

    public AIWikiClaim() {
        this.Id = "";
        this.Text = "";
        string[] empty = [];
        this.SourceIds = empty;
        this.Fidelity = AIWikiFidelity.Evidence;
        this.Confidence = AIWikiConfidence.Medium;
        this.CapturedAt = DateTime.MinValue;
        this.VerifiedAt = DateTime.MinValue;
        this.Status = AIWikiClaimStatus.Draft;
    }

    public AIWikiClaim(string id, string text, string[] sourceIds, AIWikiFidelity fidelity,
        AIWikiConfidence confidence, DateTime capturedAt, AIWikiClaimStatus status) {
        this.Id = id != null ? id : "";
        this.Text = text != null ? text : "";
        string[] s = sourceIds;
        if (s == null) {
            string[] empty = [];
            s = empty;
        }
        this.SourceIds = s;
        this.Fidelity = fidelity;
        this.Confidence = confidence;
        this.CapturedAt = capturedAt;
        this.VerifiedAt = DateTime.MinValue;
        this.Status = status;
    }

    /// <summary>该断言是否已核验（VerifiedAt 有效）。</summary>
    public bool IsVerified() {
        return this.Status == AIWikiClaimStatus.Verified
            || this.VerifiedAt.Ticks > DateTime.MinValue.Ticks;
    }

    /// <summary>是否锚定了至少一个源（G1 关键判据）。</summary>
    public bool HasSource() {
        string[] s = this.SourceIds;
        return s != null && s.Length > 0;
    }

    /// <summary>JSON 序列化（落盘用）。时间以 ToString 字符串存（秒精度，足够元数据）。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("id", this.Id);
        writer.WriteString("text", this.Text);
        writer.WriteNumber("fidelity", (int)this.Fidelity);
        writer.WriteNumber("confidence", (int)this.Confidence);
        writer.WriteString("capturedAt", this.CapturedAt.ToString());
        writer.WriteString("verifiedAt", this.VerifiedAt.ToString());
        writer.WriteNumber("status", (int)this.Status);
        writer.WritePropertyName("sourceIds");
        this.WriteStrArray(writer, this.SourceIds);
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
                if (prop == "id") {
                    this.Id = reader.GetString();
                } else if (prop == "text") {
                    this.Text = reader.GetString();
                } else if (prop == "fidelity") {
                    this.Fidelity = this.IntToFidelity(reader.GetInt32());
                } else if (prop == "confidence") {
                    this.Confidence = this.IntToConfidence(reader.GetInt32());
                } else if (prop == "capturedAt") {
                    this.CapturedAt = DateTime.Parse(reader.GetString());
                } else if (prop == "verifiedAt") {
                    this.VerifiedAt = DateTime.Parse(reader.GetString());
                } else if (prop == "status") {
                    this.Status = this.IntToClaimStatus(reader.GetInt32());
                } else if (prop == "sourceIds") {
                    this.SourceIds = this.ReadStrArray(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void WriteStrArray(JsonWriter writer, string[] arr) {
        writer.WriteStartArray();
        int n = arr != null ? arr.Length : 0;
        int i = 0;
        while (i < n) {
            writer.WriteString(arr[i]);
            i = i + 1;
        }
        writer.WriteEndArray();
    }

    private string[] ReadStrArray(JsonReader reader) {
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

    private AIWikiFidelity IntToFidelity(int v) {
        if (v == 1) { return AIWikiFidelity.Synthesis; }
        if (v == 2) { return AIWikiFidelity.Inference; }
        return AIWikiFidelity.Evidence;
    }

    private AIWikiConfidence IntToConfidence(int v) {
        if (v == 1) { return AIWikiConfidence.Medium; }
        if (v == 2) { return AIWikiConfidence.High; }
        return AIWikiConfidence.Low;
    }

    private AIWikiClaimStatus IntToClaimStatus(int v) {
        if (v == 1) { return AIWikiClaimStatus.Verified; }
        if (v == 2) { return AIWikiClaimStatus.Rejected; }
        if (v == 3) { return AIWikiClaimStatus.Deprecated; }
        return AIWikiClaimStatus.Draft;
    }
}
