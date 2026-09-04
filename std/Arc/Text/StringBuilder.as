namespace Arc.Text;

/// <summary>可变字符缓冲区——对齐 C# System.Text.StringBuilder。</summary>
public class StringBuilder {
    private int _handle;

    /// <summary>构造空缓冲区。</summary>
    [Builtin(ABI = "rt_string_builder_create")]
    public StringBuilder() {
        _handle = 0;
    }

    /// <summary>构造缓冲区并初始化为指定字符串内容。</summary>
    /// <param name="value">初始文本。null 视为空串。</param>
    [Builtin(ABI = "rt_string_builder_create_with_str")]
    public StringBuilder(string value) {
        _handle = 0;
    }

    /// <summary>构造指定容量的空缓冲区。</summary>
    /// <param name="capacity">初始容量（不含末尾 NUL）。</param>
    [Builtin(ABI = "rt_string_builder_create_with_capacity")]
    public StringBuilder(int capacity) {
        _handle = 0;
    }

    /// <summary>追加字符串到末尾，返回自身以支持链式调用。</summary>
    /// <param name="value">待追加的文本。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append")]
    public StringBuilder Append(string value) {
        return this;
    }

    /// <summary>追加 int 十进制串到末尾，返回自身。</summary>
    /// <param name="value">待追加的整数。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_int")]
    public StringBuilder Append(int value) {
        return this;
    }

    /// <summary>追加 long 十进制串到末尾，返回自身。</summary>
    /// <param name="value">待追加的长整数。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_long")]
    public StringBuilder Append(long value) {
        return this;
    }

    /// <summary>追加 float 最短十进制串到末尾，返回自身。</summary>
    /// <param name="value">待追加的单精度浮点数。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_float")]
    public StringBuilder Append(float value) {
        return this;
    }

    /// <summary>追加 double 最短十进制串到末尾，返回自身。</summary>
    /// <param name="value">待追加的双精度浮点数。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_double")]
    public StringBuilder Append(double value) {
        return this;
    }

    /// <summary>追加 "true" 或 "false" 到末尾，返回自身。</summary>
    /// <param name="value">待追加的布尔值。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_bool")]
    public StringBuilder Append(bool value) {
        return this;
    }

    /// <summary>追加单个字符到末尾，返回自身。</summary>
    /// <param name="value">待追加的字符。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_char")]
    public StringBuilder Append(char value) {
        return this;
    }

    /// <summary>追加字符串与换行符到末尾，返回自身。</summary>
    /// <param name="value">待追加的文本。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_line")]
    public StringBuilder AppendLine(string value) {
        return this;
    }

    /// <summary>追加一个换行符到末尾，返回自身。</summary>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_append_line")]
    public StringBuilder AppendLine() {
        return this;
    }

    /// <summary>清空缓冲区内容（保留已分配容量）。</summary>
    [Builtin(ABI = "rt_string_builder_clear")]
    public void Clear() {
    }

    /// <summary>在指定索引处插入字符串，返回自身。</summary>
    /// <param name="index">插入位置的字符索引（0 起始）。越界不操作。</param>
    /// <param name="value">待插入的文本。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_insert")]
    public StringBuilder Insert(int index, string value) {
        return this;
    }

    /// <summary>删除从指定索引开始的指定长度字符，返回自身。</summary>
    /// <param name="startIndex">起始字符索引（0 起始）。</param>
    /// <param name="length">删除的字符数。越界不操作。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_remove")]
    public StringBuilder Remove(int startIndex, int length) {
        return this;
    }

    /// <summary>替换缓冲区中所有旧字符串为新字符串，返回自身。</summary>
    /// <param name="oldValue">被替换的旧文本。空字符串不操作。</param>
    /// <param name="newValue">替换后的新文本。</param>
    /// <returns>当前缓冲区实例。</returns>
    [Builtin(ABI = "rt_string_builder_replace")]
    public StringBuilder Replace(string oldValue, string newValue) {
        return this;
    }

    /// <summary>确保缓冲区至少拥有指定容量（不含末尾 NUL）。</summary>
    /// <param name="capacity">所需最小容量。</param>
    [Builtin(ABI = "rt_string_builder_ensure_capacity")]
    public void EnsureCapacity(int capacity) {
    }

