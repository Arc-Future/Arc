// RFC 037 §10 AI 原生：LivePreviewHost / ArmlParser 单元测试（UnitTest.Deferred · L3 UI）。
//
// 分层（诚实标注，禁止 [Fact(Skip)] 假绿）：
//   - ArmlParserTests（纯 CPU）：ARML 字符串 → Element 树解析核心，无 GPU 依赖
//   - LivePreviewHostTests：
//       · CPU 面（无 GPU）：构造/视口默认值/未加载行为/未初始化拒绝
//       · GPU 集成面（wgpu 离屏渲染）：Initialize/LoadSpec/ApplyPatch/快照/截图/Reset
// GPU 不可用时经 Assert.Skip 运行时记为 Skipped（≠ Fact-Skip；≠ 假绿）。
//
// 已知编译器缺陷（已根因规避）：Arc 编译器存在「struct 带参构造函数体整体不执行」
// 缺陷（Color/LayoutSize 等值类型 ctor 参数全部丢失，仅字段赋值正常）。std 侧已
// 经 <see cref="Color.FromRgba"/> 静态工厂（默认构造 + 手动字段赋值）根因规避，
// Background 往返在本套件真实断言（非 Skip 顶绿）；编译器修复后工厂与带参 ctor
// 等价，无需改动测试。原始诊断探针 BrushProbeTests.as 已随诊断完成删除。
//
// 已知编译器缺陷（诚实阻塞，非 LivePreviewHost 缺陷）：泛型调用损坏——含值类型
// 字段的引用类型（如 Brush.Color）经「泛型方法实参传递链」写入 Signal&lt;T&gt;/
// DependencyProperty 时字段被置零（BrushDiagTests P17/P18 最小复现：public 泛型
// 实例方法 → public 泛型实例方法边界损坏 T 实参）。受影响断言（Background 往返）
// 保持真实断言，待编译器修复后自动转绿；禁止删断言 / Assert.Skip 顶绿。


namespace UnitTest.Arc;

using Arc;
using Arc.QIF;
using Arc.IO;
using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Markup;
using Arc.UI.Media;

