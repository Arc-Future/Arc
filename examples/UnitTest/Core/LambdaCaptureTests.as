namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// Lambda 鎹曡幏璇箟鍗曞厓娴嬭瘯锛氳鐩?LambdaCapture 绀轰緥鐨勫叏閮ㄥ満鏅€?
/// L1: class ByRef 鎹曡幏锛堟寚閽堣涔夛級
/// L2: int ByValue 鎹曡幏锛堝揩鐓ц涔夛級
/// L3: 娣峰悎鎹曡幏锛坈lass ByRef + int ByValue 鍚岄棴鍖咃級
/// L3: this 鎹曡幏锛堟樉寮?this.Base / 闅愬紡 bare Base锛?
/// L3: 寰幆鍙橀噺鎹曡幏锛坕nt ByValue 蹇収锛?
/// </summary>

class CaptureCounter {
    public int Value;

    public CaptureCounter(int value) {
        Value = value;
    }
}

class CaptureConfig {
    public int Limit;

    public CaptureConfig(int limit) {
        Limit = limit;
    }
}

class CaptureWidget {
    public int Base;

    public CaptureWidget(int b) {
        Base = b;
    }

    public int TestThisCapture() {
        Func<int, int> adder = x => x + this.Base;
        int before = adder(10);
        this.Base = this.Base + 50;
        int after = adder(10);
        return before * 1000 + after;
    }

    public int TestImplicitThis() {
        Func<int, int> adder = x => x + Base;
        return adder(10);
    }
}

public class LambdaCaptureTests
{
    [Fact]
    public void Class_ByRefCapture()
    {
        CaptureCounter c = new CaptureCounter(10);
        Func<int, int> addCounter = x => x + c.Value;

        int before = addCounter(5);
        Assert.Equal(15, before);

        c.Value = 20;
        int after = addCounter(5);
        Assert.Equal(25, after);
    }

    [Fact]
    public void Int_ByValueCapture()
    {
        int threshold = 10;
        Func<int, int> addThreshold = x => x + threshold;

        int before = addThreshold(5);
        Assert.Equal(15, before);

        threshold = 100;
        int after = addThreshold(5);
        Assert.Equal(15, after);
    }

    [Fact]
    public void MixedCapture()
    {
        CaptureConfig config = new CaptureConfig(100);
        int offset = 5;
        Func<int, bool> check = x => x + offset >= config.Limit;

        offset = 50;
        config.Limit = 60;

        Assert.False(check(10));
        Assert.True(check(55));
    }

    [Fact]
    public void ThisCapture_Explicit()
    {
        CaptureWidget w = new CaptureWidget(100);
        int result = w.TestThisCapture();
        Assert.Equal(110160, result);
    }

    [Fact]
    public void ThisCapture_Implicit()
    {
        CaptureWidget w = new CaptureWidget(7);
        int result = w.TestImplicitThis();
        Assert.Equal(17, result);
    }

    [Fact]
    public void LoopVariable_ByValueSnapshot()
    {
        int loopResult = 0;
        int j = 0;
        while (j < 3) {
            Func<int, int> addJ = x => x + j;
            j = j + 1;
            loopResult = loopResult * 10 + addJ(0);
        }
        Assert.Equal(12, loopResult);
    }
}
