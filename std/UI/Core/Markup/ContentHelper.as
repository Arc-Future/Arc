// RFC 037 D2 / RFC 037：Arc.UI — ContentHelper 文本提取器。
namespace Arc.UI;

/// <summary>Content variant 呈现值提取器（单一解包点）。</summary>
public class ContentHelper {
    public static string TextOrEmpty(Content c) {
        switch (c) {
            case Content.Text(s): return s;
            case Content.None: return "";
            default: return "";
        }
    }

    /// <summary>
    /// 解包 Content variant 的 Element 载荷；非 Element case 返回 null。
    /// </summary>
    public static Element? ElementOrNull(Content c) {
        switch (c) {
            case Content.Element(el): return el;
            default: return null;
        }
    }
}
