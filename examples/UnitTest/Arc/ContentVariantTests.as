// RFC 031 §D9：Content-like variant 隐式构造语言切片（无 Arc.UI / L3）。
// Arc.UI.Content 集成仍属 L3（Text/Resource 同为 string → 歧义拒绝；见 Deferred README）。

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

variant ContentLike {
    | None
    | Text of string
}

void ContentLikeConsume(ContentLike c) { }

class ContentLikeBox {
    public ContentLike Value;

    public void Set(ContentLike c) {
        this.Value = c;
    }
}

class ContentLikeHolder {
    private ContentLike _c;

    public ContentLike Content {
        get { return this._c; }
        set { this._c = value; }
    }

    public ContentLike MakeText() {
        return "from-return";
    }

    public ContentLikeHolder() {
        this._c = ContentLike.None;
    }
}

/// <summary>
/// Content-like variant：隐式/显式构造、字段/属性/方法实参、switch（原 Deferred ContentVariant）。
/// </summary>
public class ContentVariantTests
{
    [Fact]
    public void Content_Implicit_StringToText()
    {
        ContentLike c = "Click";
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Content_Return_Implicit()
    {
        ContentLikeHolder h = new ContentLikeHolder();
        ContentLike c = h.MakeText();
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void FreeFn_Param_Implicit()
    {
        ContentLikeConsume("Click");
        Assert.True(true);
    }

    [Fact]
    public void InstanceMethod_Param_Implicit()
    {
        ContentLikeBox b = new ContentLikeBox();
        b.Set("y");
        bool ok = false;
        switch (b.Value) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void ClassField_Explicit()
    {
        ContentLikeBox b = new ContentLikeBox();
        b.Value = ContentLike.Text("x");
        bool ok = false;
        switch (b.Value) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void ClassField_Implicit()
    {
        ContentLikeBox b = new ContentLikeBox();
        b.Value = "x";
        bool ok = false;
        switch (b.Value) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Property_Set_Explicit()
    {
        ContentLikeHolder h = new ContentLikeHolder();
        h.Content = ContentLike.Text("z");
        ContentLike c = h.Content;
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Property_Set_Implicit()
    {
        ContentLikeHolder h = new ContentLikeHolder();
        h.Content = "z";
        ContentLike c = h.Content;
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Switch_On_Property()
    {
        ContentLikeHolder h = new ContentLikeHolder();
        h.Content = ContentLike.Text("z");
        bool ok = false;
        switch (h.Content) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Content_None_Discriminator()
    {
        ContentLike c = ContentLike.None;
        bool isNone = false;
        switch (c) {
            case ContentLike.None: isNone = true; break;
            case ContentLike.Text(s): break;
        }
        Assert.True(isNone);
    }

    [Fact]
    public void Content_Explicit_Text()
    {
        ContentLike c = ContentLike.Text("Hello");
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s): ok = true; break;
            case ContentLike.None: break;
        }
        Assert.True(ok);
    }

    [Fact]
    public void Content_Extract_Payload()
    {
        ContentLike c = "Click";
        string got = "";
        bool ok = false;
        switch (c) {
            case ContentLike.Text(s):
            {
                got = s;
                ok = true;
                break;
            }
            case ContentLike.None: break;
        }
        Assert.True(ok);
        Assert.True(got == "Click");
    }
}
