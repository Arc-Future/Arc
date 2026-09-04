// Arc.ComponentModel — EnumOption / EnumOptions：强类型枚举选项模型。
//
// 通用「值 + 显示名 + 描述」选项模型（对标 System.ComponentModel 数据
// 描述域），非 UI 专属——ComboBox 绑定只是消费方之一。与
// `[DisplayName]`/`[Description]` 属性同域：`Enum.GetOptions<T>()`
// （Arc 根）编译期烘焙本类型实例（零反射，RFC 004/038）。
//
// **访问权限**：`EnumOption<T>` / `EnumOptions<T>` 为公共 API（开发者编码面）。
// 底层集合用 `List<EnumOption<T>>`，无任何反射、无字符串魔法键。

namespace Arc.ComponentModel;

using Arc.Collections;

/// <summary>
/// 单个枚举选项：枚举值 + 显示名 + 描述（下拉项 Display / 工具提示）。
/// </summary>
/// <typeparam name="T">枚举类型。</typeparam>
public class EnumOption<T> {
    /// <summary>枚举值（绑定 SelectedValue 回写目标）。</summary>
    public T Value;
    /// <summary>显示名（下拉/选中文本；对标 [DisplayName] 短标签）。</summary>
    public string DisplayName;
    /// <summary>描述（工具提示/详情；对标 [Description] 长文案）。</summary>
    public string Description;

    /// <summary>构造一个枚举选项。</summary>
    /// <param name="value">枚举值。</param>
    /// <param name="displayName">显示名。</param>
    /// <param name="description">描述（可为空串）。</param>
    public EnumOption(T value, string displayName, string description) {
        this.Value = value;
        this.DisplayName = displayName;
        this.Description = description;
    }
}

/// <summary>
/// 枚举选项集合 —— 强类型、无反射，供 ComboBox 等绑定数据源。
/// </summary>
/// <typeparam name="T">枚举类型。</typeparam>
/// <remarks>
/// 用法：
/// <code>
/// EnumOptions&lt;MyStatus&gt; options = new EnumOptions&lt;MyStatus&gt;();
/// options.Add(MyStatus.None, "无", "");
/// options.Add(MyStatus.Done, "已完成", "处理完成的状态");
/// combo.SetOptions(options);
/// </code>
/// </remarks>
public class EnumOptions<T> {
    private List<EnumOption<T>> _items;

    public EnumOptions() {
        _items = new List<EnumOption<T>>();
    }

    /// <summary>选项总数。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>追加一个选项（返回 this 支持链式 Add）。</summary>
    /// <param name="value">枚举值。</param>
    /// <param name="displayName">显示名。</param>
    /// <param name="description">描述（可为空串）。</param>
    public EnumOptions<T> Add(T value, string displayName, string description) {
        _items.Add(new EnumOption<T>(value, displayName, description));
        return this;
    }

    /// <summary>按索引取选项。</summary>
    public EnumOption<T> Get(int index) {
        return _items[index];
    }

    /// <summary>按索引取枚举值（ComboBox SelectedValue 回读路径）。</summary>
    public T ValueAt(int index) {
        return _items[index].Value;
    }
}
