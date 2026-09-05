// NatType —— 拆分自 StunClient.as（一文件一公开类型）。
namespace Arc.Net.P2P;

public enum NatType { None, FullCone, AddressRestrictedCone, PortRestrictedCone, Symmetric, Unknown }
