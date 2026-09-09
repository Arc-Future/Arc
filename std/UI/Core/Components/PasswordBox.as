// RFC 037 · PasswordBox 口令输入（对照 TextBox 最小可用）。
//
// 分层：复用 TextBoxModel / TextBoxController / ImeBridge；本类仅掩码显示面
// 与 Password API。镜像 Text = 掩码串（禁明文上屏）；内核 Text = 明文真相。
//
// 复制策略：禁剪贴板 Copy/Cut（明文不上剪贴板）；允许 Paste 入 Password；
// 明文仅经 Password/Text 属性面供应用读取。

namespace Arc.UI.Components;

using Arc.UI.Internal;
using Arc.UI.Layout;

/// <summary>口令输入框——掩码显示；编辑语义与 TextBox 同核。</summary>
public class PasswordBox : TextBox {
    /// <summary>PasswordChar 属性元数据——掩码字符（单码点字符串），默认 ●。</summary>
    public static DependencyProperty<string> PasswordCharProperty =
        RegisterProperty<string>(nameof(PasswordChar), typeof(PasswordBox), "●");

    public PasswordBox() {
        this.Type = typeof(PasswordBox);
    }

    /// <summary>掩码字符（单码点；空则回落 ●）。</summary>
    public string PasswordChar {
        get { return this.GetValue<string>(PasswordCharProperty); }
        set {
            string ch = value;
            if (ch == null || ch.Length == 0) {
                ch = "●";
            }
            this.SetValue<string>(PasswordCharProperty, ch);
            this.SyncMirrorText();
        }
    }

    /// <summary>口令明文（与 Text 同槽；WPF Password 惯用名）。</summary>
    public string Password {
        get { return this.Text; }
        set { this.Text = value; }
    }

    /// <summary>掩码几何串（与镜像 Text / 点击定位同源）。</summary>
    internal override string GeometryText() {
        return this.MaskText(this.Model().Text);
    }

    /// <summary>禁 Copy/Cut 明文出剪贴板；Paste 仍经 TextBoxController。</summary>
    internal override bool AllowsClipboardCopy() {
        return false;
    }

    /// <summary>Measure 仅掩码——组字明文不进显示面。</summary>
    protected override string BuildDisplayString() {
        return this.GeometryText();
    }

    /// <summary>镜像写掩码 Text；Composition 恒空（防 IME 预览泄密）。</summary>
    public override void SyncMirrorText() {
        if (_mirrorHandle == 0) {
            return;
        }
        string masked = this.GeometryText();
        if (masked == null) {
            masked = "";
        }
        WindowHost.ElementSetString(_mirrorHandle, "Text", masked);
        WindowHost.ElementSetString(_mirrorHandle, "CompositionText", "");
        WindowHost.ElementSetNumber(_mirrorHandle, "CaretIndex", (double)this.CaretIndex);
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectionStart", (double)this.SelectionStart);
        WindowHost.ElementSetNumber(_mirrorHandle, "SelectionLength", (double)this.SelectionLength);
        int focused = _isFocused ? 1 : 0;
        WindowHost.ElementSetBool(_mirrorHandle, "IsFocused", focused);
        FramePump.Invalidate();
    }

    string MaskText(string plain) {
        if (plain == null || plain.Length == 0) {
            return "";
        }
        string ch = this.PasswordChar;
        if (ch == null || ch.Length == 0) {
            ch = "●";
        }
        string result = "";
        int i = 0;
        int n = plain.Length;
        while (i < n) {
            result = result + ch;
            i = i + 1;
        }
        return result;
    }
}
