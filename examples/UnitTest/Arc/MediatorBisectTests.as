namespace UnitTest.Arc;

using Arc;
using Arc.DI;
using Arc.QIF;
using Arc.Web;

public class MediatorBisectTests
{
    [Fact]
    public void ScopeHandler_ResolveAndDispatch()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        IRequestHandler<PingRequest, PingResponse>? handler =
            (IRequestHandler<PingRequest, PingResponse>?)scopeSp.GetService(typeof(IRequestHandler<PingRequest, PingResponse>));
        Assert.NotNull(handler, "handler_resolve_null");
        Task<PingResponse> t = handler.HandleAsync(new PingRequest(), CancellationToken.None);
        Assert.NotNull(t, "task_null");
    }

    [Fact]
    public void ScopeMediator_CastOnly()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Assert.NotNull(mediator, "mediator_null");
    }

    [Fact]
    public void MediatorIface_ToConcrete_Downcast()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Mediator m2 = (Mediator)mediator;
        Assert.NotNull(m2, "m2_null");
    }

    [Fact]
    public void ProviderGetter_BoxToIface()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Mediator m2 = (Mediator)mediator;
        object? providerObj = m2.Provider;
        Assert.NotNull(providerObj, "providerObj_null");
    }

    [Fact]
    public void ProviderGetter_BoxToIface2()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Mediator m2 = (Mediator)mediator;
        IServiceProvider provider = m2.Provider;
        Assert.NotNull(provider, "provider_null");
    }

    [Fact]
    public void ProviderGetter_DispatchGetService()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        Mediator m2 = (Mediator)mediator;
        IServiceProvider provider = m2.Provider;
        object? h = provider.GetService(typeof(IRequestHandler<PingRequest, PingResponse>));
        Assert.NotNull(h, "handler_null");
    }

    [Fact]
    public void ScopeMediator_SendAsync_NoAwait()
    {
        ServiceCollection sc = new ServiceCollection();
        sc.AddTransient<IRequestHandler<PingRequest, PingResponse>, PingHandler>();
        sc.AddScoped<IMediator, Mediator>();
        IServiceProvider sp = sc.Build();
        IServiceScope scope = sp.CreateScope();
        IServiceProvider scopeSp = scope.GetServiceProvider();

        object? mediatorObj = scopeSp.GetService(typeof(IMediator));
        IMediator mediator = (IMediator)mediatorObj;
        PingRequest req = new PingRequest();
        req.Name = "iso";
        Task<PingResponse> t = mediator.SendAsync<PingRequest, PingResponse>(req, CancellationToken.None);
        Assert.NotNull(t, "task_null");
    }
}