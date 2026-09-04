// RFC 038: tool descriptor (schema / capability label).
namespace Arc.Agent;

/// <summary>Tool schema entry for Provider listing and sandbox pre-check.</summary>
public class AIToolDescriptor {
    public string Name;
    public string Description;
    public string Capability;
    public bool RequireApproval;
    /// <summary>参数 JSON schema（对象体，如 {"type":"object","properties":{...}}）；空则默认 {"type":"object"}。</summary>
    public string ParametersSchema;

    public AIToolDescriptor() {
        this.Name = "";
        this.Description = "";
        this.Capability = "ai.Tool";
        this.RequireApproval = false;
        this.ParametersSchema = "";
    }

    public AIToolDescriptor(string name, string capability) {
        this.Name = name != null ? name : "";
        this.Description = "";
        this.Capability = capability != null && capability != "" ? capability : "ai.Tool";
        this.RequireApproval = false;
        this.ParametersSchema = "";
    }

    public AIToolDescriptor(string name, string description, string capability, bool requireApproval) {
        this.Name = name != null ? name : "";
        this.Description = description != null ? description : "";
        this.Capability = capability != null && capability != "" ? capability : "ai.Tool";
        this.RequireApproval = requireApproval;
        this.ParametersSchema = "";
    }
}