/// <summary>
/// ArmlParser 解析核心纯 CPU 回归（LivePreviewHost.LoadSpec 的解析层）。
/// </summary>
public class ArmlParserTests
{
    [Fact]
    public void Parse_SimpleTree_Structure()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<StackPanel><Button>Save</Button><TextBlock>Hello</TextBlock></StackPanel>");
        Assert.True(r.Success);
        Assert.NotNull(r.Root);
        Assert.Equal("StackPanel", r.Root.TypeName);
        Assert.Equal(2, r.Root.Children.Count);
        Assert.Equal("Button", r.Root.Children[0].TypeName);
        Assert.Equal("TextBlock", r.Root.Children[1].TypeName);
    }

    [Fact]
    public void Parse_Button_ContentAndProperties()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<Button Width=\"120\" Height=\"40\" Background=\"#FF0000FF\">Save</Button>");
        Assert.True(r.Success);
        Assert.True(r.Root is Button);
        Button btn = (Button)r.Root;
        Assert.Equal(120.0, btn.Width, 0.001);
        Assert.Equal(40.0, btn.Height, 0.001);
        Assert.Equal("#FF0000FF", btn.Background);

        Content c = btn.Content;
        bool isText = false;
        switch (c) {
            case Content.Text(s): isText = (s == "Save"); break;
            default: break;
        }
        Assert.True(isText);
    }

    [Fact]
    public void Parse_XName_Applied()
    {
        ArmlParseResult r = ArmlParser.Parse("<Button x:Name=\"SaveBtn\">x</Button>");
        Assert.True(r.Success);
        Assert.Equal("SaveBtn", r.Root.Name);
    }

    [Fact]
    public void Parse_Nested_ParentLink()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<StackPanel><Grid><Button>OK</Button></Grid></StackPanel>");
        Assert.True(r.Success);
        Element grid = r.Root.Children[0];
        Assert.Equal("Grid", grid.TypeName);
        Assert.Equal(1, grid.Children.Count);
        Assert.Equal("Button", grid.Children[0].TypeName);
        Assert.True(grid.Parent == r.Root);
        Assert.True(grid.Children[0].Parent == grid);
    }

    [Fact]
    public void Parse_SelfClosing_Rectangle()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<StackPanel><Rectangle Width=\"100\" Height=\"50\"/></StackPanel>");
        Assert.True(r.Success);
        Assert.Equal(1, r.Root.Children.Count);
        Element child = r.Root.Children[0];
        Assert.True(child is FrameworkElement);
        FrameworkElement rect = (FrameworkElement)child;
        Assert.Equal(100.0, rect.Width, 0.001);
        Assert.Equal(50.0, rect.Height, 0.001);
    }

    [Fact]
    public void Parse_AttachedProperties()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<Canvas><Rectangle Canvas.Left=\"10\" Canvas.Top=\"20\" Grid.Row=\"1\"/></Canvas>");
        Assert.True(r.Success);
        Element rect = r.Root.Children[0];
        Assert.Equal(10.0, rect.GetAttachedNumber("Left", -1.0), 0.001);
        Assert.Equal(20.0, rect.GetAttachedNumber("Top", -1.0), 0.001);
        Assert.Equal(1.0, rect.GetAttachedNumber("Row", -1.0), 0.001);
        // 完整键（owner 前缀）双写保留
        Assert.Equal(10.0, rect.GetAttachedNumber("Canvas.Left", -1.0), 0.001);
    }

    [Fact]
    public void Parse_UnknownType_Fallback()
    {
        ArmlParseResult r = ArmlParser.Parse("<FooBar Baz=\"1\"/>");
        Assert.True(r.Success);
        Assert.NotNull(r.Root);
        Assert.Equal("FooBar", r.Root.TypeName);
        // 未知类型 fallback 到基础 Element（无布局能力），不崩溃
        Assert.False(r.Root is FrameworkElement);
    }

    [Fact]
    public void Parse_UnknownProperty_Skipped()
    {
        ArmlParseResult r = ArmlParser.Parse("<Button IsSpecial=\"true\">x</Button>");
        Assert.True(r.Success);
        Assert.NotNull(r.Root);
    }

    [Fact]
    public void Parse_Empty_ReturnsFailure()
    {
        ArmlParseResult r = ArmlParser.Parse("");
        Assert.False(r.Success);
        Assert.Null(r.Root);
        Assert.True(r.Diagnostics.Count > 0);
    }

    [Fact]
    public void Parse_NoRootElement_ReturnsFailure()
    {
        ArmlParseResult r = ArmlParser.Parse("   ");
        Assert.False(r.Success);
        Assert.Null(r.Root);
    }

    [Fact]
    public void Parse_PrologAndComments_Skipped()
    {
        ArmlParseResult r = ArmlParser.Parse(
            "<?xml version=\"1.0\"?><!-- header --><StackPanel><TextBlock>Hi</TextBlock></StackPanel>");
        Assert.True(r.Success);
        Assert.Equal("StackPanel", r.Root.TypeName);
        Assert.Equal("TextBlock", r.Root.Children[0].TypeName);
    }

    [Fact]
    public void Parse_FontSize_InheritedProperty()
    {
        ArmlParseResult r = ArmlParser.Parse("<TextBlock FontSize=\"18\">t</TextBlock>");
        Assert.True(r.Success);
        Assert.True(r.Root is TextBlock);
        TextBlock tb = (TextBlock)r.Root;
        Assert.Equal(18.0, tb.FontSize, 0.001);
    }

    [Fact]
    public void Parse_IsEnabled_Bool()
    {
        ArmlParseResult r = ArmlParser.Parse("<Button IsEnabled=\"False\">x</Button>");
        Assert.True(r.Success);
        Assert.True(r.Root is Button);
        Button btn = (Button)r.Root;
        Assert.False(btn.IsEnabled);
    }
}

