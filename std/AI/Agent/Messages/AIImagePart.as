// AIImagePart — 图像内容部件（RFC 038 M5 · Messages 层）。
namespace Arc.Agent;

/// <summary>图像内容部件（OpenAI content type=image_url）。ImageUrl 支持
/// http(s) URL 或 data:image/...;base64,... 内联数据。</summary>
public class AIImagePart : AIContentPart {
    public string ImageUrl;

    public AIImagePart(string imageUrl) : base("image_url") {
        this.ImageUrl = imageUrl != null ? imageUrl : "";
    }

    public override string BuildJson() {
        return "{\"type\":\"image_url\",\"image_url\":{\"url\":\"" + AIContentPart.JsonEsc(this.ImageUrl) + "\"}}";
    }
}
