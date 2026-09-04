namespace UnitTest.Core;

using Arc;
using Arc.QIF;

class NullSafetyInner {
    public string Tag = "inner";
    public int Num;
    public bool Flag;

    public string GetName() { return "inner-name"; }

    public int Inc() { Num = Num + 1; return Num; }
}

class NullSafetyOuter {
    public string Label = "outer";
    public NullSafetyInner? Inner;
    public NullSafetyInner Inner2;

    public NullSafetyOuter() {
        Inner = new NullSafetyInner();
        Inner2 = new NullSafetyInner();
    }
}

class NullSafetySideEffect {
    public static int Calls = 0;

    public int Ping() {
        Calls = Calls + 1;
        return 1;
    }
}

public class NullSafetyTests {
    // ── ?. 字段访问 ──

    [Fact]
    public void NullConditional_Field_NotNull()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        o.Label = "x";
        string? r = o?.Label;
        Assert.True(r != null);
        string u = r ?? "!";
        Assert.True(u == "x");
    }

    [Fact]
    public void NullConditional_Field_NullPath()
    {
        NullSafetyOuter? o = null;
        string? r = o?.Label;
        Assert.True(r == null);
    }

    // ── ?. 方法调用 ──

    [Fact]
    public void NullConditional_MethodCall_NotNull()
    {
        NullSafetyInner? i = new NullSafetyInner();
        int? r = i?.Inc();
        int n = r ?? 0;
        Assert.Equal(1, n);
    }

    [Fact]
    public void NullConditional_MethodCall_CoalesceFallback()
    {
        NullSafetyInner? i = new NullSafetyInner();
        string? t = i?.GetName();
        string v = t ?? "fallback";
        Assert.True(v == "inner-name");
    }

    [Fact]
    public void NullConditional_MethodCall_NullFallback()
    {
        NullSafetyInner? i = null;
        string? t = i?.GetName();
        string v = t ?? "fallback";
        Assert.True(v == "fallback");
    }

    // ── ?. 链式（以两步中间变量表达；单表达式链式形态当前被 typeck 拒绝，见文末缺陷记录）──

    [Fact]
    public void NullConditional_Chain_TwoStep_NotNull()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        NullSafetyInner? i = o?.Inner;
        string? t = i?.Tag;
        string v = t ?? "none";
        Assert.True(v == "inner");
    }

    [Fact]
    public void NullConditional_Chain_TwoStep_HeadNull()
    {
        NullSafetyOuter? o = null;
        NullSafetyInner? i = o?.Inner;
        string? t = i?.Tag;
        string v = t ?? "default";
        Assert.True(v == "default");
    }

    [Fact]
    public void NullConditional_Chain_TwoStep_MidNull()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        o.Inner = null;
        NullSafetyInner? i = o?.Inner;
        string? t = i?.Tag;
        string v = t ?? "mid-null";
        Assert.True(v == "mid-null");
    }

    // ── ?. 短路边界：receiver 为 null 时右侧副作用不得执行 ──

    [Fact]
    public void NullConditional_ShortCircuit_NullSkipsCall()
    {
        NullSafetySideEffect.Calls = 0;
        NullSafetySideEffect? s = null;
        int? v = s?.Ping();
        Assert.Equal(0, NullSafetySideEffect.Calls);
        int n = v ?? 0;
        Assert.Equal(0, n);
    }

    [Fact]
    public void NullConditional_NonNull_ExecutesCall()
    {
        NullSafetySideEffect.Calls = 0;
        NullSafetySideEffect? s = new NullSafetySideEffect();
        int? v = s?.Ping();
        int n = v ?? 0;
        Assert.Equal(1, NullSafetySideEffect.Calls);
        Assert.Equal(1, n);
    }

    [Fact]
    public void NullConditional_Statement_Null_NoThrow()
    {
        NullSafetyInner? i = null;
        i?.Inc();
        Assert.True(true);
    }

    // ── ?. 结果在 bool 场景的消费 ──

    [Fact]
    public void NullConditional_Bool_UnwrapTrue()
    {
        NullSafetyInner? i = new NullSafetyInner();
        i.Flag = true;
        bool? t = i?.Flag;
        bool f = t ?? false;
        if (f) {
            Assert.True(true);
            return;
        }
        Assert.True(false);
    }

    [Fact]
    public void NullConditional_Bool_CoalesceFalse()
    {
        NullSafetyInner? i = null;
        bool? t = i?.Flag;
        bool f = t ?? false;
        Assert.False(f);
    }

    // ── !. 强制解引用 ──

    [Fact]
    public void ForceDeref_Field()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        string v = o!.Label;
        Assert.True(v == "outer");
    }

    [Fact]
    public void ForceDeref_Chain_PlainDotTail()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        NullSafetyInner i2 = o!.Inner2;
        string v = i2.Tag;
        Assert.True(v == "inner");
    }

    [Fact]
    public void ForceDeref_MethodCall()
    {
        NullSafetyInner? i = new NullSafetyInner();
        string v = i!.GetName();
        Assert.True(v == "inner-name");
    }

    [Fact]
    public void ForceDeref_AfterNullCheck()
    {
        NullSafetyOuter? o = new NullSafetyOuter();
        if (o != null) {
            string v = o!.Label;
            Assert.True(v == "outer");
            return;
        }
        Assert.True(false);
    }
}

