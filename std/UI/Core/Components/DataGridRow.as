// RFC 037 §4 · M-VZ4 · Arc.UI.Components — DataGrid 行元素。
//
// DataGridRow 是虚拟化窗口物化的行载体：RowIndex 为逻辑行号（回收态 -1）。
// 单元格文本单一来源是 DataGrid 行主序扁平表（GetCell）；行对象不缓存
// List&lt;string&gt;——FrameworkElement 派生实例上 generic List 字段在部分
// 路径下未正确初始化（Count 读到垃圾值 → Add AV），故禁止行内 List 缓存。
//
// 镜像契约：PlatformTreeSync / SyncMirrorRows 按 TypeName="DataGridRow" 分派，
// 经父 DataGrid.GetCell(RowIndex, c) 写 C{i} + Layout*。

namespace Arc.UI.Components;

using Arc.UI;

/// <summary>DataGrid 行元素——虚拟化窗口物化的行载体（单元格读自父表）。</summary>
public class DataGridRow : FrameworkElement {
    private int _rowIndex;

    /// <summary>逻辑行号（-1 表示回收态）。</summary>
    public int RowIndex {
        get { return _rowIndex; }
        set { _rowIndex = value; }
    }

    /// <summary>构造空行（回收池种子）。</summary>
    public DataGridRow() {
        this.Type = typeof(DataGridRow);
        this.TypeName = "DataGridRow";
        _rowIndex = -1;
    }
}
