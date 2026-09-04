namespace Arc.Illusory;

using Arc.Collections;

/// <summary>标签包——能力/组件查询与行为互斥的判据。值语义，不可变。</summary>
/// <remarks>
/// <see cref="Add"/> / <see cref="Remove"/> 返回新实例而非原地修改，避免共享集合的别名/竞态。
/// 语义对齐 Unreal GameplayTags：仅作元数据判据，不装载行为逻辑。
/// </remarks>
public readonly struct GameplayTags {
    private readonly HashSet<string> _tags;

    /// <summary>内部包装构造——以既有骨干集合构建（无拷贝；标签包为不可变快照）。</summary>
    internal GameplayTags(HashSet<string> tags) {
        _tags = tags;
    }

    /// <summary>是否为空标签包（无任何标签）。</summary>
    public bool IsEmpty {
        get { return _tags == null || _tags.Count == 0; }
    }

    /// <summary>是否包含指定标签。</summary>
    public bool Has(string tag) {
        if (_tags == null || tag == null)
        {
            return false;
        }
        return _tags.Contains(tag);
    }

    /// <summary>新增标签，返回新实例（本实例不变）。</summary>
    public GameplayTags Add(string tag) {
        HashSet<string> next = Copy();
        next.Add(tag);
        return new GameplayTags(next);
    }

    /// <summary>移除标签，返回新实例（本实例不变）。</summary>
    public GameplayTags Remove(string tag) {
        if (_tags == null || tag == null)
        {
            return this;
        }
        HashSet<string> next = Copy();
        next.Remove(tag);
        return new GameplayTags(next);
    }

    /// <summary>是否与另一标签包有交集。</summary>
    public bool Overlaps(GameplayTags other) {
        if (_tags == null || other._tags == null)
        {
            return false;
        }
        return _tags.Overlaps(other._tags);
    }

    private HashSet<string> Copy() {
        HashSet<string> result = new HashSet<string>();
        if (_tags != null)
        {
            foreach (var tag in _tags)
            {
                result.Add(tag);
            }
        }
        return result;
    }
}