# Arc.Web

## 概述

`Arc.Web` 是 Arc 的轻量云服务框架。它以**单宿主 `WebApplication`** 装配服务、以**内置 IMediator** 承载请求处理、以**特性自动路由**把 `IRequest` 即端点、以 **SSR 声明式 HTML** 输出网站、以 **`IWebResult`** 提供多态页面结果、以 **gRPC 集成层**接入 `Arc.Net.Grpc`。

它抛弃 ASP.NET Core 体系糟粕（中间件/过滤器双轨、控制器、`HttpContext` 上帝对象、`IOptions` 间接层、程序集扫描、Startup 仪式、minimal/controller 双轨），走单一惯用法——同一机制一条正道。

### 分层架构

| 层 | 命名空间 | 职责 |
|----|----------|------|
| Core 宿主 | `Arc.Web` | `WebApplication` + IMediator + 路由 + 绑定 + 鉴权 + 配置 |
| gRPC 集成 | `Arc.Web.Grpc` | 把 `Arc.Net.Grpc` 核心挂进宿主 |
| SignalR 集成 | `Arc.Web.SignalR` | 基于 WebSocket 的实时通信 |
| 扩展点 | `Arc.Web.*` | 更多领域扩展 |

网络协议层（HTTP/TCP/WebSocket/QUIC）见 [networking-p2p.md](networking-p2p.md)；本册只讲 `Arc.Web` 框架装配与网站能力。

## 快速开始

### 1. 宿主装配

`WebApplication` 一次性装配 DI 与端点，启动即服务：

```as
using Arc.Web;

var app = new WebApplication();

app.AddServices(services => {
    services.AddScoped<IUserService, UserService>();   // 复用 std/DI，仅注册领域服务
});

app.MapGet<GetUserRequest, UserDto>("/api/users/{id}");
app.MapPost<CreateUserRequest, UserDto>("/api/users");

await app.RunAsync(8080);
```

`MapGet`/`MapPost` 显式注册端点（模板 + 泛型分发器）。特性自动路由把路由 + 方法 + 绑定集中在请求类型上。

### 2. 声明请求与处理器（IRequest 即端点）

请求类型 = 端点声明，处理器为纯 handler 函数：

```as
using Arc.Web;

[Get("/api/users/{id}")]
public class GetUserRequest : IRequest<UserDto> {
    public int Id;
}

public class GetUserHandler : IRequestHandler<GetUserRequest, UserDto> {
    public async Task<UserDto> Handle(GetUserRequest request, CancellationToken ct) {
        var user = await userService.FindByIdAsync(request.Id, ct);
        return user;
    }
}
```

### 3. 应用内调用（同一管道）

应用内部直接调用与 HTTP 端点走同一条路径（单一惯用法）：

```as
UserDto user = await mediator.SendAsync<GetUserRequest, UserDto>(
    new GetUserRequest { Id = 1 }, ct);
```

### 4. SSR 页面

网站开发采用 WPF xaml+cs 心智模型：页面标记 = 标准 HTML + 三标记绑定，code-behind = `PageHandler` 基类。

`home.html`：

```html
<main>
  <h1>{{Title}}</h1>
  <ul>
    <li a-for={post in Posts}>
      <a href={post.Slug}>{{post.Title}}</a>
      <time>{{post.PublishedAt}}</time>
    </li>
  </ul>
  <p a-if={Empty}>暂无文章</p>
  <div a-html={IntroHtml}></div>
</main>
```

`HomePage.as`：

```as
using Arc.Web;

[Get("/")]
public class HomePage : IRequest<IWebResult> { }

public class HomePageHandler : PageHandler<HomePage> {
    public override async Task<IWebResult> Handle(HomePage request) {
        var posts = await mediator.SendAsync(new ListPostsQuery(), ct);
        return View(new HomeModel { Title = "Arc Blog", Posts = posts, Empty = posts.Count == 0 });
    }
}
```

三标记绑定模型：`{{ }}` 文本插值 · `attr={ }` 属性绑定 · `a-` 短前缀指令（`a-for`/`a-if`/`a-html`）。绑定 = 属性路径（WPF 思路），复杂逻辑在模型预计算，模板无任意表达式/语句；绑定路径编译期对照模型类型解析，绑错即编译期报错。框架默认安全转义（上下文感知），XSS 由框架兜底。

## 核心 API

### WebApplication —— 单宿主

