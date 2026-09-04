// RFC 043 P3（performance-observability）：性能基线版本化 — 首编译 vs 增量基线。
//
// D9 性能门的判定依据：当前关键路径基准 ↔ 版本化基线 diff。基线按 Kind 区分
// 「首编译」（冷，全量）与「增量」（暖，缓存命中），避免冷/暖误判回归。随绿点落盘
// （AIPerfBaselineStore → target/scratch/arcagent-state/perf-baseline.json）。
namespace Arc.Agent.Harness;

using Arc;
using Arc.Text.Json;

/// <summary>基线类别（首编译冷基线 vs 增量暖基线）。</summary>
public enum AIPerfBaselineKind {
    /// <summary>首次编译（冷，全量构建）。</summary>
    FirstCompile,
    /// <summary>增量编译（暖，缓存命中）。</summary>
    Incremental
}

/// <summary>
/// 性能基线（RFC 043 P3）：某 Subject 的一次版本化墙钟/峰值内存基线。
/// 实现 <see cref="IJsonSerializable"/> / <see cref="IJsonDeserializable"/> 供
/// <see cref="AIPerfBaselineStore"/> 持久化（wall/峰值内存 long 以字符串承载，规避
/// JsonWriter 仅 WriteNumber(int) 面）。
/// </summary>
public class AIPerfBaseline : IJsonSerializable, IJsonDeserializable {
    /// <summary>基线主题（如 "D9-compile"）。</summary>
    public string Subject;
    /// <summary>基线类别（首编译 / 增量）。</summary>
    public AIPerfBaselineKind Kind;
    /// <summary>墙钟（毫秒）。</summary>
    public long WallMs;
    /// <summary>峰值内存（字节）。</summary>
    public long PeakMemoryBytes;

    public AIPerfBaseline() {
        this.Subject = "";
        this.Kind = AIPerfBaselineKind.FirstCompile;
        this.WallMs = 0;
        this.PeakMemoryBytes = 0;
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("subject", this.Subject);
        writer.WriteString("kind", AIPerfBaseline.KindName(this.Kind));
        writer.WriteString("wallMs", this.WallMs.ToString());
        writer.WriteString("peakMemoryBytes", this.PeakMemoryBytes.ToString());
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
            if (prop == "subject") {
                this.Subject = reader.GetString();
            } else if (prop == "kind") {
                this.Kind = AIPerfBaseline.ParseKind(reader.GetString());
            } else if (prop == "wallMs") {
                this.WallMs = Convert.ToInt64(reader.GetString());
            } else if (prop == "peakMemoryBytes") {
                this.PeakMemoryBytes = Convert.ToInt64(reader.GetString());
            } else {
                reader.Skip();
            }
        }
    }

    private static string KindName(AIPerfBaselineKind kind) {
        return kind == AIPerfBaselineKind.Incremental ? "Incremental" : "FirstCompile";
    }

    private static AIPerfBaselineKind ParseKind(string text) {
        return text == "Incremental" ? AIPerfBaselineKind.Incremental : AIPerfBaselineKind.FirstCompile;
    }
}
