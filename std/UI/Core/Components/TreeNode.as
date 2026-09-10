// RFC 037 · TreeNode — static hierarchical data model for TreeView.ItemsSource.
//
// Strong-typed tree shape (Header + Children); no DisplayMemberPath / string
// path projection. Observable hierarchical CollectionChanged is NOT supported
// (language gap on ObservableCollection<T> beyond string) — honest static List.

namespace Arc.UI.Components;

using Arc.Collections;

/// <summary>Static tree data node for <see cref="TreeView.ItemsSource"/>.</summary>
public class TreeNode {
    /// <summary>Node title (maps to TreeViewItem.Header).</summary>
    public string Header { get; set; }

    /// <summary>Child nodes (null-safe empty list after ctor).</summary>
    public List<TreeNode> Children { get; }

    /// <summary>Initial expand seed when materialized.</summary>
    public bool IsExpanded { get; set; }

    /// <summary>Construct empty node (Header null until set).</summary>
    public TreeNode() {
        this.Children = new List<TreeNode>();
    }

    /// <summary>Construct with header text.</summary>
    public TreeNode(string header) {
        this.Header = header;
        this.Children = new List<TreeNode>();
    }
}