| 成员 | 说明 |
|------|------|
| `AddServices(Action<IServiceCollection>)` | 装配领域服务（复用 `std/DI`） |
| `MapGet<TReq,TResp>(template[, roles])` / `MapPost` / `MapPut` / `MapDelete` / `MapPatch` | 显式注册端点（完整 HTTP 动词面；模板 + 泛型分发器；可选 roles 声明） |
| `RunAsync(port)` | 启动 HTTP 服务，路由匹配 + 绑定 + 分发 + JSON 往返 |

路由语义：路径不存在 → **404**；路径存在但方法未注册 → **405 Method Not Allowed**（REST；`EndpointRegistry.MatchAnyMethod` 区分）。

配置：框架默认 `IConfiguration` 自动解析 `app.json`，`Get<T>()` 强类型反序列化；砍 `IOptions` 间接层。DI 复用 `std/DI`，宿主内建每请求作用域。

### IRequest / IRequestHandler / IMediator

| 契约 | 说明 |
|------|------|
| `IRequest<TResponse>` | 命令/查询请求契约；请求类型 = 端点声明 |
| `IRequestHandler<TRequest, TResponse>` | 处理器契约，`Handle(request, ct)` |
| `IMediator` | 接口泛型方法 `SendAsync<TReq,TResp>`（单 handler）+ `PublishAsync<TNotif>`（多 handler）；唯一横切面载体；泛型分发不依赖静态扩展（mediator generic-dispatch verdict (internal proposal)） |

| 方法特性 | 路由模板 |
|----------|----------|
| `[Get("/api/users/{id}")]` | GET |
| `[Post("/api/users")]` | POST |
| `[Put(...)]` / `[Delete(...)]` / `[Patch(...)]` | PUT / DELETE / PATCH |

绑定约定：路径 `{id}` → 同名标量；GET/DELETE 查询串 → 同名属性；POST/PUT/PATCH body → `Arc.Text.Json` 反序列化（PATCH 与 PUT 同走 body，不落入路径参数分支）。请求上下文收窄为显式 `RequestContext`（无 `HttpContext` 上帝对象）。

### 横切面与鉴权

| 面 | 说明 |
|----|------|
| `IPipelineBehavior` | **唯一**横切面管道（合并中间件/过滤器双轨） |
| `[Authorize]` | 鉴权标记 |
| `IAuthorizationBehavior` | 可扩展鉴权行为（默认角色鉴权，可扩展动态鉴权/DB 支撑） |

认证方默认不内置；token/cookie → principal 认证面可插拔。校验走 `IPipelineBehavior`（无 `DataAnnotations`/`ModelState` 自动 400 耦合）。

### SSR 与 IWebResult

| 形式 | 形态 | 安全 |
|------|------|------|
| 文本插值 | `{{Property}}` | 默认转义 |
| 属性绑定 | `attr={Property}` | 默认转义（上下文感知） |
| 原始 HTML | `a-html={Property}` | 显式退出 |
| 循环 / 条件 | `a-for={x in Xs}` / `a-if={B}` | — |

`IWebResult` 多态结果族：`IHtmlView`（视图）/`IRedirectResult`（重定向，PRG）/`IFileResult`（文件/二进制）。`PageHandler` 提供 `View(T)`/`Partial(T)`/`Redirect(url)`/`File(data, contentType)`。HTML 页面与 JSON API 走同一条 `IMediator`/`IPipelineBehavior`/DI 管道。

支持组件/Layout 标记级片段复用、表单往返校验回显、会话态（cookie 面）、流式 TTFB、基础静态资源服务（MIME + 缓存头）、部分内容响应（206 Partial Content / Range 按需切片）。

### gRPC 集成

`Arc.Web.Grpc` 把 `Arc.Net.Grpc` 核心（HTTP/2 传输、四种调用类型）挂进 `WebApplication`：unary 调用 → `IMediator.SendAsync`，复用同一管道与鉴权。gRPC 核心与宿主解耦、独立可复用；`Arc.Web.Grpc` 仅作集成适配层，不重复实现协议。

## 边界

- **网络协议层**（HTTP/TCP/WebSocket/QUIC、HTTP/1.1/2/3）见 [networking-p2p.md](networking-p2p.md)；本册只讲 `Arc.Web` 框架装配与 SSR。
- **gRPC 协议核心**（`Arc.Net.Grpc`：HTTP/2 传输、四种调用类型）见 [networking-p2p.md](networking-p2p.md)。
- **DI 容器 / 配置**见 [di.md](di.md)；本册只讲宿主装配方式。
- **网站定位**为「SSR 内容站点 + 前端应用后端」；Arc 原生非同构，无客户端运行时/JS 水合，不对标 Next.js。

---

上一节：[orm.md](orm.md) · 下一节：[networking-p2p.md](networking-p2p.md)