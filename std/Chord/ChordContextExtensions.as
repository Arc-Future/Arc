// ChordContextExtensions —— ChordContext 类型化扩展（RFC 045 D5/D11/D12 便捷面）。
//
// 核心 API 的强类型便捷形态：事件/瀑布/服务/配置的 (T) 转换、单依赖
// 注入与贡献点注册。全部构建于 ChordContext 核心面之上，无新语义。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


/// <summary>
/// ChordContext 类型化扩展：强类型事件、瀑布、服务/配置解析与贡献点注册。
/// </summary>
public static class ChordContextExtensions {
    /// <summary>类型化取服务（沿祖先链；缺失返回 null）。</summary>
    public static T? GetService<T>(this ChordContext ctx, string name) where T : class {
        object? value = ctx.GetService(name);
        return value != null ? (T)value : null;
    }

    /// <summary>类型化取配置（沿祖先链；缺失返回 null）。</summary>
    public static T? GetConfig<T>(this ChordContext ctx, string name) where T : class {
        object? value = ctx.GetConfig(name);
        return value != null ? (T)value : null;
    }

    /// <summary>类型化订阅事件（撤销 = 退订）。</summary>
    public static IDisposable On<T>(this ChordContext ctx, string name, Action<T> listener) {
        Action<object?> handler = (payload) => {
            listener((T)payload);
        };
        return ctx.On(name, handler);
    }

    /// <summary>类型化订阅事件（触发即退订）。</summary>
    public static IDisposable Once<T>(this ChordContext ctx, string name, Action<T> listener) {
        Action<object?> handler = (payload) => {
            listener((T)payload);
        };
        return ctx.Once(name, handler);
    }

    /// <summary>类型化触发事件（自身 + 后代）。</summary>
    public static void Emit<T>(this ChordContext ctx, string name, T payload) {
        ctx.Emit(name, (object)payload);
    }

    /// <summary>类型化订阅瀑布：handler(payload, next) 串联，不调 next 即拦截。</summary>
    public static IDisposable OnWaterfall<T>(this ChordContext ctx, string name, Func<T, Func<T, T>, T> handler) {
        return ctx.OnWaterfall(name, (payload, next) => {
            T current = (T)payload;
            return handler(current, (item) => {
                object carried = next(item);
                return (T)carried;
            });
        });
    }

    /// <summary>类型化触发瀑布：返回末端产出。</summary>
    public static T Waterfall<T>(this ChordContext ctx, string name, T payload) {
        object result = ctx.Waterfall(name, payload);
        return (T)result;
    }

    /// <summary>类型化提供服务（撤销 = 撤销提供，恢复旧条目）。</summary>
    public static IDisposable Provide<T>(this ChordContext ctx, string name, T instance) {
        return ctx.Provide(name, instance);
    }

    /// <summary>
    /// 向统一注册表投递贡献（D11 修订）：解析 IContributeRegistry，按 hostId
    /// 路由至插件容器并透传组织元数据；撤销 = registry.Unregister——纳入
    /// 作用域账本 / 事务 / 失败回滚。注册表未就绪或容器未注册 → 抛出 →
    /// 触发失败回滚（依赖顺序由 IToneRequirements 前置）。
    /// </summary>
    public static IDisposable Contribute(this ChordContext ctx, string hostId, IContribute contribute) {
        return ctx.Contribute(hostId, contribute, new ContributeOptions());
    }

    public static IDisposable Contribute(this ChordContext ctx, string hostId, IContribute contribute, ContributeOptions options) {
        IContributeRegistry? registry = ctx.GetService<IContributeRegistry>();
        if (registry == null) {
            throw new Exception("Arc.Chord: 统一贡献注册表未就绪");
        }
        return ctx.Effect(() => {
            registry.Register(hostId, contribute, options);
            return new DisposableAction(() => registry.Unregister(hostId, contribute));
        });
    }

    /// <summary>
    /// 注册插件容器（容器热插拔，撤销 = registry.Remove；纳入作用域账本 /
    /// 事务 / 失败回滚）。
    /// </summary>
    public static IDisposable AddHost(this ChordContext ctx, IContributeHost host) {
        IContributeRegistry? registry = ctx.GetService<IContributeRegistry>();
        if (registry == null) {
            throw new Exception("Arc.Chord: 统一贡献注册表未就绪");
        }
        return ctx.Effect(() => {
            registry.Add(host);
            return new DisposableAction(() => registry.Remove(host));
        });
    }

    /// <summary>单依赖注入便捷形态。</summary>
    public static IDisposable Inject(this ChordContext ctx, string name, Action<ChordContext> callback) {
        string[] names = new string[1];
        names[0] = name;
        return ctx.Inject(names, callback);
    }

    /// <summary>单依赖反应式注入便捷形态。</summary>
    public static IDisposable InjectReactive(this ChordContext ctx, string name, Action<ChordContext> callback) {
        string[] names = new string[1];
        names[0] = name;
        return ctx.InjectReactive(names, callback);
    }
}
