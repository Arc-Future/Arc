// RFC 037 D3.6 / RFC 037 D4: Arc.UI.Styling — ResourceDictionary 资源字典。
//
// 资源字典——键值对存储。Value 使用 ResourceValue variant
// （RFC 037 标签联合）替代 object——零装箱，类型安全。
//
// 使用模式（隐式构造，typeck 自动重写）：
//   dict.Add("AccentColor", "#FF0000");   // → ResourceValue.String("#FF0000")
//   dict.Add("FontSize", 14.0);           // → ResourceValue.Number(14.0)

namespace Arc.UI.Styling;

using Arc.Collections;

/// <summary>资源字典（含 MergedDictionaries 合成，对标 WPF ResourceDictionary）。</summary>
public class ResourceDictionary {
    private List<ResourceEntry> _entries;
    private List<Style> _styles;
    private List<DataTemplate> _templates;

    /// <summary>合并字典（先本字典本地条目，再按序合并；后合并者覆盖同名 key）。</summary>
    public List<ResourceDictionary> MergedDictionaries;

    public ResourceDictionary() {
        _entries = new List<ResourceEntry>();
        _styles = new List<Style>();
        _templates = new List<DataTemplate>();
        this.MergedDictionaries = new List<ResourceDictionary>();
    }

    /// <summary>资源条目数量。</summary>
    public int Count {
        get { return _entries.Count; }
    }

    /// <summary>已注册样式数量（隐式/显式 Style 块）。</summary>
    public int StyleCount {
        get { return _styles.Count; }
    }

    /// <summary>添加样式到字典。</summary>
    public void AddStyle(Style style) {
        _styles.Add(style);
    }

    /// <summary>获取所有样式（仅本地条目；全链收集见 CollectStyles）。</summary>
    public List<Style> GetAllStyles() {
        return _styles;
    }

    /// <summary>
    /// 按键名查找样式（本地优先，其次逆序合并 MergedDictionaries——后合并者
    /// 覆盖，语义对齐 TryLookup/LookupTemplate）。BasedOn 父样式解析、显式
    /// Style 键引用均经此入口，可跨主题/合并字典命中。未命中返回 null。
    /// </summary>
    public Style LookupStyle(string key) {
        if (key == null) {
            return null;
        }
        foreach (var s in _styles) {
            if (s.Key == key) {
                return s;
            }
        }
        for (int i = this.MergedDictionaries.Count - 1; i >= 0; i--) {
            if (this.MergedDictionaries[i] != null) {
                Style merged = this.MergedDictionaries[i].LookupStyle(key);
                if (merged != null) {
                    return merged;
                }
            }
        }
        return null;
    }

    /// <summary>
    /// 按覆盖序收集全部样式到 into（本地在后覆盖先合并）：逆序
    /// MergedDictionaries 先收（最早合并者先应用、可被后合并者覆盖），本地
    /// 最后收（本地覆盖一切）。样式应用「后加者胜」——收集序即应用序，
    /// 与 TryLookup「后合并者覆盖」同构。调用方提供非 null into。
    /// </summary>
    public void CollectStyles(List<Style> into) {
        for (int i = this.MergedDictionaries.Count - 1; i >= 0; i--) {
            if (this.MergedDictionaries[i] != null) {
                this.MergedDictionaries[i].CollectStyles(into);
            }
        }
        foreach (var s in _styles) {
            into.Add(s);
        }
    }

    /// <summary>
    /// 注册隐式数据模板（WPF DataTemplate DataType 隐式键对齐）。键为
    /// DataType 字符串（非显式资源名）；同 DataType 后注册覆盖（与 Add 覆盖语义一致）。
    /// </summary>
    public void AddTemplate(DataTemplate template) {
        if (template == null) {
            return;
        }
        for (int i = 0; i < _templates.Count; i = i + 1) {
            if (_templates[i].DataType == template.DataType) {
                _templates[i] = template;
                return;
            }
        }
        _templates.Add(template);
    }

    /// <summary>
    /// 按数据类型名查找隐式数据模板（本地条目优先，其次按序合并
    /// MergedDictionaries，语义对齐 TryLookup）。未命中返回 null。
    /// </summary>
    public DataTemplate LookupTemplate(string dataType) {
        if (dataType == null) {
            return null;
        }
        for (int i = 0; i < _templates.Count; i++) {
            if (_templates[i].DataType == dataType) {
                return _templates[i];
            }
        }
        for (int i = this.MergedDictionaries.Count - 1; i >= 0; i--) {
            if (this.MergedDictionaries[i] != null) {
                DataTemplate merged = this.MergedDictionaries[i].LookupTemplate(dataType);
                if (merged != null) {
                    return merged;
                }
            }
        }
        return null;
    }

    /// <summary>添加资源条目。若 key 已存在则覆盖。</summary>
    public void Add(string key, ResourceValue value) {
        for (int i = 0; i < _entries.Count; i = i + 1) {
            if (_entries[i].Key == key) {
                ResourceEntry e = _entries[i];
                e.Value = value;
                _entries[i] = e;
                return;
            }
        }
        _entries.Add(new ResourceEntry(key, value));
    }

    /// <summary>
    /// 查找资源（本地条目优先，其次按序合并 MergedDictionaries；后合并者覆盖）。
    /// 未找到时返回 ResourceValue.String("")。
    /// </summary>
    public ResourceValue Lookup(string key) {
        ResourceValue v = ResourceValue.String("");
        if (this.TryLookup(key, ref v)) {
            return v;
        }
        return ResourceValue.String("");
    }

    /// <summary>
    /// 尝试查找资源并返回命中与否（规避空串值歧义）。本地条目优先，
    /// 其次按序合并 MergedDictionaries（后合并者覆盖同名 key）。
    /// </summary>
    public bool TryLookup(string key, ref ResourceValue value) {
        for (int i = 0; i < _entries.Count; i++) {
            if (_entries[i].Key == key) {
                value = _entries[i].Value;
                return true;
            }
        }
        for (int i = this.MergedDictionaries.Count - 1; i >= 0; i--) {
            if (this.MergedDictionaries[i] != null && this.MergedDictionaries[i].TryLookup(key, ref value)) {
                return true;
            }
        }
        return false;
    }
}
