// MainWindow.arml.as: 主窗口 code-behind（合并自旧案例的 code-behind）。
//
// 事件处理器可见性：`protected`（WPF code-behind 惯例）。ARML `Click=` 经
// codegen 转为 `child_N.OnClick(_ => this.OnX())`——partial class 类内调用，
// protected 合法（typeck 不强制 public）。例外：
//   - `Message`（x:Bind 绑定源）须 public（RFC 026 M4 切片约定）
//   - `OnLoaded()` override 须与基类可见性一致，不能改为 protected
//
// 签名契约（与 ARML Click= 绑定一致）：
//   - OnClickHello()      ← 分区 1 的 Button Click="OnClickHello"
//   - OnPrimaryClick()    ← 分区 2 的 Primary Button Click="OnPrimaryClick"
//   - OnSecondaryClick()  ← 分区 2 的 Secondary Button Click="OnSecondaryClick"
//   - OnChangeMessage()   ← 分区 3 的 Button Click="OnChangeMessage"
//   - OnLoaded() override ← 分区 4/9/10 在窗口加载后演示 ItemsSource、CodeEditor 与 DataGrid
//
// x:Bind 绑定源：`Message` 是 `Signal<string>` 字段（RFC 026 M4 垂直切片）。
//   注意：勿用 `Title` 作绑定源——与 Window.Title（窗口标题 DP）同名冲突。
//
// 已知挂账：codegen 跳过 `x:Name`（RFC 026 M4+ 命名查找未实现），分区 4/9
// 无法通过命名字段引用 ARML 声明的 `<ListView x:Name="ItemsList"/>` /
// `<CodeEditor x:Name="EditorView"/>`，故 OnLoaded 内用局部实例演示同款
// API；命名查找落地后可改为直接操作命名实例。

namespace ArmlDemo;

using Arc;
using Arc.Collections;
using Arc.UI.Components;

public partial class MainWindow : Window {
    int _clickCount = 0;
    int _primaryCount = 0;
    int _secondaryCount = 0;

    /// <summary>x:Bind 绑定源（分区 3；勿与 Window.Title 同名）。</summary>
    /// <remarks>RFC 026 M4 切片：x:Bind 绑定源须为 code-behind 的 `[Observable]`
    /// auto-property（codegen 生成 `this.ObserveProperty("Message")` 静态定址订阅），
    /// 而非裸 Signal 字段——typeck 仅允许编译器合成隐藏通道的属性可订阅。</remarks>
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
        Console.WriteLine("Secondary clicked! count=" + _secondaryCount.ToString());
    }

    /// <summary>分区 3：Click="OnChangeMessage" 处理器——Signal.Set 触发 x:Bind 刷新。</summary>
    protected void OnChangeMessage() {
        Message = "Message updated via [Observable] setter";
    }

    /// <summary>窗口加载后：分区 4 ItemsSource 演示 + 分区 9 CodeEditor M-CE1 smoke。</summary>
    /// <remarks>分区 4/9 直接通过 `x:Name` 命名字段（`ItemsList`/`EditorView`）引用
    /// ARML 声明的元素——MainWindow.InitializeComponent（App.g.as 调用）在
    /// Show→OnLoaded 之前执行，命名字段已赋值（RFC 037 partial 合并 + M4 命名查找）。</remarks>
    public override void OnLoaded() {
        // ---- 分区 4：ListView / ItemContainerGenerator（x:Name=ItemsList）----
        //   ListView.ItemsSource = List<string> → ItemContainerGenerator 在
        //   ItemsHost 下物化 TextBlock 子元素。
        List<string> items = new List<string>();
        items.Add("Alpha");
        items.Add("Beta");
        items.Add("Gamma");
        this.ItemsList.ItemsSource = items;
        this.ItemsList.OnLoaded();
        Console.WriteLine("List items=" + this.ItemsList.ItemContainerGenerator.ItemsHost.Children.Count.ToString());

        // ---- 分区 9：CodeEditor 视口虚拟化 smoke（x:Name=EditorView）----
        // OpenPath 经 MemoryMappedFile + Piece Table；失败回退 SetText 3 行。
        // RenderVirtualizedLines 两次（offset 0 / 半程）保留 M-CE1 验收语义。
        if (!this.EditorView.OpenPath("examples/ArmlDemo/fixture/sample.txt")) {
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

        // ---- 分区 10：DataGrid 行虚拟化表格（x:Name=BooksGrid）----
        //   声明式 API：AddColumn（固定宽 / 0=自动均分）→ AddRow → SelectIndex。
        //   自管视口：VerticalOffset 驱动物化窗口，表头恒定置顶。
        this.BooksGrid.AddColumn("名称", 160.0);
        this.BooksGrid.AddColumn("版本", 90.0);
        this.BooksGrid.AddColumn("状态", 0.0);
        this.BooksGrid.AddRow("Arc 编译器", "0.9", "Ingesting");
        this.BooksGrid.AddRow("Arc.UI 框架", "1.0", "Mass production");
        this.BooksGrid.AddRow("ArmlDemo", "1.0", "Stable");
        this.BooksGrid.SelectIndex(1);
        this.BooksGrid.EnsureViewportMaterialization();
        Console.WriteLine(
            "grid rows=" + this.BooksGrid.RowCount.ToString()
            + " first=" + this.BooksGrid.FirstMaterializedIndex.ToString()
            + " last=" + this.BooksGrid.LastMaterializedIndex.ToString()
            + " sel=" + this.BooksGrid.SelectionChanged.Value);
    }
}
