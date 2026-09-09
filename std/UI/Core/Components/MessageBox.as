// RFC 037 · MessageBox：模态对话框走 Popup 轨（禁原生 MessageBox / Win32 对话框）。
//
// **契约**：
//   - ShowAsync → Task&lt;MessageBoxResult&gt;（TCS 桥接按钮点击；帧泵继续跑）
//   - 按钮集：OK / OKCancel / YesNo / YesNoCancel
//   - 图标：MessageBoxImage 自绘几何（色块 + 符号；None 无图标）
//   - Popup.IsLightDismissEnabled = false（点蒙层不关；Esc 由本类自管）
//   - 回调一律静态方法组 + _active 锚点（实例方法组 ByRef 悬垂 UB）
//
// **后置**：主题 Style 多绑定面板 / 多实例队列 / Path 级矢量图标。

namespace Arc.UI.Components;

using Arc;
using Arc.UI;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;
using Arc.UI.Styling;

/// <summary>
/// 模态消息框（Popup 轨）。用法：
/// <c>await MessageBox.ShowAsync(owner, text, caption, MessageBoxButton.YesNo, MessageBoxImage.Question, ct);</c>
/// </summary>
public class MessageBox {
    static Popup _popup;
    static TaskCompletionSource<MessageBoxResult> _tcs;
    static MessageBoxButton _buttons;
    static bool _completed;

    private MessageBox() {
    }

    /// <summary>
    /// 在指定宿主窗口上打开模态消息框（无图标）；用户点按钮或 Esc 后 Task 完成。
    /// 同窗口同时仅一个活跃实例——再次 Show 会先以 Cancel 结束前者。
    /// </summary>
    public static Task<MessageBoxResult> ShowAsync(
        Window owner,
        string messageText,
        string caption,
        MessageBoxButton buttons,
        CancellationToken cancellationToken)
    {
        return MessageBox.ShowAsync(
            owner,
            messageText,
            caption,
            buttons,
            MessageBoxImage.None,
            cancellationToken);
    }

    /// <summary>
    /// 在指定宿主窗口上打开模态消息框（可选几何图标）；用户点按钮或 Esc 后 Task 完成。
    /// </summary>
    public static Task<MessageBoxResult> ShowAsync(
        Window owner,
        string messageText,
        string caption,
        MessageBoxButton buttons,
        MessageBoxImage image,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (_popup != null && !_completed) {
            MessageBox.Complete(MessageBoxResult.Cancel);
        }

        string text = messageText;
        if (text == null) {
            text = "";
        }
        string title = caption;
        if (title == null) {
            title = "";
        }

        TaskCompletionSource<MessageBoxResult> tcs = new TaskCompletionSource<MessageBoxResult>();
        _tcs = tcs;
        _buttons = buttons;
        _completed = false;

        Popup popup = new Popup();
        popup.IsLightDismissEnabled = false;
        popup.Child = MessageBox.BuildContent(title, text, buttons, image);
        _popup = popup;

        Window? host = owner;
        if (host == null && Application.Current != null) {
            host = Application.Current.MainWindow;
        }
        double winW = 720.0;
        double winH = 480.0;
        if (host != null) {
            winW = host.Width;
            winH = host.Height;
            if (winW <= 0.0 && host.DesiredSize.Width > 0.0) {
                winW = host.DesiredSize.Width;
            }
            if (winH <= 0.0 && host.DesiredSize.Height > 0.0) {
                winH = host.DesiredSize.Height;
            }
        }
        if (winW <= 0.0) {
            winW = 720.0;
        }
        if (winH <= 0.0) {
            winH = 480.0;
        }

        double dialogW = 400.0;
        double dialogH = 176.0;
        if (image != MessageBoxImage.None) {
            dialogH = 192.0;
        }
        double placeX = (winW - dialogW) * 0.5;
        double placeY = (winH - dialogH) * 0.5;
        if (placeX < ControlMetrics.SpacingLG) {
            placeX = ControlMetrics.SpacingLG;
        }
        if (placeY < ControlMetrics.SpacingLG) {
            placeY = ControlMetrics.SpacingLG;
        }
        popup.PlacementX = placeX;
        popup.PlacementY = placeY;

        if (cancellationToken.CanBeCanceled) {
            Action cancelCb = MessageBox.OnTokenCanceledStatic;
            cancellationToken.Register(cancelCb);
        }

        if (host != null) {
            popup.Open(host);
        } else {
            MessageBox.CompleteCanceled();
        }
        return tcs.Task;
    }

