// AITextPart — 文本内容部件（RFC 038 M5 · Messages 层）。
namespace Arc.Agent;

/// <summary>文本内容部件（OpenAI content type=text）。</summary>
public class AITextPart : AIContentPart {
    public string Text;

    public AITextPart(string text) : base("text") {
        this.Text = text != null ? text : "";
    }

    public override string BuildJson() {
        return "{\"type\":\"text\",\"text\":\"" + AIContentPart.JsonEsc(this.Text) + "\"}";
    }
}
