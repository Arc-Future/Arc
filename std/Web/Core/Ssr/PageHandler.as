// PageHandler —— SSR 页面处理器基类（RFC 040 §5 code-behind）：强类型模型渲染入口。
// WPF xaml+cs 心智：页面标记 = <TRequest>.html 模板 + code-behind = PageHandler 派生类 +
// DataContext = View(model) 的强类型模型。
// 编译器将 View(model)/Partial(model) 调用在编译期改写为模板渲染函数调用（模板 <TRequest>.html
// 编译期解析 + 绑定路径对照模型类型检查 + 生成类型安全渲染代码 + 默认转义）。
// 本基类的方法体为契约占位：正常 SSR 项目由编译器改写绕过，仅无编译器改写接入时兜底。
namespace Arc.Web;

using Arc;

/// <summary>
/// SSR 页面处理器基类（code-behind，对标 WPF 窗口 code-behind）。
/// TRequest 为页面请求类型（IRequest&lt;IWebResult&gt;），对应模板文件 &lt;TRequest&gt;.html。
/// 派生类实现 HandleAsync 并返回 View(model) / Partial(model) / Redirect(url) / File(data, contentType)。
/// </summary>
public abstract class PageHandler<TRequest> : IRequestHandler<TRequest, IWebResult> {
    /// <summary>处理页面请求（派生类实现业务逻辑并返回 View/Redirect/File 结果）。</summary>
    public abstract Task<IWebResult> HandleAsync(TRequest request, CancellationToken cancellationToken);

    /// <summary>以强类型模型渲染页面。编译器改写为渲染函数调用；未改写时返回空视图（契约占位）。</summary>
    public IHtmlView View<TModel>(TModel model) {
        return new HtmlViewResult("");
    }

    /// <summary>以强类型模型渲染页面片段（组件/Layout 级复用）。编译器改写同上。</summary>
    public IHtmlView Partial<TModel>(TModel model) {
        return new HtmlViewResult("");
    }

    /// <summary>返回重定向结果（PRG 模式）。</summary>
    public IRedirectResult Redirect(string url) {
        return new RedirectResult(url);
    }

    /// <summary>返回内联文件/二进制结果（无下载文件名）。</summary>
    public IFileResult File(byte[] data, string contentType) {
        return new FileResult(data, contentType, "");
    }

    /// <summary>返回下载文件/二进制结果（带 Content-Disposition 文件名）。</summary>
    public IFileResult File(byte[] data, string contentType, string fileName) {
        return new FileResult(data, contentType, fileName);
    }
}
