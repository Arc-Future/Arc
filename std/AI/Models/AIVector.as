// RFC 041 §7.5：AIVector — 语义向量值类型（嵌入 / CLIP / 人脸识别共用）。
//
// 由服务内部从 Tensor 翻译（Float32 [dim] → Values），不进 Arc.AI 核心；
// 向量本体由用户侧落库，不直送 LLM（§7.7 示例②）。余弦相似度为领域惯例（CLIP/检索）。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;
using Arc.Collections;

/// <summary>语义向量（RFC 041 §7.5）。<see cref="Values"/> 为 Float32 分量。</summary>
public class AIVector {
    /// <summary>向量分量（Float32）。</summary>
    public List<float> Values { get; set; }

    public AIVector() {
        this.Values = new List<float>();
    }

    /// <summary>向量维度（= Values.Count）。</summary>
    public int Dimension {
        get { return this.Values != null ? this.Values.Count : 0; }
    }

    /// <summary>从分量列表构造（拷贝，不共享引用）。</summary>
    public static AIVector FromValues(List<float> values) {
        AIVector v = new AIVector();
        if (values != null) {
            int i = 0;
            while (i < values.Count) {
                v.Values.Add(values[i]);
                i = i + 1;
            }
        }
        return v;
    }

    /// <summary>从 Float32 张量读取（[dim] 或 [1, dim] 展平）。</summary>
    public static AIVector FromTensor(Tensor t) {
        AIVector v = new AIVector();
        if (t != null) {
            List<float> data = t.ReadFloat();
            if (data != null) {
                int i = 0;
                while (i < data.Count) {
                    v.Values.Add(data[i]);
                    i = i + 1;
                }
            }
        }
        return v;
    }

    /// <summary>余弦相似度（-1..1）。维度取两向量较短者；任一方零范数 → 0。</summary>
    public float CosineSimilarity(AIVector other) {
        if (other == null) {
            return (float)0.0;
        }
        int n = this.Values.Count;
        int m = other.Values.Count;
        int len = n < m ? n : m;
        if (len == 0) {
            return (float)0.0;
        }
        double dot = 0.0;
        double a2 = 0.0;
        double b2 = 0.0;
        int i = 0;
        while (i < len) {
            double x = (double)this.Values[i];
            double y = (double)other.Values[i];
            dot = dot + x * y;
            a2 = a2 + x * x;
            b2 = b2 + y * y;
            i = i + 1;
        }
        if (a2 <= 0.0 || b2 <= 0.0) {
            return (float)0.0;
        }
        double denom = Math.Sqrt(a2) * Math.Sqrt(b2);
        if (denom <= 0.0) {
            return (float)0.0;
        }
        return (float)(dot / denom);
    }
}
