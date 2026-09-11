// MainWindow.arml.as: 主窗口 code-behind（合并自旧案例的 code-behind）。
//
// 事件处理器可见性：`protected`（WPF code-behind 惯例）。ARML `Click=` 经
// codegen 转为 `child_N.OnClick(_ => this.OnX())`——partial class 类内调用，
// protected 合法（typeck 不强制 public）。例外：
//   - 绑定源（Caption / Greeting / Draft / Message / Model / Items / Click / …）须 public
//   - `OnLoaded()` override 须与基类可见性一致，不能改为 protected
//
// 签名契约（与 ARML Click= / Command= 绑定一致）：
//   - OnClickHello() / OnToggleClickArmed() / OnPrimaryClick() / OnSecondaryClick()
//   - OnChangeCaption() / OnResetCaption() / OnMutateGreeting()
//   - OnChangeMessage() / OnResetMessage() / OnAppendMessage() / OnFillDraft()
//   - OnRenameModel() / OnReplaceModel() / OnToggleFeatureEnabled()
//   - OnOpenDemoPopup() ← 分区 2 Popup 演示
//   - OnOpenStackedPopup() ← 多弹层 Z 序（后开在上 · Esc LIFO）
//   - OnShowMessageBoxOk/OkCancel/YesNo/YesNoCancel ← MessageBox 按钮集 + 图标
//   - OnLoaded() ← 分区 2 ComboBox ItemsSource / 4/5/8
//
// Slider / Popup Closed / ComboBox / MessageBox 回调用静态方法组 + 锚点（实例方法组 ByRef 悬垂 UB）。

namespace ArmlDemo;

using Arc;
using Arc.Collections;
using Arc.ComponentModel;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Styling;

public partial class MainWindow : Window {
    int _clickCount = 0;
    int _helloCommandCount = 0;
    int _primaryCount = 0;
    int _secondaryCount = 0;
    int _modelGen = 0;
    bool _clickArmed = true;

    /// <summary>Slider ValueChanged 静态路由锚点。</summary>
    static MainWindow _volumeHost;

    /// <summary>ListView/DataGrid SelectionChanged 静态路由锚点（M-D0）。</summary>
    static MainWindow _selectionHost;

    /// <summary>ListView SelectionChanged 回调命中次数（OnLoaded 冒烟）。</summary>
    static int _listSelHits;

    /// <summary>DataGrid SelectionChanged 回调命中次数（OnLoaded 冒烟）。</summary>
    static int _gridSelHits;

    /// <summary>TreeView SelectionChanged 静态路由锚点。</summary>
    static MainWindow _treeHost;

    /// <summary>Hello RelayCommand 静态路由锚点。</summary>
    static MainWindow _helloHost;

    /// <summary>演示弹层（复用；轻关闭）。</summary>
    Popup _demoPopup;
    Popup _stackedPopup;

    /// <summary>Window.Title 源（勿与 Window.Title 同名）。</summary>
    [Observable] public string Caption { get; set; }

    /// <summary>Plain OneWay 源——无通知通道，加载快照。</summary>
    public string Greeting { get; set; }

    /// <summary>TextBox TwoWay 源。</summary>
    [Observable] public string Draft { get; set; }

    /// <summary>OneWay 通知源（Bind 页 Message 段）。</summary>
    [Observable] public string Message { get; set; }

    /// <summary>嵌套 Binding Model.Name 父对象；替换触发重订叶。</summary>
    [Observable] public BindModel Model { get; set; }

    /// <summary>ListView ItemsSource 源。</summary>
    public ObservableCollection<string> Items { get; }

    /// <summary>Hello / Bind 页 Command=&quot;{Binding Click}&quot;。</summary>
    public RelayCommand Click { get; }

    /// <summary>IsEnabled=&quot;{Binding FeatureEnabled}&quot; 源。</summary>
    [Observable] public bool FeatureEnabled { get; set; }

    /// <summary>Button Content=&quot;{Binding CommandLabel}&quot;（编译期 setter）。</summary>
    public string CommandLabel { get; }

