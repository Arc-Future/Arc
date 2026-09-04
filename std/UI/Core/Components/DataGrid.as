// RFC 037 §4 · M-VZ4 · Arc.UI.Components — DataGrid 表格控件。
//
// DataGrid 是行虚拟化表格：固定表头 + 斑马纹行 + 行选中（Accent 高亮），
// 面向管理后台风格的数据展示。滚动模型对齐 CodeEditor——**自管视口**
// （VerticalOffset 驱动物化窗口，表头恒定置顶，无需 ScrollView 外壳）。
//
// WPF 同构层级对照：
//   WPF: Control → ItemsControl → Primitives.Selector → Primitives.MultiSelector → DataGrid
//   Arc:  Control → ItemsControl → Primitives.Selector → Primitives.MultiSelector → DataGrid
//        （多列单元格自管视口与基类项宿主管线正交：ownsItemsHost=false 跳过
//        VirtualizingStackPanel 装配，行虚拟化窗口独立实现）
//
// 选择语义面（SelectedIndex DP + SelectIndex 模板方法 + 平台镜像高亮同步 +
// SelectionChanged Signal 通道 + OnSelectionChanged 订阅）由 Primitives.Selector
// 承载（RFC 037 §5.3）；多选面（SelectionMode/SelectedItems/SelectAll）由
// Primitives.MultiSelector 承载；本类仅保留类型身份、自管视口管线与差异钩子：
//   - ItemDataAt override：多选数据采集点 = 指定行首列单元格
//   - SelectionItemCount override：可选条目总数 = 逻辑行总数（含未物化行）
//   - SelectionPayload override：SelectionChanged 载荷 = 选中行首列文本
//   - OnSelectionApplied override：重刷虚拟化窗口（选中行 Accent 高亮重渲）
//
// 编程模型（声明式 API）：
//   DataGrid grid = new DataGrid();
//   grid.AddColumn("名称", 160.0);
//   grid.AddColumn("版本", 0.0);          // 0 = 自动均分剩余宽
//   grid.AddRow("Arc", "1.0");
//   grid.SelectIndex(0);                   // → SelectionChanged（载荷=选中行首列文本）
//
// 虚拟化纪律（RFC 037 §4 · M-VZ4）：只物化可见窗口行（ItemViewport 算术），
// 窗口外行回收进池复用（滚动零新建）；Extent = rowCount × stride 纯算术。
//
// 镜像契约：grid 镜像携带 ColumnCount/Header{i}/Width{i}/RowHeight/HeaderHeight/
// SelectedIndex；行镜像（DataGridRow 子元素）携带 ItemIndex + C{i} 单元格串 +
// Layout*。C 命中（rt_ui_datagrid_hit_row）按行镜像 layout_y 命中写 HitItemIndex，
// Arc 侧 RouteDataGridClick 读取后 SelectIndex。
//
// Signal 通道：SelectionChanged（Signal&lt;string&gt;，载荷=选中行首列文本，
// 同 ListView SelectionChanged 载荷语义）。

namespace Arc.UI.Components;

using Arc;
using Arc.Collections;
using Arc.UI;
using Arc.UI.Components.Primitives;
using Arc.UI.Layout;

/// <summary>行虚拟化表格控件——固定表头 + 斑马纹行 + 行选中。</summary>
public class DataGrid : MultiSelector {
    private List<DataGridColumn> _columns;
    private List<string> _cells;      // 行主序扁平单元格（row * columnCount + col）
    private int _rowCount;
    private List<DataGridRow> _rowPool;
    private ItemViewport _viewport;
    private double _lastViewportHeight;

    /// <summary>构造空表格（ownsItemsHost=false：自管视口，跳过基类项宿主装配）。</summary>
    public DataGrid() : base(false) {
        this.Type = typeof(DataGrid);
        this.TypeName = "DataGrid";
        _columns = new List<DataGridColumn>();
        _cells = new List<string>();
        _rowCount = 0;
        _rowPool = new List<DataGridRow>();
        _viewport = new ItemViewport();
        _lastViewportHeight = ItemViewport.DefaultViewportHeight;
    }

    // ===== 静态依赖属性元数据（RFC 037 D1 WPF 同构）=====
    //
    // SelectedIndex 及其 DP 由 Primitives.Selector 承载（跨层同名 static 字段
    // 独立存储，保留派生版会与基类 SelectIndex 写点分裂为两个 DP 实例——必须收敛）。