/// <summary>
/// LivePreviewHost 单元测试：CPU 行为面 + wgpu GPU 集成面。
/// </summary>
public class LivePreviewHostTests
{
    // ===== CPU 面（无 GPU 依赖）=====

    [Fact]
    public void Constructor_Defaults()
    {
        LivePreviewHost host = new LivePreviewHost();
        Assert.NotNull(host);
        Assert.Equal(800.0, host.ViewportWidth, 0.001);
        Assert.Equal(600.0, host.ViewportHeight, 0.001);
        Assert.Null(host.RootElement);
    }

    [Fact]
    public void ResizeViewport_BeforeInit_UpdatesDims()
    {
        LivePreviewHost host = new LivePreviewHost();
        host.ResizeViewport(320.0, 240.0);
        Assert.Equal(320.0, host.ViewportWidth, 0.001);
        Assert.Equal(240.0, host.ViewportHeight, 0.001);
        Assert.Null(host.RootElement);
    }

    [Fact]
    public void GetLayoutSnapshot_BeforeLoad_Null()
    {
        LivePreviewHost host = new LivePreviewHost();
        Assert.Null(host.GetLayoutSnapshot());
    }

    [Fact]
    public void ApplyPatch_NotInitialized_False()
    {
        LivePreviewHost host = new LivePreviewHost();
        Assert.False(host.ApplyPatch("Root/Button", "Content", "x"));
    }

    [Fact]
    public void ApplyPatches_NotInitialized_Zero()
    {
        LivePreviewHost host = new LivePreviewHost();
        List<PropertyPatch> patches = new List<PropertyPatch>();
        PropertyPatch p = new PropertyPatch();
        p.ElementPath = "Root/Button";
        p.PropertyName = "Content";
        p.Value = "x";
        patches.Add(p);
        Assert.Equal(0, host.ApplyPatches(patches));
    }

    [Fact]
    public void CapturePng_NotInitialized_False()
    {
        LivePreviewHost host = new LivePreviewHost();
        Assert.False(host.CapturePng("nope.png"));
    }

    [Fact]
    public void Reset_EmptyHost_NoThrow()
    {
        LivePreviewHost host = new LivePreviewHost();
        host.Reset();
        Assert.Null(host.RootElement);
        Assert.Null(host.GetLayoutSnapshot());
    }

    // ===== GPU 集成面（wgpu 离屏；GPU 不可用则 Assert.Skip）=====

    private LivePreviewHost RequireHost() {
        LivePreviewHost host = new LivePreviewHost();
        bool ok = host.Initialize();
        if (!ok) {
            Assert.Skip("wgpu GPU 不可用，跳过 LivePreviewHost GPU 集成测试");
            return null;
        }
        return host;
    }

