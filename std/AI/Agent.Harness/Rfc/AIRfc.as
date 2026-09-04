// RFC 043 H-2c：AIRfc 聚合根 — Spec 面（Intention/Design/Acceptance）+ Plan 面 = AIPlan 引用。
// 禁止平行 PlanSpec；禁止把 HarnessAnchor 当终态事实源。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.Text;
using Arc.Text.Json;

/// <summary>意图面：可感知结果（非技术细节）。</summary>
public class AIIntentionSpec : IJsonSerializable, IJsonDeserializable {
    public string Text;

    public AIIntentionSpec() {
        this.Text = "";
    }

    public AIIntentionSpec(string text) {
        this.Text = text != null ? text : "";
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("text", this.Text);
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
            if (prop == "text") {
                this.Text = reader.GetString();
            } else {
                reader.Skip();
            }
        }
    }
}

/// <summary>设计面：远见 / 收敛 / 结构 / 模式 / 决策理由。</summary>
public class AIDesignSpec : IJsonSerializable, IJsonDeserializable {
    public string Foresight;
    public string Convergence;
    public string Structure;
    public string Patterns;
    public string Rationale;

    public AIDesignSpec() {
        this.Foresight = "";
        this.Convergence = "";
        this.Structure = "";
        this.Patterns = "";
        this.Rationale = "";
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("foresight", this.Foresight);
        writer.WriteString("convergence", this.Convergence);
        writer.WriteString("structure", this.Structure);
        writer.WriteString("patterns", this.Patterns);
        writer.WriteString("rationale", this.Rationale);
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
            if (prop == "foresight") {
                this.Foresight = reader.GetString();
            } else if (prop == "convergence") {
                this.Convergence = reader.GetString();
            } else if (prop == "structure") {
                this.Structure = reader.GetString();
            } else if (prop == "patterns") {
                this.Patterns = reader.GetString();
            } else if (prop == "rationale") {
                this.Rationale = reader.GetString();
            } else {
                reader.Skip();
            }
        }
    }
}

/// <summary>
/// 验收条目：场景 + 断言 + 可选验证命令/测试名——「验收对照」的源头。
/// D5 自审证明 / D3 用例级判定都以结构化条目为准；纯文本兼容期由
/// <see cref="AIAcceptanceSpec"/> 两态承载。
/// </summary>
public class AIAcceptanceItem : IJsonSerializable, IJsonDeserializable {
    public string Scenario;
    public string Assertions;
    /// <summary>可选：可执行验证命令（如 `arc test <proj> --logger json`）。</summary>
    public string VerifyCommand;
    /// <summary>可选：证明测试名（如 `ExportTests.HeaderRow`；供 `--list-tests` / `--logger json` 对照）。</summary>
    public string TestName;

    public AIAcceptanceItem() {
        this.Scenario = "";
        this.Assertions = "";
        this.VerifyCommand = "";
        this.TestName = "";
    }

    public AIAcceptanceItem(string scenario, string assertions) {
        this.Scenario = scenario != null ? scenario : "";
        this.Assertions = assertions != null ? assertions : "";
        this.VerifyCommand = "";
        this.TestName = "";
    }

    public bool IsEmpty {
        get {
            return (this.Scenario == null || this.Scenario == "")
                && (this.Assertions == null || this.Assertions == "");
        }
    }

    /// <summary>折叠为单行（D5 槽位 / 上下文块用）。</summary>
    public string ToLine() {
        string s = (this.Scenario != null && this.Scenario != "") ? this.Scenario : this.Assertions;
        if (this.TestName != null && this.TestName != "") {
            s = s + " [test: " + this.TestName + "]";
        }
        if (this.VerifyCommand != null && this.VerifyCommand != "") {
            s = s + " [verify: " + this.VerifyCommand + "]";
        }
        return s;
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("scenario", this.Scenario);
        writer.WriteString("assertions", this.Assertions);
        writer.WriteString("verifyCommand", this.VerifyCommand);
        writer.WriteString("testName", this.TestName);
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
            if (prop == "scenario") {
                this.Scenario = reader.GetString();
            } else if (prop == "assertions") {
                this.Assertions = reader.GetString();
            } else if (prop == "verifyCommand") {
                this.VerifyCommand = reader.GetString();
            } else if (prop == "testName") {
                this.TestName = reader.GetString();
            } else {
                reader.Skip();
            }
        }
    }
}

