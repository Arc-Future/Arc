// RFC 037 §4 · M-VZ4 · Arc.UI.Components — DataGrid 行元素。
//
// DataGridRow 是虚拟化窗口物化的行载体：Cells 按列序存放单元格文本，
// RowIndex 为逻辑行号（回收态 -1）。行由 DataGrid 池化管理——窗口外的行
// 回收复用（同 ItemContainerGenerator 纪律：滚动零新建）。
//
// 镜像契约：PlatformTreeSync 按 TypeName="DataGridRow" 分派，写
// ItemIndex + C{i} 单元格串 + Layout*（命中测试/wgpu 渲染消费）。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.UI;

/// <summary>DataGrid 行元素——虚拟化窗口物化的行载体。</summary>
public class DataGridRow : FrameworkElement {
    /// <summary>行单元格文本（按列序；不足列数以空串补齐）。</summary>
    public List<string> Cells { get; set; }

    /// <summary>逻辑行号（-1 表示回收态）。</summary>
    public int RowIndex { get; set; }

    /// <summary>构造空行（回收池种子）。</summary>
    public DataGridRow() {
        this.Type = typeof(DataGridRow);
        this.TypeName = "DataGridRow";
        this.Cells = new List<string>();
        this.RowIndex = -1;
    }
}
