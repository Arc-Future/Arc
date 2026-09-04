// RFC 037 §10 AI 原生：DpValueConverter —— 字符串 → DP 类型化值的单一事实来源。
//
// ArmlParser（ARML 属性解析）与 LivePreviewHost（ApplyPatch 实时修改）共用
// 同一套「字符串 → 依赖属性（DependencyProperty<T>）类型化值」转换逻辑。
// 收敛为单一职责组件，杜绝双实现漂移（此前两处各自复制一份 is/cast 分支）。
//
// 支持转换（对齐 DP 注册的类型面）：
//   - bool / double / int / string：数值与布尔字面量解析
//   - Brush：hex / 命名色字符串 → SolidColorBrush
//   - Content：字符串 → Content.Text
//   - HorizontalAlignment / VerticalAlignment：XAML 风格枚举名
//   - object：兜底存原字符串
//
// 未知 DP 类型返回 false，由调用方决定是否视为失败（解析器静默跳过 /
// ApplyPatch 返回失败）。

namespace Arc.UI.Markup;

using Arc.UI;
using Arc.UI.Media;

/// <summary>
/// 字符串 → 依赖属性类型化值转换器。按 DP 泛型实际类型分派，
/// 替代调用方手写 is/cast 分支。
/// </summary>
internal static class DpValueConverter {
    /// <summary>
    /// 按 DP 泛型类型解析字符串值并设置到元素。
    /// </summary>
    /// <param name="element">目标元素。</param>
    /// <param name="dpObj">经 ResolveProperty 解析的 DP（object 擦除视图）。</param>
    /// <param name="value">字符串值。</param>
    /// <returns>是否成功设置（未知 DP 类型返回 false）。</returns>
    public static bool SetValue(Element element, object dpObj, string value) {
        if (element == null || dpObj == null) {
            return false;
        }

        // bool DP
        if (dpObj is DependencyProperty<bool>) {
            bool boolVal = false;
            if (value == "True" || value == "true" || value == "1") {
                boolVal = true;
            }
            element.SetValue<bool>((DependencyProperty<bool>)dpObj, boolVal);
            return true;
        }

        // double DP
        if (dpObj is DependencyProperty<double>) {
            double numVal = 0.0;
            double.TryParse(value, out numVal);
            element.SetValue<double>((DependencyProperty<double>)dpObj, numVal);
            return true;
        }

        // int DP
        if (dpObj is DependencyProperty<int>) {
            int intVal = 0;
            int.TryParse(value, out intVal);
            element.SetValue<int>((DependencyProperty<int>)dpObj, intVal);
            return true;
        }

        // string DP
        if (dpObj is DependencyProperty<string>) {
            element.SetValue<string>((DependencyProperty<string>)dpObj, value);
            return true;
        }

        // Brush DP（颜色字符串 → SolidColorBrush）
        if (dpObj is DependencyProperty<Brush>) {
            element.SetValue<Brush>((DependencyProperty<Brush>)dpObj, Brush.FromString(value));
            return true;
        }

        // Content variant DP（字符串 → Content.Text）
        if (dpObj is DependencyProperty<Content>) {
            element.SetValue<Content>((DependencyProperty<Content>)dpObj, Content.Text(value));
            return true;
        }

        // HorizontalAlignment 枚举 DP
        if (dpObj is DependencyProperty<HorizontalAlignment>) {
            HorizontalAlignment ha = HorizontalAlignment.Stretch;
            if (value == "Left") { ha = HorizontalAlignment.Left; }
            else if (value == "Center") { ha = HorizontalAlignment.Center; }
            else if (value == "Right") { ha = HorizontalAlignment.Right; }
            else if (value == "Stretch") { ha = HorizontalAlignment.Stretch; }
            element.SetValue<HorizontalAlignment>((DependencyProperty<HorizontalAlignment>)dpObj, ha);
            return true;
        }

        // VerticalAlignment 枚举 DP
        if (dpObj is DependencyProperty<VerticalAlignment>) {
            VerticalAlignment va = VerticalAlignment.Stretch;
            if (value == "Top") { va = VerticalAlignment.Top; }
            else if (value == "Center") { va = VerticalAlignment.Center; }
            else if (value == "Bottom") { va = VerticalAlignment.Bottom; }
            else if (value == "Stretch") { va = VerticalAlignment.Stretch; }
            element.SetValue<VerticalAlignment>((DependencyProperty<VerticalAlignment>)dpObj, va);
            return true;
        }

        // object DP 兜底（原字符串值）
        if (dpObj is DependencyProperty<object>) {
            element.SetValue<object>((DependencyProperty<object>)dpObj, value);
            return true;
        }

        return false;
    }
}
