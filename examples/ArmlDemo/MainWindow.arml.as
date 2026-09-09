// MainWindow.arml.as: 主窗口 code-behind（合并自旧案例的 code-behind）。
//
// 事件处理器可见性：`protected`（WPF code-behind 惯例）。ARML `Click=` 经
// codegen 转为 `child_N.OnClick(_ => this.OnX())`——partial class 类内调用，
// protected 合法（typeck 不强制 public）。例外：
//   - `Message`（x:Bind 绑定源）须 public（RFC 026 M4 切片约定）
//   - `OnLoaded()` override 须与基类可见性一致，不能改为 protected
//
// 签名契约（与 ARML Click= 绑定一致）：
//   - OnClickHello() / OnPrimaryClick() / OnSecondaryClick() / OnChangeMessage()
//   - OnOpenDemoPopup() ← 分区 2 Popup M1 演示
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
    int _primaryCount = 0;
    int _secondaryCount = 0;

    /// <summary>Slider ValueChanged 静态路由锚点。</summary>
    static MainWindow _volumeHost;

    /// <summary>演示弹层（复用；轻关闭）。</summary>
    Popup _demoPopup;

    /// <summary>x:Bind 绑定源（分区 3；勿与 Window.Title 同名）。</summary>
    [Observable] public string Message { get; set; }

    public MainWindow() {
        Message = "Hello, x:Bind!";
    }

    /// <summary>分区 1：Click="OnClickHello" 处理器。</summary>
    protected void OnClickHello() {
        _clickCount = _clickCount + 1;
        Console.WriteLine("Button clicked! count=" + _clickCount.ToString());
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

    /// <summary>分区 3：Click="OnChangeMessage" 处理器。</summary>
    protected void OnChangeMessage() {
        Message = "Message updated via [Observable] setter";
    }

    /// <summary>分区 3：追加 tick——验证连续 SyncText Invalidate 闭环。</summary>
    protected void OnAppendMessage() {
        _secondaryCount = _secondaryCount + 1;
        Message = "Bind tick #" + _secondaryCount.ToString();
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

    /// <summary>演示弹层关闭静态路由（日志）。</summary>
    static void OnDemoPopupClosedStatic(bool isOpen) {
        Console.WriteLine("DemoPopup closed");
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

        ObservableCollection<string> items = new ObservableCollection<string>();
        items.Add("Alpha");
        items.Add("Beta");
        items.Add("Gamma");
        items.Add("Delta");
        items.Add("Epsilon");
        items.Add("Zeta");
        items.Add("Eta");
        items.Add("Theta");
        this.ItemsList.ItemsSource = items;
        this.ItemsList.OnLoaded();
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
        items.Add("Obs multi-A");
        peerItems.Add("Peer-B");
        // 禁再 OnLoaded：Extent 须经 OnViewChanged→ApplyCollectionChange 更新（证明多槽路由）。
        Console.WriteLine(
            "List items=" + this.ItemsList.ItemContainerGenerator.ItemsHost.Children.Count.ToString()
            + " extent0=" + listExtent0.ToString()
            + " extent1=" + this.ItemsList.ContentExtentHeight.ToString()
            + " peer0=" + peerExtent0.ToString()
            + " peer1=" + peerList.ContentExtentHeight.ToString());

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
            + " sel=" + this.BooksGrid.SelectionChanged.Value);

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

    /// <summary>Slider 值变更静态路由：刷新 VolumeLabel。</summary>
    static void OnVolumeChangedStatic(double value) {
        MainWindow host = _volumeHost;
        if (host != null && host.VolumeLabel != null) {
            host.VolumeLabel.Text = "Volume: " + ((int)value).ToString();
        }
    }
}
