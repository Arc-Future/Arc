// Arc.UI.Components — RadioButton 单选按钮（对标 WPF ToggleButton 派生）。
//
// WPF 同构：ContentControl → ToggleButton → RadioButton。
// Arc：ToggleButton → RadioButton；GroupName 互斥（空名=同父兄弟；非空=窗口树内同组）。
//
// 点击/键盘 Activate：已选中保持选中；未选中则先清同组再勾选（禁自取消）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;

/// <summary>单选按钮——同组互斥勾选；视觉为圆形 chrome（RenderTree）。</summary>
public class RadioButton : ToggleButton {
    /// <summary>GroupName 属性元数据——互斥组名；空串表示仅同父兄弟互斥。</summary>
    public static DependencyProperty<string> GroupNameProperty =
        RegisterProperty<string>(nameof(GroupName), typeof(RadioButton), "");

    /// <summary>互斥组名（空=同 Parent 兄弟；非空=根树内同名互斥）。</summary>
    public string GroupName {
        get { return this.GetValue<string>(GroupNameProperty); }
        set { this.SetValue<string>(GroupNameProperty, value); }
    }

    public RadioButton() {
        this.Type = typeof(RadioButton);
        this.TypeName = "RadioButton";
    }

    /// <summary>点击/Activate：已勾选不翻转；未勾选则清同组后勾选。</summary>
    public override void RaiseToggle() {
        if (this.IsChecked) {
            return;
        }
        this.UncheckGroupPeers();
        this.IsChecked = true;
    }

    void UncheckGroupPeers() {
        string groupKey = this.GroupName;
        if (groupKey == null) {
            groupKey = "";
        }
        if (groupKey.Length == 0) {
            Element parent = this.Parent;
            if (parent == null || parent.Children == null) {
                return;
            }
            this.UncheckInChildren(parent.Children, groupKey, true);
            return;
        }
        Element root = this;
        while (root.Parent != null) {
            root = root.Parent;
        }
        this.UncheckInSubtree(root, groupKey);
    }

    void UncheckInSubtree(Element node, string groupKey) {
        if (node == null) {
            return;
        }
        if (node is RadioButton) {
            RadioButton peer = (RadioButton)node;
            if (peer != this && peer.IsChecked) {
                string peerKey = peer.GroupName;
                if (peerKey == null) {
                    peerKey = "";
                }
                if (peerKey == groupKey) {
                    peer.IsChecked = false;
                }
            }
        }
        List<Element> kids = node.Children;
        if (kids != null) {
            this.UncheckInChildren(kids, groupKey, false);
        }
    }

    void UncheckInChildren(List<Element> kids, string groupKey, bool siblingsOnly) {
        int n = kids.Count;
        for (int i = 0; i < n; i = i + 1) {
            Element child = kids[i];
            if (siblingsOnly) {
                if (child is RadioButton) {
                    RadioButton peer = (RadioButton)child;
                    if (peer != this && peer.IsChecked) {
                        peer.IsChecked = false;
                    }
                }
            } else {
                this.UncheckInSubtree(child, groupKey);
            }
        }
    }
}
