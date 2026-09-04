// RFC 038 上下文成体系：AIContextSourceRef — 上下文块来源/引用（RAG 引文）。
//
// 真实 RAG 检索块需附引文以支撑可溯源性——块正文 + 来源引用分离，扁平化时渲染为
// "Sources:" 注脚。引用承载 Title / Uri / Note 三个可空面。
namespace Arc.Agent;

/// <summary>
/// 上下文块来源/引用（RAG 检索块的引文；审计 + 可溯源性）。由 <see cref="AIContextBlock"/>
/// 携带，扁平化为 system 消息时渲染为来源注脚。
/// </summary>
public class AIContextSourceRef {
    /// <summary>来源标题（如文档名 / 章节名）。</summary>
    public string Title;
    /// <summary>来源定位（Uri / 路径 / Id；可空）。</summary>
    public string Uri;
    /// <summary>备注（如来源类型 / 权限；可空）。</summary>
    public string Note;

    public AIContextSourceRef() {
        this.Title = "";
        this.Uri = "";
        this.Note = "";
    }

    public AIContextSourceRef(string title, string uri) {
        this.Title = title != null ? title : "";
        this.Uri = uri != null ? uri : "";
        this.Note = "";
    }

    public AIContextSourceRef(string title, string uri, string note) {
        this.Title = title != null ? title : "";
        this.Uri = uri != null ? uri : "";
        this.Note = note != null ? note : "";
    }
}