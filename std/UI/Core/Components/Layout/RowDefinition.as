// RFC 037 D5.2: Grid 行定义（Height = Auto / * / px）。

namespace Arc.UI.Components.Layout;

using Arc.UI.Layout;

/// <summary>Grid 单行定义。</summary>
public class RowDefinition {
    /// <summary>行高（默认 1*）。</summary>
    public GridLength Height;

    public RowDefinition() {
        this.Height = GridLength.Star(1.0);
    }
}
