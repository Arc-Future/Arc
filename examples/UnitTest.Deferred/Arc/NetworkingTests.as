namespace UnitTest.Arc;

using Arc;
using Arc.Net;
using Arc.QIF;

/// <summary>
/// Arc.Net Http/Tcp MVP 纯逻辑回归（Deferred：不随核心 UnitTest 默认跑）。
/// 权威非 Skip e2e 原：<c>crates/arc-integration/tests/net_e2e.rs</c>
/// （Uri/Cookie/UriBuilder + Tcp 环回 <c>net_tcp_loopback_mvp</c>；已随
/// arc-integration 退场，a2627a0f）。
/// 禁止 Fact-Skip 顶绿；不测 HttpClient GET / DNS / TLS / P2P；不扩协议。
/// </summary>
public class NetworkingTests
{
    [Fact]
    public void Uri_Parse_Parts()
    {
        Uri u = new Uri("http://example.com:8080/path?q=1#s");
        Assert.True(u.Scheme == "http");
        Assert.True(u.Host == "example.com");
        Assert.True(u.Port == 8080);
        Assert.True(u.AbsolutePath == "/path");
        Assert.True(u.Query == "?q=1");
        Assert.True(u.Fragment == "#s");
    }

    [Fact]
    public void Cookie_Parse_NamePath()
    {
        Cookie c = Cookie.Parse("sess=abc; Path=/");
        Assert.True(c.Name == "sess");
        Assert.True(c.Value == "abc");
        Assert.True(c.Path == "/");
    }
}
