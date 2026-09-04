// OnnxNative — ONNX Runtime C ABI 门面（std/AI/Onnx 内部使用）。
//
// 统一封装 `onnx` native 模块（crates/arc/native/onnx.ani，load="auto"）的
// 调用约定：
//   - 门闩：<see cref="IsAvailable"/> 查询 native 库运行时是否可加载
//     （Native.IsAvailable("onnx")；库经 ARC_ONNX_LIB 目录懒加载）。
//   - 错误协议：shim 返回 0 成功 / 非零失败；失败后末次错误串经
//     onnx_last_error 取回。本门面把返回码收敛为 <see cref="OnnxException"/>。
//   - 本类为**内部实现细节**（internal）——仅经 InferenceSession /
//     SessionOptions / OnnxTensor 使用，不对类库使用者暴露。
namespace Arc.AI.Onnx;

using Arc;
using Arc.Collections;
using Arc.Text;

/// <summary>ONNX Runtime C ABI 门面（内部实现细节）。</summary>
/// 注：Arc `static class` 不支持字段，故用普通类承载静态成员（对齐
/// BindingOperations/BindingRegistry 惯例）。
internal class OnnxNative {
    /// <summary>错误串取回缓冲容量（字节）。</summary>
    private const int ErrorBufferSize = 1024;

    /// <summary>ONNX Runtime native 库是否可用（`load="auto"` 门闩）。
    /// 推荐业务侧以此做可选功能灰化，而非依赖异常做流程控制。</summary>
    public static bool IsAvailable {
        get { return Native.IsAvailable("onnx"); }
    }

    /// <summary>库不可用时抛出 <see cref="OnnxNotAvailableException"/>。</summary>
    public static void EnsureAvailable() {
        // 注：必须全限定引用——裸 `IsAvailable` 会与编译器特判的
        // `Native.IsAvailable` 名称冲突（"undefined name"），故显式限定。
        if (!OnnxNative.IsAvailable) {
            throw new OnnxNotAvailableException(
                "ONNX Runtime native library is not available. Configure ARC_ONNX_LIB to the " +
                "directory containing onnx_shim.dll (plus onnxruntime.dll) before running.");
        }
    }

    /// <summary>取回 shim 末次错误串（无错误返回空串）。</summary>
    public static string LastError() {
        List<byte> buf = new List<byte>();
        int i = 0;
        while (i < ErrorBufferSize) {
            buf.Add((byte)0);
            i = i + 1;
        }
        onnx.onnx_last_error(buf);
        return Encoding.GetString(buf.ToArray());
    }

    /// <summary>返回码非 0 时抛 <see cref="OnnxException"/>（携带 shim 末次错误）。</summary>
    public static void ThrowIfFailed(int rc) {
        if (rc != 0) {
            throw new OnnxException(LastError());
        }
    }

    /// <summary>分配填充 n 个零的 List&lt;long&gt;（native 零拷贝 size 注入 = Count）。</summary>
    public static List<long> AllocLongs(long n) {
        List<long> l = new List<long>();
        long i = 0;
        while (i < n) {
            l.Add((long)0);
            i = i + 1;
        }
        return l;
    }

    /// <summary>分配填充 n 个零的 List&lt;int&gt;。</summary>
    public static List<int> AllocInts(long n) {
        List<int> l = new List<int>();
        long i = 0;
        while (i < n) {
            l.Add(0);
            i = i + 1;
        }
        return l;
    }

    /// <summary>分配填充 n 个零的 List&lt;float&gt;。</summary>
    public static List<float> AllocFloats(long n) {
        List<float> l = new List<float>();
        long i = 0;
        while (i < n) {
            l.Add((float)0.0);
            i = i + 1;
        }
        return l;
    }

    /// <summary>分配填充 n 个零的 List&lt;double&gt;。</summary>
    public static List<double> AllocDoubles(long n) {
        List<double> l = new List<double>();
        long i = 0;
        while (i < n) {
            l.Add(0.0);
            i = i + 1;
        }
        return l;
    }

    /// <summary>分配填充 n 个零的 List&lt;byte&gt;。</summary>
    public static List<byte> AllocBytes(long n) {
        List<byte> l = new List<byte>();
        long i = 0;
        while (i < n) {
            l.Add((byte)0);
            i = i + 1;
        }
        return l;
    }

    /// <summary>原生 ONNX 元素类型数值 → 共享 <see cref="TensorElementType"/> 枚举
    /// （仅宿主可承载的类型化缓冲有映射；其余后端元素类型 → <see cref="TensorElementType.Undefined"/>）。</summary>
    public static TensorElementType FromTensorElementType(int v) {
        if (v == 1) { return TensorElementType.Float32; }
        if (v == 11) { return TensorElementType.Float64; }
        if (v == 6) { return TensorElementType.Int32; }
        if (v == 7) { return TensorElementType.Int64; }
        if (v == 2) { return TensorElementType.UInt8; }
        return TensorElementType.Undefined;
    }
}
