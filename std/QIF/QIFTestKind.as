namespace Arc.QIF;

/// <summary>QIF 测试类型枚举（L1-L7 七层体系）。</summary>
public enum QIFTestKind {
    Fact, 
    Theory, 
    Integration, 
    E2e, 
    Benchmark, 
    Property, 
    Snapshot, 
    Contract
}
