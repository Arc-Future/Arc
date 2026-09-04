namespace Arc;

/// <summary>
/// 数学函数库——对齐 C# <c>System.Math</c> 的<strong>诚实子集</strong>（非完备对等）。
/// codegen 直射 LLVM intrinsic 或 libm；无 <c>rt_math_*</c> ABI。
/// Stable 面证据：<c>UnitTest/Arc/MathTests</c>、<c>math_e2e</c>
///（含 <c>Clamp(int|long)</c>、负向 Floor、Asin/Atan2、Sinh、long Min/Max、
/// <c>CopySign</c>/<c>Cbrt</c>/<c>Hypot</c>/<c>IEEERemainder</c>）。
/// </summary>
public class Math {
    /// <summary>圆周率 π。</summary>
    public const double PI = 3.1415926535897931;

    /// <summary>自然对数底 e。</summary>
    public const double E = 2.7182818284590451;

    // ── 三角函数（Sin/Cos = LLVM；Tan/Asin/Acos/Atan/Atan2 = libm）──

    [Builtin(ABI = "rt_math_sin")]
    public static double Sin(double x) { return 0; }

    [Builtin(ABI = "rt_math_cos")]
    public static double Cos(double x) { return 0; }

    [Builtin(ABI = "rt_math_tan")]
    public static double Tan(double x) { return 0; }

    [Builtin(ABI = "rt_math_asin")]
    public static double Asin(double x) { return 0; }

    [Builtin(ABI = "rt_math_acos")]
    public static double Acos(double x) { return 0; }

    [Builtin(ABI = "rt_math_atan")]
    public static double Atan(double x) { return 0; }

    [Builtin(ABI = "rt_math_atan2")]
    public static double Atan2(double y, double x) { return 0; }

    // ── 双曲函数（libm）──

    [Builtin(ABI = "rt_math_sinh")]
    public static double Sinh(double x) { return 0; }

    [Builtin(ABI = "rt_math_cosh")]
    public static double Cosh(double x) { return 0; }

    [Builtin(ABI = "rt_math_tanh")]
    public static double Tanh(double x) { return 0; }

    // ── 幂与对数 ──

    [Builtin(ABI = "rt_math_sqrt")]
    public static double Sqrt(double x) { return 0; }

    [Builtin(ABI = "rt_math_exp")]
    public static double Exp(double x) { return 0; }

    [Builtin(ABI = "rt_math_log")]
    public static double Log(double x) { return 0; }

    [Builtin(ABI = "rt_math_log10")]
    public static double Log10(double x) { return 0; }

    [Builtin(ABI = "rt_math_log2")]
    public static double Log2(double x) { return 0; }

    [Builtin(ABI = "rt_math_pow")]
    public static double Pow(double x, double y) { return 0; }

    // ── 舍入（LLVM floor/ceil/round/trunc）──

    [Builtin(ABI = "rt_math_ceil")]
    public static double Ceiling(double x) { return 0; }

    [Builtin(ABI = "rt_math_floor")]
    public static double Floor(double x) { return 0; }

    // 银行家舍入（round-half-to-even，对齐 C# Math.Round 默认语义）
    [Builtin(ABI = "rt_math_round")]
    public static double Round(double x) { return 0; }

    [Builtin(ABI = "rt_math_truncate")]
    public static double Truncate(double x) { return 0; }

    // ── 符号与钳制（codegen 直射 icmp/select 或 minnum/maxnum）──
    // Clamp(int|long)：假前提 min <= max；min > max 行为未定义（非 C# 抛错）。

    [Builtin(ABI = "rt_math_sign_int")]
    public static int Sign(int x) { return 0; }

    [Builtin(ABI = "rt_math_sign_double")]
    public static int Sign(double x) { return 0; }

    [Builtin(ABI = "rt_math_clamp")]
    public static double Clamp(double value, double min, double max) { return 0; }

    [Builtin(ABI = "rt_math_clamp_int")]
    public static int Clamp(int value, int min, int max) { return 0; }

    [Builtin(ABI = "rt_math_clamp_long")]
    public static long Clamp(long value, long min, long max) { return 0; }

    // ── 绝对值 ──

    [Builtin(ABI = "rt_math_abs_double")]
    public static double Abs(double x) { return 0; }

    [Builtin(ABI = "rt_math_abs_int")]
    public static int Abs(int x) { return 0; }

    [Builtin(ABI = "rt_math_abs_long")]
    public static long Abs(long x) { return 0; }

    // ── 最大/最小值（按重载类型分派）──

    [Builtin(ABI = "rt_math_min_double")]
    public static double Min(double a, double b) { return 0; }

    [Builtin(ABI = "rt_math_min_int")]
    public static int Min(int a, int b) { return 0; }

    [Builtin(ABI = "rt_math_min_long")]
    public static long Min(long a, long b) { return 0; }

    [Builtin(ABI = "rt_math_max_double")]
    public static double Max(double a, double b) { return 0; }

    [Builtin(ABI = "rt_math_max_int")]
    public static int Max(int a, int b) { return 0; }

    [Builtin(ABI = "rt_math_max_long")]
    public static long Max(long a, long b) { return 0; }

    // ── 融合乘加 ──

    [Builtin(ABI = "rt_math_fma")]
    public static double Fma(double a, double b, double c) { return 0; }

    // ── 符号/根/斜边/IEEE 余数（CopySign = LLVM；其余 = libm）──
    // 后置：float 变体；DivRem/BigMul；NaN 传播细规。

    /// <summary>幅值取自 <paramref name="x"/>，符号取自 <paramref name="y"/>。</summary>
    [Builtin(ABI = "rt_math_copysign")]
    public static double CopySign(double x, double y) { return 0; }

    /// <summary>立方根（libm <c>cbrt</c>）。</summary>
    [Builtin(ABI = "rt_math_cbrt")]
    public static double Cbrt(double x) { return 0; }

    /// <summary>√(x²+y²)，溢出友好（libm <c>hypot</c>）。</summary>
    [Builtin(ABI = "rt_math_hypot")]
    public static double Hypot(double x, double y) { return 0; }

    /// <summary>IEEE 754 remainder（libm <c>remainder</c>；非 C <c>fmod</c>）。</summary>
    [Builtin(ABI = "rt_math_ieee_remainder")]
    public static double IEEERemainder(double x, double y) { return 0; }
}