/// <summary>
/// 验收面：场景 + 断言（测试先行锁定）。迁移期两态——纯文本（Scenarios/Assertions）
/// 兼容既有流程；结构化条目（Items）为「验收对照」新面，/revise 鼓励使用。
/// </summary>
public class AIAcceptanceSpec : IJsonSerializable, IJsonDeserializable {
    public string Scenarios;
    public string Assertions;
    /// <summary>结构化验收条目（场景 + 断言 + 可选验证命令/测试名）；非空时优先于纯文本。</summary>
    public List<AIAcceptanceItem> Items;

    public AIAcceptanceSpec() {
        this.Scenarios = "";
        this.Assertions = "";
        this.Items = new List<AIAcceptanceItem>();
    }

    /// <summary>是否已定义可验收内容（结构化条目或纯文本任一非空）。</summary>
    public bool IsEmpty {
        get {
            return (this.Scenarios == null || this.Scenarios == "")
                && (this.Assertions == null || this.Assertions == "")
                && !this.HasStructuredItems;
        }
    }

    public bool HasStructuredItems {
        get { return this.Items != null && this.Items.Count > 0; }
    }

    /// <summary>追加结构化条目（场景/断言 + 可选测试名/验证命令）。</summary>
    public AIAcceptanceItem AddItem(string scenario, string assertions, string? testName, string? verifyCommand) {
        if (this.Items == null) {
            this.Items = new List<AIAcceptanceItem>();
        }
        AIAcceptanceItem item = new AIAcceptanceItem(scenario, assertions);
        if (testName != null) {
            item.TestName = testName;
        }
        if (verifyCommand != null) {
            item.VerifyCommand = verifyCommand;
        }
        this.Items.Add(item);
        return item;
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("scenarios", this.Scenarios);
        writer.WriteString("assertions", this.Assertions);
        writer.WritePropertyName("items");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.Items.Count) {
            this.Items[i].WriteJson(writer);
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
            if (prop == "items") {
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
                    AIAcceptanceItem item = new AIAcceptanceItem();
                    item.ReadJson(reader);
                    this.Items.Add(item);
                }
            } else {
                if (!reader.Read()) {
                    return;
                }
                if (prop == "scenarios") {
                    this.Scenarios = reader.GetString();
                } else if (prop == "assertions") {
                    this.Assertions = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}

/// <summary>
/// AIRfc 聚合根：跨任务、跨版本的需求与交付唯一事实源。
/// Plan 面仅引用 Arc.Agent.AIPlan，不复制步骤状态机。
/// </summary>
public class AIRfc : IJsonSerializable, IJsonDeserializable {
    public string RfcId;
    public int Revision;
    public AIIntentionSpec Intention;
    public AIDesignSpec Design;
    public AIAcceptanceSpec Acceptance;

    /// <summary>所引用 AIPlan 的稳定标识（会话内可再解析为 AIPlan 实例）。</summary>
    public string PlanId;

    /// <summary>可选：已解析的计划句柄；序列化/跨会话以 PlanId 为准。null = 尚未 AttachPlan。</summary>
    public AIPlan? Plan;

    public List<AIRfcWorkItem> WorkItems;

    /// <summary>运行态（Active / Superseded / Rejected / Contested / Frozen / Closed / Cancelled）；完整边表见 airfc §4。</summary>
    public AIRfcStatus Status;

    /// <summary>
    /// 本版 Revision 的来源（会话 / 分支 / 发起方）。L2 Spec 矛盾判定（B1）按来源区分：
    /// 同来源修订 = 正常 refine，异来源覆盖同 acceptance 项 = 冲突升级。
    /// </summary>
    public string Source;

    public AIRfc() {
        this.RfcId = "";
        this.Revision = 0;
        this.Intention = new AIIntentionSpec();
        this.Design = new AIDesignSpec();
        this.Acceptance = new AIAcceptanceSpec();
        this.PlanId = "";
        this.Plan = null;
        this.WorkItems = new List<AIRfcWorkItem>();
        this.Status = AIRfcStatus.Active;
        this.Source = "";
    }

    /// <summary>折叠为上下文文本（前缀稳定，便于 KV cache）。</summary>
    public string ToContextBlock() {
        string t = this.Intention != null ? this.Intention.Text : "";
        string g = "";
        if (this.Plan != null) {
            g = this.Plan.Goal != null ? this.Plan.Goal : "";
        } else if (this.PlanId != null && this.PlanId != "") {
            g = this.PlanId;
        }
        string v = this.Acceptance != null ? this.Acceptance.Assertions : "";
        string items = "";
        if (this.Acceptance != null && this.Acceptance.HasStructuredItems) {
            StringBuilder ib = new StringBuilder();
            int i = 0;
            int n = this.Acceptance.Items.Count;
            while (i < n) {
                ib.Append("    " + (i + 1) + ". " + this.Acceptance.Items[i].ToLine() + "\n");
                i = i + 1;
            }
            items = ib.ToString();
        }
        string block = "[airfc " + this.RfcId + " v" + this.Revision + "]\n"
            + "intention: " + t + "\n"
            + "plan: " + g + "\n"
            + "acceptance: " + v + "\n";
        if (items != "") {
            block = block + "acceptance-items:\n" + items;
        }
        return block;
    }

    /// <summary>
    /// 序列化为 JSON 对象（含 Revision / Status / WorkItems / PlanId / Spec 三面；
    /// 运行句柄 <see cref="Plan"/> 不落盘，跨会话以 PlanId 为准）。
    /// </summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("rfcId", this.RfcId);
        writer.WriteNumber("revision", this.Revision);
        writer.WriteString("status", AIRfcStatusCodec.ToWireString(this.Status));
        writer.WriteString("planId", this.PlanId);
        writer.WriteString("source", this.Source);
        if (this.Intention != null) {
            writer.WritePropertyName("intention");
            this.Intention.WriteJson(writer);
        } else {
            writer.WriteNull("intention");
        }
        if (this.Design != null) {
            writer.WritePropertyName("design");
            this.Design.WriteJson(writer);
        } else {
            writer.WriteNull("design");
        }
        if (this.Acceptance != null) {
            writer.WritePropertyName("acceptance");
            this.Acceptance.WriteJson(writer);
        } else {
            writer.WriteNull("acceptance");
        }
        writer.WritePropertyName("workItems");
        writer.WriteStartArray();
        int i = 0;
        while (i < this.WorkItems.Count) {
            this.WorkItems[i].WriteJson(writer);
            i = i + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>从 JSON 对象就地填充（与 <see cref="WriteJson"/> 同构；Plan 句柄恢复为 null）。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop == "intention") {
                if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
                    continue;
                }
                AIIntentionSpec spec = new AIIntentionSpec();
                spec.ReadJson(reader);
                this.Intention = spec;
            } else if (prop == "design") {
                if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
                    continue;
                }
                AIDesignSpec spec = new AIDesignSpec();
                spec.ReadJson(reader);
                this.Design = spec;
            } else if (prop == "acceptance") {
                if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
                    continue;
                }
                AIAcceptanceSpec spec = new AIAcceptanceSpec();
                spec.ReadJson(reader);
                this.Acceptance = spec;
            } else if (prop == "workItems") {
                if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
                    continue;
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
                    AIRfcWorkItem item = new AIRfcWorkItem();
                    item.ReadJson(reader);
                    this.WorkItems.Add(item);
                }
            } else {
                if (!reader.Read()) {
                    return;
                }
                if (prop == "rfcId") {
                    this.RfcId = reader.GetString();
                } else if (prop == "revision") {
                    this.Revision = reader.GetInt32();
                } else if (prop == "status") {
                    this.Status = AIRfcStatusCodec.FromWireString(reader.GetString());
                } else if (prop == "planId") {
                    this.PlanId = reader.GetString();
                } else if (prop == "source") {
                    this.Source = reader.GetString();
                } else {
                    reader.Skip();
                }
            }
        }
    }
}