    /// <summary>
    /// Esc：OKCancel / YesNoCancel → Cancel；YesNo → No；OK-only → OK。
    /// KeyboardRouter 在轻关闭失败后、主窗 Close 前调用。
    /// </summary>
    internal static bool TryHandleEscape() {
        if (_popup == null || _completed || !_popup.IsOpen) {
            return false;
        }
        if (_buttons == MessageBoxButton.OKCancel || _buttons == MessageBoxButton.YesNoCancel) {
            MessageBox.Complete(MessageBoxResult.Cancel);
        } else if (_buttons == MessageBoxButton.YesNo) {
            MessageBox.Complete(MessageBoxResult.No);
        } else {
            MessageBox.Complete(MessageBoxResult.OK);
        }
        return true;
    }

    /// <summary>是否有未完成的模态 MessageBox（调试 / 门控）。</summary>
    internal static bool HasActive() {
        return _popup != null && !_completed && _popup.IsOpen;
    }

    static FrameworkElement BuildContent(
        string title,
        string messageText,
        MessageBoxButton buttons,
        MessageBoxImage image)
    {
        string surface = "#00000000";
        string textPrimary = "#00000000";
        if (Application.Current != null) {
            string s = Application.Current.ResolveColor(BuiltInTheme.Surface);
            if (s != null && s.Length > 0) {
                surface = s;
            }
            string t = Application.Current.ResolveColor(BuiltInTheme.TextPrimary);
            if (t != null && t.Length > 0) {
                textPrimary = t;
            }
        }

        StackPanel root = new StackPanel();
        root.TypeName = "StackPanel";
        root.Orientation = Orientation.Vertical;
        root.Spacing = ControlMetrics.SpacingMD;
        root.Width = 400.0;
        root.Background = surface;

        if (title.Length > 0) {
            TextBlock captionBlock = new TextBlock();
            captionBlock.TypeName = "TextBlock";
            captionBlock.Text = title;
            captionBlock.FontSize = ControlMetrics.FontBodySize + 2.0;
            captionBlock.FontWeight = "Bold";
            captionBlock.Foreground = textPrimary;
            captionBlock.Width = 368.0;
            root.AddChild(captionBlock);
        }

        StackPanel bodyRow = new StackPanel();
        bodyRow.TypeName = "StackPanel";
        bodyRow.Orientation = Orientation.Horizontal;
        bodyRow.Spacing = ControlMetrics.SpacingMD;

        if (image != MessageBoxImage.None) {
            bodyRow.AddChild(MessageBox.BuildIcon(image));
        }

        TextBlock body = new TextBlock();
        body.TypeName = "TextBlock";
        body.Text = messageText;
        body.FontSize = ControlMetrics.FontBodySize;
        body.Foreground = textPrimary;
        if (image != MessageBoxImage.None) {
            body.Width = 320.0;
        } else {
            body.Width = 368.0;
        }
        bodyRow.AddChild(body);
        root.AddChild(bodyRow);

        StackPanel row = new StackPanel();
        row.TypeName = "StackPanel";
        row.Orientation = Orientation.Horizontal;
        row.Spacing = ControlMetrics.SpacingSM;
        row.Height = ControlMetrics.ControlHeight;

        if (buttons == MessageBoxButton.OKCancel) {
            Button cancelBtn = MessageBox.MakeButton("Cancel", "Default");
            Action<bool> cancelHandler = MessageBox.OnCancelClickedStatic;
            cancelBtn.OnClick(cancelHandler);
            row.AddChild(cancelBtn);

            Button okBtn = MessageBox.MakeButton("OK", "Primary");
            Action<bool> okHandler = MessageBox.OnOkClickedStatic;
            okBtn.OnClick(okHandler);
            row.AddChild(okBtn);
        } else if (buttons == MessageBoxButton.YesNo) {
            Button noBtn = MessageBox.MakeButton("No", "Default");
            Action<bool> noHandler = MessageBox.OnNoClickedStatic;
            noBtn.OnClick(noHandler);
            row.AddChild(noBtn);

            Button yesBtn = MessageBox.MakeButton("Yes", "Primary");
            Action<bool> yesHandler = MessageBox.OnYesClickedStatic;
            yesBtn.OnClick(yesHandler);
            row.AddChild(yesBtn);
        } else if (buttons == MessageBoxButton.YesNoCancel) {
            Button cancelBtn = MessageBox.MakeButton("Cancel", "Default");
            Action<bool> cancelHandler = MessageBox.OnCancelClickedStatic;
            cancelBtn.OnClick(cancelHandler);
            row.AddChild(cancelBtn);

            Button noBtn = MessageBox.MakeButton("No", "Default");
            Action<bool> noHandler = MessageBox.OnNoClickedStatic;
            noBtn.OnClick(noHandler);
            row.AddChild(noBtn);

            Button yesBtn = MessageBox.MakeButton("Yes", "Primary");
            Action<bool> yesHandler = MessageBox.OnYesClickedStatic;
            yesBtn.OnClick(yesHandler);
            row.AddChild(yesBtn);
        } else {
            Button okBtn = MessageBox.MakeButton("OK", "Primary");
            Action<bool> okHandler = MessageBox.OnOkClickedStatic;
            okBtn.OnClick(okHandler);
            row.AddChild(okBtn);
        }

        root.AddChild(row);
        return root;
    }