    public MainWindow() {
        _helloHost = this;
        this.Caption = "Arc.UI — ArmlDemo";
        this.Greeting = "Plain OneWay — snapshot at load.";
        this.Draft = "Edit me (TwoWay)";
        this.Message = "Hello, Binding!";
        this.FeatureEnabled = true;
        this.CommandLabel = "Content bind";
        this.Model = new BindModel();
        this.Model.Name = "Nested Alice";
        this.Items = new ObservableCollection<string>();
        this.Items.Add("Alpha");
        this.Items.Add("Beta");
        this.Items.Add("Gamma");
        this.Items.Add("Delta");
        this.Items.Add("Epsilon");
        this.Items.Add("Zeta");
        this.Items.Add("Eta");
        this.Items.Add("Theta");
        this.Click = new RelayCommand(MainWindow.OnHelloCommandStatic, MainWindow.CanHelloCommandStatic);
    }

    /// <summary>分区 1：Click="OnClickHello" 处理器。</summary>
    protected void OnClickHello() {
        _clickCount = _clickCount + 1;
        this.SetHelloStatus("Click Me · count=" + _clickCount.ToString());
        Console.WriteLine("Button clicked! count=" + _clickCount.ToString());
    }

    /// <summary>Hello RelayCommand 静态 Execute（Command=&quot;{Binding Click}&quot;）。</summary>
    static void OnHelloCommandStatic(object parameter) {
        MainWindow host = _helloHost;
        if (host == null) {
            return;
        }
        host._helloCommandCount = host._helloCommandCount + 1;
        host.SetHelloStatus("RelayCommand · count=" + host._helloCommandCount.ToString());
        Console.WriteLine("Hello RelayCommand count=" + host._helloCommandCount.ToString());
    }

    /// <summary>Click.CanExecute — Toggle CanExecute 翻转后 RaiseCanExecuteChanged。</summary>
    static bool CanHelloCommandStatic(object parameter) {
        MainWindow host = _helloHost;
        if (host == null) {
            return false;
        }
        return host._clickArmed;
    }

    /// <summary>翻转 Click.CanExecute 并通知按钮同步 IsEnabled。</summary>
    protected void OnToggleClickArmed() {
        _clickArmed = !_clickArmed;
        if (this.Click != null) {
            this.Click.RaiseCanExecuteChanged();
        }
        string state = "armed";
        if (!_clickArmed) {
            state = "disarmed";
        }
        this.SetHelloStatus("CanExecute " + state);
    }

    /// <summary>Hello 页状态行。</summary>
    void SetHelloStatus(string text) {
        if (this.HelloStatusLabel != null) {
            this.HelloStatusLabel.Text = text;
        }
    }

    /// <summary>分区 2：Primary action 按钮 Click 处理器。</summary>
    protected void OnPrimaryClick() {
        _primaryCount = _primaryCount + 1;
        Console.WriteLine("Primary clicked! count=" + _primaryCount.ToString());
    }

    /// <summary>分区 2：Secondary action 按钮 Click 处理器。</summary>
    protected void OnSecondaryClick() {
        _secondaryCount = _secondaryCount + 1;
    }

    /// <summary>Window.Title 活绑定：写 Caption。</summary>
    protected void OnChangeCaption() {
        this.Caption = "ArmlDemo · bound title";
    }

    /// <summary>恢复落地窗标题。</summary>
    protected void OnResetCaption() {
        this.Caption = "Arc.UI — ArmlDemo";
    }

    /// <summary>改普通 Greeting——OneWay 快照不刷新，对照可通知 Message。</summary>
    protected void OnMutateGreeting() {
        this.Greeting = "Mutated Greeting (UI stays snapshot)";
    }

    /// <summary>分区 3：Click="OnChangeMessage" 处理器。</summary>
    protected void OnChangeMessage() {
        this.Message = "Message updated via Observable setter";
    }

    /// <summary>恢复 Message 初值。</summary>
    protected void OnResetMessage() {
        this.Message = "Hello, Binding!";
    }

    /// <summary>分区 3：追加 tick——验证连续 SyncText Invalidate 闭环。</summary>
    protected void OnAppendMessage() {
        _secondaryCount = _secondaryCount + 1;
        this.Message = "Bind tick #" + _secondaryCount.ToString();
    }

