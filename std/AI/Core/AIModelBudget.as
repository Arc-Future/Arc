// AIModelBudget — 内存预算记账（RFC 041 §7.2）。
//
// 按注册 SizeBytes 估算常驻字节（+ 峰值工作区留待按需）；注册表经
// AddResident/RemoveResident 记账，ResidentBytes 可审计。MemoryBudgetBytes == 0
// = 不设限（AvailableBytes 返回 long.MaxValue）。internal 变更由注册表驱动，
// 只读面公开（门面只读 Budget 读统计/记账）。
namespace Arc.AI;

/// <summary>
/// 模型常驻内存预算（RFC 041 §7.2）。只读面（<see cref="BudgetBytes"/> /
/// <see cref="ResidentBytes"/> / <see cref="AvailableBytes"/>）公开供审计；
/// 记账（AddResident/RemoveResident）为包内驱动。
/// </summary>
public class AIModelBudget {
    private long _budgetBytes;
    private long _residentBytes;

    /// <summary>由注册表按选项构造。</summary>
    internal AIModelBudget(long budgetBytes) {
        _budgetBytes = budgetBytes;
        _residentBytes = 0;
    }

    /// <summary>预算上限（字节；0 = 不设限）。</summary>
    public long BudgetBytes {
        get { return _budgetBytes; }
    }

    /// <summary>当前常驻字节（已加载模型 SizeBytes 之和）。</summary>
    public long ResidentBytes {
        get { return _residentBytes; }
    }

    /// <summary>剩余可用字节（0 预算 = long.MaxValue 表示不设限）。</summary>
    public long AvailableBytes {
        get {
            if (_budgetBytes == 0) {
                // 编译器缺陷规避（对齐 YamlParser 先例）：原始类型 long 不解析
                // long.MaxValue 静态成员，用字面量等价表达「不设限」。
                return 9223372036854775807;
            }
            long available = _budgetBytes - _residentBytes;
            if (available < 0) {
                available = 0;
            }
            return available;
        }
    }

    /// <summary>是否可容纳新增 <paramref name="bytes"/>（0 预算恒为 true）。</summary>
    internal bool CanFit(long bytes) {
        return this.AvailableBytes >= bytes;
    }

    /// <summary>记账：加载成功增加常驻字节。</summary>
    internal void AddResident(long bytes) {
        _residentBytes = _residentBytes + bytes;
    }

    /// <summary>记账：卸载时减少常驻字节（下限钳到 0）。</summary>
    internal void RemoveResident(long bytes) {
        long resident = _residentBytes - bytes;
        if (resident < 0) {
            resident = 0;
        }
        _residentBytes = resident;
    }
}