    /// <summary>将缓冲区内容拷贝为不可变字符串。</summary>
    /// <returns>缓冲区内容的字符串副本。</returns>
    [Builtin(ABI = "rt_string_builder_to_string")]
    public string ToString() {
        return "";
    }

    /// <summary>将指定范围的缓冲区内容拷贝为不可变字符串。</summary>
    /// <param name="startIndex">起始字符索引（0 起始）。</param>
    /// <param name="length">拷贝的字符数。</param>
    /// <returns>子串副本。越界返回空串。</returns>
    [Builtin(ABI = "rt_string_builder_to_string_range")]
    public string ToString(int startIndex, int length) {
        return "";
    }

    /// <summary>当前缓冲区的字符长度。</summary>
    [Builtin(ABI = "rt_string_builder_length")]
    public int Length { get; }

    /// <summary>当前缓冲区的容量（不含末尾 NUL）。</summary>
    [Builtin(ABI = "rt_string_builder_get_capacity")]
    public int Capacity { get; }

    /// <summary>索引器：获取或设置指定索引处的字符（<c>sb[i]</c>）。</summary>
    /// <param name="index">字符索引（0 起始）。越界读返回 '\0'，越界写不操作。</param>
    /// <remarks>codegen 拦截 get_Item/set_Item → <c>rt_text_sb_{get,set}_char</c>（非空 stub）。</remarks>
    public char this[int index] {
        get { return '\0'; }
        set { }
    }

    // ── 格式化追加 ──

    /// <summary>追加复合格式字符串。占位符 {0}, {1}, ... 按索引替换为对应参数。</summary>
    /// <param name="format">复合格式字符串。{{ 与 }} 转义字面花括号；非法占位符按字面输出。</param>
    /// <param name="args">格式参数（<c>params ReadOnlySpan&lt;string&gt;</c> 零堆栈脱糖，对齐 C# 语义）。</param>
    /// <returns>当前缓冲区实例。</returns>
    /// <remarks>纯 Arc 实现（复用 Append 系列 ABI 分段追加），与 Arc.Logging 消息模板格式化同风格。</remarks>
    public StringBuilder AppendFormat(string format, params ReadOnlySpan<string> args) {
        if (format == null) {
            return this;
        }
        int i = 0;
        int n = format.Length;
        while (i < n) {
            string ch = format.Substring(i, 1);
            if (ch == "{") {
                if (i + 1 < n && format.Substring(i + 1, 1) == "{") {
                    this.Append("{");
                    i = i + 2;
                    continue;
                }
                int close = format.IndexOf("}", i + 1);
                if (close < 0) {
                    this.Append(ch);
                    i = i + 1;
                    continue;
                }
                string token = format.Substring(i + 1, close - (i + 1)).Trim();
                if (this.IsDigits(token)) {
                    int idx = Convert.ToInt32(token);
                    if (idx >= 0 && idx < args.Length) {
                        this.Append(args[idx]);
                    }
                } else {
                    // 非法占位符（非纯数字）：按字面输出原样片段。
                    this.Append(format.Substring(i, close - i + 1));
                }
                i = close + 1;
                continue;
            }
            if (ch == "}") {
                if (i + 1 < n && format.Substring(i + 1, 1) == "}") {
                    this.Append("}");
                    i = i + 2;
                    continue;
                }
                this.Append(ch);
                i = i + 1;
                continue;
            }
            this.Append(ch);
            i = i + 1;
        }
        return this;
    }

    /// <summary>判断字符串是否为纯数字（0-9 组成，非空）。供格式占位符索引校验。</summary>
    /// <param name="s">待判定字符串。</param>
    /// <returns>全为数字返回 true，否则 false。</returns>
    /// <remarks>实例方法（非 static）：StringBuilder 为 stub facade 类，static 调用会被 MIR
    /// 路由到 <c>Class.Method</c> 源码形供 codegen 拦截，未登记拦截器即成 undefined value；
    /// 实例方法走常规 mangled 符号路径，可与本体一同发射。</remarks>
    private bool IsDigits(string s) {
        if (s == null || s.Length == 0) {
            return false;
        }
        int k = 0;
        while (k < s.Length) {
            string c = s.Substring(k, 1);
            if (c.Compare("0") < 0 || c.Compare("9") > 0) {
                return false;
            }
            k = k + 1;
        }
        return true;
    }
}
