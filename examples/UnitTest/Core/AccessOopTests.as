namespace UnitTest.Core;

using Arc;
using Arc.QIF;

// ── 访问权限 ──

class PrivateWallet {
    private int _balance;

    public void Deposit(int n) {
        _balance += n;
    }

    public int Balance() {
        return _balance;
    }

    public void Reset(int n) {
        _balance = n;
    }
}

class ProtectedBase {
    protected int Shield;

    public ProtectedBase(int shield) {
        Shield = shield;
    }

    protected int Reveal() {
        return Shield * 10;
    }

    public int RevealInside() {
        return Reveal();
    }
}

class ProtectedDerived : ProtectedBase {
    public int Boost;

    public ProtectedDerived(int shield, int boost) : base(shield) {
        Boost = boost;
    }

    public int RevealFromDerived() {
        return Reveal();
    }

    public int ShieldFromDerived() {
        return Shield;
    }
}

class ProtectedPropBase {
    protected int Seed { get; set; }

    public ProtectedPropBase(int seed) {
        Seed = seed;
    }

    public int SeedInside {
        get { return Seed; }
    }
}

class ProtectedPropDerived : ProtectedPropBase {
    public ProtectedPropDerived(int seed) : base(seed) {
    }

    public int SeedFromDerived() {
        return Seed;
    }
}

class DefaultVis {
    int _hidden;

    public DefaultVis() {
        _hidden = 9;
    }

    public int Peek() {
        return _hidden;
    }

    int Twice() {
        return _hidden * 2;
    }

    public int CallTwice() {
        return Twice();
    }
}

class PublicFace {
    public int Id;
    public static int Tag {
        get { return 77; }
    }

    public int Doubled() {
        return Id * 2;
    }
}

// ── 自动属性 ──

class AutoBox {
    public int Count { get; set; }

    public int Doubled {
        get { return Count * 2; }
    }
}

class AutoInitBox {
    public int Count { get; set; } = 7;
    public string Label { get; } = "auto";
    public bool Flag { get; set; } = true;
}

class PrivateSetBox {
    public int Size { get; private set; }

    public PrivateSetBox(int size) {
        Size = size;
    }

    public void Grow(int n) {
        Size += n;
    }
}

class ExprGetBox {
    private int _v;

    public ExprGetBox(int v) {
        _v = v;
    }

    public int V => _v;
    public int Doubled { get => _v * 2; }
}

// ── OOP 基础 ──

abstract class ExprNode {
    public abstract int Eval();

    public int EvalTwice() {
        return Eval() * 2;
    }
}

class NumNode : ExprNode {
    private int _n;

    public NumNode(int n) {
        _n = n;
    }

    public override int Eval() {
        return _n;
    }
}

abstract class AbsChainA {
    public abstract int Compute();
}

abstract class AbsChainB : AbsChainA {
    public override abstract int Compute();
}

class AbsChainLeaf : AbsChainB {
    public override int Compute() {
        return 7;
    }
}

class VRoot {
    public virtual int Value() {
        return 1;
    }

    public virtual string Name {
        get { return "root"; }
    }
}

class VMid : VRoot {
    public override int Value() {
        return 20;
    }

    public override string Name {
        get { return "mid"; }
    }
}

class VLeaf : VMid {
    public override int Value() {
        return 300;
    }

    public override string Name {
        get { return "leaf"; }
    }
}

class CtorBase {
    public int Tag;

    public CtorBase(int tag) {
        Tag = tag;
    }
}

class CtorDerived : CtorBase {
    public int Extra;

    public CtorDerived(int tag, int extra) : base(tag) {
        Extra = extra;
    }
}

interface IFly {
    int Fly();
}

interface ISwim {
    int Swim();
}

class Amphibian : IFly, ISwim {
    public int Fly() {
        return 1;
    }

    public int Swim() {
        return 2;
    }
}

class OverloadGreeter {
    public string Greet(string name) {
        return "Hi " + name;
    }

    public string Greet(string name, string punct) {
        return "Hi " + name + punct;
    }

    public int Add(int a, int b = 1) {
        return a + b;
    }
}

// ── 加码形态：protected virtual / base 调用链 / internal 成员 ──

class PVirtualBase {
    protected virtual int Score() {
        return 1;
    }

    public int ScorePublic() {
        return Score();
    }
}

class PVirtualMid : PVirtualBase {
    protected override int Score() {
        return 2;
    }
}

class PVirtualLeaf : PVirtualMid {
    protected override int Score() {
        return 3;
    }
}

class PPropBase {
    protected virtual string Tier {
        get { return "base"; }
    }

    public string TierPublic {
        get { return Tier; }
    }
}

class PPropDerived : PPropBase {
    protected override string Tier {
        get { return "derived"; }
    }
}

class BaseCallChain {
    public virtual int Value() {
        return 1;
    }
}

class BaseCallMid : BaseCallChain {
    public override int Value() {
        return base.Value() + 10;
    }
}

class BaseCallLeaf : BaseCallMid {
    public override int Value() {
        return base.Value() + 100;
    }
}

