// OnnxTensor — ONNX Runtime 张量（拥有式 Value 句柄封装）。
//
// 封装 shim 的`拥有式`张量语义：onnx_create_tensor_* 创建 Ort::Value 时，
// ONNX 已把调用方数据拷贝进自有存储——Value 句柄存活期与调用方数据缓冲无关。
// 本类持有该句柄（NativePtr），Dispose 时经 onnx_release_value 释放。
//
// 输入侧用类型化静态工厂（CreateFloat/CreateDouble/CreateInt32/CreateInt64/
// CreateByte，一对一映射 shim 的 typed create）；输出侧（InferenceSession.Run
// 结果）由内部构造持有输出句柄。元数据（Shape/ElementType/Total）惰性查询。
namespace Arc.AI.Onnx;

using Arc;
using Arc.Collections;

/// <summary>ONNX Runtime 张量（拥有式句柄封装；内部实现细节，不对外暴露）。
/// 实现 <see cref="IDisposable"/>；业务侧经 <see cref="Arc.AI.Tensor"/> 消费。</summary>
internal class OnnxTensor : IDisposable {
    /// <summary>输出形状探针初始维度容量（>8 时二次放大查询）。</summary>
    private const int ShapeProbeCap = 8;

    private bool _ownsHandle;
    private List<long> _shape;
    private bool _metadataQueried;
    private TensorElementType _elementType;
    private long _total;

    /// <summary>内部构造：包装原生 Value 句柄。仅本包使用。</summary>
    internal OnnxTensor(NativePtr value, bool ownsHandle) {
        Handle = value;
        _ownsHandle = ownsHandle;
        _shape = null;
        _metadataQueried = false;
        _elementType = TensorElementType.Undefined;
        _total = 0;
    }

    /// <summary>原生 Value 句柄（内部使用，勿直接透传）。</summary>
    internal NativePtr Handle { get; set; }

    // ── 输入工厂（一对一映射 shim typed create；拥有式，调用方数据缓冲可即释）──

    /// <summary>创建单精度浮点张量。</summary>
    /// <param name="shape">各维度尺寸。</param>
    /// <param name="data">行主序元素数据（元素数须等于 shape 乘积）。</param>
    public static OnnxTensor CreateFloat(List<long> shape, List<float> data) {
        OnnxNative.EnsureAvailable();
        NativePtr v = null;
        int rc = onnx.onnx_create_tensor_float(shape, data, out v);
        OnnxNative.ThrowIfFailed(rc);
        return new OnnxTensor(v, true);
    }

    /// <summary>创建双精度浮点张量。</summary>
    public static OnnxTensor CreateDouble(List<long> shape, List<double> data) {
        OnnxNative.EnsureAvailable();
        NativePtr v = null;
        int rc = onnx.onnx_create_tensor_double(shape, data, out v);
        OnnxNative.ThrowIfFailed(rc);
        return new OnnxTensor(v, true);
    }

    /// <summary>创建有符号 32 位整数张量。</summary>
    public static OnnxTensor CreateInt32(List<long> shape, List<int> data) {
        OnnxNative.EnsureAvailable();
        NativePtr v = null;
        int rc = onnx.onnx_create_tensor_i32(shape, data, out v);
        OnnxNative.ThrowIfFailed(rc);
        return new OnnxTensor(v, true);
    }

    /// <summary>创建有符号 64 位整数张量。</summary>
    public static OnnxTensor CreateInt64(List<long> shape, List<long> data) {
        OnnxNative.EnsureAvailable();
        NativePtr v = null;
        int rc = onnx.onnx_create_tensor_i64(shape, data, out v);
        OnnxNative.ThrowIfFailed(rc);
        return new OnnxTensor(v, true);
    }

    /// <summary>创建无符号 8 位（byte）张量。</summary>
    public static OnnxTensor CreateByte(List<long> shape, List<byte> data) {
        OnnxNative.EnsureAvailable();
        NativePtr v = null;
        int rc = onnx.onnx_create_tensor_byte(shape, data, out v);
        OnnxNative.ThrowIfFailed(rc);
        return new OnnxTensor(v, true);
    }

