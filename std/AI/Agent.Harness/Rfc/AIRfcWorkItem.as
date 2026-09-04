// RFC 043 P3（H-2c 升版）：AIRfc 下可并行工作项 —— 可绑定不同 Session / TaskRun；
// 新增 DependsOn（任务图依赖）+ Scope（预声明写面，合并冲突仲裁依据）。
namespace Arc.Agent.Harness;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>AIRfc 下的可并行工作项；可绑定不同 Session / TaskRun；支持任务图依赖与写面声明。</summary>
public class AIRfcWorkItem : IJsonSerializable, IJsonDeserializable {
    public string WorkItemId;
    public string RfcId;
    public string Title;
    public string? SessionId;
    public string? TaskRunId;
    /// <summary>Open / InProgress / Blocked / Done / Failed / Cancelled。</summary>
    public AIRfcWorkItemStatus Status;
    /// <summary>前置工作项 Id（空 = 无依赖；引用不存在的工作项 Id 为非法，见 AIRfcTaskGraph 校验）。</summary>
    public List<string> DependsOn;
    /// <summary>预声明写面（文件 / 路径 / 资源）；合并冲突仲裁依据，经 AICoordinator ToolPath 租约仲裁。</summary>
    public List<string> Scope;

    public AIRfcWorkItem() {
        this.WorkItemId = "";
        this.RfcId = "";
        this.Title = "";
        this.SessionId = null;
        this.TaskRunId = null;
        this.Status = AIRfcWorkItemStatus.Open;
        this.DependsOn = new List<string>();
        this.Scope = new List<string>();
    }

    /// <summary>是否依赖尚未完成的前置工作项（任务图拓扑判定用）。</summary>
    public bool HasUnfinishedDependency(List<AIRfcWorkItem> all) {
        if (this.DependsOn == null || this.DependsOn.Count == 0) {
            return false;
        }
        int i = 0;
        while (i < this.DependsOn.Count) {
            AIRfcWorkItem? dep = AIRfcWorkItem.FindItem(all, this.DependsOn[i]);
            if (dep == null) {
                return true;
            }
            if (dep.Status != AIRfcWorkItemStatus.Done) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>按 Id 查工作项；无则 null。</summary>
    public static AIRfcWorkItem? FindItem(List<AIRfcWorkItem> all, string workItemId) {
        if (all == null) {
            return null;
        }
        int i = 0;
        while (i < all.Count) {
            AIRfcWorkItem item = all[i];
            if (item != null && item.WorkItemId == workItemId) {
                return item;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>序列化为 JSON 对象（含可空 SessionId/TaskRunId 与依赖/写面数组；供 AIRfc 持久化）。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("workItemId", this.WorkItemId);
        writer.WriteString("rfcId", this.RfcId);
        writer.WriteString("title", this.Title);
        writer.WriteString("sessionId", this.SessionId);
        writer.WriteString("taskRunId", this.TaskRunId);
        writer.WriteString("status", AIRfcWorkItemStatusCodec.ToWireString(this.Status));
        writer.WritePropertyName("dependsOn");
        writer.WriteStartArray();
        int d = 0;
        while (d < this.DependsOn.Count) {
            writer.WriteString(this.DependsOn[d]);
            d = d + 1;
        }
        writer.WriteEndArray();
        writer.WritePropertyName("scope");
        writer.WriteStartArray();
        int s = 0;
        while (s < this.Scope.Count) {
            writer.WriteString(this.Scope[s]);
            s = s + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>从 JSON 对象就地填充（与 <see cref="WriteJson"/> 同构）。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop == "dependsOn") {
                this.ReadStringArray(reader, this.DependsOn);
            } else if (prop == "scope") {
                this.ReadStringArray(reader, this.Scope);
            } else {
                if (!reader.Read()) {
                    return;
                }
                if (prop == "workItemId") {
                    this.WorkItemId = reader.GetString();
                } else if (prop == "rfcId") {
                    this.RfcId = reader.GetString();
                } else if (prop == "title") {
                    this.Title = reader.GetString();
                } else if (prop == "sessionId") {
                    this.SessionId = AIRfcWorkItem.ReadNullableString(reader);
                } else if (prop == "taskRunId") {
                    this.TaskRunId = AIRfcWorkItem.ReadNullableString(reader);
                } else if (prop == "status") {
                    this.Status = AIRfcWorkItemStatusCodec.FromWireString(reader.GetString());
                } else {
                    reader.Skip();
                }
            }
        }
    }

    /// <summary>解析字符串数组（写入既有列表；JSON 非数组 → 保留现状）。</summary>
    private void ReadStringArray(JsonReader reader, List<string> target) {
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return;
        }
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                cont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.String) {
                continue;
            }
            target.Add(reader.GetString());
        }
    }

    /// <summary>可空字符串读取：JSON null → null，其余走 GetString。</summary>
    private static string? ReadNullableString(JsonReader reader) {
        if (reader.TokenType == JsonTokenType.Null) {
            return null;
        }
        return reader.GetString();
    }
}
