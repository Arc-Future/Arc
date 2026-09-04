namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// FFI NativeListInterop 单元测试：验证 List&lt;T&gt; 到 C 的零拷贝 marshal。
/// codegen 将 List&lt;T&gt; 参数展开为 (ptr buffer, i32 size)，C 侧通过
/// rt_list_buffer_and_size ABI 零拷贝访问内部数据。
/// </summary>
public class NativeListInteropTests
{
    // ── arc_test.rt_native_test_sum_list ──

    [Fact]
    public void List_SumViaNative()
    {
        var xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);

        var sum = arc_test.rt_native_test_sum_list(xs);
        Assert.Equal(6, sum);
    }

    [Fact]
    public void List_SizeViaNative()
    {
        var xs = new List<int>();
        xs.Add(10);
        xs.Add(20);
        xs.Add(30);
        xs.Add(40);

        var size = arc_test.rt_native_test_list_size(xs);
        Assert.Equal(4, size);
    }

    [Fact]
    public void List_NativeSum_1to5()
    {
        var xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        xs.Add(4);
        xs.Add(5);

        var sum = arc_test.rt_native_test_sum_list(xs);
        Assert.Equal(15, sum);
    }
}
