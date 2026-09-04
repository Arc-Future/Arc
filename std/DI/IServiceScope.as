// IServiceScope - scope interface (RFC 023 M0)
namespace Arc.DI;

public interface IServiceScope : IDisposable {
    IServiceProvider GetServiceProvider();

    /// <summary>释放作用域：级联 Dispose 作用域内所有 IDisposable 实例。</summary>
    void Dispose();
}
