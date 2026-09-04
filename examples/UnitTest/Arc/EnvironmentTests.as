namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Arc.Environment 诚实子集单元测试（非 Fact-Skip）。
/// 覆盖 Get/Set 往返、未设置空串、NewLine 平台值、ProcessorCount/Platform。
/// 无 TickCount*（RFC 072 单一惯用法 = Stopwatch）。
/// </summary>
public class EnvironmentTests
{
    [Fact]
    public void GetSet_RoundTrip()
    {
        string key = "ARC_UNITTEST_ENV_KEY";
        int setOk = Environment.SetEnvironmentVariable(key, "unit-env");
        Assert.Equal(1, setOk);
        Assert.Equal("unit-env", Environment.GetEnvironmentVariable(key));
        int delOk = Environment.SetEnvironmentVariable(key, "");
        Assert.Equal(1, delOk);
        Assert.Equal("", Environment.GetEnvironmentVariable(key));
    }

    [Fact]
    public void Get_Unset_ReturnsEmpty()
    {
        string missing = Environment.GetEnvironmentVariable("ARC_UNITTEST_ENV_NEVER_SET_XYZ");
        Assert.Equal("", missing);
    }

    [Fact]
    public void NewLine_Matches_Platform()
    {
        string nl = Environment.NewLine();
        if (Environment.IsWindows())
        {
            Assert.Equal("\r\n", nl);
        }
        else
        {
            Assert.Equal("\n", nl);
        }
    }

    [Fact]
    public void ProcessorCount_And_Platform_NonEmpty()
    {
        Assert.True(Environment.ProcessorCount() >= 1);
        Assert.True(Environment.Platform().Length > 0);
        Assert.True(Environment.MachineName().Length > 0);
        Assert.True(Environment.UserName().Length > 0);
    }
}