class CtorExprBase {
    public int Tag;

    public CtorExprBase(int tag) {
        Tag = tag;
    }
}

class CtorExprDerived : CtorExprBase {
    public CtorExprDerived(int tag) : base(tag * 2) {
    }
}

class InternalMemberHost {
    internal int Via = 5;

    internal int Triple() {
        return Via * 3;
    }
}

/// <summary>
/// 访问权限体系与 OOP 基础语法单元测试：
/// 覆盖 private 封装、protected 继承链、internal 跨文件、默认可见性、
/// public 暴露、自动属性（get/set 简写、初值、private set、表达式体 get）、
/// abstract 模板方法与 override abstract 链、两级 virtual/override 链、
/// 构造器 : base(args)、接口多实现、方法重载与参数默认值。
/// </summary>
public class AccessOopTests
{
    // ── 访问权限：private 封装 ──

    [Fact]
    public void Private_Field_EncapsulatedReadWrite()
    {
        PrivateWallet w = new PrivateWallet();
        w.Deposit(30);
        w.Deposit(12);
        Assert.Equal(42, w.Balance());
    }

    [Fact]
    public void Private_Field_ResetViaPublicMethod()
    {
        PrivateWallet w = new PrivateWallet();
        w.Deposit(100);
        w.Reset(5);
        Assert.Equal(5, w.Balance());
    }

    // ── 访问权限：protected ──

    [Fact]
    public void Protected_Field_DerivedAccess()
    {
        ProtectedDerived d = new ProtectedDerived(4, 1);
        Assert.Equal(4, d.ShieldFromDerived());
    }

    [Fact]
    public void Protected_Method_DerivedCall()
    {
        ProtectedDerived d = new ProtectedDerived(3, 0);
        Assert.Equal(30, d.RevealFromDerived());
    }

    [Fact]
    public void Protected_Method_BaseSelfCall()
    {
        ProtectedBase b = new ProtectedBase(2);
        Assert.Equal(20, b.RevealInside());
    }

    [Fact]
    public void Protected_Property_DerivedRead()
    {
        ProtectedPropDerived d = new ProtectedPropDerived(6);
        Assert.Equal(6, d.SeedFromDerived());
    }

    [Fact]
    public void Protected_Property_BaseSelfRead()
    {
        ProtectedPropBase b = new ProtectedPropBase(8);
        Assert.Equal(8, b.SeedInside);
    }

    // ── 访问权限：internal 跨文件 ──

    [Fact]
    public void Internal_Type_CrossFileUse()
    {
        InternalVault v = new InternalVault(42);
        Assert.Equal(42, v.Open());
    }

    [Fact]
    public void Internal_Static_CrossFileCall()
    {
        InternalVault a = new InternalVault(4);
        InternalVault b = new InternalVault(5);
        Assert.Equal(405, InternalOps.Combine(a, b));
    }

    [Fact]
    public void Default_Visibility_TopLevelClass_CrossFileReachable()
    {
        DefaultLevel d = new DefaultLevel();
        Assert.Equal(5, d.Kick());
    }

    // ── 访问权限：默认可见性（无修饰符成员）──

    [Fact]
    public void Default_Visibility_Field_ReachableInClass()
    {
        DefaultVis v = new DefaultVis();
        Assert.Equal(9, v.Peek());
    }

    [Fact]
    public void Default_Visibility_Method_ReachableInClass()
    {
        DefaultVis v = new DefaultVis();
        Assert.Equal(18, v.CallTwice());
    }

    // ── 访问权限：public 暴露 ──

    [Fact]
    public void Public_Member_FieldAndMethod()
    {
        PublicFace f = new PublicFace();
        f.Id = 21;
        Assert.Equal(21, f.Id);
        Assert.Equal(42, f.Doubled());
    }

    [Fact]
    public void Public_Member_StaticProperty()
    {
        Assert.Equal(77, PublicFace.Tag);
    }

    // ── 自动属性 ──

    [Fact]
    public void AutoProperty_GetSet_RoundTrip()
    {
        AutoBox b = new AutoBox();
        b.Count = 5;
        Assert.Equal(5, b.Count);
    }

    [Fact]
    public void AutoProperty_GetOnlyDerived()
    {
        AutoBox b = new AutoBox();
        b.Count = 9;
        Assert.Equal(18, b.Doubled);
    }

    [Fact]
    public void AutoProperty_Initializer_Int()
    {
        AutoInitBox b = new AutoInitBox();
        Assert.Equal(7, b.Count);
    }

    [Fact]
    public void AutoProperty_Initializer_StringAndBool()
    {
        AutoInitBox b = new AutoInitBox();
        Assert.True(b.Label == "auto");
        Assert.True(b.Flag);
    }

    [Fact]
    public void AutoProperty_Initializer_SetStillWritable()
    {
        AutoInitBox b = new AutoInitBox();
        b.Count = 10;
        b.Flag = false;
        Assert.Equal(10, b.Count);
        Assert.False(b.Flag);
    }

