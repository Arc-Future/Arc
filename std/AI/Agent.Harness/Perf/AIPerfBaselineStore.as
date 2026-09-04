// RFC 043 P3（performance-observability）：性能基线存储 — 版本化基线落盘/恢复。
//
// D9 门按 Subject + Kind（首编译/增量）维护基线，落盘
// target/scratch/arcagent-state/perf-baseline.json（随绿点落盘，禁源码树）。Record
// 按 (Subject, Kind) upsert；Find 读取；Load/Save 为进程内快照与磁盘往返。
namespace Arc.Agent.Harness;

using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Text.Json;

/// <summary>
/// 性能基线存储（RFC 043 P3）：内存列表 + JSON 落盘。单线程宿主约束（同注册表），
/// 不加锁。经 <see cref="AIPerfBaselineStore.SaveAsync"/> / <see cref="LoadAsync"/>
/// 与磁盘往返；<see cref="Record"/> / <see cref="Find"/> 为内存面。
/// </summary>
public class AIPerfBaselineStore {
    private const string StateRelDir = "target/scratch/arcagent-state";
    private const string StateFile = "perf-baseline.json";

    private List<AIPerfBaseline> _baselines;

    public AIPerfBaselineStore() {
        _baselines = new List<AIPerfBaseline>();
    }

    /// <summary>当前内存内基线数。</summary>
    public int Count {
        get { return _baselines.Count; }
    }

    /// <summary>按 Subject + Kind 查基线；无 → null。</summary>
    public AIPerfBaseline? Find(string subject, AIPerfBaselineKind kind) {
        int i = 0;
        while (i < _baselines.Count) {
            AIPerfBaseline b = _baselines[i];
            if (b.Subject == subject && b.Kind == kind) {
                return b;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>某 Subject 是否有任意基线。</summary>
    public bool HasBaseline(string subject) {
        int i = 0;
        while (i < _baselines.Count) {
            if (_baselines[i].Subject == subject) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>记录/更新基线（按 Subject + Kind upsert）。</summary>
    public AIPerfBaseline Record(string subject, AIPerfBaselineKind kind, long wallMs, long peakMemoryBytes) {
        AIPerfBaseline? existing = this.Find(subject, kind);
        if (existing != null) {
            existing.WallMs = wallMs;
            existing.PeakMemoryBytes = peakMemoryBytes;
            return existing;
        }
        AIPerfBaseline b = new AIPerfBaseline();
        b.Subject = subject;
        b.Kind = kind;
        b.WallMs = wallMs;
        b.PeakMemoryBytes = peakMemoryBytes;
        _baselines.Add(b);
        return b;
    }

    /// <summary>落盘全部基线到 <c>target/scratch/arcagent-state/perf-baseline.json</c>。</summary>
    public async Task<bool> SaveAsync(string project, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        if (project == null || project == "") {
            return false;
        }
        string dir = Path.Combine(project, AIPerfBaselineStore.StateRelDir);
        if (!await this.EnsureDirectoryAsync(dir)) {
            return false;
        }
        AIPerfBaselineList state = new AIPerfBaselineList();
        state.Baselines = _baselines;
        string json = JsonSerializer.Serialize((IJsonSerializable)state);
        return await File.WriteAllTextAsync(Path.Combine(dir, AIPerfBaselineStore.StateFile), json);
    }

    /// <summary>从磁盘恢复全部基线（覆盖内存列表）。无文件 → false。</summary>
    public async Task<bool> LoadAsync(string project, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        if (project == null || project == "") {
            return false;
        }
        string path = Path.Combine(
            Path.Combine(project, AIPerfBaselineStore.StateRelDir), AIPerfBaselineStore.StateFile);
        bool exists = await File.ExistsAsync(path);
        if (!exists) {
            return false;
        }
        string json = await File.ReadAllTextAsync(path);
        if (json == null || json == "") {
            return false;
        }
        AIPerfBaselineList state = new AIPerfBaselineList();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)state);
        _baselines.Clear();
        if (state.Baselines != null) {
            int i = 0;
            while (i < state.Baselines.Count) {
                _baselines.Add(state.Baselines[i]);
                i = i + 1;
            }
        }
        return true;
    }

    /// <summary>逐层创建目录（rt_dir_create 仅建单层）。</summary>
    private async Task<bool> EnsureDirectoryAsync(string dir) {
        bool exists = await Directory.ExistsAsync(dir);
        if (exists) {
            return true;
        }
        string parent = Path.GetDirectoryName(dir);
        if (parent != null && parent != "" && parent != dir) {
            bool okParent = await this.EnsureDirectoryAsync(parent);
            if (!okParent) {
                return false;
            }
        }
        return await Directory.CreateDirectoryAsync(dir);
    }
}

/// <summary>性能基线列表 JSON 外壳（<c>{"baselines":[...]}</c>；与 Store 配套，不参与业务 API）。</summary>
internal class AIPerfBaselineList : IJsonSerializable, IJsonDeserializable {
    public List<AIPerfBaseline> Baselines;

    public AIPerfBaselineList() {
        this.Baselines = new List<AIPerfBaseline>();
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WritePropertyName("baselines");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.Baselines.Count) {
            this.Baselines[i].WriteJson(writer);
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
            if (prop != "baselines") {
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
                AIPerfBaseline b = new AIPerfBaseline();
                b.ReadJson(reader);
                this.Baselines.Add(b);
            }
        }
    }
}
