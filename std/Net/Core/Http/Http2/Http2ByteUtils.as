// S2 (RFC 033 §2.4): Arc.Net — 字节数组小工具。
//
// 语言禁 `new T[expr]` 动态尺寸（与 std/Drawing/QrCodeWriter.as 同例）：定长零填充
// 缓冲以 List<byte> + Add((byte)0) + ToArray() 构造。

namespace Arc.Net;

using Arc.Collections;

/// <summary>字节数组工具（零填充分配）。</summary>
internal class Http2ByteUtils {
    /// <summary>n 字节零填充数组。</summary>
    internal static byte[] ZeroBytes(int n) {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < n) {
            buf.Add((byte)0);
            i = i + 1;
        }
        return buf.ToArray();
    }
}
