// L3 Deferred · Minimal UI（Window 属性面）。
//
// 保持 Deferred 隔离——禁止回迁 examples/UnitTest 顶绿。
// 纯逻辑骨架回归见 ui_skeleton_honesty_e2e（非本文件）。
// Window 显示链路 / ARML / wgpu = 另排 UI 扩张 Sprint。

namespace UnitTest.Arc;

using Arc;
using Arc.UI;
using Arc.UI.Components;
using Arc.QIF;

public class MinimalUITests
{
    [Fact]
    public void Window_Create()
    {
        Window w = new Window();
        Assert.NotNull(w);
    }

    [Fact]
    public void Window_Title()
    {
        Window w = new Window();
        w.Title = "Test";
        Assert.Equal("Test", w.Title);
    }

    [Fact]
    public void Window_Width()
    {
        Window w = new Window();
        w.Width = 800.0;
        Assert.Equal(800.0, w.Width, 0.001);
    }
}
