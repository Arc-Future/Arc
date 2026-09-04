// Arc.Data 独立库：DataRow — 数据行（对标 C# System.Data.DataRow 常用子集）。
//
// Arc 无 boxed object，列值以「类型化槽」存储（每列按 ColumnType 分派到对应
// 类型列表的同序号槽），NULL 以每列布尔标志位显式承载。取值 API 分两层：
//   - 按列名：GetInt(name)/GetBool(name)/GetString(name) —— 实现 IEvalContext，
//     供表达式树内存求值；未知列名硬错误（报错 > 静默）。
//   - 按序号：GetIntValue(ordinal) 等 —— 供物化器直接按列写/读。
//
// 严谨性约定（对齐 C# System.Data.StrongTypingException）：
//   - 类型化访问器按列元数据校验：请求类型与列类型不符 → InvalidOperationException
//     （禁静默强转）。
//   - NULL 语义：IsNull(ordinal) 判断；类型化取值遇 NULL 返回该类型默认值
//     （C# GetField<T> 语义），或由调用方先用 IsNull 判断。
namespace Arc.Data;

using Arc;
using Arc.Collections;
using Arc.Linq.Expressions;

/// <summary>
/// 数据行——DataTable 的一行。持有所属表引用（列元数据）+ 类型化值槽 + NULL 标志。
/// </summary>
public class DataRow : IEvalContext {
    private List<int> _intValues;
    private List<long> _longValues;
    private List<double> _doubleValues;
    private List<bool> _boolValues;
    private List<string> _stringValues;
    private List<DateTime> _dateTimeValues;
    private List<Guid> _guidValues;
    private List<bool> _nullFlags;

    /// <summary>构造数据行并初始化与表列数一致的「同序号」类型化槽。</summary>
    /// <remarks>
    /// 各类型化列表均按列序号（0..ColumnCount-1）建槽，取值/写值以列序号为下标——
    /// 每列在其对应类型列表的同序号槽存储，非该类型的槽保留默认值（供按类型访问时
    /// 与列元数据校验一致）。NULL 标志独立同序号承载。
    /// </remarks>
    /// <param name="table">所属数据表（提供列元数据）。</param>
    public DataRow(DataTable table) {
        this.Table = table;
        _intValues = new List<int>();
        _longValues = new List<long>();
        _doubleValues = new List<double>();
        _boolValues = new List<bool>();
        _stringValues = new List<string>();
        _dateTimeValues = new List<DateTime>();
        _guidValues = new List<Guid>();
        _nullFlags = new List<bool>();
        int n = table.ColumnCount();
        int i = 0;
        while (i < n) {
            _intValues.Add(0);
            _longValues.Add((long)0);
            _doubleValues.Add(0.0);
            _boolValues.Add(false);
            _stringValues.Add("");
            _dateTimeValues.Add(new DateTime(0));
            _guidValues.Add(Guid.Empty);
            _nullFlags.Add(false);
            i = i + 1;
        }
    }

    /// <summary>所属数据表。</summary>
    public DataTable Table { get; }

    /// <summary>列数。</summary>
    public int ColumnCount() {
        return this.Table.ColumnCount();
    }

    // ── NULL 语义 ──

    /// <summary>按序号判断该列是否为 NULL。</summary>
    public bool IsNull(int ordinal) {
        return _nullFlags[ordinal];
    }

    /// <summary>按列名判断该列是否为 NULL；列不存在硬错误。</summary>
    public bool IsNull(string name) {
        int ord = this.RequireOrdinal(name);
        return _nullFlags[ord];
    }

    /// <summary>按序号将该列置为 NULL。</summary>
    public void SetNull(int ordinal) {
        _nullFlags[ordinal] = true;
    }

    /// <summary>按列名将该列置为 NULL；列不存在硬错误。</summary>
    public void SetNull(string name) {
        int ord = this.RequireOrdinal(name);
        _nullFlags[ord] = true;
    }

    // ── 按列名取值（IEvalContext 层）──

    /// <summary>上下文是否提供指定列。</summary>
    public bool Has(string name) {
        return this.Table.GetOrdinal(name) >= 0;
    }

    /// <summary>按列名取整数值；列不存在或非 Int 硬错误。</summary>
    public int GetInt(string name) {
        int ord = this.RequireOrdinal(name);
        return this.GetIntValue(ord);
    }

    /// <summary>按列名取布尔值；列不存在或非 Bool 硬错误。</summary>
    public bool GetBool(string name) {
        int ord = this.RequireOrdinal(name);
        return this.GetBoolValue(ord);
    }