    /// <summary>从代码写 Draft，TwoWay 盒与 OneWay 回显一起更新。</summary>
    protected void OnFillDraft() {
        this.Draft = "Filled from code";
    }

    /// <summary>改 Model.Name 叶——Observable 通知，OneWay 立刻刷新。</summary>
    protected void OnRenameModel() {
        if (this.Model != null) {
            this.Model.Name = "Renamed leaf";
        }
    }

    /// <summary>替换 Model 父对象——嵌套路径重订叶。</summary>
    protected void OnReplaceModel() {
        _modelGen = _modelGen + 1;
        BindModel next = new BindModel();
        next.Name = "Replaced #" + _modelGen.ToString();
        this.Model = next;
    }

    /// <summary>翻转 FeatureEnabled → IsEnabled 活绑定。</summary>
    protected void OnToggleFeatureEnabled() {
        this.FeatureEnabled = !this.FeatureEnabled;
    }

    /// <summary>
    /// 分区 2：打开演示 Popup（Open(this) 显式 Owner + IsLightDismissEnabled）。
    /// ComboBox 下拉同轨另见 ThemeCombo；本路径验收独立 Popup。
    /// </summary>
    protected void OnOpenDemoPopup() {
        if (_demoPopup != null && _demoPopup.IsOpen) {
            _demoPopup.Close();
            return;
        }
        if (_demoPopup == null) {
            _demoPopup = new Popup();
            _demoPopup.IsLightDismissEnabled = true;
            TextBlock body = new TextBlock();
            body.Text = "Popup M1 demo — click backdrop or Esc to dismiss.";
            body.FontSize = 14.0;
            body.Width = 360.0;
            body.Height = 72.0;
            if (Application.Current != null) {
                body.Background = Application.Current.ResolveColor(BuiltInTheme.Surface);
                body.Foreground = Application.Current.ResolveColor(BuiltInTheme.TextPrimary);
            }
            _demoPopup.Child = body;
            Action<bool> closed = MainWindow.OnDemoPopupClosedStatic;
            _demoPopup.OnClosed(closed);
        }
        _demoPopup.PlacementX = 160.0;
        _demoPopup.PlacementY = 180.0;
        _demoPopup.Open(this);
        Console.WriteLine("DemoPopup open=" + _demoPopup.IsOpen.ToString());
    }

    /// <summary>
    /// 分区 2：叠第二层轻关闭 Popup（后开在上；Esc/蒙层先关本层，下层仍开）。
    /// </summary>
    protected void OnOpenStackedPopup() {
        if (_stackedPopup != null && _stackedPopup.IsOpen) {
            _stackedPopup.Close();
            return;
        }
        if (_stackedPopup == null) {
            _stackedPopup = new Popup();
            _stackedPopup.IsLightDismissEnabled = true;
            TextBlock body = new TextBlock();
            body.Text = "Stacked Popup — on top; Esc/backdrop closes this first.";
            body.FontSize = 14.0;
            body.Width = 380.0;
            body.Height = 72.0;
            if (Application.Current != null) {
                body.Background = Application.Current.ResolveColor(BuiltInTheme.Surface);
                body.Foreground = Application.Current.ResolveColor(BuiltInTheme.TextPrimary);
            }
            _stackedPopup.Child = body;
            Action<bool> closed = MainWindow.OnStackedPopupClosedStatic;
            _stackedPopup.OnClosed(closed);
        }
        _stackedPopup.PlacementX = 220.0;
        _stackedPopup.PlacementY = 240.0;
        _stackedPopup.Open(this);
        Console.WriteLine("StackedPopup open=" + _stackedPopup.IsOpen.ToString());
    }

    /// <summary>演示弹层关闭静态路由（日志）。</summary>
    static void OnDemoPopupClosedStatic(bool isOpen) {
        Console.WriteLine("DemoPopup closed");
    }

    /// <summary>叠层弹层关闭静态路由（日志）。</summary>
    static void OnStackedPopupClosedStatic(bool isOpen) {
        Console.WriteLine("StackedPopup closed");
    }

    /// <summary>分区 2：MessageBox OK（ShowAsync 火忘；结果经静态续跑日志）。</summary>
    protected void OnShowMessageBoxOk() {
        this.RunMessageBoxOkAsync();
    }

