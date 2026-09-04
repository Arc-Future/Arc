// RFC 041 §1.5: Arc.AI 共享抽象核心 — 宿主张量 Tensor。
//
// 承载宿主可见张量，抽象掉 ONNX Value 与 IREE buffer_view 的设备/内存所有权差异：
// 后端适配器把各自原生张量转换（读回/注入）为本宿主张量，业务侧经统一类型消费、
// 不感知后端差异。class（引用语义，RFC 041 定稿）：持宿主类型化缓冲 + Shape/
// ElementType 元数据；适配器可零拷贝返回/注入。镜像 OnnxTensor 的面
// （Shape/ElementType/Rank/Total + 类型化读写）。不动 std/Arc/Math/Tensor.as
// 冻结面（R7；该 stub 不承载推理）。
namespace Arc.AI;

using Arc.Collections;

/// <summary>
/// 宿主张量（值承载类型）：持类型化元素缓冲 + 形状元数据。
/// 静态工厂创建（CreateFloat/CreateDouble/CreateInt32/CreateInt64/CreateByte/
/// CreateInt16，一对一映射宿主类型化缓冲）；类型化读取 ReadFloat/ReadDouble/
/// ReadInt32/ReadInt64/ReadByte/ReadInt16。元数据（Shape/ElementType/Rank/Total）
/// 构造即确定。
/// </summary>
public class Tensor {
    private List<long> _shape;
    private TensorElementType _elementType;
    private List<float> _f32;
    private List<double> _f64;
    private List<int> _i32;
    private List<long> _i64;
    private List<byte> _u8;
    private List<short> _i16;

    // 无参内部构造：所有字段由静态工厂在方法体内赋值。对齐 OnnxTensor 的
    // 「元数据惰性赋字段」既有模式，规避构造器参数→引用字段赋值的编译器缺陷
    // （在途迭代期；构造器不承载引用参数赋值）。
    private Tensor() {
        _shape = null;
        _elementType = TensorElementType.Undefined;
        _f32 = null;
        _f64 = null;
        _i32 = null;
        _i64 = null;
        _u8 = null;
        _i16 = null;
    }

    // ── 静态工厂（一对一映射宿主类型化缓冲）──

    /// <summary>创建单精度浮点张量。</summary>
    /// <param name="shape">各维度尺寸。</param>
    /// <param name="data">行主序元素数据（元素数须等于 shape 乘积）。</param>
    public static Tensor CreateFloat(List<long> shape, List<float> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.Float32;
        t._f32 = data;
        return t;
    }

    /// <summary>创建双精度浮点张量。</summary>
    public static Tensor CreateDouble(List<long> shape, List<double> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.Float64;
        t._f64 = data;
        return t;
    }

    /// <summary>创建有符号 32 位整数张量。</summary>
    public static Tensor CreateInt32(List<long> shape, List<int> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.Int32;
        t._i32 = data;
        return t;
    }

    /// <summary>创建有符号 64 位整数张量。</summary>
    public static Tensor CreateInt64(List<long> shape, List<long> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.Int64;
        t._i64 = data;
        return t;
    }

    /// <summary>创建无符号 8 位（byte）张量。</summary>
    public static Tensor CreateByte(List<long> shape, List<byte> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.UInt8;
        t._u8 = data;
        return t;
    }

    /// <summary>创建有符号 16 位整数张量（PCM int16 音频 / 部分量化推理输出；RFC 041 §7.4）。</summary>
    /// <param name="shape">各维度尺寸。</param>
    /// <param name="data">行主序元素数据（元素数须等于 shape 乘积）。</param>
    public static Tensor CreateInt16(List<long> shape, List<short> data) {
        Tensor t = new Tensor();
        t._shape = shape;
        t._elementType = TensorElementType.Int16;
        t._i16 = data;
        return t;
    }

    // ── 元数据 ──

    /// <summary>元素数据类型。</summary>
    public TensorElementType ElementType {
        get { return _elementType; }
    }

    /// <summary>各维度尺寸（未知维度为 -1）。</summary>
    public List<long> Shape {
        get { return _shape; }
    }

    /// <summary>张量阶数（维度数）。</summary>
    public int Rank {
        get { return _shape != null ? _shape.Count : 0; }
    }

    /// <summary>元素总数（shape 乘积；空 shape 视为 1）。</summary>
    public long Total {
        get {
            if (_shape == null) { return 0; }
            long n = 1;
            int i = 0;
            while (i < _shape.Count) {
                n = n * _shape[i];
                i = i + 1;
            }
            return n;
        }
    }

    // ── 类型化读取（返回宿主缓冲）──

    /// <summary>读取全部元素为单精度浮点（行主序）。</summary>
    public List<float> ReadFloat() {
        return _f32;
    }

    /// <summary>读取全部元素为双精度浮点。</summary>
    public List<double> ReadDouble() {
        return _f64;
    }

    /// <summary>读取全部元素为有符号 32 位整数。</summary>
    public List<int> ReadInt32() {
        return _i32;
    }

    /// <summary>读取全部元素为有符号 64 位整数。</summary>
    public List<long> ReadInt64() {
        return _i64;
    }

    /// <summary>读取全部元素为无符号 8 位（byte）。</summary>
    public List<byte> ReadByte() {
        return _u8;
    }

    /// <summary>读取全部元素为有符号 16 位整数（行主序）。</summary>
    public List<short> ReadInt16() {
        return _i16;
    }
}
