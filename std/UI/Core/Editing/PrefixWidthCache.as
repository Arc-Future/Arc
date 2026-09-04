// RFC 037 §8 修订（text-editing.md §4）：前缀宽度缓存——命中测试几何加速。
//
// 按 TextBoxModel.Version 失效（版本+文本+字体指纹任一变化重算），
// 替代逐前缀 MeasureText O(n) 重测；同一版本内重复点击/候选窗锚定
// 直接命中。缓存区间语义：_widths[i] = 前缀 [0, i) 的宽度，i ∈ [0, n]。

namespace Arc.UI.Editing;

using Arc.Collections;
using Arc.UI.Layout;

/// <summary>
/// 前缀宽度缓存（版本失效；点击定位与 IME 候选窗锚定共享）。
/// </summary>
internal class PrefixWidthCache {
    string _text = "";
    int _version = -1;
    double _fontSize;
    string _family;
    string _weight;
    List<double> _widths;

    public PrefixWidthCache() {
        _widths = new List<double>();
        _widths.Add(0.0);
    }

    /// <summary>
    /// 确保缓存对当前指纹有效（未失效则 no-op）。度量委托全局 ITextMetrics。
    /// </summary>
    public void Ensure(string text, int version, double fontSize, string family, string weight) {
        if (text == null) {
            text = "";
        }
        if (_version == version && _text == text && _fontSize == fontSize
            && _family == family && _weight == weight) {
            return;
        }
        if (!TextMeasuring.IsAvailable()) {
            return;
        }
        _version = version;
        _text = text;
        _fontSize = fontSize;
        _family = family;
        _weight = weight;
        _widths = new List<double>();
        _widths.Add(0.0);
        int n = text.Length;
        int i = 1;
        while (i <= n) {
            LayoutSize sz = TextMeasuring.Current.MeasureText(
                text.Substring(0, i), fontSize, 0.0, 0.0, family, weight);
            _widths.Add(sz.Width);
            i = i + 1;
        }
    }

    /// <summary>前缀 [0, end) 的宽度（缓存无效或越界回退 0）。</summary>
    public double WidthOfPrefix(int end) {
        if (end < 0 || end >= _widths.Count) {
            return 0.0;
        }
        return _widths[end];
    }

    /// <summary>
    /// 距目标横坐标最近的字符边界索引（点击定位：targetX 相对字形原点）。
    /// </summary>
    public int NearestIndexTo(double targetX) {
        int n = _widths.Count;
        if (n <= 1) {
            return 0;
        }
        if (targetX <= 0.0) {
            return 0;
        }
        int best = 0;
        double bestDist = targetX;
        int i = 1;
        while (i < n) {
            double d = _widths[i] - targetX;
            if (d < 0.0) {
                d = -d;
            }
            if (d < bestDist) {
                bestDist = d;
                best = i;
            }
            i = i + 1;
        }
        return best;
    }
}
