//! L1 批量：接口分派与枚举标志回归集（9 case）。
//!
//! 从 iface_dispatch_batch_e2e.rs 提取，保留原始语法。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_iface_dispatch_batch() {
    assert_compiles_batch(
        "iface_dispatch",
        &[
            (
                "overload_resolve",
                r#"using Arc;

public class OrTmp {
    public void Msg(string a) {
        this.Msg(a, "");
    }

    public void Msg(string a, string b) {
        Console.WriteLine("two:" + a + "|" + b);
    }
}

void Main() {
    OrTmp t = new OrTmp();
    t.Msg("hello");
    Console.WriteLine("overload_done");
}
"#,
            ),
            (
                "dispatch_overload_slots",
                r#"using Arc;

class DsCalc {
    public virtual int Describe(int value) { return value * 10; }
    public virtual string Describe(string text) { return "base-string:" + text; }
}

class DsCalcDerived : DsCalc {
    public override int Describe(int value) { return value * 100; }
}

void Main() {
    DsCalcDerived d = new DsCalcDerived();
    if (d.Describe(5) == 500 && d.Describe("hi") == "base-string:hi") {
        Console.WriteLine("dispatch_overload_slots_ok");
    }
}
"#,
            ),
            (
                "dispatch_implicit_override",
                r#"using Arc;

abstract class DgBase {
    public abstract Task<string> Complete(string request);
}

class DgImpl : DgBase {
    public Task<string> Complete(string request) {
        return Task.FromResult("done:" + request);
    }
}

void Main() {
    DgBase b = new DgImpl();
    Task<string> t = b.Complete("hi");
    if (t.Result == "done:hi") {
        Console.WriteLine("dispatch_implicit_override_ok");
    }
}
"#,
            ),
            (
                "g3_normal_form_preference",
                r#"using Arc;

class GpCalc {
    public int Sum(params ReadOnlySpan<int> xs) {
        return 1;
    }

    public int Sum(int a) {
        return a;
    }

    public string Pick(params ReadOnlySpan<string> parts) {
        return "params";
    }

    public string Pick(string single) {
        return "single";
    }
}

void Main() {
    GpCalc c = new GpCalc();
    if (c.Sum(5) != 5) {
        Console.WriteLine("fail:sum-normal");
        return;
    }
    if (c.Pick("x") != "single") {
        Console.WriteLine("fail:pick-normal");
        return;
    }
    if (c.Sum(1, 2, 3) != 1) {
        Console.WriteLine("fail:sum-expanded");
        return;
    }
    if (c.Pick("a", "b") != "params") {
        Console.WriteLine("fail:pick-expanded");
        return;
    }
    Console.WriteLine("g3_normal_form_ok");
}
"#,
            ),
            (
                "enum_compare",
                r#"using Arc;

enum Status { Idle, Running, Done }

void Main() {
    Status s = Status.Running;
    string n = "";
    if (s == Status.Idle) {
        n = "idle";
    } else if (s == Status.Running) {
        n = "run";
    } else {
        n = "done";
    }
    if (n == "run") {
        Console.WriteLine("enum_compare_ok");
    }
}
"#,
            ),
            (
                "else_if_chain",
                r#"using Arc;

string Classify(int x) {
    string r = "";
    if (x < 0) { r = "neg"; }
    else if (x == 0) { r = "zero"; }
    else if (x < 10) { r = "small"; }
    else { r = "large"; }
    return r;
}

void Main() {
    if (Classify(5) != "small") {
        Console.WriteLine("fail:positive");
        return;
    }
    if (Classify(-3) != "neg") {
        Console.WriteLine("fail:negative");
        return;
    }
    if (Classify(100) != "large") {
        Console.WriteLine("fail:large");
        return;
    }
    if (Classify(0) != "zero") {
        Console.WriteLine("fail:zero");
        return;
    }
    Console.WriteLine("else_if_chain_ok");
}
"#,
            ),
            (
                "enum_flags_bitwise",
                r#"using Arc;

[Flags]
public enum EfAccess {
    None    = 0,
    Read    = 1,
    Write   = 2,
    Execute = 4,
}

void Main() {
    EfAccess rw = EfAccess.Read | EfAccess.Write;
    EfAccess combined = EfAccess.Read | EfAccess.Write | EfAccess.Execute;
    EfAccess has_rw = combined & (EfAccess.Read | EfAccess.Write);
    EfAccess toggle = EfAccess.Read | EfAccess.Write;
    EfAccess toggled = toggle ^ EfAccess.Write;
    EfAccess not_none = ~EfAccess.None;
    EfAccess all_bits = (EfAccess)(-1);
    EfAccess not_all = ~all_bits;
    EfAccess flags = EfAccess.None;
    flags |= EfAccess.Read;
    flags |= EfAccess.Write;
    EfAccess mask = EfAccess.Read | EfAccess.Write | EfAccess.Execute;
    mask &= EfAccess.Read | EfAccess.Write;
    EfAccess xf = EfAccess.Read | EfAccess.Write;
    xf ^= EfAccess.Write;
    EfAccess flag1 = (EfAccess)(1 << 0);
    EfAccess flag2 = (EfAccess)(1 << 1);
    EfAccess flag3 = (EfAccess)(1 << 2);
    Console.WriteLine("bitwise_ok");
}
"#,
            ),
            (
                "enum_flags_no_flags",
                r#"using Arc;

public enum EfMode {
    None = 0,
    A = 1,
    B = 2,
    C = 4,
}

void Main() {
    EfMode m = EfMode.A | EfMode.B;
    EfMode has = m & EfMode.B;
    EfMode x = m ^ EfMode.A;
    EfMode n = ~EfMode.None;
    EfMode shifted = EfMode.A << 2;
    EfMode shr = EfMode.C >> 1;
    EfMode acc = EfMode.None;
    acc |= EfMode.A;
    acc |= EfMode.C;
    acc &= EfMode.A;
    Console.WriteLine("nb_ok");
}
"#,
            ),
            (
                "enum_flags_util",
                r#"using Arc;

[Flags]
public enum EfPerms {
    None    = 0,
    Read    = 1,
    Write   = 2,
    Execute = 4,
}

void Main() {
    EfPerms p = EfPerms.Read | EfPerms.Write;
    bool hasRead = Enum.HasFlag(p, EfPerms.Read);
    bool hasWrite = Enum.HasFlag(p, EfPerms.Write);
    bool hasExec = Enum.HasFlag(p, EfPerms.Execute);
    bool f = Enum.HasFlag(EfPerms.Read, EfPerms.Read);
    bool noFlag = Enum.HasFlag(EfPerms.None, EfPerms.Read);
    bool defRead = Enum.IsDefined(EfPerms.Read);
    bool defCombined = Enum.IsDefined(EfPerms.Read | EfPerms.Write);
    bool defUnknown = Enum.IsDefined((EfPerms)99);
    Console.WriteLine("util_ok");
}
"#,
            ),
        ],
    );
}
