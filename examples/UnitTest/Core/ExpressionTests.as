namespace UnitTest.Core;

using Arc;
using Arc.Linq.Expressions;

/// <summary>
/// 表达式树语言层单元测试：覆盖 Expression 运行时构造、求值（EvalInt/EvalBool/EvalString）、
/// 常量/二元/一元/条件/捕获变量/成员访问/形参绑定表达式。
/// 对标 C# System.Linq.Expressions.Expression 运行时能力。
/// 所有测试不依赖 Arc.Orm（SqlTranslator / DataRow）。
/// </summary>

// ── 轻量 IEvalContext 桩实现（仅供测试使用，不依赖 DataRow） ──

public class TestEvalContext : IEvalContext {
    /// <summary>已绑定形参名；空串表示未绑定形参。</summary>
    public string BoundParam;
    /// <summary>已绑定形参的 int 值。</summary>
    public int BoundInt;
    /// <summary>已绑定形参的 bool 值。</summary>
    public bool BoundBool;
    /// <summary>已绑定形参的 string 值。</summary>
    public string BoundString;
    /// <summary>成员 Active 的 bool 槽位（供 MemberExpression.EvalBool → GetBool）。</summary>
    public bool Active;

    public TestEvalContext() {
        BoundParam = "";
        BoundInt = 0;
        BoundBool = false;
        BoundString = "";
        Active = true;
    }

    public bool Has(string name) {
        if (name == "Age") { return true; }
        if (name == "Score") { return true; }
        if (name == "Name") { return true; }
        if (name == "Title") { return true; }
        if (name == "Active") { return true; }
        if (BoundParam != "") {
            if (name == BoundParam) { return true; }
        }
        return false;
    }

    public int GetInt(string name) {
        if (BoundParam != "") {
            if (name == BoundParam) { return BoundInt; }
        }
        if (name == "Age") { return 20; }
        if (name == "Score") { return 85; }
        return 0;
    }

    public bool GetBool(string name) {
        if (BoundParam != "") {
            if (name == BoundParam) { return BoundBool; }
        }
        if (name == "Active") { return Active; }
        return false;
    }

    public string GetString(string name) {
        if (BoundParam != "") {
            if (name == BoundParam) { return BoundString; }
        }
        if (name == "Name") { return "Alice"; }
        if (name == "Title") { return "Test"; }
        return "";
    }

    /// <summary>Scores 集合：下标 0→10、1→20（IndexExpression Eval）。</summary>
    public bool HasAt(string name, int index) {
        if (name == "Scores") {
            if (index == 0) { return true; }
            if (index == 1) { return true; }
        }
        if (name == "Tags") {
            if (index == 0) { return true; }
        }
        return false;
    }

    public int GetIntAt(string name, int index) {
        if (name == "Scores") {
            if (index == 0) { return 10; }
            if (index == 1) { return 20; }
        }
        return 0;
    }

    public bool GetBoolAt(string name, int index) { return false; }

    public string GetStringAt(string name, int index) {
        if (name == "Tags") {
            if (index == 0) { return "alpha"; }
        }
        return "";
    }
}

// ── 主测试类 ──

public class ExpressionTests
{
    // ══════════════════════════════════════════════════════════
    // 常量表达式求值
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void ConstantExpr_EvalInt_ReturnsLiteral()
    {
        Expression<Func<int>> expr = () => 42;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(42, result);
    }

