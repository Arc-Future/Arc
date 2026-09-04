// BitConverter — C# System.BitConverter 诚实最小面（主机端序）。
//
// 已落地：IsLittleEndian()；GetBytes(int|long|float|double)；ToInt32/ToInt64/ToSingle/ToDouble；
// SingleToInt32Bits/Int32BitsToSingle/DoubleToInt64Bits/Int64BitsToDouble（IEEE 754 位型重释）。
// Arc 无静态属性 → IsLittleEndian 为方法（同 Environment.*）。
// 后置：short/bool、GetBytes 返回值端序切换、TryWriteBytes（Span 写面无挂载）。
// 位重释实现：codegen 直射 LLVM bitcast（编译期内建，零运行时开销），字节编解码复用
// rt_bitconverter_* i32/i64 ABI（与 int/long 端序行为一致）——无新增 rt_* 符号。
namespace Arc;

/// <summary>
/// 基元 ↔ 字节数组（主机端序），对齐 C# <c>System.BitConverter</c> 常用子集。
/// </summary>
public static class BitConverter {
    /// <summary>主机是否小端。Arc 无静态属性，以方法暴露（同 <c>Environment.Is64BitProcess</c>）。</summary>
    [Builtin(ABI = "rt_bitconverter_is_little_endian")]
    public static bool IsLittleEndian() { return false; }

    /// <summary>将 <c>int</c> 编码为 4 字节（主机端序）。</summary>
    [Builtin(ABI = "rt_bitconverter_get_bytes_i32")]
    public static byte[] GetBytes(int value) { return null; }

    /// <summary>将 <c>long</c> 编码为 8 字节（主机端序）。</summary>
    [Builtin(ABI = "rt_bitconverter_get_bytes_i64")]
    public static byte[] GetBytes(long value) { return null; }

    /// <summary>将 <c>float</c> 编码为 4 字节（主机端序；IEEE 754 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:float→i32 + rt_bitconverter_get_bytes_i32")]
    public static byte[] GetBytes(float value) { return null; }

    /// <summary>将 <c>double</c> 编码为 8 字节（主机端序；IEEE 754 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:double→i64 + rt_bitconverter_get_bytes_i64")]
    public static byte[] GetBytes(double value) { return null; }

    /// <summary>自 <paramref name="value"/>[<paramref name="startIndex"/>..] 读 4 字节为 <c>int</c>（主机端序）。</summary>
    [Builtin(ABI = "rt_bitconverter_to_i32")]
    public static int ToInt32(byte[] value, int startIndex) { return 0; }

    /// <summary>自 <paramref name="value"/>[<paramref name="startIndex"/>..] 读 8 字节为 <c>long</c>（主机端序）。</summary>
    [Builtin(ABI = "rt_bitconverter_to_i64")]
    public static long ToInt64(byte[] value, int startIndex) { return 0; }

    /// <summary>自 <paramref name="value"/>[<paramref name="startIndex"/>..] 读 4 字节为 <c>float</c>（主机端序；位型重释）。</summary>
    [Builtin(ABI = "rt_bitconverter_to_i32 + bitcast:i32→float")]
    public static float ToSingle(byte[] value, int startIndex) { return 0; }

    /// <summary>自 <paramref name="value"/>[<paramref name="startIndex"/>..] 读 8 字节为 <c>double</c>（主机端序；位型重释）。</summary>
    [Builtin(ABI = "rt_bitconverter_to_i64 + bitcast:i64→double")]
    public static double ToDouble(byte[] value, int startIndex) { return 0; }

    /// <summary>将 <c>float</c> 的 IEEE 754 位型重释为 <c>int</c>（NaN/Inf/-0 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:float→i32")]
    public static int SingleToInt32Bits(float value) { return 0; }

    /// <summary>将 <c>int</c> 位型重释为 <c>float</c>（NaN/Inf/-0 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:i32→float")]
    public static float Int32BitsToSingle(int value) { return 0; }

    /// <summary>将 <c>double</c> 的 IEEE 754 位型重释为 <c>long</c>（NaN/Inf/-0 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:double→i64")]
    public static long DoubleToInt64Bits(double value) { return 0; }

    /// <summary>将 <c>long</c> 位型重释为 <c>double</c>（NaN/Inf/-0 位型精确保留）。</summary>
    [Builtin(ABI = "bitcast:i64→double")]
    public static double Int64BitsToDouble(long value) { return 0; }
}