    [Fact]
    public void Initialize_Success()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        Assert.Equal(800.0, host.ViewportWidth, 0.001);
        Assert.Equal(600.0, host.ViewportHeight, 0.001);
    }

    [Fact]
    public void LoadSpec_BuildsTree()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        ArmlParseResult r = host.LoadSpec(
            "<StackPanel><Button>Save</Button><TextBlock>Hello</TextBlock></StackPanel>");
        Assert.True(r.Success);
        Assert.NotNull(host.RootElement);
        Assert.Equal("StackPanel", host.RootElement.TypeName);
        Assert.Equal(2, host.RootElement.Children.Count);
    }

    [Fact]
    public void LoadSpec_WithViewport_Resizes()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<StackPanel/>", 320.0, 240.0);
        Assert.Equal(320.0, host.ViewportWidth, 0.001);
        Assert.Equal(240.0, host.ViewportHeight, 0.001);
    }

    [Fact]
    public void ApplyPatch_ByTypePath_UpdatesContent()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<StackPanel><Button x:Name=\"SaveBtn\">Save</Button></StackPanel>");
        Assert.True(host.ApplyPatch("Root/StackPanel/Button", "Content", "Updated"));

        Button btn = (Button)host.RootElement.Children[0];
        Content c = btn.Content;
        bool isText = false;
        switch (c) {
            case Content.Text(s): isText = (s == "Updated"); break;
            default: break;
        }
        Assert.True(isText);
    }

    [Fact]
    public void ApplyPatch_ByName_UpdatesContent()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<StackPanel><Button x:Name=\"SaveBtn\">Save</Button></StackPanel>");
        Assert.True(host.ApplyPatch("Root/SaveBtn", "Content", "Renamed"));

        Button btn = (Button)host.RootElement.Children[0];
        Content c = btn.Content;
        bool isText = false;
        switch (c) {
            case Content.Text(s): isText = (s == "Renamed"); break;
            default: break;
        }
        Assert.True(isText);
    }

    [Fact]
    public void ApplyPatch_Background_UpdatesBrush()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<Button>Save</Button>");
        Assert.True(host.ApplyPatch("Root/Button", "Background", "#FF00FF00"));

        Button btn = (Button)host.RootElement;
        Assert.Equal("#FF00FF00", btn.Background);
    }

    [Fact]
    public void ApplyPatch_UnknownPath_False()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<Button>Save</Button>");
        Assert.False(host.ApplyPatch("Root/Missing", "Content", "x"));
    }

    [Fact]
    public void ApplyPatches_Batch_AppliesAll()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec(
            "<StackPanel><Button x:Name=\"A\">1</Button><Button x:Name=\"B\">2</Button></StackPanel>");

        List<PropertyPatch> patches = new List<PropertyPatch>();
        PropertyPatch p1 = new PropertyPatch();
        p1.ElementPath = "Root/A";
        p1.PropertyName = "Content";
        p1.Value = "one";
        patches.Add(p1);
        PropertyPatch p2 = new PropertyPatch();
        p2.ElementPath = "Root/B";
        p2.PropertyName = "Content";
        p2.Value = "two";
        patches.Add(p2);
        Assert.Equal(2, host.ApplyPatches(patches));

        Button a = (Button)host.RootElement.Children[0];
        Button b = (Button)host.RootElement.Children[1];
        bool okA = false;
        bool okB = false;
        Content ca = a.Content;
        Content cb = b.Content;
        switch (ca) {
            case Content.Text(s): okA = (s == "one"); break;
            default: break;
        }
        switch (cb) {
            case Content.Text(s): okB = (s == "two"); break;
            default: break;
        }
        Assert.True(okA);
        Assert.True(okB);
    }

    [Fact]
    public void GetLayoutSnapshot_AfterLoad()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec(
            "<StackPanel><TextBlock x:Name=\"Title\">Hello</TextBlock></StackPanel>");
        LayoutSnapshotNode root = host.GetLayoutSnapshot();
        Assert.NotNull(root);
        Assert.Equal("StackPanel", root.TypeName);
        Assert.Equal(1, root.Children.Count);
        LayoutSnapshotNode child = root.Children[0];
        Assert.Equal("TextBlock", child.TypeName);
        Assert.Equal("Title", child.Name);
        Assert.True(child.Properties.ContainsKey("Text"));
        Assert.Equal("Hello", child.Properties["Text"]);
    }

    [Fact]
    public void CapturePng_SavesFile()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<StackPanel><Button Background=\"#FF3C8DBC\">Save</Button></StackPanel>");

        string path = "obj/live_preview_test.png";
        if (File.Exists(path)) {
            File.Delete(path);
        }
        Assert.True(host.CapturePng(path));
        Assert.True(File.Exists(path));
        File.Delete(path);
    }

    [Fact]
    public void Reset_ClearsTree()
    {
        LivePreviewHost host = this.RequireHost();
        if (host == null) {
            return;
        }
        host.LoadSpec("<StackPanel><Button>Save</Button></StackPanel>");
        Assert.NotNull(host.RootElement);

        host.Reset();
        Assert.Null(host.RootElement);
        Assert.Null(host.GetLayoutSnapshot());
        Assert.False(host.ApplyPatch("Root/Button", "Content", "x"));
    }
}