    /// <summary>RowHeight 属性元数据——行高（px），默认 0（由 FontSize 估算）。</summary>
    public static DependencyProperty<double> RowHeightProperty =
        RegisterProperty<double>(nameof(RowHeight), typeof(DataGrid), 0.0);

    /// <summary>HeaderHeight 属性元数据——表头高（px），默认 32。</summary>
    public static DependencyProperty<double> HeaderHeightProperty =
        RegisterProperty<double>(nameof(HeaderHeight), typeof(DataGrid), 32.0);

    /// <summary>VerticalOffset 属性元数据——行区垂直滚动偏移（px），默认 0。</summary>
    public static DependencyProperty<double> VerticalOffsetProperty =
        RegisterProperty<double>(nameof(VerticalOffset), typeof(DataGrid), 0.0);

    // ===== 公共属性 wrapper：委托 Element.GetValue<T>/SetValue<T> =====
    //
    // SelectedIndex wrapper 继承基类（写镜像同步统一经 SelectIndex 模板方法）。

    /// <summary>行高（px）；0 = 由 FontSize 估算。</summary>
    public double RowHeight {
        get { return this.GetValue<double>(RowHeightProperty); }
        set {
            this.SetValue<double>(RowHeightProperty, value);
            this.RefreshWindow();
        }
    }

    /// <summary>表头高（px）。</summary>
    public double HeaderHeight {
        get { return this.GetValue<double>(HeaderHeightProperty); }
        set {
            this.SetValue<double>(HeaderHeightProperty, value);
            this.RefreshWindow();
        }
    }

    /// <summary>行区垂直滚动偏移（px）；驱动虚拟化窗口移动（表头恒定置顶）。</summary>
    public double VerticalOffset {
        get { return this.GetValue<double>(VerticalOffsetProperty); }
        set {
            this.SetValue<double>(VerticalOffsetProperty, value);
            this.RefreshWindow();
        }
    }

    // ===== 列模型 =====

    /// <summary>列数。</summary>
    public int ColumnCount {
        get { return _columns.Count; }
    }

    /// <summary>逻辑行总数（含未物化行）。</summary>
    public int RowCount {
        get { return _rowCount; }
    }

    /// <summary>读取列头文本（越界返回空串）。</summary>
    /// <param name="index">列索引。</param>
    public string GetColumnHeader(int index) {
        if (index < 0 || index >= _columns.Count) {
            return "";
        }
        return _columns[index].Header;
    }

    /// <summary>读取列宽（px；越界返回 0）。</summary>
    /// <param name="index">列索引。</param>
    public double GetColumnWidth(int index) {
        if (index < 0 || index >= _columns.Count) {
            return 0.0;
        }
        return _columns[index].Width;
    }

    /// <summary>新增一列并返回列元数据。AddRow 前须至少一列（报错 &gt; 静默）。</summary>
    /// <param name="header">列头文本。</param>
    /// <param name="width">列宽（px）；0 = 自动均分剩余宽度。</param>
    public DataGridColumn AddColumn(string header, double width) {
        DataGridColumn column = new DataGridColumn(header, width);
        _columns.Add(column);
        this.RefreshWindow();
        return column;
    }

    // ===== 行模型（行主序扁平单元格；AddRow 按列数补齐空串）=====

    /// <summary>追加一行（单列数据；不足列数以空串补齐）。</summary>
    public void AddRow(string cell0) {
        this.AppendCells(cell0, "", "", "");
    }

    /// <summary>追加一行（两列数据；不足列数以空串补齐）。</summary>
    public void AddRow(string cell0, string cell1) {
        this.AppendCells(cell0, cell1, "", "");
    }

    /// <summary>追加一行（三列数据；不足列数以空串补齐）。</summary>
    public void AddRow(string cell0, string cell1, string cell2) {
        this.AppendCells(cell0, cell1, cell2, "");
    }

    /// <summary>追加一行（四列数据；不足列数以空串补齐）。</summary>
    public void AddRow(string cell0, string cell1, string cell2, string cell3) {
        this.AppendCells(cell0, cell1, cell2, cell3);
    }

    /// <summary>读取单元格文本（越界返回空串）。</summary>
    /// <param name="row">行索引。</param>
    /// <param name="col">列索引。</param>
    public string GetCell(int row, int col) {
        int cols = _columns.Count;
        if (row < 0 || row >= _rowCount || col < 0 || col >= cols) {
            return "";
        }
        return _cells[row * cols + col];
    }