/*
 * 缺陷与限制记录（编译探测 + 运行验证结论，2026-08-28，cargo build -p arc）
 *
 * [DEFECT-1] 单表达式链式 ?. 不被 typeck 接受（解析与 codegen 均支持，仅 typeck 拒绝）
 *   形态:   `NullSafetyOuter? o; string v = o?.Inner.Tag;`（?. 后接普通 .）
 *   指纹:   error: OOP: cannot access member `Tag` on nullable expression; use `?.` or `!.` (typeck/check_expr.rs)
 *   绕行:   拆两步 `NullSafetyInner? i = o?.Inner;` 再 `i?.Tag`
 *
 * [DEFECT-2] 链式 ?.?. 同样被 typeck 拒绝（与 DEFECT-1 同根因：中间段结果为 T?，非 Ident receiver 的成员访问被拒）
 *   形态:   `o?.Inner?.Tag`
 *   指纹:   同 DEFECT-1
 *
 * [LIMIT-3] `T? == 字面量` 直接比较被 typeck 拒绝（T? 只能与 null 比较，或先经 ?? 解包）
 *   形态:   `string? r = o?.Label; if (r == "x") { ... }`
 *   指纹:   error: type mismatch: expected string, found string?
 *
 * [LIMIT-4] null 条件索引 `a?[i]` 不支持（解析失败）
 *   形态:   `arr?[0]`
 *   指纹:   parse error: expected expression, found RBracket
 *
 * [LIMIT-5] null 合并赋值 `??=` 不支持（与 AST 无对应节点一致）
 *   形态:   `s ??= "x";`
 *   指纹:   parse error: expected expression, found Eq
 *
 * [LIMIT-6] `!.` 链中段结果已是非空 T，继续 `!.` 报 receiver 非 nullable
 *   形态:   `o!.Inner!.Tag`（Inner 为非空字段时）
 *   指纹:   error: OOP: `!.` requires nullable receiver, found `NullSafetyInner`
 *
 * [DEFECT-7] 内联成员访问表达式参与 ?? 或字符串比较时 codegen 把引用 ptrtoint 成 i32，
 *            LLVM 拒绝编译（typeck 通过）。触发形态与 LLVM 指纹：
 *     a) `(r ?? "!") == "x"`             -> constant expression type mismatch: got 'ptr' but expected 'i32'
 *     b) `string v = i?.GetName() ?? "fallback";` -> '%tN' defined with type 'i32' but expected 'ptr'
 *     c) `string v = o!.Inner2.Tag;`     -> 同 b
 *     d) `Assert.True(o!.Label == "outer");` -> 同 a
 *   绕行:   ?./!. 结果先存入 T? 局部变量、?? 结果先存入 T 局部变量，再做比较/传参。
 *
 * [DEFECT-8] `?.` 属性访问不调用 getter，被降级为直接读 backfield（副作用静默丢失）
 *   形态:   `int? v = s?.Boom;`（Boom 为带副作用的 getter）
 *   表现:   receiver 非空时 getter 体不执行；LLVM 中为 GEP+load 而非 getter 调用。
 *           因此短路计数器验证必须用方法调用形态（s?.Ping()）。
 *
 * [DEFECT-9] `!.` 作为赋值目标时整个赋值语句被静默丢弃（无警告，LLVM 中无对应 store）
 *   形态:   `NullSafetyInner? i = new NullSafetyInner(); i!.Flag = true;`
 *   表现:   运行期赋值未发生，后续读取得到默认值。
 *   绕行:   对 nullable 变量直接成员赋值 `i.Flag = true;`。
 *
 * [DEFECT-10] `bool? == true` 内联比较运行时恒为 false（typeck 通过、独立探测项目复现）
 *   形态:   `PInner? i = new PInner(); i.Flag = true; if (i?.Flag == true) { ... }`
 *   绕行:   两步 `bool? t = i?.Flag; bool f = t ?? false;`（见 NullConditional_Bool_UnwrapTrue）
 */