    /// <summary>分区 2：MessageBox OKCancel。</summary>
    protected void OnShowMessageBoxOkCancel() {
        this.RunMessageBoxOkCancelAsync();
    }

    /// <summary>分区 2：MessageBox YesNo + Information 图标。</summary>
    protected void OnShowMessageBoxYesNo() {
        this.RunMessageBoxYesNoAsync();
    }

    /// <summary>分区 2：MessageBox YesNoCancel + Question 图标。</summary>
    protected void OnShowMessageBoxYesNoCancel() {
        this.RunMessageBoxYesNoCancelAsync();
    }

    /// <summary>OK-only ShowAsync：蒙层不关；Esc=OK；Information 色块图标。</summary>
    async Task RunMessageBoxOkAsync() {
        MessageBoxResult result = await MessageBox.ShowAsync(
            this,
            "MessageBox OK only. Backdrop does not dismiss; Esc = OK.",
            "Information",
            MessageBoxButton.OK,
            MessageBoxImage.Information,
            CancellationToken.None);
        Console.WriteLine("MessageBox OK result=" + ((int)result).ToString());
    }

    /// <summary>OKCancel ShowAsync：Esc=Cancel；Warning 图标。</summary>
    async Task RunMessageBoxOkCancelAsync() {
        MessageBoxResult result = await MessageBox.ShowAsync(
            this,
            "Confirm with OK or Cancel (Esc = Cancel).",
            "Warning",
            MessageBoxButton.OKCancel,
            MessageBoxImage.Warning,
            CancellationToken.None);
        Console.WriteLine("MessageBox OKCancel result=" + ((int)result).ToString());
    }

    /// <summary>YesNo ShowAsync：Esc=No；Error 图标。</summary>
    async Task RunMessageBoxYesNoAsync() {
        MessageBoxResult result = await MessageBox.ShowAsync(
            this,
            "Delete this item? Yes or No (Esc = No).",
            "Error",
            MessageBoxButton.YesNo,
            MessageBoxImage.Error,
            CancellationToken.None);
        Console.WriteLine("MessageBox YesNo result=" + ((int)result).ToString());
    }

    /// <summary>YesNoCancel ShowAsync：Esc=Cancel；Question 图标。</summary>
    async Task RunMessageBoxYesNoCancelAsync() {
        MessageBoxResult result = await MessageBox.ShowAsync(
            this,
            "Save changes before closing? Yes / No / Cancel (Esc = Cancel).",
            "Question",
            MessageBoxButton.YesNoCancel,
            MessageBoxImage.Question,
            CancellationToken.None);
        Console.WriteLine("MessageBox YesNoCancel result=" + ((int)result).ToString());
    }