    /// <summary>清空全部行（列保留）。SelectIndex(-1) 经 OnSelectionApplied 重刷窗口
    /// （回收全部行 + 镜像行折叠 + SelectedIndex 高亮复位）。</summary>
    public void ClearRows() {
        _cells.Clear();
        _rowCount = 0;
        this.SelectIndex(-1);
    }

    void AppendCells(string cell0, string cell1, string cell2, string cell3) {
        int cols = _columns.Count;
        if (cols == 0) {
            throw new InvalidOperationException("DataGrid.AddRow: AddColumn first (no columns)");
        }
        _cells.Add(cell0);
        if (cols > 1) {
            _cells.Add(cell1);
        }
        if (cols > 2) {
            _cells.Add(cell2);
        }
        if (cols > 3) {
            _cells.Add(cell3);
        }
        // 列数超过 4：剩余列补空串（保持行主序扁平 stride = 列数）
        int pad = cols - 4;
        if (pad > 0) {
            int i = 0;
            while (i < pad) {
                _cells.Add("");
                i++;
            }
        }
        _rowCount = _rowCount + 1;
        this.RefreshWindow();
    }

    // ===== 虚拟化窗口（RFC 037 §4 · M-VZ4：只物化可见行，池化复用）=====

    /// <summary>行区内容总高（rowCount × stride；算术 extent，零 Measure）。</summary>
    public double ContentExtentHeight {
        get { return _viewport.ExtentHeight; }
    }

    /// <summary>窗口首行索引（无物化窗口返回 0）。</summary>
    public int FirstMaterializedIndex {
        get { return _viewport.FirstIndex; }
    }

    /// <summary>窗口末行索引（空窗口返回 -1）。</summary>
    public int LastMaterializedIndex {
        get { return _viewport.LastIndex; }
    }

    /// <summary>无 Measure pass 时按默认视口物化（OnLoaded 入口）。</summary>
    public void EnsureViewportMaterialization() {
        this.RefreshWindow();
    }

    void RefreshWindow() {
        this.UpdateViewportWindow(_lastViewportHeight);
        this.MaterializeWindowRows();
        this.SyncGridMirrorProps();
        this.SyncMirrorRows();
    }

    void UpdateViewportWindow(double viewportHeight) {
        double stride = this.ResolveRowStride();
        _viewport.Update(this.VerticalOffset, viewportHeight, _rowCount, stride, 0.0, 0.0);
    }

