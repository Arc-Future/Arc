namespace UnitTest.Arc;

using Arc;
using Arc.ComponentModel;
using Arc.QIF;

/// <summary>
/// Arc.ComponentModel 诚实子集（非 Fact-Skip；非 L3 DI 全量）。
/// 覆盖 attribute 字面量构造与属性回读；本地化 typeof+nameof /
/// 运行时 GetCustomAttributes / DI 自动注册后置。
/// </summary>
public class ComponentModelTests
{
    [Fact]
    public void RequiredAttribute_Construct()
    {
        RequiredAttribute a = new RequiredAttribute();
        Assert.True(a != null);
    }

    [Fact]
    public void MaxLengthAttribute_Max()
    {
        MaxLengthAttribute a = new MaxLengthAttribute(50);
        Assert.Equal(50, a.Max);
    }

    [Fact]
    public void OrderAttribute_Order()
    {
        OrderAttribute a = new OrderAttribute(3);
        Assert.Equal(3, a.Order);
    }

    [Fact]
    public void CategoryAttribute_Literal()
    {
        CategoryAttribute a = new CategoryAttribute("appearance");
        Assert.Equal("appearance", a.Category);
        Assert.True(a.ResourceType == null);
        Assert.Equal("", a.ResourceKey);
    }

    [Fact]
    public void DisplayNameAttribute_Literal()
    {
        DisplayNameAttribute a = new DisplayNameAttribute("Full Name");
        Assert.Equal("Full Name", a.DisplayName);
        Assert.True(a.ResourceType == null);
    }

    [Fact]
    public void DescriptionAttribute_Literal()
    {
        DescriptionAttribute a = new DescriptionAttribute("help text");
        Assert.Equal("help text", a.Description);
        Assert.True(a.ResourceType == null);
    }

    [Fact]
    public void BindableAttribute_BoolAndTwoWay()
    {
        BindableAttribute a = new BindableAttribute(true);
        Assert.True(a.Bindable);

        BindableAttribute b = new BindableAttribute(BindingDirection.TwoWay);
        Assert.True(b.Bindable);
        Assert.True(b.Direction == BindingDirection.TwoWay);
    }

    [Fact]
    public void KeyAndTableAndColumn_Construct()
    {
        KeyAttribute key = new KeyAttribute();
        Assert.True(key != null);

        TableAttribute table = new TableAttribute("Users");
        Assert.Equal("Users", table.Name);

        ColumnAttribute col = new ColumnAttribute("Id");
        Assert.Equal("Id", col.Name);
    }

    [Fact]
    public void EditorBrowsable_State()
    {
        EditorBrowsableAttribute a = new EditorBrowsableAttribute(EditorBrowsableState.Never);
        Assert.True(a.State == EditorBrowsableState.Never);
    }

    [Fact]
    public void ImmutableObject_Yes()
    {
        ImmutableObjectAttribute a = new ImmutableObjectAttribute(true);
        Assert.True(a.Immutable);
    }
}
