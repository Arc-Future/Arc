namespace UnitTest.Core;

using Arc;
using Arc.QIF;

class PatAnimal {
    public virtual int Tag() { return 0; }
    public virtual string Kind() { return "animal"; }
}

class PatDog : PatAnimal {
    public override int Tag() { return 1; }
    public override string Kind() { return "dog"; }
    public string Bark() { return "woof"; }
}

class PatCat : PatAnimal {
    public override int Tag() { return 2; }
    public override string Kind() { return "cat"; }
}

class PatBird : PatAnimal {
    public override int Tag() { return 3; }
    public override string Kind() { return "bird"; }
}

/// <summary>
/// 模式匹配单元测试：SwitchExpr / TypePatterns / when 守卫 / Is 基础与逻辑组合。
/// </summary>
public class PatternMatchTests
{
    // ── switch 表达式 (type pattern) ──

    [Fact]
    public void SwitchExpr_Dog()
    {
        PatAnimal a = new PatDog();
        string s = a switch {
            PatDog d => d.Bark(),
            PatCat c => "meow",
            _ => "unknown"
        };
        Assert.True(s == "woof");
    }

    [Fact]
    public void SwitchExpr_Cat()
    {
        PatAnimal a = new PatCat();
        string s = a switch {
            PatDog d => d.Bark(),
            PatCat c => "meow:" + c.Tag(),
            _ => "unknown"
        };
        Assert.True(s == "meow:2");
    }

    [Fact]
    public void SwitchExpr_Other()
    {
        PatAnimal a = new PatBird();
        string s = a switch {
            PatDog d => d.Bark(),
            PatCat c => "meow",
            var other => other.Kind()
        };
        Assert.True(s == "bird");
    }

    // ── switch 表达式 (value pattern) ──

    [Fact]
    public void SwitchExpr_ValuePattern()
    {
        int n = 2;
        string s = n switch {
            0 => "zero",
            1 => "one",
            2 => "two",
            _ => "other"
        };
        Assert.True(s == "two");
    }

    [Fact]
    public void SwitchExpr_ValuePattern_Default()
    {
        int n = 99;
        string s = n switch {
            0 => "zero",
            1 => "one",
            _ => "other"
        };
        Assert.True(s == "other");
    }

    // ── switch 表达式 混合臂（类型模式 + when 关系守卫 + var 兜底）──

    [Fact]
    public void SwitchExpr_MixedArms()
    {
        PatAnimal a = new PatBird();
        string s = a switch {
            PatDog d when d.Tag() > 0 => "dog:" + d.Bark(),
            PatCat c => "cat",
            var rest => "rest:" + rest.Kind()
        };
        Assert.True(s == "rest:bird");
    }

    [Fact]
    public void SwitchExpr_MixedArms_WhenGuardMiss()
    {
        PatAnimal a = new PatCat();
        string s = a switch {
            PatDog d when d.Tag() > 99 => "dog",
            PatCat c when c.Tag() == 2 => "cat2",
            _ => "other"
        };
        Assert.True(s == "cat2");
    }

    // ── type pattern switch 语句 ──

    [Fact]
    public void TypePattern_Dog()
    {
        PatAnimal a = new PatDog();
        string kind = "unknown";
        switch (a) {
            case PatDog d: kind = d.Bark(); break;
            case PatCat c: kind = "cat"; break;
            default: kind = "other"; break;
        }
        Assert.True(kind == "woof");
    }

    [Fact]
    public void TypePattern_Cat()
    {
        PatAnimal a = new PatCat();
        int tag = 0;
        switch (a) {
            case PatDog d: tag = d.Tag(); break;
            case PatCat c: tag = c.Tag(); break;
            default: break;
        }
        Assert.Equal(2, tag);
    }

    [Fact]
    public void TypePattern_WhenClause()
    {
        PatAnimal a = new PatDog();
        string result = "none";
        switch (a) {
            case PatDog d when d.Tag() == 99: result = "tag99"; break;
            case PatDog d when d.Tag() == 1: result = "tag1"; break;
            case PatCat c: result = "cat"; break;
            default: result = "other"; break;
        }
        Assert.True(result == "tag1");
    }

    // ── is 表达式 ──

    [Fact]
    public void Is_TypeNarrowing_True()
    {
        PatAnimal a = new PatDog();
        bool result = a is PatDog;
        Assert.True(result);
    }

    [Fact]
    public void Is_TypeNarrowing_False()
    {
        PatAnimal a = new PatCat();
        bool result = a is PatDog;
        Assert.False(result);
    }

    [Fact]
    public void Is_Binding()
    {
        PatAnimal a = new PatDog();
        if (a is PatDog d) {
            Assert.True(d.Bark() == "woof");
        } else {
            Assert.True(false);
        }
    }

    [Fact]
    public void Is_VarPattern()
    {
        PatAnimal a = new PatDog();
        if (a is var v) {
            Assert.True(v.Kind() == "dog");
        } else {
            Assert.True(false);
        }
    }

    [Fact]
    public void Is_NullPattern()
    {
        PatAnimal a = null;
        Assert.True(a is null);
        PatAnimal b = new PatDog();
        Assert.False(b is null);
        Assert.True(b is not null);
    }

    [Fact]
    public void Is_ConstantPattern()
    {
        int n = 5;
        Assert.True(n is 5);
        Assert.False(n is 6);
        string s = "a";
        Assert.True(s is "a");
        Assert.False(s is "b");
    }

    [Fact]
    public void Is_LogicalOr()
    {
        int n = 2;
        Assert.True(n is 1 or 2);
        Assert.False(n is 1 or 3);
    }

    [Fact]
    public void Is_LogicalNot()
    {
        int n = 5;
        Assert.True(n is not 6);
        Assert.False(n is not 5);
    }

    [Fact]
    public void Is_LogicalAnd()
    {
        int n = 5;
        Assert.True(n is not 6 and not 7);
        Assert.False(n is not 5 and not 6);
        Assert.True(n is 5 and not 6);
    }

    [Fact]
    public void Is_NotNull()
    {
        PatAnimal a = new PatDog();
        bool result = a != null;
        Assert.True(result);
    }

    [Fact]
    public void Is_Null()
    {
        PatAnimal a = null;
        bool result = a == null;
        Assert.True(result);
    }
}
