// IreeNative — IREE Runtime C ABI 门面（std/AI/Iree 内部使用）。
//
// 统一封装 `iree` native 模块（crates/arc/native/iree.ani，load="auto"）的
// 调用约定（对齐 OnnxNative）：
//   - 门闩：<see cref="IsAvailable"/> 查询 native 库运行时是否可加载
//     （Native.IsAvailable("iree")；库经 ARC_IREE_LIB 目录懒加载）。
//   - 错误协议：shim 返回 0 成功 / 非零失败；失败后末次错误串经
//     iree_last_error 取回。本门面把返回码收敛为 <see cref="IreeException"/>。
//   - 本类为**内部实现细节**（internal）——仅经 IreeModelFactory /
//     （后续 M-I 的 IreeSession 等）使用，不对类库使用者暴露。
namespace Arc.AI.Iree;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Text;

/// <summary>IREE Runtime C ABI 门面（内部实现细节）。</summary>
/// 注：Arc `static class` 不支持字段，故用普通类承载静态成员（对齐
/// OnnxNative/BindingOperations 惯例）。
internal class IreeNative {
    /// <summary>错误串取回缓冲容量（字节）。</summary>
    private const int ErrorBufferSize = 1024;

    /// <summary>IREE Runtime native 库是否可用（`load="auto"` 门闩）。
    /// 推荐业务侧以此做可选功能灰化，而非依赖异常做流程控制。</summary>
    public static bool IsAvailable {
        get { return Native.IsAvailable("iree"); }
    }

    /// <summary>库不可用时抛出 <see cref="IreeNotAvailableException"/>。</summary>
    public static void EnsureAvailable() {
        // 注：必须全限定引用——裸 `IsAvailable` 会与编译器特判的
        // `Native.IsAvailable` 名称冲突（"undefined name"），故显式限定。
        if (!IreeNative.IsAvailable) {
            throw new IreeNotAvailableException(
                "IREE Runtime native library is not available. Configure ARC_IREE_LIB to the " +
                "directory containing iree_shim.dll (plus IREE runtime DLLs) before running.");
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
        iree.iree_last_error(buf);
        return Encoding.GetString(buf.ToArray());
    }

    /// <summary>返回码非 0 时抛 <see cref="IreeException"/>（携带 shim 末次错误）。</summary>
    public static void ThrowIfFailed(int rc) {
        if (rc != 0) {
            throw new IreeException(LastError());
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

    /// <summary>shim 归一化 buffer_view 元素码 → 共享 <see cref="TensorElementType"/>。
    /// shim 把 IREE HAL 元素类型统一归一为 1..5（与 <see cref="TensorElementType"/>
    /// 对齐；见 iree.ani 契约）；未映射元素码 → <see cref="TensorElementType.Undefined"/>。</summary>
    public static TensorElementType FromBufferElementType(int v) {
        if (v == 1) { return TensorElementType.Float32; }
        if (v == 2) { return TensorElementType.Float64; }
        if (v == 3) { return TensorElementType.Int32; }
        if (v == 4) { return TensorElementType.Int64; }
        if (v == 5) { return TensorElementType.UInt8; }
        return TensorElementType.Undefined;
    }
}
