namespace UnitTest.QIF;

using Arc;
using Arc.QIF;
using Arc.Threading;

/// <summary>
/// Collection 并行边界语料：同集合内串行、不同集合可并行（对标 xUnit）。
/// 过滤：Trait~qif_collection。
/// </summary>
[Trait("category", "unit")]
[Trait("qif_collection", "serial_within")]
[Collection("QifSharedSerial")]
public class QIFCollectionSharedATests
{
    private static Lock _lock = new Lock();
    private static int _sharedCounter = 0;
    private static int _maxSeen = 0;
    private static int _inflight = 0;

    [Fact]
    public void SharedA_Step1_SerialWithinCollection()
    {
        this.EnterLeave();
        Assert.GreaterOrEqual(_sharedCounter, 1);
    }

    [Fact]
    public void SharedA_Step2_SerialWithinCollection()
    {
        this.EnterLeave();
        Assert.GreaterOrEqual(_sharedCounter, 1);
    }

    private void EnterLeave()
    {
        lock (_lock) {
            _inflight = _inflight + 1;
            if (_inflight > _maxSeen) {
                _maxSeen = _inflight;
            }
        }
        // 忙等拉开窗口；同集合内不得出现 inflight>1
        long seed = 1;
        for (int i = 0; i < 800000; i = i + 1) {
            seed = seed * 1103515245 + 12345;
        }
        lock (_lock) {
            Assert.Equal(1, _inflight);
            _inflight = _inflight - 1;
            _sharedCounter = _sharedCounter + 1;
        }
        Assert.True(seed != 0);
        Assert.Equal(1, _maxSeen);
    }
}

[Trait("category", "unit")]
[Trait("qif_collection", "serial_within")]
[Collection("QifSharedSerial")]
public class QIFCollectionSharedBTests
{
    [Fact]
    public void SharedB_AlsoInSameCollection_Serial()
    {
        // 与 SharedA 同集合：并行调度下仍须串行完成（不额外争用 SharedA 计数）
        Assert.True(true);
    }
}

[Trait("category", "unit")]
[Trait("qif_collection", "implicit_class")]
public class QIFCollectionImplicitClassTests
{
    [Fact]
    public void ImplicitClass_RunsAsOwnCollection()
    {
        Assert.True(true);
    }
}
