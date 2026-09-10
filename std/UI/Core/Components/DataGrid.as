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
// 编程模型（声明式 API · ItemsSource 唯一行入口）：
//   DataGrid grid = new DataGrid();
//   grid.AddColumn("名称", 160.0);
//   grid.AddColumn("版本", 0.0);          // 0 = 自动均分剩余宽
//   List<List<string>> rows = ...;         // 每行 = 单元格列表
//   grid.ItemsSource = rows;               // 多列；单列可用 List<string>
//   // 可观察：ObservableCollection<List<string>>（多列）/
//   // ObservableCollection<string>（单列）→ 集合变更增量改 _cells，勿全量重建
//   grid.SelectIndex(0);                   // → SelectionChanged（载荷=选中行首列文本）
//   grid.SelectionMode = "Multiple";       // 程序化多选
//   grid.SelectItem(0); grid.SelectItem(2); // 累加 SelectedItems + SelectionChanged
//   grid.SelectIndexWithMods(2, 2);         // Ctrl+点击切换；Shift=1 范围选
//
// 禁 AddRow(string…) 字符串重载双轨——行数据一律经 ItemsSource。
// 多选：程序化 SelectItem/SelectAll ✅；Ctrl/Shift 修饰键手势最小面 ✅
// （PointerRouter HitMods → SelectIndexWithMods；GUI 手测后置）。
// 虚拟化纪律（RFC 037 §4 · M-VZ4）：只物化可见窗口行（ItemViewport 算术），
// 窗口外行回收进池复用（滚动零新建）；Extent = rowCount × stride 纯算术。
//
// Observable 订阅：多实例并发——每宿主登记稳定 route id，OnChanged 回调只按值
// 捕获 int（BindingOperations 同款逃逸闭包纪律；禁捕获类引用）。静态
// `_obsByRoute` 表按 id 回查宿主（ItemsControl/ItemSourceView 同款多槽）。
//
// 镜像契约：grid 镜像携带 ColumnCount/Header{i}/Width{i}/RowHeight/HeaderHeight/
// SelectedIndex；行镜像（DataGridRow 子元素）携带 ItemIndex + C{i} 单元格串 +
// Layout*。C 命中（rt_ui_datagrid_hit_row）写 HitItemIndex + HitMods，
// Arc 侧 RouteDataGridClick → SelectIndexWithMods。
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

    /// <summary>多列可观察源活引用（null = 非动态轨）。</summary>
    private ObservableCollection<List<string>> _obsNested;
    /// <summary>单列可观察源活引用（null = 非动态轨）。</summary>
    private ObservableCollection<string> _obsSingle;
    private int _obsToken;

    /// <summary>本实例在 <see cref="_obsHosts"/> 中的槽位（-1 = 未登记）。</summary>
    private int _obsRouteSlot;

    /// <summary>动态轨多宿主路由表（槽位 → DataGrid；退订置 null，槽不复用紧缩）。</summary>
    private static List<DataGrid> _obsHosts;

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
        _obsNested = null;
        _obsSingle = null;
        _obsToken = -1;
        _obsRouteSlot = -1;
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
        RegisterProperty<double>(nameof(HeaderHeight), typeof(DataGrid), ControlMetrics.ControlHeight);

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

    /// <summary>新增一列并返回列元数据。写入 ItemsSource 前须至少一列（报错 &gt; 静默）。</summary>
    /// <param name="header">列头文本。</param>
    /// <param name="width">列宽（px）；0 = 自动均分剩余宽度。</param>
    public DataGridColumn AddColumn(string header, double width) {
        DataGridColumn column = new DataGridColumn(header, width);
        _columns.Add(column);
        this.RefreshWindow();
        return column;
    }

    // ===== 行模型（ItemsSource 唯一入口；行主序扁平单元格）=====

    /// <summary>
    /// 自管视口：ItemsSource 物化为多列单元格表。
    /// 支持 <c>List&lt;List&lt;string&gt;&gt;</c> / <c>ObservableCollection&lt;List&lt;string&gt;&gt;</c>
    /// （多列）与 <c>List&lt;string&gt;</c> / <c>ObservableCollection&lt;string&gt;</c>（单列）；
    /// 可观察源首绑全量快照，其后 <c>CollectionChanged</c> 增量改行；null / 未知类型清空。
    /// 不走基类项宿主管线。
    /// </summary>
    protected override void MaterializeFromItemsSource() {
        this.ReleaseObservableSource();
        object src = this.ItemsSource;
        if (src == null) {
            this.ResetRowModel();
            return;
        }
        if (src is ObservableCollection<List<string>>) {
            this.BindObservableNested((ObservableCollection<List<string>>)src);
            return;
        }
        if (src is ObservableCollection<string>) {
            this.BindObservableSingle((ObservableCollection<string>)src);
            return;
        }
        if (src is List<List<string>>) {
            this.LoadFromNestedRows((List<List<string>>)src);
            return;
        }
        if (src is List<string>) {
            this.LoadFromSingleColumn((List<string>)src);
            return;
        }
        this.ResetRowModel();
    }

    void ReleaseObservableSource() {
        if (_obsNested != null) {
            _obsNested.Unsubscribe(_obsToken);
            _obsNested = null;
        }
        if (_obsSingle != null) {
            _obsSingle.Unsubscribe(_obsToken);
            _obsSingle = null;
        }
        _obsToken = -1;
        this.UnregisterObsRoute();
    }

    /// <summary>登记多宿主路由槽（幂等）；退订前保持有效供逃逸回调回查。</summary>
    void EnsureObsRoute() {
        if (_obsRouteSlot >= 0) {
            return;
        }
        if (_obsHosts == null) {
            _obsHosts = new List<DataGrid>();
        }
        _obsRouteSlot = _obsHosts.Count;
        _obsHosts.Add(this);
    }

    void UnregisterObsRoute() {
        if (_obsRouteSlot < 0) {
            return;
        }
        if (_obsHosts != null && _obsRouteSlot < _obsHosts.Count) {
            _obsHosts[_obsRouteSlot] = null;
        }
        _obsRouteSlot = -1;
    }

    void BindObservableNested(ObservableCollection<List<string>> src) {
        _obsNested = src;
        this.EnsureObsRoute();
        int routeSlot = _obsRouteSlot;
        // 只按值捕获 routeSlot（int）；禁捕获 this / 集合引用（逃逸闭包 UB）。
        // 形参类型由 OnChanged 目标委托推断——禁在 lambda 上写嵌套泛型（>> 词法）。
        _obsToken = src.OnChanged((args) => {
            DataGrid.DispatchNestedChange(routeSlot, args);
        });
        this.LoadFromNestedRowsSnapshot(src);
    }

    void BindObservableSingle(ObservableCollection<string> src) {
        _obsSingle = src;
        this.EnsureObsRoute();
        int routeSlot = _obsRouteSlot;
        _obsToken = src.OnChanged((args) => {
            DataGrid.DispatchSingleChange(routeSlot, args);
        });
        this.LoadFromSingleColumnSnapshot(src);
    }

    private static void DispatchNestedChange(int routeSlot, CollectionChangedEventArgs<List<string>> args) {
        if (_obsHosts == null || routeSlot < 0 || routeSlot >= _obsHosts.Count) {
            return;
        }
        DataGrid host = _obsHosts[routeSlot];
        if (host == null) {
            return;
        }
        host.ApplyNestedCollectionChange(args);
    }

    private static void DispatchSingleChange(int routeSlot, CollectionChangedEventArgs<string> args) {
        if (_obsHosts == null || routeSlot < 0 || routeSlot >= _obsHosts.Count) {
            return;
        }
        DataGrid host = _obsHosts[routeSlot];
        if (host == null) {
            return;
        }
        host.ApplySingleCollectionChange(args);
    }

    /// <summary>多列可观察：按动作增量改扁平单元格，再 RefreshWindow（禁全量 Clear+重灌）。</summary>
    void ApplyNestedCollectionChange(CollectionChangedEventArgs<List<string>> args) {
        int cols = _columns.Count;
        if (cols == 0) {
            return;
        }
        CollectionChangeAction action = args.Action;
        if (action == CollectionChangeAction.Add || action == CollectionChangeAction.Insert) {
            this.InsertRowCellsAt(args.Index, args.NewItem, cols);
            _rowCount = _rowCount + 1;
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Remove) {
            this.RemoveRowCellsAt(args.Index, cols);
            _rowCount = _rowCount - 1;
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Update) {
            this.ReplaceRowCellsAt(args.Index, args.NewItem, cols);
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Move) {
            List<string> moved = this.ExtractRowCellsAt(args.OldIndex, cols);
            this.RemoveRowCellsAt(args.OldIndex, cols);
            this.InsertRowCellsBlockAt(args.Index, moved);
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Clear) {
            _cells.Clear();
            _rowCount = 0;
            this.SelectIndex(-1);
            this.RefreshWindow();
        }
    }

    /// <summary>单列可观察：与多列同构，行载荷为单 string。</summary>
    void ApplySingleCollectionChange(CollectionChangedEventArgs<string> args) {
        int cols = _columns.Count;
        if (cols == 0) {
            return;
        }
        CollectionChangeAction action = args.Action;
        if (action == CollectionChangeAction.Add || action == CollectionChangeAction.Insert) {
            List<string> row = new List<string>();
            string added = args.NewItem;
            if (added == null) {
                added = "";
            }
            row.Add(added);
            this.InsertRowCellsAt(args.Index, row, cols);
            _rowCount = _rowCount + 1;
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Remove) {
            this.RemoveRowCellsAt(args.Index, cols);
            _rowCount = _rowCount - 1;
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Update) {
            List<string> row = new List<string>();
            string updated = args.NewItem;
            if (updated == null) {
                updated = "";
            }
            row.Add(updated);
            this.ReplaceRowCellsAt(args.Index, row, cols);
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Move) {
            List<string> moved = this.ExtractRowCellsAt(args.OldIndex, cols);
            this.RemoveRowCellsAt(args.OldIndex, cols);
            this.InsertRowCellsBlockAt(args.Index, moved);
            this.ClampSelectionAfterMutation();
            this.RefreshWindow();
            return;
        }
        if (action == CollectionChangeAction.Clear) {
            _cells.Clear();
            _rowCount = 0;
            this.SelectIndex(-1);
            this.RefreshWindow();
        }
    }

    void ClampSelectionAfterMutation() {
        int sel = this.SelectedIndex;
        if (sel < 0) {
            return;
        }
        if (_rowCount == 0) {
            this.SelectIndex(-1);
            return;
        }
        if (sel >= _rowCount) {
            this.SelectIndex(_rowCount - 1);
        }
    }

    void InsertRowCellsAt(int rowIndex, List<string> row, int cols) {
        if (rowIndex < 0) {
            rowIndex = 0;
        }
        if (rowIndex > _rowCount) {
            rowIndex = _rowCount;
        }
        int insertAt = rowIndex * cols;
        int c = 0;
        while (c < cols) {
            string cell = "";
            if (row != null && c < row.Count && row[c] != null) {
                cell = row[c];
            }
            _cells.Insert(insertAt + c, cell);
            c++;
        }
    }

    void InsertRowCellsBlockAt(int rowIndex, List<string> block) {
        int cols = _columns.Count;
        if (block == null || cols == 0) {
            return;
        }
        if (rowIndex < 0) {
            rowIndex = 0;
        }
        if (rowIndex > _rowCount) {
            rowIndex = _rowCount;
        }
        int insertAt = rowIndex * cols;
        int i = 0;
        int n = block.Count;
        while (i < n) {
            _cells.Insert(insertAt + i, block[i]);
            i++;
        }
    }

    void RemoveRowCellsAt(int rowIndex, int cols) {
        if (rowIndex < 0 || rowIndex >= _rowCount || cols <= 0) {
            return;
        }
        int start = rowIndex * cols;
        int c = 0;
        while (c < cols) {
            _cells.RemoveAt(start);
            c++;
        }
    }

    void ReplaceRowCellsAt(int rowIndex, List<string> row, int cols) {
        if (rowIndex < 0 || rowIndex >= _rowCount || cols <= 0) {
            return;
        }
        int baseIdx = rowIndex * cols;
        int c = 0;
        while (c < cols) {
            string cell = "";
            if (row != null && c < row.Count && row[c] != null) {
                cell = row[c];
            }
            _cells[baseIdx + c] = cell;
            c++;
        }
    }

    List<string> ExtractRowCellsAt(int rowIndex, int cols) {
        List<string> block = new List<string>();
        if (rowIndex < 0 || rowIndex >= _rowCount || cols <= 0) {
            return block;
        }
        int baseIdx = rowIndex * cols;
        int c = 0;
        while (c < cols) {
            block.Add(_cells[baseIdx + c]);
            c++;
        }
        return block;
    }

    void LoadFromNestedRowsSnapshot(ObservableCollection<List<string>> rows) {
        int cols = _columns.Count;
        if (cols == 0) {
            throw new InvalidOperationException("DataGrid.ItemsSource: AddColumn first (no columns)");
        }
        _cells.Clear();
        _rowCount = 0;
        if (rows == null) {
            this.SelectIndex(-1);
            this.RefreshWindow();
            return;
        }
        int r = 0;
        while (r < rows.Count) {
            this.AppendRowCells(rows[r], cols);
            _rowCount = _rowCount + 1;
            r++;
        }
        this.SelectIndex(-1);
        this.RefreshWindow();
    }

    void LoadFromSingleColumnSnapshot(ObservableCollection<string> values) {
        int cols = _columns.Count;
        if (cols == 0) {
            throw new InvalidOperationException("DataGrid.ItemsSource: AddColumn first (no columns)");
        }
        _cells.Clear();
        _rowCount = 0;
        if (values == null) {
            this.SelectIndex(-1);
            this.RefreshWindow();
            return;
        }
        int i = 0;
        while (i < values.Count) {
            List<string> row = new List<string>();
            string v = values[i];
            row.Add(v != null ? v : "");
            this.AppendRowCells(row, cols);
            _rowCount = _rowCount + 1;
            i++;
        }
        this.SelectIndex(-1);
        this.RefreshWindow();
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
        this.ResetRowModel();
    }

    void ResetRowModel() {
        this.ReleaseObservableSource();
        _cells.Clear();
        _rowCount = 0;
        this.SelectIndex(-1);
        this.RefreshWindow();
    }

    void LoadFromNestedRows(List<List<string>> rows) {
        int cols = _columns.Count;
        if (cols == 0) {
            throw new InvalidOperationException("DataGrid.ItemsSource: AddColumn first (no columns)");
        }
        _cells.Clear();
        _rowCount = 0;
        if (rows == null) {
            this.SelectIndex(-1);
            this.RefreshWindow();
            return;
        }
        int r = 0;
        while (r < rows.Count) {
            List<string> row = rows[r];
            this.AppendRowCells(row, cols);
            _rowCount = _rowCount + 1;
            r++;
        }
        this.SelectIndex(-1);
        this.RefreshWindow();
    }

    void LoadFromSingleColumn(List<string> values) {
        int cols = _columns.Count;
        if (cols == 0) {
            throw new InvalidOperationException("DataGrid.ItemsSource: AddColumn first (no columns)");
        }
        _cells.Clear();
        _rowCount = 0;
        if (values == null) {
            this.SelectIndex(-1);
            this.RefreshWindow();
            return;
        }
        int i = 0;
        while (i < values.Count) {
            List<string> row = new List<string>();
            row.Add(values[i]);
            this.AppendRowCells(row, cols);
            _rowCount = _rowCount + 1;
            i++;
        }
        this.SelectIndex(-1);
        this.RefreshWindow();
    }

    /// <summary>按列数写入一行：缺列补空串，超列截断。</summary>
    void AppendRowCells(List<string> row, int cols) {
        int c = 0;
        while (c < cols) {
            string cell = "";
            if (row != null && c < row.Count && row[c] != null) {
                cell = row[c];
            }
            _cells.Add(cell);
            c++;
        }
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
        List<DataGridRow> recycle = new List<DataGridRow>();
        int count = this.Children.Count;
        int i = 0;
        while (i < count) {
            Element raw = this.Children[i];
            if (!(raw is DataGridRow)) {
                i++;
                continue;
            }
            DataGridRow row = (DataGridRow)raw;
            if (row.RowIndex < first || row.RowIndex > last) {
                recycle.Add(row);
            }
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
        int idx = first;
        while (idx <= last) {
            DataGridRow row = this.FindRowByIndex(idx);
            if (row == null) {
                row = this.TakePooledRow();
                row.RowIndex = idx;
                this.AddChild(row);
            } else {
                row.RowIndex = idx;
            }
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
            return est.Height + ControlMetrics.SpacingSM;
        }
        return ControlMetrics.ControlHeight;
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
                // 与 ArrangeChild 一致：写绝对 LayoutX/Y（相对窗口根）；禁相对客户区覆盖。
                WindowHost.ElementSetNumber(rowHandle, "ItemIndex", (double)row.RowIndex);
                WindowHost.ElementSetNumber(rowHandle, "LayoutX", row.LayoutX);
                WindowHost.ElementSetNumber(rowHandle, "LayoutY", row.LayoutY);
                WindowHost.ElementSetNumber(rowHandle, "LayoutWidth", w);
                WindowHost.ElementSetNumber(rowHandle, "LayoutHeight", stride);
                int c = 0;
                int cols = _columns.Count;
                while (c < cols) {
                    WindowHost.ElementSetString(rowHandle, "C" + c, this.GetCell(row.RowIndex, c));
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
