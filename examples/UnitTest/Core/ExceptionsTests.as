namespace UnitTest.Core;

using Arc;
using Arc.QIF;

public class ExceptionsTests
{
    [Fact]
    public void TryFinally_FinallyExecutes()
    {
        bool finallyRan = false;
        try { int x = 42; } finally { finallyRan = true; }
        Assert.True(finallyRan);
    }

    [Fact]
    public void TryCatch_CatchesException()
    {
        bool caught = false;
        try { throw new Exception(); } catch { caught = true; }
        Assert.True(caught);
    }

    [Fact]
    public void TryCatchFinally_BothRun()
    {
        bool caught = false;
        bool finallyRan = false;
        try { throw new Exception(); } catch { caught = true; } finally { finallyRan = true; }
        Assert.True(caught);
        Assert.True(finallyRan);
    }

    [Fact]
    public void CatchWhen_Filters()
    {
        int x = 0;
        try { throw new Exception(); } catch when (x == 0) { x = 2; }
        Assert.Equal(2, x);
    }

    [Fact]
    public void Exception_Message_RoundTrip()
    {
        Exception ex = new Exception("boom");
        Assert.True(ex.Message.Length == 4);
    }

    /// <summary>
    /// StackTrace：构造后 null；throw/catch 后非 null，且含可读符号帧（函数名）。
    /// </summary>
    [Fact]
    public void Exception_StackTrace_CapturedOnThrow()
    {
        Exception constructed = new Exception("x");
        Assert.True(constructed.StackTrace == null);

        Exception caught = null;
        try {
            throw new Exception("y");
        } catch (Exception ex) {
            caught = ex;
        }
        Assert.True(caught != null);
        Assert.True(caught.StackTrace != null);
        Assert.True(caught.StackTrace.Length > 0);
        Assert.Contains("at ", caught.StackTrace);
        // 符号完备：嵌入 dbg 表默认发射；锁定可读符号而非仅地址。
        Assert.Contains("Exception_StackTrace_CapturedOnThrow", caught.StackTrace);

        InvalidOperationException derived = null;
        try {
            throw new InvalidOperationException("z");
        } catch (InvalidOperationException ex) {
            derived = ex;
        }
        Assert.True(derived != null);
        Assert.True(derived.StackTrace != null);
        Assert.Contains("at ", derived.StackTrace);
        Assert.Contains("Exception_StackTrace_CapturedOnThrow", derived.StackTrace);
    }

    /// <summary>
    /// ToString：构造后仅 Message；throw/catch 后含 Message + StackTrace（可读符号帧）。
    /// </summary>
    [Fact]
    public void Exception_ToString_AppendsStackTrace()
    {
        Exception constructed = new Exception("pre");
        Assert.Equal("pre", constructed.ToString());

        Exception caught = null;
        try {
            throw new Exception("boom");
        } catch (Exception ex) {
            caught = ex;
        }
        Assert.True(caught != null);
        string s = caught.ToString();
        Assert.Contains("boom", s);
        Assert.Contains("at ", s);
        Assert.Contains("Exception_ToString_AppendsStackTrace", s);
    }

    [Fact]
    public void InvalidOperationException_Construct()
    {
        InvalidOperationException ex = new InvalidOperationException("bad state");
        Assert.True(ex.Message.Length > 0);
    }

    [Fact]
    public void ArgumentException_MessageAndParamNameField()
    {
        ArgumentException ex = new ArgumentException("msg");
        ex.ParamName = "p";
        Assert.True(ex.Message.Length == 3);
        Assert.True(ex.ParamName.Length == 1);
    }

    [Fact]
    public void ArgumentNullException_Construct()
    {
        ArgumentNullException ex = new ArgumentNullException("name");
        Assert.True(ex.ParamName.Length == 4);
    }

    [Fact]
    public void ArgumentOutOfRangeException_Construct()
    {
        ArgumentOutOfRangeException ex = new ArgumentOutOfRangeException("index");
        Assert.True(ex.ParamName.Length == 5);
    }

    [Fact]
    public void NotSupportedException_Construct()
    {
        NotSupportedException ex = new NotSupportedException("nope");
        Assert.True(ex.Message.Length == 4);
    }

    [Fact]
    public void Exception_InnerException_ToStringChain()
    {
        Exception inner = new Exception("inner");
        Exception outer = new Exception("outer", inner);
        Assert.True(outer.InnerException != null);
        Assert.Equal("inner", outer.InnerException.Message);
        string s = outer.ToString();
        Assert.Contains("outer", s);
        Assert.Contains("--->", s);
        Assert.Contains("inner", s);
    }

    [Fact]
    public void Exception_SourceAndHResult_RoundTrip()
    {
        Exception ex = new Exception("x");
        ex.Source = "UnitTest";
        Assert.Equal("UnitTest", ex.Source);
        Assert.True(ex.HResult != 0);
        ex.HResult = 42;
        Assert.Equal(42, ex.Code);
    }

    [Fact]
    public void FormatException_ThrowAndCatch()
    {
        FormatException caught = null;
        try {
            throw new FormatException("bad format");
        } catch (FormatException ex) {
            caught = ex;
        }
        Assert.True(caught != null);
        Assert.Equal("bad format", caught.Message);
    }

    [Fact]
    public void ObjectDisposedException_ObjectName()
    {
        ObjectDisposedException ex = new ObjectDisposedException("stream");
        Assert.Equal("stream", ex.ObjectName);
        ObjectDisposedException withMsg = new ObjectDisposedException("ms", "already disposed");
        Assert.Equal("ms", withMsg.ObjectName);
        Assert.Equal("already disposed", withMsg.Message);
    }

    [Fact]
    public void IOException_WithInner()
    {
        Exception inner = new Exception("disk");
        IOException ex = new IOException("io fail", inner);
        Assert.Equal("io fail", ex.Message);
        Assert.True(ex.InnerException != null);
        Assert.Equal("disk", ex.InnerException.Message);
    }

    [Fact]
    public void OperationCanceledException_Construct()
    {
        OperationCanceledException ex = new OperationCanceledException("canceled");
        Assert.Equal("canceled", ex.Message);
    }

    [Fact]
    public void NotImplementedException_ThrowAndCatch()
    {
        bool caught = false;
        try {
            throw new NotImplementedException("todo");
        } catch (NotImplementedException ex) {
            caught = true;
            Assert.Equal("todo", ex.Message);
        }
        Assert.True(caught);
    }

    [Fact]
    public void ArgumentNullException_WithMessage()
    {
        ArgumentNullException ex = new ArgumentNullException("name", "was null");
        Assert.Equal("name", ex.ParamName);
        Assert.Equal("was null", ex.Message);
    }

    [Fact]
    public void ArgumentOutOfRangeException_WithMessage()
    {
        ArgumentOutOfRangeException ex = new ArgumentOutOfRangeException("index", "oor");
        Assert.Equal("index", ex.ParamName);
        Assert.Equal("oor", ex.Message);
    }
}
