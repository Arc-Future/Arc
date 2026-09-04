// RFC 037 §4 · M-VZ4 · Arc.UI.Components — DataGrid 列元数据。
//
// DataGridColumn 描述表格的一列：列头文本与列宽。列是纯元数据（非 Element），
// 由 DataGrid 持有并在镜像/wgpu 渲染分支按序消费（Header{i}/Width{i}）。
//
// 列宽语义（对齐管理后台表格惯例）：
//   - Width > 0：固定宽度（px）
//   - Width == 0：自动列——与所有自动列均分剩余宽度（固定列铺完后）

namespace Arc.UI.Components;

/// <summary>DataGrid 列元数据——列头文本与列宽（0 = 自动均分剩余宽度）。</summary>
public class DataGridColumn {
    /// <summary>列头文本。</summary>
    public string Header { get; set; }

    /// <summary>列宽（px）；0 表示自动——与其他自动列均分剩余宽度。</summary>
    public double Width { get; set; }

    /// <summary>构造列元数据。</summary>
    /// <param name="header">列头文本。</param>
    /// <param name="width">列宽（0 = 自动）。</param>
    public DataGridColumn(string header, double width) {
        this.Header = header;
        this.Width = width;
    }
}
