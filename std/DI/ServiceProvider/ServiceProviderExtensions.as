// ServiceProviderExtensions (RFC 023 M1)
// v1.0 修订 (2026-07-24):
//   - CreateScope 通过 IServiceScopeFactory 接口解析 (对齐 MEDI 语义)
namespace Arc.DI;

using Arc.Collections;

public static class ServiceProviderExtensions {
    public static T? GetService<T>(this IServiceProvider sp) {
        object? result = sp.GetService(typeof(T));
        return (T)result;
    }

    public static T GetRequiredService<T>(this IServiceProvider sp) {
        object? result = sp.GetService(typeof(T));
        if (result == null) {
            throw new Exception("Required service is not registered.");
        }
        return (T)result;
    }

    /// <summary>解析某类型的全部注册实例（强类型列表）。未注册返回空列表（非 null）。</summary>
    public static List<T> GetServices<T>(this IServiceProvider sp) {
        List<object?> all = sp.GetServices(typeof(T));
        List<T> results = new List<T>();
        for (int i = 0; i < all.Count; i++) {
            results.Add((T)all[i]);
        }
        return results;
    }

    public static T? GetKeyedService<T>(this IServiceProvider sp, object? key) {
        object? result = sp.GetKeyedService(typeof(T), key);
        return (T)result;
    }

    public static T GetRequiredKeyedService<T>(this IServiceProvider sp, object? key) {
        object? result = sp.GetKeyedService(typeof(T), key);
        if (result == null) {
            throw new Exception("Required keyed service is not registered.");
        }
        return (T)result;
    }

    /// 创建作用域 — 通过 IServiceScopeFactory 接口解析 (对齐 MEDI 语义)。
    public static IServiceScope CreateScope(this IServiceProvider sp) {
        var factory = sp.GetService(typeof(IServiceScopeFactory));
        if (factory == null) {
            throw new Exception("IServiceScopeFactory is not registered; root container must implement it.");
        }
        var sf = (IServiceScopeFactory)factory;
        return sf.CreateScope();
    }
}