    // ── 元数据（惰性查询 + 缓存）──

    /// <summary>元素数据类型。</summary>
    public TensorElementType ElementType {
        get {
            this.QueryMetadata();
            return _elementType;
        }
    }

    /// <summary>元素总数（shape 乘积）。</summary>
    public long Total {
        get {
            this.QueryMetadata();
            return _total;
        }
    }

    /// <summary>各维度尺寸（未知维度为 -1）。</summary>
    public List<long> Shape {
        get {
            this.QueryMetadata();
            return _shape;
        }
    }

    /// <summary>张量阶数（维度数）。</summary>
    public int Rank {
        get { return this.Shape.Count; }
    }

    private void QueryMetadata() {
        if (_metadataQueried) {
            return;
        }
        int elemType = 0;
        int rc = onnx.onnx_tensor_get_elem_type(Handle, out elemType);
        OnnxNative.ThrowIfFailed(rc);
        _elementType = OnnxNative.FromTensorElementType(elemType);

        long total = 0;
        rc = onnx.onnx_tensor_get_total(Handle, out total);
        OnnxNative.ThrowIfFailed(rc);
        _total = total;

        // 两遍形状查询：先探测维度数，超探针容量再放大。
        List<long> probe = OnnxNative.AllocLongs((long)ShapeProbeCap);
        int dimCount = 0;
        rc = onnx.onnx_tensor_get_shape(Handle, probe, out dimCount);
        OnnxNative.ThrowIfFailed(rc);
        if (dimCount <= ShapeProbeCap) {
            List<long> shape = new List<long>();
            int i = 0;
            while (i < dimCount) {
                shape.Add(probe[i]);
                i = i + 1;
            }
            _shape = shape;
        } else {
            List<long> full = OnnxNative.AllocLongs((long)dimCount);
            rc = onnx.onnx_tensor_get_shape(Handle, full, out dimCount);
            OnnxNative.ThrowIfFailed(rc);
            _shape = full;
        }
        _metadataQueried = true;
    }

    // ── 类型化读取（逐类型映射 shim typed read）──

    /// <summary>读取全部元素为单精度浮点（行主序）。</summary>
    public List<float> ReadFloat() {
        List<float> buf = OnnxNative.AllocFloats(this.Total);
        int len = 0;
        int rc = onnx.onnx_tensor_read_float(Handle, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return buf;
    }

    /// <summary>读取全部元素为双精度浮点。</summary>
    public List<double> ReadDouble() {
        List<double> buf = OnnxNative.AllocDoubles(this.Total);
        int len = 0;
        int rc = onnx.onnx_tensor_read_double(Handle, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return buf;
    }

    /// <summary>读取全部元素为有符号 32 位整数。</summary>
    public List<int> ReadInt32() {
        List<int> buf = OnnxNative.AllocInts(this.Total);
        int len = 0;
        int rc = onnx.onnx_tensor_read_i32(Handle, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return buf;
    }

    /// <summary>读取全部元素为有符号 64 位整数。</summary>
    public List<long> ReadInt64() {
        List<long> buf = OnnxNative.AllocLongs(this.Total);
        int len = 0;
        int rc = onnx.onnx_tensor_read_i64(Handle, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return buf;
    }

    /// <summary>读取全部元素为无符号 8 位（byte）。</summary>
    public List<byte> ReadByte() {
        List<byte> buf = OnnxNative.AllocBytes(this.Total);
        int len = 0;
        int rc = onnx.onnx_tensor_read_byte(Handle, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return buf;
    }

    /// <summary>释放原生 Value 句柄（幂等）。</summary>
    public void Dispose() {
        if (_ownsHandle && Handle != null) {
            onnx.onnx_release_value(Handle);
            Handle = null;
            _ownsHandle = false;
        }
    }
}
