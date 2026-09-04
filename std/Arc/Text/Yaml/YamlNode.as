namespace Arc.Text.Yaml;

using Arc.Collections;

// YAML 文档节点 —— 统一三种节点（标量/序列/映射）。
//
// 对齐 Json 的「文档模型」消费方式：解析得到整棵 YamlNode 树，命中的字段
// 经 Get/GetString/GetBoolean/GetInt32 等查询读取。Agent Skills frontmatter
// （name/description/license/compatibility/metadata/allowed-tools）即以此消费。
//
// 诚实边界：YamlNode 是显式文档树，不做隐式 JSON 强转——标量仅携带
// ScalarKind + 原始文本，boolean/int 由消费方按需 GetBoolean/GetInt32 取。
public class YamlNode
{
    public YamlNodeKind Kind;
    public YamlScalarKind ScalarKind;
    public string Scalar;
    public List<YamlMapEntry> Entries;
    public List<YamlNode> Items;

    // 标量解析结果（Bool/Int/Float 的强类型值；类型不匹配时 Get* 返回默认）。
    public bool BoolValue;
    public long LongValue;
    public int IntValue;
    public double DoubleValue;

    public YamlNode()
    {
        this.Kind = YamlNodeKind.Scalar;
        this.ScalarKind = YamlScalarKind.String;
        this.Scalar = "";
        this.Entries = new List<YamlMapEntry>();
        this.Items = new List<YamlNode>();
        this.BoolValue = false;
        this.LongValue = 0;
        this.IntValue = 0;
        this.DoubleValue = 0.0;
    }

    // ── 种类判定 ──
    public bool IsScalar()
    {
        return this.Kind == YamlNodeKind.Scalar;
    }

    public bool IsMapping()
    {
        return this.Kind == YamlNodeKind.Mapping;
    }

    public bool IsSequence()
    {
        return this.Kind == YamlNodeKind.Sequence;
    }

    public bool IsNull()
    {
        return this.Kind == YamlNodeKind.Scalar && this.ScalarKind == YamlScalarKind.Null;
    }

    // ── 标量读取 ──
    public string GetString()
    {
        if (this.Kind != YamlNodeKind.Scalar || this.ScalarKind == YamlScalarKind.Null)
        {
            return "";
        }
        return this.Scalar;
    }

    /// <summary>布尔值：标量为 Bool 时返回其值；否则默认 false。</summary>
    public bool GetBoolean()
    {
        if (this.Kind != YamlNodeKind.Scalar || this.ScalarKind != YamlScalarKind.Bool)
        {
            return false;
        }
        return this.BoolValue;
    }

    /// <summary>整数值：标量为 Int 时返回其值；否则默认 0。</summary>
    public int GetInt32()
    {
        if (this.Kind != YamlNodeKind.Scalar || this.ScalarKind != YamlScalarKind.Int)
        {
            return 0;
        }
        return this.IntValue;
    }

    /// <summary>长整数值：标量为 Int 时返回 64 位值；否则默认 0。</summary>
    public long GetInt64()
    {
        if (this.Kind != YamlNodeKind.Scalar || this.ScalarKind != YamlScalarKind.Int)
        {
            return 0;
        }
        return this.LongValue;
    }

    /// <summary>浮点值：标量为 Float 时返回其值，为 Int 时返回整型提升；否则默认 0。</summary>
    public double GetDouble()
    {
        if (this.Kind != YamlNodeKind.Scalar)
        {
            return 0.0;
        }
        if (this.ScalarKind == YamlScalarKind.Float)
        {
            return this.DoubleValue;
        }
        if (this.ScalarKind == YamlScalarKind.Int)
        {
            return (double)this.LongValue;
        }
        return 0.0;
    }

    // ── 映射读取 ──
    /// <summary>按键查映射值；不存在返回 null。</summary>
    public YamlNode Get(string key)
    {
        if (this.Kind != YamlNodeKind.Mapping || key == null)
        {
            return null;
        }
        int n = this.Entries.Count;
        int i = 0;
        while (i < n)
        {
            YamlMapEntry e = this.Entries[i];
            if (e != null && e.Key != null && e.Key.Kind == YamlNodeKind.Scalar && e.Key.Scalar == key)
            {
                return e.Value;
            }
            i = i + 1;
        }
        return null;
    }

    /// <summary>映射的全部条目（有序）。</summary>
    public List<YamlMapEntry> GetEntries()
    {
        return this.Entries;
    }

    /// <summary>序列的全部元素（有序）。</summary>
    public List<YamlNode> GetItems()
    {
        return this.Items;
    }

    /// <summary>序列/映射元素数。</summary>
    public int Count()
    {
        if (this.Kind == YamlNodeKind.Sequence)
        {
            return this.Items.Count;
        }
        if (this.Kind == YamlNodeKind.Mapping)
        {
            return this.Entries.Count;
        }
        return 0;
    }

    /// <summary>构造空标量节点。</summary>
    public static YamlNode CreateScalar()
    {
        return new YamlNode();
    }

    /// <summary>构造空映射节点。</summary>
    public static YamlNode CreateMapping()
    {
        YamlNode n = new YamlNode();
        n.Kind = YamlNodeKind.Mapping;
        return n;
    }

    /// <summary>构造空序列节点。</summary>
    public static YamlNode CreateSequence()
    {
        YamlNode n = new YamlNode();
        n.Kind = YamlNodeKind.Sequence;
        return n;
    }

    /// <summary>追加映射条目（解析器使用）。</summary>
    public void AddMapEntry(YamlMapEntry entry)
    {
        if (entry != null)
        {
            this.Entries.Add(entry);
        }
    }

    /// <summary>追加序列元素（解析器使用）。</summary>
    public void AddItem(YamlNode item)
    {
        if (item != null)
        {
            this.Items.Add(item);
        }
    }
}