    /// <summary>窗口加载后：分区 2 ComboBox / 5 Slider / 4·8 数据装载。</summary>
    public override void OnLoaded() {
        this.WireThemeCombo();
        this.WireVolumeSlider();
        this.WireSelectionSubscribe();
        this.WireDemoTree();

        this.ItemsList.OnLoaded();
        this.ItemsList.SelectIndex(1);
        double listExtent0 = this.ItemsList.ContentExtentHeight;
        // 多实例并发订阅：第二 ListView 独立集合；两源各自 Add 后两 Extent 均增长（破单活跃槽）。
        ListView peerList = new ListView();
        peerList.Height = 280.0;
        peerList.Width = 640.0;
        ObservableCollection<string> peerItems = new ObservableCollection<string>();
        peerItems.Add("Peer-A");
        peerList.ItemsSource = peerItems;
        peerList.OnLoaded();
        double peerExtent0 = peerList.ContentExtentHeight;
        this.Items.Add("Obs multi-A");
        peerItems.Add("Peer-B");
        // 禁再 OnLoaded：Extent 须经 OnViewChanged→ApplyCollectionChange 更新（证明多槽路由）。
        Console.WriteLine(
            "List items=" + this.ItemsList.ItemContainerGenerator.ItemsHost.Children.Count.ToString()
            + " extent0=" + listExtent0.ToString()
            + " extent1=" + this.ItemsList.ContentExtentHeight.ToString()
            + " peer0=" + peerExtent0.ToString()
            + " peer1=" + peerList.ContentExtentHeight.ToString()
            + " listSelHits=" + _listSelHits.ToString()
            + " listSel=" + this.ItemsList.SelectionChanged.Value);

        if (!this.EditorView.OpenPath("Assets/fixture/sample.txt")) {
            this.EditorView.SetText("virtualized line 0\nvirtualized line 1\nvirtualized line 2\n");
        }

        this.EditorView.VerticalOffset = 0.0;
        this.EditorView.RenderVirtualizedLines();
        int cmd0 = this.EditorView.LastDrawCommandCount;

        this.EditorView.VerticalOffset = this.EditorView.ContentExtentHeight * 0.5;
        this.EditorView.RenderVirtualizedLines();
        int cmd1 = this.EditorView.LastDrawCommandCount;

        int lineCount = this.EditorView.Document.LineCount;
        Console.WriteLine(
            "OK lines=" + lineCount.ToString()
            + " draw0=" + cmd0.ToString()
            + " draw1=" + cmd1.ToString()
            + " extent=" + this.EditorView.ContentExtentHeight.ToString());

        this.BooksGrid.AddColumn("名称", 160.0);
        this.BooksGrid.AddColumn("版本", 90.0);
        this.BooksGrid.AddColumn("状态", 0.0);
        ObservableCollection<List<string>> books = new ObservableCollection<List<string>>();
        books.Add(MainWindow.BookRow("Arc 编译器", "0.9", "Ingesting"));
        books.Add(MainWindow.BookRow("Arc.UI 框架", "1.0", "Mass production"));
        books.Add(MainWindow.BookRow("ArmlDemo", "1.0", "Stable"));
        books.Add(MainWindow.BookRow("Popup 浮层", "1.0", "M1"));
        books.Add(MainWindow.BookRow("wgpu 渲染", "1.0", "Unique backend"));
        this.BooksGrid.ItemsSource = books;
        this.BooksGrid.SelectIndex(1);
        // 程序化多选 + Ctrl/Shift 手势冒烟（SelectIndexWithMods 模拟 PointerRouter）。
        this.BooksGrid.ClearSelection();
        this.BooksGrid.SelectionMode = "Multiple";
        this.BooksGrid.SelectItem(0);
        this.BooksGrid.SelectItem(2);
        int multiCount = this.BooksGrid.SelectedItems.Count;
        this.BooksGrid.SelectIndexWithMods(0, 0);
        this.BooksGrid.SelectIndexWithMods(2, 1);
        this.BooksGrid.SelectIndexWithMods(1, 2);
        int modsSel = this.BooksGrid.SelectedItems.Count;
        // Observable 行增量：Add 后 RowCount+1，禁全量重建路径（同绑定实例上变更）。
        books.Add(MainWindow.BookRow("Obs 增量行", "1.0", "Live"));
        // 多实例并发订阅：第二网格独立集合；两源各自 Add 后两 RowCount 均 +1（破单活跃槽）。
        DataGrid peerGrid = new DataGrid();
        peerGrid.AddColumn("项", 120.0);
        ObservableCollection<List<string>> peerBooks = new ObservableCollection<List<string>>();
        peerBooks.Add(MainWindow.BookRow("Peer-A", "1.0", "Idle"));
        peerGrid.ItemsSource = peerBooks;
        books.Add(MainWindow.BookRow("Obs 多实例-A", "1.0", "Live"));
        peerBooks.Add(MainWindow.BookRow("Peer-B", "1.0", "Live"));
        this.BooksGrid.EnsureViewportMaterialization();
        peerGrid.EnsureViewportMaterialization();
        Console.WriteLine(
            "grid rows=" + this.BooksGrid.RowCount.ToString()
            + " peer=" + peerGrid.RowCount.ToString()
            + " first=" + this.BooksGrid.FirstMaterializedIndex.ToString()
            + " last=" + this.BooksGrid.LastMaterializedIndex.ToString()
            + " sel=" + this.BooksGrid.SelectionChanged.Value
            + " multiSel=" + multiCount.ToString()
            + " modsSel=" + modsSel.ToString()
            + " gridSelHits=" + _gridSelHits.ToString());

        // 程序化打开第 8 页（Data）——复现/验收崩溃；ARML_SELECT_DATA=1 时启用（禁鼠标坐标）。
        string selectData = Environment.GetEnvironmentVariable("ARML_SELECT_DATA");
        if (selectData != null && selectData == "1" && this.DemoTabs != null) {
            this.DemoTabs.SelectedIndex = 7;
            Console.WriteLine("ARML_SELECT_DATA SelectedIndex=7");
        }
    }

