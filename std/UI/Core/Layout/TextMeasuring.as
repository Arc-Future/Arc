// RFC 037 §9 / custom-fonts：布局文本度量服务入口。
//
// Current 由 WgpuRender.Initialize 挂接、Shutdown 卸下。
// 注册正道仍是 Application.Fonts.RegisterFamily（Workstream B）；本类只做度量挂钩，
// 不另立注册 API。无服务时 EstimateTextSize 诚实占位（禁字节启发式）。

namespace Arc.UI.Layout;

/// <summary>布局文本度量单一服务入口（同源 atlas）。</summary>
public class TextMeasuring {
    /// <summary>当前度量实现；null = atlas/后端尚未就绪。</summary>
    public static ITextMetrics Current;

    /// <summary>挂接度量实现（幂等覆盖；通常为 WgpuRender）。</summary>
    public static void Attach(ITextMetrics metrics) {
        TextMeasuring.Current = metrics;
    }

    /// <summary>卸下指定实现；仅当仍为当前实例时清空（避免误清新后端）。</summary>
    public static void Detach(ITextMetrics metrics) {
        if (TextMeasuring.Current == metrics) {
            TextMeasuring.Current = null;
        }
    }

    /// <summary>度量服务是否已挂接。</summary>
    public static bool IsAvailable() {
        return TextMeasuring.Current != null;
    }
}
