// RFC 043 references/work-summary：每工作单元结构化小结（领导偏差判定主表面）。
namespace Arc.Agent.Harness;
using Arc;

/// <summary>回合/步骤小结五字段（每字段 ≤1 行语义；困难/绕过与发现必答）。</summary>
public class AIWorkSummary {
    public string UnitId;
    public string Did;
    public string Alignment;
    public string Verification;
    public string Difficulty;
    public string Findings;

    public AIWorkSummary() {
        this.UnitId = "";
        this.Did = "";
        this.Alignment = "";
        this.Verification = "";
        this.Difficulty = "无";
        this.Findings = "无";
    }

    public AIWorkSummary(string unitId, string did, string alignment, string verification) {
        this.UnitId = unitId != null ? unitId : "";
        this.Did = did != null ? did : "";
        this.Alignment = alignment != null ? alignment : "";
        this.Verification = verification != null ? verification : "";
        this.Difficulty = "无";
        this.Findings = "无";
    }

    /// <summary>格式化为决策面文本（非聊天记录）。</summary>
    public string Format() {
        string diff = this.Difficulty != null && this.Difficulty != "" ? this.Difficulty : "无";
        string find = this.Findings != null && this.Findings != "" ? this.Findings : "无";
        return "■ 小结 " + this.UnitId + "\n"
            + "  做了什么  " + this.Did + "\n"
            + "  对齐      " + this.Alignment + "\n"
            + "  验证      " + this.Verification + "\n"
            + "  困难/绕过 " + diff + "\n"
            + "  发现      " + find + "\n";
    }

    /// <summary>是否暴露需升级的发现（非「无」即触发评审信号）。</summary>
    public bool HasFindings {
        get {
            return this.Findings != null && this.Findings != "" && this.Findings != "无";
        }
    }

    /// <summary>是否声明绕过（非「无」须上报）。</summary>
    public bool HasBypass {
        get {
            if (this.Difficulty == null || this.Difficulty == "" || this.Difficulty == "无") {
                return false;
            }
            return this.Difficulty.IndexOf("绕过") >= 0;
        }
    }
}