    [Fact]
    public void ConstantExpr_EvalBool_ReturnsLiteralTrue()
    {
        Expression<Func<bool>> expr = () => true;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void ConstantExpr_EvalBool_ReturnsLiteralFalse()
    {
        Expression<Func<bool>> expr = () => false;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.False(result);
    }

    [Fact]
    public void ConstantExpr_EvalString_ReturnsLiteral()
    {
        Expression<Func<string>> expr = () => "hello";
        TestEvalContext ctx = new TestEvalContext();
        string result = expr.EvalString(ctx);
        Assert.True(result == "hello");
    }

    // ══════════════════════════════════════════════════════════
    // 二元表达式求值：算术
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void BinaryExpr_Add_EvalInt()
    {
        Expression<Func<int>> expr = () => 1 + 2;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(3, result);
    }

    [Fact]
    public void BinaryExpr_Sub_EvalInt()
    {
        Expression<Func<int>> expr = () => 10 - 3;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(7, result);
    }

    [Fact]
    public void BinaryExpr_Mul_EvalInt()
    {
        Expression<Func<int>> expr = () => 4 * 5;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(20, result);
    }

    [Fact]
    public void BinaryExpr_Div_EvalInt()
    {
        Expression<Func<int>> expr = () => 20 / 4;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(5, result);
    }

    // ══════════════════════════════════════════════════════════
    // 二元表达式求值：比较与逻辑
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void BinaryExpr_GreaterThan_EvalBool()
    {
        Expression<Func<bool>> expr = () => 10 > 5;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_LessThan_EvalBool()
    {
        Expression<Func<bool>> expr = () => 3 < 8;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_GreaterOrEqual_EvalBool()
    {
        Expression<Func<bool>> expr = () => 7 >= 7;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_Equal_EvalBool()
    {
        Expression<Func<bool>> expr = () => 5 == 5;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_NotEqual_EvalBool()
    {
        Expression<Func<bool>> expr = () => 5 != 3;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_BoolEqual_EvalBool()
    {
        // bool == 必须走 EvalBool（BoolValue），勿经 EvalInt（IntValue 默认为 0）。
        Expression<Func<bool>> expr = () => true == true;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void BinaryExpr_BoolNotEqual_EvalBool()
    {
        Expression<Func<bool>> expr = () => true != false;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void BinaryExpr_BoolEqualFalse_EvalBool()
    {
        // 若误用 EvalInt：true/false 的 IntValue 均为 0 → 0==0 假阳性。
        Expression<Func<bool>> expr = () => true == false;
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(expr.EvalBool(ctx));
    }

    [Fact]
    public void BinaryExpr_And_EvalBool()
    {
        Expression<Func<bool>> expr = () => true && true;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_Or_EvalBool()
    {
        Expression<Func<bool>> expr = () => false || true;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void BinaryExpr_And_ShortCircuitFalse()
    {
        Expression<Func<bool>> expr = () => true && false;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.False(result);
    }

    // ══════════════════════════════════════════════════════════
    // 复合表达式
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void CompoundExpr_ArithmeticChain_EvalInt()
    {
        Expression<Func<int>> expr = () => 1 + 2 * 3;
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(7, result);
    }

    [Fact]
    public void CompoundExpr_ParenGroup_EvalInt()
    {
        Expression<Func<int>> expr = () => (1 + 2) * (3 - 1);
        TestEvalContext ctx = new TestEvalContext();
        int result = expr.EvalInt(ctx);
        Assert.Equal(6, result);
    }

    [Fact]
    public void CompoundExpr_CompareWithArith_EvalBool()
    {
        Expression<Func<bool>> expr = () => 1 + 2 > 0;
        TestEvalContext ctx = new TestEvalContext();
        bool result = expr.EvalBool(ctx);
        Assert.True(result);
    }

    // ══════════════════════════════════════════════════════════
    // ParameterExpression ↔ IEvalContext 形参绑定
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void ParameterExpr_BoundInt_EvalInt()
    {
        Expression<Func<int, int>> expr = x => x;
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "x";
        ctx.BoundInt = 42;
        Assert.Equal(42, expr.EvalInt(ctx));
    }

    [Fact]
    public void ParameterExpr_BoundInt_Compare_EvalBool()
    {
        Expression<Func<int, bool>> expr = x => x > 10;
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "x";
        ctx.BoundInt = 15;
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void ParameterExpr_BoundString_EvalString()
    {
        Expression<Func<string, string>> expr = s => s;
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "s";
        ctx.BoundString = "hello";
        Assert.True(expr.EvalString(ctx) == "hello");
    }

    [Fact]
    public void ParameterExpr_BoundBool_EvalBool()
    {
        Expression<Func<bool, bool>> expr = b => b;
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "b";
        ctx.BoundBool = true;
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void ParameterExpr_BoundBool_Equal_EvalBool()
    {
        Expression<Func<bool, bool>> expr = b => b == true;
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "b";
        ctx.BoundBool = true;
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void ParameterExpr_Unbound_HasIsFalse()
    {
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(ctx.Has("x"));
    }

    [Fact]
    public void ParameterExpr_Unbound_Throws()
    {
        // Lambda.EvalInt → Parameter.EvalInt 嵌套虚分派；未绑定须抛且可被 catch。
        Expression<Func<int, int>> expr = x => x;
        TestEvalContext ctx = new TestEvalContext();
        bool caught = false;
        try {
            expr.EvalInt(ctx);
        } catch {
            caught = true;
        }
        Assert.True(caught);
    }

    // ══════════════════════════════════════════════════════════
    // 捕获变量表达式
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void CaptureExpr_CapturedInt_EvalBool()
    {
        int age = 18;
        // 无 Lambda 形参：右侧 Capture 快照即可求值
        Expression<Func<bool>> isAdult = () => 20 > age;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(isAdult.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_MultipleCaptures_EvalBool()
    {
        int min = 10;
        int max = 20;
        Expression<Func<bool>> inRange = () => 15 >= min && 15 <= max;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(inRange.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_CapturedBool_EvalBool()
    {
        bool flag = true;
        Expression<Func<bool>> expr = () => flag;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_CapturedBoolFalse_EvalBool()
    {
        bool flag = false;
        Expression<Func<bool>> expr = () => flag;
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(expr.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_CapturedBool_Equal_EvalBool()
    {
        bool flag = true;
        Expression<Func<bool>> expr = () => flag == true;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_CapturedBool_EqualFalse_EvalBool()
    {
        bool flag = true;
        Expression<Func<bool>> expr = () => flag == false;
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(expr.EvalBool(ctx));
    }

    // ══════════════════════════════════════════════════════════
    // 带结构体成员访问的表达式（需 IEvalContext 提供成员值）
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void MemberExpr_WithEvalContext_EvalBool()
    {
        Expression<Func<User, bool>> isAdult = u => u.Age >= 18;
        TestEvalContext ctx = new TestEvalContext();
        bool result = isAdult.EvalBool(ctx);
        Assert.True(result);
    }

    [Fact]
    public void MemberExpr_WithEvalContext_EvalBoolFalse()
    {
        Expression<Func<User, bool>> isSenior = u => u.Age >= 65;
        TestEvalContext ctx = new TestEvalContext();
        bool result = isSenior.EvalBool(ctx);
        Assert.False(result);
    }

    [Fact]
    public void MemberExpr_BoolField_GetBool_EvalBool()
    {
        Expression<Func<User, bool>> isActive = u => u.Active;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = true;
        Assert.True(isActive.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_BoolField_GetBool_EvalBoolFalse()
    {
        Expression<Func<User, bool>> isActive = u => u.Active;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = false;
        Assert.False(isActive.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_BoolEqualConstant_EvalBool()
    {
        Expression<Func<User, bool>> isActive = u => u.Active == true;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = true;
        Assert.True(isActive.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_BoolEqualConstant_EvalBoolFalse()
    {
        Expression<Func<User, bool>> isActive = u => u.Active == true;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = false;
        Assert.False(isActive.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_BoolEqualMember_EvalBool()
    {
        // Member==Member：两侧 TypeName 均为 bool，须走 EvalBool（非 EvalInt）。
        Expression<Func<User, bool>> same = u => u.Active == u.Active;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = true;
        Assert.True(same.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_IntEqualMember_EvalBool()
    {
        // Member==Member int：两侧 TypeName 为 int，走 EvalInt。
        Expression<Func<User, bool>> ageEqScore = u => u.Age == u.Score;
        TestEvalContext ctx = new TestEvalContext();
        // Age=20, Score=85 → false
        Assert.False(ageEqScore.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_IntEqualMember_SameField_EvalBool()
    {
        Expression<Func<User, bool>> ageEqAge = u => u.Age == u.Age;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(ageEqAge.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_Unbound_Throws()
    {
        // Extra 在 User 上存在，但 TestEvalContext.Has 未绑定 → 须抛且可 catch。
        Expression<Func<User, int>> expr = u => u.Extra;
        TestEvalContext ctx = new TestEvalContext();
        bool caught = false;
        try {
            expr.EvalInt(ctx);
        } catch {
            caught = true;
        }
        Assert.True(caught);
    }

    [Fact]
    public void MemberExpr_StringField_EvalString()
    {
        Expression<Func<User, string>> getName = u => u.Name;
        TestEvalContext ctx = new TestEvalContext();
        string result = getName.EvalString(ctx);
        Assert.True(result == "Alice");
    }

    // ══════════════════════════════════════════════════════════
    // Expression<T> → Expression 运行时类型转换
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void Expression_ExplicitCast_FromGeneric()
    {
        Expression<Func<int>> intExpr = () => 42;
        Expression expr = intExpr;
        Assert.True(true);
    }

    [Fact]
    public void Expression_NodeType_IsLambda()
    {
        Expression<Func<int>> expr = () => 42;
        Expression raw = expr;
        Assert.True(raw.NodeType == ExpressionType.Lambda);
    }

    // ══════════════════════════════════════════════════════════
    // 字符串 == / !=（须走 EvalString，勿经 EvalInt 假阳性）
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void BinaryExpr_StringEqual_EvalBool()
    {
        Expression<Func<bool>> expr = () => "hello" == "hello";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void BinaryExpr_StringEqual_Different_EvalBoolFalse()
    {
        // 若误用 EvalInt：两侧 IntValue 均为 0 → 0==0 假阳性。
        Expression<Func<bool>> expr = () => "hello" == "world";
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(expr.EvalBool(ctx));
    }

    [Fact]
    public void BinaryExpr_StringNotEqual_EvalBool()
    {
        Expression<Func<bool>> expr = () => "hello" != "world";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_StringEqualConstant_EvalBool()
    {
        Expression<Func<User, bool>> isAlice = u => u.Name == "Alice";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(isAlice.EvalBool(ctx));
    }

    [Fact]
    public void MemberExpr_StringEqualConstant_EvalBoolFalse()
    {
        Expression<Func<User, bool>> isBob = u => u.Name == "Bob";
        TestEvalContext ctx = new TestEvalContext();
        Assert.False(isBob.EvalBool(ctx));
    }

    [Fact]
    public void CaptureExpr_CapturedString_Equal_EvalBool()
    {
        string expected = "Alice";
        Expression<Func<bool>> expr = () => "Alice" == expected;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void ParameterExpr_BoundString_Equal_EvalBool()
    {
        Expression<Func<string, bool>> expr = s => s == "hello";
        TestEvalContext ctx = new TestEvalContext();
        ctx.BoundParam = "s";
        ctx.BoundString = "hello";
        Assert.True(expr.EvalBool(ctx));
    }

    // ══════════════════════════════════════════════════════════
    // ConditionalExpression（三元）Eval
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void ConditionalExpr_TrueBranch_EvalInt()
    {
        Expression<Func<int>> expr = () => true ? 1 : 2;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(1, expr.EvalInt(ctx));
    }

    [Fact]
    public void ConditionalExpr_FalseBranch_EvalInt()
    {
        Expression<Func<int>> expr = () => false ? 1 : 2;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(2, expr.EvalInt(ctx));
    }

    [Fact]
    public void ConditionalExpr_CompareCond_EvalInt()
    {
        Expression<Func<int>> expr = () => 10 > 5 ? 42 : 0;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(42, expr.EvalInt(ctx));
    }

    [Fact]
    public void ConditionalExpr_StringBranches_EvalString()
    {
        Expression<Func<string>> expr = () => true ? "yes" : "no";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalString(ctx) == "yes");
    }

    [Fact]
    public void ConditionalExpr_StringEqual_Nested_EvalBool()
    {
        // 三元结果 TypeName=string，嵌套 == 须走 EvalString。
        Expression<Func<bool>> expr = () => (true ? "a" : "b") == "a";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void ConditionalExpr_MemberCond_EvalBool()
    {
        Expression<Func<User, bool>> expr = u => u.Active ? true : false;
        TestEvalContext ctx = new TestEvalContext();
        ctx.Active = true;
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void UnaryExpr_Not_EvalBool()
    {
        Expression<Func<bool>> expr = () => !false;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }

    [Fact]
    public void UnaryExpr_Neg_EvalInt()
    {
        Expression<Func<int>> expr = () => -7;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(-7, expr.EvalInt(ctx));
    }

    // ══════════════════════════════════════════════════════════
    // IndexExpression / CastExpression Eval（RFC 022 §9.4.8）
    // ══════════════════════════════════════════════════════════

    [Fact]
    public void IndexExpr_Scores0_EvalInt()
    {
        MemberExpression obj = new MemberExpression();
        obj.MemberName = "Scores";
        ConstantExpression ix = new ConstantExpression();
        ix.IntValue = 0;
        ix.TypeName = "int";
        IndexExpression expr = new IndexExpression();
        expr.Object = obj;
        expr.Index = ix;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(10, expr.EvalInt(ctx));
    }

    [Fact]
    public void IndexExpr_Scores1_EvalInt()
    {
        MemberExpression obj = new MemberExpression();
        obj.MemberName = "Scores";
        ConstantExpression ix = new ConstantExpression();
        ix.IntValue = 1;
        IndexExpression expr = new IndexExpression();
        expr.Object = obj;
        expr.Index = ix;
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(20, expr.EvalInt(ctx));
    }

    [Fact]
    public void IndexExpr_Tags0_EvalString()
    {
        ParameterExpression obj = new ParameterExpression();
        obj.Name = "Tags";
        ConstantExpression ix = new ConstantExpression();
        ix.IntValue = 0;
        IndexExpression expr = new IndexExpression();
        expr.Object = obj;
        expr.Index = ix;
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalString(ctx) == "alpha");
    }

    [Fact]
    public void IndexExpr_Unbound_Throws()
    {
        MemberExpression obj = new MemberExpression();
        obj.MemberName = "Missing";
        ConstantExpression ix = new ConstantExpression();
        ix.IntValue = 0;
        IndexExpression expr = new IndexExpression();
        expr.Object = obj;
        expr.Index = ix;
        TestEvalContext ctx = new TestEvalContext();
        bool threw = false;
        try {
            expr.EvalInt(ctx);
        } catch (InvalidOperationException) {
            threw = true;
        }
        Assert.True(threw);
    }

    [Fact]
    public void CastExpr_Forward_EvalInt()
    {
        ConstantExpression inner = new ConstantExpression();
        inner.IntValue = 42;
        CastExpression expr = new CastExpression();
        expr.Expr = inner;
        expr.TargetType = "int";
        TestEvalContext ctx = new TestEvalContext();
        Assert.Equal(42, expr.EvalInt(ctx));
    }

    [Fact]
    public void CastExpr_Forward_EvalBool()
    {
        ConstantExpression inner = new ConstantExpression();
        inner.BoolValue = true;
        inner.TypeName = "bool";
        CastExpression expr = new CastExpression();
        expr.Expr = inner;
        expr.TargetType = "bool";
        TestEvalContext ctx = new TestEvalContext();
        Assert.True(expr.EvalBool(ctx));
    }
}

// ── 测试用结构体（对标准 QueryExpressions 示例） ──

struct User {
    public int Age;
    public int Score;
    public string Name;
    public bool Active;
    /// <summary>故意不在 TestEvalContext.Has 中绑定，供 Unbound_Throws。</summary>
    public int Extra;
}