    /// <summary>DataGrid 多列行工厂（ItemsSource = List/ObservableCollection&lt;List&lt;string&gt;&gt;）。</summary>
    static List<string> BookRow(string name, string version, string status) {
        List<string> row = new List<string>();
        row.Add(name);
        row.Add(version);
        row.Add(status);
        return row;
    }

    /// <summary>分区 2：ComboBox ItemsSource（非泛型基座）+ ComboBox&lt;T&gt; SetOptions 冒烟。</summary>
    void WireThemeCombo() {
        if (this.ThemeCombo == null) {
            return;
        }
        List<string> themes = new List<string>();
        themes.Add("Light");
        themes.Add("Dark");
        themes.Add("High Contrast");
        this.ThemeCombo.ItemsSource = themes;
        this.ThemeCombo.SelectIndex(0);

        // ComboBox<T>：基类 DP 全名限定后 arc-prune-001 可过（ARML 仍走 ComboBoxBase）。
        ComboBox<DemoThemeKind> typed = new ComboBox<DemoThemeKind>();
        EnumOptions<DemoThemeKind> opts = Enum.GetOptions<DemoThemeKind>();
        typed.SetOptions(opts);
        typed.SelectIndex(0);
        Console.WriteLine(
            "ComboBox<T> GetOptions count=" + typed.OptionCount.ToString()
            + " sel=" + ((int)typed.SelectedValue).ToString());
    }

    /// <summary>分区 5：Slider → VolumeLabel（静态回调路由）。</summary>
    void WireVolumeSlider() {
        if (this.VolumeSlider == null || this.VolumeLabel == null) {
            return;
        }
        _volumeHost = this;
        Action<double> handler = MainWindow.OnVolumeChangedStatic;
        this.VolumeSlider.OnValueChanged(handler);
        this.VolumeLabel.Text = "Volume: " + ((int)this.VolumeSlider.Value).ToString();
    }

    /// <summary>Section 9: TreeView ItemsSource FlatIndex viewport (M-VZ4) smoke.</summary>
    void WireDemoTree() {
        if (this.DemoTree == null || this.TreeSelectionLabel == null) {
            return;
        }
        _treeHost = this;
        Action<string> handler = MainWindow.OnTreeSelectionChangedStatic;
        this.DemoTree.OnSelectionChanged(handler);

        List<TreeNode> roots = MainWindow.BuildDemoTreeNodes();
        this.DemoTree.ItemsSource = roots;
        this.DemoTree.EnsureViewportMaterialization();
        int visibleRows = this.DemoTree.VisibleRowCount;
        int childCount = 0;
        if (this.DemoTree.Children != null) {
            childCount = this.DemoTree.Children.Count;
        }
        this.DemoTree.SelectFlatIndex(0);
        this.DemoTree.SelectFlatIndex(1);
        string downSmoke = this.DemoTree.SelectionChanged.Value;
        this.DemoTree.SelectFlatIndex(0);
        double extent = this.DemoTree.ContentExtentHeight;
        this.DemoTree.VerticalOffset = extent * 0.5;
        this.DemoTree.EnsureViewportMaterialization();
        int first = this.DemoTree.FirstMaterializedIndex;
        int last = this.DemoTree.LastMaterializedIndex;
        this.DemoTree.VerticalOffset = 0.0;
        this.DemoTree.EnsureViewportMaterialization();
        this.DemoTree.SelectFlatIndex(0);
        string focusable = this.DemoTree.Focusable ? "1" : "0";
        string tabStop = this.DemoTree.IsTabStop ? "1" : "0";
        string tagOk = "0";
        object sel = this.DemoTree.SelectedItem;
        if (sel is TreeViewItem) {
            TreeViewItem tvi = (TreeViewItem)sel;
            if (tvi.Tag is TreeNode) {
                tagOk = "1";
            }
        }
        string virtOk = "0";
        if (visibleRows > 40 && last < visibleRows - 1 && childCount < visibleRows) {
            virtOk = "1";
        }
        Console.WriteLine(
            "TreeView M-VZ4 visible=" + visibleRows.ToString()
            + " kids=" + childCount.ToString()
            + " first=" + first.ToString()
            + " last=" + last.ToString()
            + " extent=" + ((int)extent).ToString()
            + " virt=" + virtOk
            + " Focusable=" + focusable
            + " IsTabStop=" + tabStop
            + " downSmoke=" + downSmoke
            + " tag=" + tagOk
            + " sel=" + this.DemoTree.SelectionChanged.Value);
    }

