// Buffer — C# System.Buffer 诚实最小面（byte[] BlockCopy）。
//
// 已落地：BlockCopy(byte[], srcOffset, byte[], dstOffset, count)。
// count 为字节数；对 byte[] 与元素数一致。任意 Array 字节级拷贝后置。
// 实现复用 rt_array_copy（elem_size=1）。
namespace Arc;

/// <summary>字节缓冲工具，对齐 C# <c>System.Buffer</c> 最小子集。</summary>
public static class Buffer {
    /// <summary>
    /// 将 <paramref name="count"/> 字节从 <paramref name="src"/> 拷到 <paramref name="dst"/>
    ///（偏移为字节下标；本 Stable 面仅 <c>byte[]</c>）。
    /// </summary>
    [Builtin(ABI = "rt_array_copy")]
    public static void BlockCopy(byte[] src, int srcOffset, byte[] dst, int dstOffset, int count) { }
}
