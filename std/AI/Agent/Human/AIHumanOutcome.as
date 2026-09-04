// RFC 038 —— 门闩关闭结果（Approve/Reject/Input/Cancel）。
namespace Arc.Agent;

/// <summary>人类门闩关闭后的结果。</summary>
public class AIHumanOutcome {
    public AIHumanDecision Decision;
    public string EditedToolName;
    public string EditedToolArguments;
    public string InputText;
    public string RejectReason;

    public AIHumanOutcome() {
        this.Decision = AIHumanDecision.Rejected;
        this.EditedToolName = "";
        this.EditedToolArguments = "";
        this.InputText = "";
        this.RejectReason = "";
    }

    public static AIHumanOutcome Approved(string toolName, string toolArguments) {
        AIHumanOutcome o = new AIHumanOutcome();
        o.Decision = AIHumanDecision.Approved;
        o.EditedToolName = toolName != null ? toolName : "";
        o.EditedToolArguments = toolArguments != null ? toolArguments : "";
        return o;
    }

    public static AIHumanOutcome Rejected(string reason) {
        AIHumanOutcome o = new AIHumanOutcome();
        o.Decision = AIHumanDecision.Rejected;
        o.RejectReason = reason != null ? reason : "";
        return o;
    }

    public static AIHumanOutcome Input(string text) {
        AIHumanOutcome o = new AIHumanOutcome();
        o.Decision = AIHumanDecision.InputProvided;
        o.InputText = text != null ? text : "";
        return o;
    }

    public static AIHumanOutcome Cancelled() {
        AIHumanOutcome o = new AIHumanOutcome();
        o.Decision = AIHumanDecision.Cancelled;
        o.RejectReason = "cancelled";
        return o;
    }
}
