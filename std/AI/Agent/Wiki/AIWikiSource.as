// RFC 038：AIWikiSource —— 不可变原始源（防腐防线 L1 实体）。
//
// 源不可变：任何变更 = 新版本（新 AIWikiSource 实例，PreviousId 指向旧版本），
// 绝不在原地改写内容。内容指纹 Fingerprint 由构造时按 Content 计算（确定性、可复现）。
// Ingest 以本类型为输入，把其中的页面与断言整合进知识图。
namespace Arc.Agent;
using Arc;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>
/// 不可变原始源：一段带指纹与版本链的原始内容，连同其产出的知识图载荷（页面 + 断言）。
/// 构造即定形；Ingest 只读消费，永不修改源本体（防腐 L1：源不可变 + 指纹）。
/// </summary>
public class AIWikiSource : IJsonSerializable {
    /// <summary>源唯一标识（SourceId；G9 重复检测键）。</summary>
    public string Id { get; }
    /// <summary>不可变原始内容（任何变更即新版本）。</summary>
    public string Content { get; }
    /// <summary>内容指纹（构造时由 Content 确定性计算）。</summary>
    public string Fingerprint { get; }
    /// <summary>捕获时间。</summary>
    public DateTime CapturedAt { get; }
    /// <summary>版本链上一版本 Id（"" = 根版本）。</summary>
    public string PreviousId { get; }
    /// <summary>本源产出的知识页。</summary>
    public List<AIWikiPage> Pages { get; }
    /// <summary>本源产出的断言。</summary>
    public List<AIWikiClaim> Claims { get; }

    public AIWikiSource(string id, string content, DateTime capturedAt, string previousId,
        List<AIWikiPage> pages, List<AIWikiClaim> claims) {
        this.Id = id != null ? id : "";
        this.Content = content != null ? content : "";
        this.Fingerprint = AIWikiSource.ComputeFingerprint(this.Content);
        this.CapturedAt = capturedAt;
        this.PreviousId = previousId != null ? previousId : "";
        this.Pages = pages != null ? pages : new List<AIWikiPage>();
        this.Claims = claims != null ? claims : new List<AIWikiClaim>();
    }

    /// <summary>是否为根版本（无上一版本）。</summary>
    public bool IsRoot() {
        return this.PreviousId == "";
    }

    /// <summary>JSON 序列化（落盘用）。源不可变，反序列化由 AIWiki 构造新实例还原，故仅写。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("id", this.Id);
        writer.WriteString("content", this.Content);
        writer.WriteString("capturedAt", this.CapturedAt.ToString());
        writer.WriteString("previousId", this.PreviousId);
        writer.WritePropertyName("pages");
        writer.WriteStartArray();
        int np = this.Pages.Count;
        int pi = 0;
        while (pi < np) {
            this.Pages[pi].WriteJson(writer);
            pi = pi + 1;
        }
        writer.WriteEndArray();
        writer.WritePropertyName("claims");
        writer.WriteStartArray();
        int nc = this.Claims.Count;
        int ci = 0;
        while (ci < nc) {
            this.Claims[ci].WriteJson(writer);
            ci = ci + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>确定性内容指纹：FNV-1a 逐字符哈希（跨进程可复现；防腐 L1）。</summary>
    private static string ComputeFingerprint(string content) {
        string c = content != null ? content : "";
        int h = -2128831035;
        int n = c.Length;
        int i = 0;
        while (i < n) {
            char ch = c[i];
            h = h ^ HashCode.HashValue(ch);
            h = h * 16777619;
            i = i + 1;
        }
        return "fp-" + HashCode.HashValue(h);
    }
}
