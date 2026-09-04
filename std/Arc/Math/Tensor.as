namespace Arc;

/// <summary>二维张量门面，提供元素存取与逐元素/矩阵运算。</summary>
/// <typeparam name="T">元素类型。</typeparam>
public class Tensor<T> {
    private int _handle;

    /// <summary>构造指定行列数的张量，元素初始化为零值。</summary>
    /// <param name="rows">行数。</param>
    /// <param name="cols">列数。</param>
    public Tensor(int rows, int cols) {
        _handle = 0;
    }

    /// <summary>张量维数（阶）。</summary>
    public int Rank {
        get {
            return 0;
        }
    }

    /// <summary>行数。</summary>
    public int Rows {
        get {
            return 0;
        }
    }

    /// <summary>列数。</summary>
    public int Cols {
        get {
            return 0;
        }
    }

    /// <summary>元素总数（Rows * Cols）。</summary>
    public int Total {
        get {
            return 0;
        }
    }

    /// <summary>读取指定位置的元素。</summary>
    /// <param name="i">行下标。</param>
    /// <param name="j">列下标。</param>
    /// <returns>位置 (i, j) 处的元素。</returns>
    public T Get(int i, int j) {
        return 0;
    }

    /// <summary>写入指定位置的元素。</summary>
    /// <param name="i">行下标。</param>
    /// <param name="j">列下标。</param>
    /// <param name="v">待写入的值。</param>
    public void Set(int i, int j, T v) { }

    /// <summary>逐元素加法。</summary>
    /// <param name="other">同形右操作数。</param>
    /// <returns>逐元素相加后的新张量。</returns>
    public Tensor<T> Add(Tensor<T> other) {
        return 0;
    }

    /// <summary>逐元素减法。</summary>
    /// <param name="other">同形右操作数。</param>
    /// <returns>逐元素相减后的新张量。</returns>
    public Tensor<T> Sub(Tensor<T> other) {
        return 0;
    }

    /// <summary>逐元素乘法。</summary>
    /// <param name="other">同形右操作数。</param>
    /// <returns>逐元素相乘后的新张量。</returns>
    public Tensor<T> Mul(Tensor<T> other) {
        return 0;
    }

    /// <summary>矩阵乘法。</summary>
    /// <param name="other">右操作数（其行数须等于本张量列数）。</param>
    /// <returns>矩阵相乘后的新张量。</returns>
    public Tensor<T> Matmul(Tensor<T> other) {
        return 0;
    }
}