    /// <summary>Demo tree: shallow roots + expanded Bulk (>=80 leaves) for FlatIndex viewport.</summary>
    static List<TreeNode> BuildDemoTreeNodes() {
        List<TreeNode> roots = new List<TreeNode>();
        TreeNode docs = new TreeNode("Documents");
        docs.IsExpanded = true;
        docs.Children.Add(new TreeNode("Specs"));
        TreeNode notes = new TreeNode("Notes");
        notes.IsExpanded = true;
        notes.Children.Add(new TreeNode("Draft"));
        docs.Children.Add(notes);
        roots.Add(docs);
        TreeNode images = new TreeNode("Images");
        images.Children.Add(new TreeNode("Logo"));
        roots.Add(images);
        roots.Add(new TreeNode("Readme"));
        TreeNode bulk = new TreeNode("Bulk");
        bulk.IsExpanded = true;
        int i = 0;
        while (i < 80) {
            bulk.Children.Add(new TreeNode("Leaf-" + i.ToString()));
            i++;
        }
        roots.Add(bulk);
        return roots;
    }

    /// <summary>TreeView 选中静态路由：刷新标签。</summary>
    static void OnTreeSelectionChangedStatic(string header) {
        MainWindow host = _treeHost;
        if (host != null && host.TreeSelectionLabel != null) {
            string text = header;
            if (text == null || text.Length == 0) {
                text = "(none)";
            }
            host.TreeSelectionLabel.Text = "Selected: " + text;
        }
    }

    /// <summary>Slider 值变更静态路由：刷新 VolumeLabel。</summary>
    static void OnVolumeChangedStatic(double value) {
        MainWindow host = _volumeHost;
        if (host != null && host.VolumeLabel != null) {
            host.VolumeLabel.Text = "Volume: " + ((int)value).ToString();
        }
    }

    /// <summary>分区 4/8：ListView/DataGrid SelectionChanged Subscribe 冒烟（M-D0）。</summary>
    void WireSelectionSubscribe() {
        _selectionHost = this;
        _listSelHits = 0;
        _gridSelHits = 0;
        if (this.ItemsList != null) {
            this.ItemsList.OnSelectionChanged(MainWindow.OnListSelectionChangedStatic);
        }
        if (this.BooksGrid != null) {
            this.BooksGrid.OnSelectionChanged(MainWindow.OnGridSelectionChangedStatic);
        }
    }

    /// <summary>ListView 选择变更静态路由。</summary>
    static void OnListSelectionChangedStatic(string item) {
        _listSelHits = _listSelHits + 1;
        MainWindow host = _selectionHost;
        if (host != null) {
            string text = item;
            if (text == null || text.Length == 0) {
                text = "(none)";
            }
            if (host.ListSelectionLabel != null) {
                host.ListSelectionLabel.Text = "Selected: " + text;
            }
            Console.WriteLine("ListView SelectionChanged item=" + item + " hits=" + _listSelHits.ToString());
        }
    }

    /// <summary>DataGrid 选择变更静态路由。</summary>
    static void OnGridSelectionChangedStatic(string item) {
        _gridSelHits = _gridSelHits + 1;
        MainWindow host = _selectionHost;
        if (host != null) {
            Console.WriteLine("DataGrid SelectionChanged item=" + item + " hits=" + _gridSelHits.ToString());
        }
    }

}
