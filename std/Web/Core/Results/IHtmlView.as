// IHtmlView —— HTML 视图结果（RFC 040 §5）：SSR 页面响应载体。
// 编译器将 PageHandler.View(model) 改写为 new HtmlViewResult(渲染函数(model))，绑定路径编译期检查。
namespace Arc.Web;

/// <summary>HTML 视图结果：承载编译期 SSR 渲染完成的 HTML。</summary>
public interface IHtmlView : IWebResult {
    /// <summary>渲染完成的 HTML 文档字符串。</summary>
    string Html { get; }
}
