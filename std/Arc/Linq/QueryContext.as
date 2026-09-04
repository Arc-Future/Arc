namespace Arc.Linq;

/// 查询上下文——持某 Provider，供扩展方法使用。
public class QueryContext {
    /// <summary>关联的查询提供者，供扩展方法使用。</summary>
    public IQueryProvider Provider { get; set; }

    /// <summary>构造查询上下文，Provider 初始化为 null。</summary>
    public QueryContext() {
        Provider = null;
    }
}