    [Fact]
    public void AutoProperty_PrivateSet_WriteInsideOnly()
    {
        PrivateSetBox b = new PrivateSetBox(3);
        b.Grow(2);
        Assert.Equal(5, b.Size);
    }

    [Fact]
    public void AutoProperty_ExpressionBodiedGet()
    {
        ExprGetBox b = new ExprGetBox(11);
        Assert.Equal(11, b.V);
        Assert.Equal(22, b.Doubled);
    }

    // ── OOP：abstract ──

    [Fact]
    public void Abstract_TemplateMethod_Dispatch()
    {
        ExprNode e = new NumNode(4);
        Assert.Equal(8, e.EvalTwice());
    }

    [Fact]
    public void Abstract_DirectOverride()
    {
        ExprNode e = new NumNode(9);
        Assert.Equal(9, e.Eval());
    }

    [Fact]
    public void Abstract_OverrideAbstractChain()
    {
        AbsChainB b = new AbsChainLeaf();
        Assert.Equal(7, b.Compute());
    }

    // ── OOP：virtual 两级链 ──

    [Fact]
    public void Virtual_TwoLevelChain_LeafDispatch()
    {
        VRoot r = new VLeaf();
        Assert.Equal(300, r.Value());
        Assert.True(r.Name == "leaf");
    }

    [Fact]
    public void Virtual_TwoLevelChain_MidDispatch()
    {
        VRoot r = new VMid();
        Assert.Equal(20, r.Value());
        Assert.True(r.Name == "mid");
    }

    [Fact]
    public void Virtual_RootDefault()
    {
        VRoot r = new VRoot();
        Assert.Equal(1, r.Value());
        Assert.True(r.Name == "root");
    }

    // ── OOP：构造器 : base(args) ──

    [Fact]
    public void Ctor_BaseArgs_FieldsInitialized()
    {
        CtorDerived d = new CtorDerived(3, 4);
        Assert.Equal(3, d.Tag);
        Assert.Equal(4, d.Extra);
    }

    [Fact]
    public void Ctor_BaseArgs_BaseOnly()
    {
        CtorBase b = new CtorBase(9);
        Assert.Equal(9, b.Tag);
    }

    // ── OOP：接口多实现 ──

    [Fact]
    public void Interface_MultiImpl_ViaInterfaces()
    {
        Amphibian a = new Amphibian();
        IFly f = a;
        ISwim s = a;
        Assert.Equal(1, f.Fly());
        Assert.Equal(2, s.Swim());
    }

    [Fact]
    public void Interface_MultiImpl_Combined()
    {
        Amphibian a = new Amphibian();
        IFly f = a;
        ISwim s = a;
        Assert.Equal(3, f.Fly() + s.Swim());
    }

    // ── OOP：重载与默认值 ──

    [Fact]
    public void Overload_ByArity()
    {
        OverloadGreeter g = new OverloadGreeter();
        Assert.True(g.Greet("A") == "Hi A");
        Assert.True(g.Greet("A", "!") == "Hi A!");
    }

    [Fact]
    public void DefaultParam_OmittedArg()
    {
        OverloadGreeter g = new OverloadGreeter();
        Assert.Equal(6, g.Add(5));
    }

    [Fact]
    public void DefaultParam_ExplicitArg()
    {
        OverloadGreeter g = new OverloadGreeter();
        Assert.Equal(7, g.Add(5, 2));
    }

    // ── 加码形态：protected virtual ──

    [Fact]
    public void ProtectedVirtual_Method_Chain()
    {
        PVirtualBase b = new PVirtualLeaf();
        Assert.Equal(3, b.ScorePublic());
    }

    [Fact]
    public void ProtectedVirtual_Method_Mid()
    {
        PVirtualBase b = new PVirtualMid();
        Assert.Equal(2, b.ScorePublic());
    }

    [Fact]
    public void ProtectedVirtual_Property_Override()
    {
        PPropBase b = new PPropDerived();
        Assert.True(b.TierPublic == "derived");
    }

    // ── 加码形态：override 内 base.method() 调用链 ──

    [Fact]
    public void BaseCall_TwoLevelChain()
    {
        BaseCallChain c = new BaseCallMid();
        Assert.Equal(11, c.Value());
    }

    [Fact]
    public void BaseCall_ThreeLevelChain()
    {
        BaseCallChain c = new BaseCallLeaf();
        Assert.Equal(111, c.Value());
    }

    // ── 加码形态：base(args) 表达式实参 / internal 成员 ──

    [Fact]
    public void Ctor_BaseArgs_Expression()
    {
        CtorExprDerived d = new CtorExprDerived(4);
        Assert.Equal(8, d.Tag);
    }

    [Fact]
    public void Internal_Member_FieldAccess()
    {
        InternalMemberHost h = new InternalMemberHost();
        Assert.Equal(5, h.Via);
    }

    [Fact]
    public void Internal_Member_MethodCall()
    {
        InternalMemberHost h = new InternalMemberHost();
        Assert.Equal(15, h.Triple());
    }
}
