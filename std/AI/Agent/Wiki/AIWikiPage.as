// RFC 038：AIWikiPage —— 互链知识页（知识图节点）。
//
// 在 RFC 038 扁平页（Path/Body/Meta）之上增量扩展为互链知识页：
//   - PageId 稳定标识（G9 重复别名检测键，与 Path 别名并行）
//   - Type/Status 分类与生命周期
//   - ClaimIds 引用断言（断言命中/合成入口）
//   - Links 互链目标（路径别名）；Backlinks 反向引用（由 Ingest/Lint 重建）
// 保留既有 Path/Body/Meta/Clone，向后兼容 Upsert/Get 与 AIWikiContextProvider。
namespace Arc.Agent;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>知识页类型（分类/检索辅助）。</summary>
public enum AIWikiPageType {
    /// <summary>概念页。</summary>
    Concept,
    /// <summary>主题页。</summary>
    Topic,
    /// <summary>文章页。</summary>
    Article,
    /// <summary>索引/目录页。</summary>
    Index,
}

/// <summary>知识页生命周期状态。</summary>
public enum AIWikiPageStatus {
    /// <summary>草稿。</summary>
    Draft,
    /// <summary>已发布。</summary>
    Published,
    /// <summary>已归档。</summary>
    Archived,
}

public class AIWikiPage : IJsonSerializable, IJsonDeserializable {
    public string Path;
    public string Body;
    public AIWikiMeta Meta;
    /// <summary>稳定页面标识（与 Path 别名并行；G9 检测键）。</summary>
    public string PageId;
    /// <summary>页面类型。</summary>
    public AIWikiPageType Type;
    /// <summary>页面状态。</summary>
    public AIWikiPageStatus Status;
    /// <summary>本页引用的断言 Id。</summary>
    public string[] ClaimIds;
    /// <summary>本页互链目标（目标页 Path 别名）。</summary>
    public string[] Links;
    /// <summary>反向引用（链向本页的页面 Path；Ingest/Lint 重建）。</summary>
    public List<string> Backlinks;

    public AIWikiPage() {
        this.Path = ""; this.Body = ""; this.Meta = new AIWikiMeta();
        this.PageId = ""; this.Type = AIWikiPageType.Concept; this.Status = AIWikiPageStatus.Draft;
        string[] empty = [];
        this.ClaimIds = empty;
        this.Links = empty;
        this.Backlinks = new List<string>();
    }
    public AIWikiPage(string path, string body, AIWikiMeta meta) {
        this.Path = path != null ? path : "";
        this.Body = body != null ? body : "";
        this.Meta = meta != null ? meta : new AIWikiMeta();
        this.PageId = this.Path;
        this.Type = AIWikiPageType.Concept; this.Status = AIWikiPageStatus.Draft;
        string[] empty = [];
        this.ClaimIds = empty;
        this.Links = empty;
        this.Backlinks = new List<string>();
    }

    /// <summary>互链知识页构造（PageId 与 Path 解耦的图节点）。</summary>
    public AIWikiPage(string path, string body, AIWikiMeta meta, string pageId, AIWikiPageType type,
        AIWikiPageStatus status, string[] claimIds, string[] links) {
        this.Path = path != null ? path : "";
        this.Body = body != null ? body : "";
        this.Meta = meta != null ? meta : new AIWikiMeta();
        this.PageId = pageId != null ? pageId : this.Path;
        this.Type = type;
        this.Status = status;
        string[] c = claimIds;
        if (c == null) {
            string[] empty = [];
            c = empty;
        }
        this.ClaimIds = c;
        string[] l = links;
        if (l == null) {
            string[] empty2 = [];
            l = empty2;
        }
        this.Links = l;
        this.Backlinks = new List<string>();
    }

    public AIWikiPage Clone() {
        AIWikiMeta src = this.Meta;
        AIWikiMeta m = new AIWikiMeta();
        if (src != null) {
            m.Title = src.Title;
            m.Version = src.Version;
            m.Source = src.Source;
            List<string> tagList = new List<string>();
            string[] tags = src.Tags;
            int n = tags != null ? tags.Length : 0;
            int i = 0;
            while (i < n) {
                tagList.Add(tags[i]);
                i = i + 1;
            }
            m.Tags = tagList.ToArray();
        }
        return new AIWikiPage(this.Path, this.Body, m, this.PageId, this.Type, this.Status,
            this.ClaimIds, this.Links);
    }

    /// <summary>JSON 序列化（落盘用）。Backlinks 为派生反向引用，不持久化（加载后重建）。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("path", this.Path);
        writer.WriteString("body", this.Body);
        writer.WriteString("pageId", this.PageId);
        writer.WriteNumber("type", (int)this.Type);
        writer.WriteNumber("status", (int)this.Status);
        writer.WritePropertyName("meta");
        this.Meta.WriteJson(writer);
        writer.WritePropertyName("claimIds");
        this.WriteStrArray(writer, this.ClaimIds);
        writer.WritePropertyName("links");
        this.WriteStrArray(writer, this.Links);
        writer.WriteEndObject();
    }

    /// <summary>JSON 反序列化（落盘加载）。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "path") {
                    this.Path = reader.GetString();
                } else if (prop == "body") {
                    this.Body = reader.GetString();
                } else if (prop == "pageId") {
                    this.PageId = reader.GetString();
                } else if (prop == "type") {
                    this.Type = this.IntToPageType(reader.GetInt32());
                } else if (prop == "status") {
                    this.Status = this.IntToPageStatus(reader.GetInt32());
                } else if (prop == "meta") {
                    this.Meta.ReadJson(reader);
                } else if (prop == "claimIds") {
                    this.ClaimIds = this.ReadStrArray(reader);
                } else if (prop == "links") {
                    this.Links = this.ReadStrArray(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void WriteStrArray(JsonWriter writer, string[] arr) {
        writer.WriteStartArray();
        int n = arr != null ? arr.Length : 0;
        int i = 0;
        while (i < n) {
            writer.WriteString(arr[i]);
            i = i + 1;
        }
        writer.WriteEndArray();
    }

    private string[] ReadStrArray(JsonReader reader) {
        List<string> list = new List<string>();
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                break;
            }
            if (reader.TokenType == JsonTokenType.String) {
                list.Add(reader.GetString());
            }
        }
        return list.ToArray();
    }

    private AIWikiPageType IntToPageType(int v) {
        if (v == 1) { return AIWikiPageType.Topic; }
        if (v == 2) { return AIWikiPageType.Article; }
        if (v == 3) { return AIWikiPageType.Index; }
        return AIWikiPageType.Concept;
    }

    private AIWikiPageStatus IntToPageStatus(int v) {
        if (v == 1) { return AIWikiPageStatus.Published; }
        if (v == 2) { return AIWikiPageStatus.Archived; }
        return AIWikiPageStatus.Draft;
    }
}
