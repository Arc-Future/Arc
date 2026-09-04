namespace UnitTest.Arc;

using Arc;
using Arc.DI;
using Arc.QIF;
using Arc.Web;

/// <summary>隔离 Mediator 泛型分发（不经 HTTP），定位 0xC0000005 根因。</summary>
public class MediatorIsolateTests
{
    [Fact]
    public void Mediator_ScopeResolveHandler_Cast()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        IRequestHandler<PingRequest, PingResponse>? handler =
            (IRequestHandler<PingRequest, PingResponse>?)scopeSp.GetService(typeof(IRequestHandler<PingRequest, PingResponse>));
        Assert.NotNull(handler, "handler_resolve_null");
    }

    [Fact]
    public async Task Mediator_ScopeResolveMediator_NoCast()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        Assert.NotNull(mediatorObj, "mediator_resolve_null");
    }

    [Fact]
    public async Task Mediator_SendAsync_ResolvesHandler()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        PingRequest req = new PingRequest();
        req.Name = "iso";
        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Task<PingResponse> t = mediator.SendAsync<PingRequest, PingResponse>(req, CancellationToken.None);
        PingResponse resp = await t;
        Assert.Equal("pong:iso", resp.Reply);
    }

    [Fact]
    public void Mediator_SendAsync_NoAwait_NoCrash()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        PingRequest req = new PingRequest();
        req.Name = "iso";
        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Task<PingResponse> t = mediator.SendAsync<PingRequest, PingResponse>(req, CancellationToken.None);
        Assert.NotNull(t, "task_null");
    }

    [Fact]
    public async Task Mediator_HandlerHandleAsync_Direct()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        IServiceProvider sp = sc.Build();

        IRequestHandler<PingRequest, PingResponse>? handler =
            (IRequestHandler<PingRequest, PingResponse>?)sp.GetService(typeof(IRequestHandler<PingRequest, PingResponse>));
        Assert.NotNull(handler, "handler_resolve_null");

        PingRequest req = new PingRequest();
        req.Name = "iso";
        Task<PingResponse> t = handler.HandleAsync(req, CancellationToken.None);
        PingResponse resp = await t;
        Assert.Equal("pong:iso", resp.Reply);
    }

    [Fact]
    public void Mediator_HandlerHandleAsync_NoAwait()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        IServiceProvider sp = sc.Build();

        IRequestHandler<PingRequest, PingResponse>? handler =
            (IRequestHandler<PingRequest, PingResponse>?)sp.GetService(typeof(IRequestHandler<PingRequest, PingResponse>));
        Assert.NotNull(handler, "handler_resolve_null");

        PingRequest req = new PingRequest();
        req.Name = "iso";
        Task<PingResponse> t = handler.HandleAsync(req, CancellationToken.None);
        Assert.NotNull(t, "task_null");
    }
}