    /// <summary>按列名取字符串值；列不存在或非 String 硬错误。</summary>
    public string GetString(string name) {
        int ord = this.RequireOrdinal(name);
        return this.GetStringValue(ord);
    }

    /// <summary>DataRow 无集合下标槽；一律未绑定。</summary>
    public bool HasAt(string name, int index) { return false; }
    public int GetIntAt(string name, int index) { return 0; }
    public bool GetBoolAt(string name, int index) { return false; }
    public string GetStringAt(string name, int index) { return ""; }

    // ── 按序号读取（物化器层）──

    /// <summary>按序号取整数值；列类型非 Int 硬错误。</summary>
    public int GetIntValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.Int);
        return _intValues[ordinal];
    }

    /// <summary>按序号取长整数值；列类型非 Long 硬错误。</summary>
    public long GetLongValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.Long);
        return _longValues[ordinal];
    }

    /// <summary>按序号取浮点值；列类型非 Double 硬错误。</summary>
    public double GetDoubleValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.Double);
        return _doubleValues[ordinal];
    }

    /// <summary>按序号取布尔值；列类型非 Bool 硬错误。</summary>
    public bool GetBoolValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.Bool);
        return _boolValues[ordinal];
    }

    /// <summary>按序号取字符串值；列类型非 String 硬错误。</summary>
    public string GetStringValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.String);
        return _stringValues[ordinal];
    }

    /// <summary>按序号取日期时间值；列类型非 DateTime 硬错误。</summary>
    public DateTime GetDateTimeValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.DateTime);
        return _dateTimeValues[ordinal];
    }

    /// <summary>按序号取 GUID 值；列类型非 Guid 硬错误。</summary>
    public Guid GetGuidValue(int ordinal) {
        this.RequireType(ordinal, ColumnType.Guid);
        return _guidValues[ordinal];
    }

    // ── 按序号写入（物化器层）──

    /// <summary>按序号写整数值（清 NULL 标志）。</summary>
    public void SetIntValue(int ordinal, int value) {
        this.RequireType(ordinal, ColumnType.Int);
        _intValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写长整数值（清 NULL 标志）。</summary>
    public void SetLongValue(int ordinal, long value) {
        this.RequireType(ordinal, ColumnType.Long);
        _longValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写浮点值（清 NULL 标志）。</summary>
    public void SetDoubleValue(int ordinal, double value) {
        this.RequireType(ordinal, ColumnType.Double);
        _doubleValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写布尔值（清 NULL 标志）。</summary>
    public void SetBoolValue(int ordinal, bool value) {
        this.RequireType(ordinal, ColumnType.Bool);
        _boolValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写字符串值（清 NULL 标志）。</summary>
    public void SetStringValue(int ordinal, string value) {
        this.RequireType(ordinal, ColumnType.String);
        _stringValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写日期时间值（清 NULL 标志）。</summary>
    public void SetDateTimeValue(int ordinal, DateTime value) {
        this.RequireType(ordinal, ColumnType.DateTime);
        _dateTimeValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    /// <summary>按序号写 GUID 值（清 NULL 标志）。</summary>
    public void SetGuidValue(int ordinal, Guid value) {
        this.RequireType(ordinal, ColumnType.Guid);
        _guidValues[ordinal] = value;
        _nullFlags[ordinal] = false;
    }

    private int RequireOrdinal(string name) {
        int ord = this.Table.GetOrdinal(name);
        if (ord < 0) {
            throw new InvalidOperationException("DataRow has no column: " + name);
        }
        return ord;
    }

    /// <summary>类型校验：列实际类型与访问器请求类型不符 → 硬错误（禁静默强转）。</summary>
    private void RequireType(int ordinal, ColumnType expected) {
        ColumnType actual = this.Table.GetColumnType(ordinal);
        if (actual != expected) {
            throw new InvalidOperationException(
                "DataRow column type mismatch: requested " + this.TypeName(expected)
                + " but column is " + this.TypeName(actual));
        }
    }

    private string TypeName(ColumnType t) {
        if (t == ColumnType.Int) { return "Int"; }
        if (t == ColumnType.Long) { return "Long"; }
        if (t == ColumnType.Double) { return "Double"; }
        if (t == ColumnType.Bool) { return "Bool"; }
        if (t == ColumnType.String) { return "String"; }
        if (t == ColumnType.DateTime) { return "DateTime"; }
        return "Guid";
    }
}