    /// <summary>物化窗口行：区间外回收进池，区间内取池复用并重绑单元格。</summary>
    /// <remarks>NLL 迭代失效纪律：读写分拆——先只读收集待回收行（循环内仅 get_Item），
    /// 再统一移除（循环内仅 mutator）；池取封装进 TakePooledRow（非循环体不配对）。</remarks>
    void MaterializeWindowRows() {
        int first = _viewport.FirstIndex;
        int last = _viewport.LastIndex;
        if (_rowCount == 0 || last < first) {
            this.RecycleAllRows();
            return;
        }
        // 回收收集：只读扫描（get_Item on this + 记录到局部列表）
        List<DataGridRow> recycle = new List<DataGridRow>();
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            DataGridRow row = (DataGridRow)this.Children[i];
            if (row.RowIndex < first || row.RowIndex > last) {
                recycle.Add(row);
            }
            i++;
        }
        // 统一移除（mutator on this，无同层 get_Item 配对）
        int r = 0;
        int recycleCount = recycle.Count;
        while (r < recycleCount) {
            DataGridRow row = recycle[r];
            row.RowIndex = -1;
            this.Children.Remove(row);
            row.Parent = null;
            _rowPool.Add(row);
            r++;
        }
        // 区间内取池补位 + 重绑
        int idx = first;
        while (idx <= last) {
            DataGridRow row = this.FindRowByIndex(idx);
            if (row == null) {
                row = this.TakePooledRow();
                row.RowIndex = idx;
                this.AddChild(row);
            }
            this.BindRowCells(row, idx);
            idx++;
        }
    }

    void RecycleAllRows() {
        List<DataGridRow> recycle = new List<DataGridRow>();
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            recycle.Add((DataGridRow)this.Children[i]);
            i++;
        }
        int r = 0;
        int recycleCount = recycle.Count;
        while (r < recycleCount) {
            DataGridRow row = recycle[r];
            row.RowIndex = -1;
            this.Children.Remove(row);
            row.Parent = null;
            _rowPool.Add(row);
            r++;
        }
    }

    /// <summary>池取尾复用；池空新建（调用方循环外无 get_Item/RemoveAt 同层配对）。</summary>
    DataGridRow TakePooledRow() {
        if (_rowPool.Count > 0) {
            int tail = _rowPool.Count - 1;
            DataGridRow row = _rowPool[tail];
            _rowPool.RemoveAt(tail);
            return row;
        }
        return new DataGridRow();
    }

    DataGridRow FindRowByIndex(int index) {
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            DataGridRow row = (DataGridRow)this.Children[i];
            if (row.RowIndex == index) {
                return row;
            }
            i++;
        }
        return null;
    }

    void BindRowCells(DataGridRow row, int index) {
        int cols = _columns.Count;
        row.Cells.Clear();
        int c = 0;
        while (c < cols) {
            row.Cells.Add(this.GetCell(index, c));
            c++;
        }
    }

    double ResolveRowStride() {
        double h = this.RowHeight;
        if (h > 0.0) {
            return h;
        }
        LayoutSize est = LayoutHelper.EstimateTextSize(
            "X", this.FontSize,
            LayoutHelper.MinTextPaddingX, LayoutHelper.MinTextPaddingY,
            this.FontFamily, this.FontWeight);
        if (est.Height > 0.0) {
            return est.Height + 8.0;
        }
        return 32.0;
    }

    // ===== 选择语义差异钩子（SelectIndex 流程入口 / SyncMirrorSelection 镜像同步 /
    // BindPlatformMirror 登记 / SelectionChanged 通道均由 Primitives.Selector 承载）=====

    /// <summary>多选数据采集点：指定行首列单元格（MultiSelector.ItemDataAt 覆写；
    /// 自管视口无数据源视图，行数据经 _cells 定位）。</summary>
    protected override object ItemDataAt(int index) {
        if (index < 0 || index >= _rowCount) {
            return null;
        }
        return this.GetCell(index, 0);
    }

    /// <summary>可选条目总数 = 逻辑行总数（含未物化行；SelectIndex 校验上界）。</summary>
    protected override int SelectionItemCount() {
        return _rowCount;
    }

    /// <summary>选中后附加同步：重刷虚拟化窗口（选中行 Accent 高亮重渲；
    /// BindPlatformMirror 绑定复位时亦触发，镜像行随窗口重建）。</summary>
    protected override void OnSelectionApplied() {
        this.RefreshWindow();
    }

    /// <summary>SelectionChanged 载荷：选中行首列文本（无选中为空串）。</summary>
    protected override string SelectionPayload() {
        return this.GetCell(this.SelectedIndex, 0);
    }

    // ===== 控件事件通道（RFC 037 §5.3 · Signal 单引擎）=====
    //
    // SelectionChanged（Signal<string>，载荷=选中行首列文本，同 ListView 载荷语义）
    // 与 OnSelectionChanged 便捷订阅由 Primitives.Selector 承载，载荷经
    // SelectionPayload 虚钩子提取。

    // ===== 平台镜像同步（动态窗口：行复用重绑，超编行折叠）=====

    void SyncGridMirrorProps() {
        if (_mirrorHandle == 0) {
            return;
        }
        WindowHost.ElementSetNumber(_mirrorHandle, "ColumnCount", (double)_columns.Count);
        WindowHost.ElementSetNumber(_mirrorHandle, "RowHeight", this.ResolveRowStride());
        WindowHost.ElementSetNumber(_mirrorHandle, "HeaderHeight", this.HeaderHeight);
        WindowHost.ElementSetNumber(_mirrorHandle, "RowCount", (double)_rowCount);
        int i = 0;
        int count = _columns.Count;
        while (i < count) {
            WindowHost.ElementSetString(_mirrorHandle, "Header" + i, _columns[i].Header);
            WindowHost.ElementSetNumber(_mirrorHandle, "Width" + i, _columns[i].Width);
            i++;
        }
    }

    /// <summary>窗口行镜像同步：镜像行复用重绑（ItemIndex + C{i} + Layout*），
    /// 镜像行数不足则增建，超出 Arc 行数则折叠（ItemIndex=-1 + 高 0）。</summary>
    void SyncMirrorRows() {
        if (_mirrorHandle == 0) {
            return;
        }
        int arcRows = this.Children.Count;
        int mirrorRows = WindowHost.ElementGetChildCount(_mirrorHandle);
        while (mirrorRows < arcRows) {
            long rowHandle = WindowHost.ElementCreate("DataGridRow");
            WindowHost.ElementAddChild(_mirrorHandle, rowHandle);
            mirrorRows = mirrorRows + 1;
        }
        double stride = this.ResolveRowStride();
        double headerH = this.HeaderHeight;
        double w = this.RenderWidth > 0.0 ? this.RenderWidth : 320.0;
        int i = 0;
        while (i < mirrorRows) {
            long rowHandle = WindowHost.ElementGetChild(_mirrorHandle, i);
            if (rowHandle == 0) {
                i++;
                continue;
            }
            if (i < arcRows) {
                DataGridRow row = (DataGridRow)this.Children[i];
                double y = headerH + (double)row.RowIndex * stride - this.VerticalOffset;
                WindowHost.ElementSetNumber(rowHandle, "ItemIndex", (double)row.RowIndex);
                WindowHost.ElementSetNumber(rowHandle, "LayoutX", 0.0);
                WindowHost.ElementSetNumber(rowHandle, "LayoutY", y);
                WindowHost.ElementSetNumber(rowHandle, "LayoutWidth", w);
                WindowHost.ElementSetNumber(rowHandle, "LayoutHeight", stride);
                int c = 0;
                int cols = row.Cells.Count;
                while (c < cols) {
                    WindowHost.ElementSetString(rowHandle, "C" + c, row.Cells[c]);
                    c++;
                }
            } else {
                // 超编镜像行折叠（ItemIndex=-1 + 高 0；wgpu 分支跳过）
                WindowHost.ElementSetNumber(rowHandle, "ItemIndex", -1.0);
                WindowHost.ElementSetNumber(rowHandle, "LayoutHeight", 0.0);
            }
            i++;
        }
    }

    // ===== 布局（自管视口：表头恒定置顶，行区按偏移滚动）=====

    /// <summary>Loaded 即物化默认窗口（同 ItemsControl.OnLoaded 前例）。</summary>
    public override void OnLoaded() {
        this.RefreshWindow();
    }

    protected override LayoutSize MeasureOverride(LayoutSize availableSize) {
        double availW = availableSize.Width;
        double availH = availableSize.Height;
        bool hBounded = availH > 0.0 && availH < LayoutHelper.Unbounded;
        double rowsViewport = _lastViewportHeight;
        if (hBounded) {
            rowsViewport = availH - this.HeaderHeight;
            if (rowsViewport < 0.0) {
                rowsViewport = 0.0;
            }
            _lastViewportHeight = rowsViewport > 0.0 ? rowsViewport : _lastViewportHeight;
        }
        this.UpdateViewportWindow(rowsViewport);
        this.MaterializeWindowRows();

        // 窗口行测量：宽 = 可用宽（有界），高 = 行 stride（固定等高）
        double stride = this.ResolveRowStride();
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            FrameworkElement child = (FrameworkElement)this.Children[i];
            LayoutHelper.MeasureChild(child, new LayoutSize(availW, LayoutHelper.Unbounded));
            i++;
        }

        double extentRows = _viewport.ExtentHeight;
        double rowsH = extentRows;
        if (hBounded) {
            if (rowsH > rowsViewport) {
                rowsH = rowsViewport;
            }
        } else {
            if (rowsH > _lastViewportHeight) {
                rowsH = _lastViewportHeight;
            }
        }
        double w = availW;
        if (w <= 0.0 || w >= LayoutHelper.Unbounded) {
            w = 320.0;
        }
        double h = this.HeaderHeight + rowsH;
        return new LayoutSize(w, h);
    }

    protected override void ArrangeOverride(LayoutSize finalSize) {
        double stride = this.ResolveRowStride();
        double headerH = this.HeaderHeight;
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            DataGridRow row = (DataGridRow)this.Children[i];
            double y = headerH + (double)row.RowIndex * stride - this.VerticalOffset;
            LayoutHelper.ArrangeChild(this, row, 0.0, y, finalSize.Width, stride);
            i++;
        }
        this.SyncMirrorRows();
    }
}