    static FrameworkElement BuildIcon(MessageBoxImage image) {
        string fill = MessageBox.ResolveThemeOr(BuiltInTheme.Primary);
        string glyph = "i";
        if (image == MessageBoxImage.Warning) {
            fill = MessageBox.ResolveThemeOr(BuiltInTheme.Warning);
            glyph = "!";
        } else if (image == MessageBoxImage.Error) {
            fill = MessageBox.ResolveThemeOr(BuiltInTheme.Danger);
            glyph = "X";
        } else if (image == MessageBoxImage.Question) {
            fill = MessageBox.ResolveThemeOr(BuiltInTheme.Primary);
            glyph = "?";
        } else {
            fill = MessageBox.ResolveThemeOr(BuiltInTheme.Primary);
            glyph = "i";
        }

        string onAccent = MessageBox.ResolveThemeOr(BuiltInTheme.TextOnAccent);

        StackPanel cell = new StackPanel();
        cell.TypeName = "StackPanel";
        cell.Orientation = Orientation.Vertical;
        cell.Width = ControlMetrics.MessageBoxIconSize;
        cell.Height = ControlMetrics.MessageBoxIconSize;
        cell.Background = fill;

        TextBlock mark = new TextBlock();
        mark.TypeName = "TextBlock";
        mark.Text = glyph;
        mark.FontSize = ControlMetrics.FontBodySize + 4.0;
        mark.FontWeight = "Bold";
        mark.Foreground = onAccent;
        mark.Width = ControlMetrics.MessageBoxIconSize;
        cell.AddChild(mark);
        return cell;
    }

    /// <summary>解析主题色；主题不可用时回落 Transparent 哨兵（禁 hex 主题色双源）。</summary>
    static string ResolveThemeOr(string key) {
        if (Application.Current != null) {
            string c = Application.Current.ResolveColor(key);
            if (c != null && c.Length > 0) {
                return c;
            }
        }
        return "#00000000";
    }

    static Button MakeButton(string label, string styleKey) {
        Button btn = new Button();
        btn.TypeName = "Button";
        btn.Content = Content.Text(label);
        // Style 键驱动变体（与 ARML 作者面同构短键；禁 Appearance DP）
        btn.Style = styleKey;
        btn.Width = 88.0;
        btn.Height = ControlMetrics.ControlHeight;
        btn.FontSize = ControlMetrics.FontBodySize;
        return btn;
    }

    static void OnOkClickedStatic(bool clicked) {
        MessageBox.Complete(MessageBoxResult.OK);
    }

    static void OnCancelClickedStatic(bool clicked) {
        MessageBox.Complete(MessageBoxResult.Cancel);
    }

    static void OnYesClickedStatic(bool clicked) {
        MessageBox.Complete(MessageBoxResult.Yes);
    }

    static void OnNoClickedStatic(bool clicked) {
        MessageBox.Complete(MessageBoxResult.No);
    }

    static void OnTokenCanceledStatic() {
        MessageBox.CompleteCanceled();
    }

    static void Complete(MessageBoxResult result) {
        if (_completed) {
            return;
        }
        _completed = true;
        Popup popup = _popup;
        _popup = null;
        if (popup != null && popup.IsOpen) {
            popup.Close();
        }
        TaskCompletionSource<MessageBoxResult> tcs = _tcs;
        _tcs = null;
        if (tcs != null) {
            tcs.SetResult(result);
        }
    }

    static void CompleteCanceled() {
        if (_completed) {
            return;
        }
        _completed = true;
        Popup popup = _popup;
        _popup = null;
        if (popup != null && popup.IsOpen) {
            popup.Close();
        }
        TaskCompletionSource<MessageBoxResult> tcs = _tcs;
        _tcs = null;
        if (tcs != null) {
            tcs.SetCanceled();
        }
    }
}
