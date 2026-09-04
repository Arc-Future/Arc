namespace UnitTest.Core;

using Arc;
using Arc.Reflection;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// RFC 052 M3+ 最小面：typeof Name/FullName/BaseType + GetMethods/GetFields/GetProperties 成员名枚举。
/// obj.GetType() 永久剔除（RFC 052 §11）；自定义属性 / PropertyType / 继承合并属残余。
/// 无 [Fact(Skip)]。
/// </summary>
class RefShape {
    public int Tag;
    public string Label { get; set; }
    public virtual string Kind() { return "shape"; }
}

class RefCircle : RefShape {
    public override string Kind() { return "circle"; }
}

public class ReflectionTests
{
    [Fact]
    public void Typeof_ReturnsNonNull()
    {
        Type t = typeof(RefShape);
        Assert.NotNull(t);
    }

    [Fact]
    public void TypeId_NonZero()
    {
        Type t = typeof(RefShape);
        Assert.True(t.TypeId != 0);
    }

    [Fact]
    public void TypeId_DifferentClasses_HaveDifferentIds()
    {
        Type a = typeof(RefShape);
        Type b = typeof(RefCircle);
        Assert.True(a.TypeId != 0);
        Assert.True(b.TypeId != 0);
        Assert.NotEqual(a.TypeId, b.TypeId);
    }

    [Fact]
    public void VtableRedirect_TypeId()
    {
        Type concrete = typeof(RefCircle);
        Type asBase = concrete;
        Assert.Equal(concrete.TypeId, asBase.TypeId);
    }

    [Fact]
    public void Typeof_InlineTypeId_MatchesVariable()
    {
        Type t = typeof(RefShape);
        Assert.Equal(typeof(RefShape).TypeId, t.TypeId);
    }

    [Fact]
    public void NullableType_TypeId_Access()
    {
        Type? t = typeof(RefShape);
        Assert.True(t != null);
        if (t != null) {
            Assert.True(t.TypeId != 0);
        }
    }

    [Fact]
    public void Name_EqualsSimpleTypeName()
    {
        Type t = typeof(RefShape);
        Assert.Equal("RefShape", t.Name);
    }

    [Fact]
    public void FullName_NonEmpty()
    {
        Type t = typeof(RefShape);
        Assert.True(t.FullName != null);
        Assert.True(t.FullName.Length > 0);
        Assert.Equal(t.Name, t.FullName);
    }

    [Fact]
    public void BaseType_ParentName()
    {
        Type t = typeof(RefCircle);
        Type? b = t.BaseType;
        Assert.NotNull(b);
        if (b != null) {
            Assert.Equal("RefShape", b.Name);
        }
    }

    [Fact]
    public void GetMethods_ContainsKind()
    {
        Type t = typeof(RefShape);
        List<MethodInfo> methods = t.GetMethods();
        Assert.True(methods != null);
        Assert.True(methods.Count >= 1);
        bool found = false;
        for (int i = 0; i < methods.Count; i++) {
            MethodInfo m = methods[i];
            if (m.Name == "Kind") {
                found = true;
            }
        }
        Assert.True(found);
    }

    [Fact]
    public void GetFields_ContainsTag()
    {
        Type t = typeof(RefShape);
        List<FieldInfo> fields = t.GetFields();
        Assert.True(fields != null);
        Assert.True(fields.Count >= 1);
        bool found = false;
        for (int i = 0; i < fields.Count; i++) {
            FieldInfo f = fields[i];
            if (f.Name == "Tag") {
                found = true;
            }
        }
        Assert.True(found);
    }

    [Fact]
    public void GetProperties_ContainsLabel()
    {
        Type t = typeof(RefShape);
        List<PropertyInfo> props = t.GetProperties();
        Assert.True(props != null);
        Assert.True(props.Count >= 1);
        bool found = false;
        for (int i = 0; i < props.Count; i++) {
            PropertyInfo p = props[i];
            if (p.Name == "Label") {
                found = true;
            }
        }
        Assert.True(found);
    }
}
