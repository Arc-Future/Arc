// RFC 041 §7.5：Embedding 域请求/响应模型（对齐 OpenAI /v1/embeddings）。
//
// Input[] 即批量（条数由数组长度决定）；Data[i].Index 对齐 Input 位置。向量本体由
// 用户侧落库，不直送 LLM（§7.7 示例②）。Arc 无可空值类型——Dimensions 以 <=0 表示未设置。
namespace Arc.AI.Models;

using Arc.Collections;

/// <summary>嵌入请求（对齐 OpenAI /v1/embeddings）。</summary>
public class AIEmbedRequest {
    /// <summary>模型标识（本地门面下可省略，由组合根默认 ModelId 填充）。</summary>
    public string Model { get; set; }
    /// <summary>输入文本批量（Input[]；条数由数组长度决定）。</summary>
    public List<string> Input { get; set; }
    /// <summary>可选降维（&lt;=0 = 不降维）。</summary>
    public int Dimensions { get; set; }
    /// <summary>编码格式（默认 Float）。</summary>
    public AIEncodingFormat EncodingFormat { get; set; }

    public AIEmbedRequest() {
        this.Model = "";
        this.Input = new List<string>();
        this.Dimensions = 0;
        this.EncodingFormat = AIEncodingFormat.Float;
    }
}

/// <summary>嵌入结果（对齐 OpenAI embeddings 返回值）。</summary>
public class AIEmbedResult : AIModelResult {
    /// <summary>模型标识（回显）。</summary>
    public string Model { get; set; }
    /// <summary>data[{index, vector}]（顺序对齐 Input 位置）。</summary>
    public List<AIEmbeddingData> Data { get; set; }
    /// <summary>用量（回显）。</summary>
    public AIUsage Usage { get; set; }

    public AIEmbedResult() {
        this.Model = "";
        this.Data = new List<AIEmbeddingData>();
        this.Usage = new AIUsage();
    }
}

/// <summary>单条嵌入数据（对齐 OpenAI embeddings data 项）。</summary>
public class AIEmbeddingData {
    /// <summary>对应 Input 位置（0-based）。</summary>
    public int Index { get; set; }
    /// <summary>嵌入向量（EncodingFormat=Base64 时解码后仍为 Vector）。</summary>
    public AIVector Vector { get; set; }

    public AIEmbeddingData() {
        this.Index = 0;
        this.Vector = new AIVector();
    }
}
