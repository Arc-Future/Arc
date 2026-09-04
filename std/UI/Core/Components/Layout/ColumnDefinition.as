// RFC 037 D5.2: Grid 列定义（Width = Auto / * / px）。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>Grid 单列定义。</summary>
public class ColumnDefinition {
    /// <summary>列宽（默认 1*）。</summary>
    public GridLength Width;

    public ColumnDefinition() {
        this.Width = GridLength.Star(1.0);
    }
}
