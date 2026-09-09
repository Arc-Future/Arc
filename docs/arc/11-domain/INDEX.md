# 第十一章 · 领域库使用

> **目标**：按领域选用 UI / AI / ORM / Web / Net / DI / Chord。  
> **说明**：本册讲「如何用」；设计取舍见仓库 [`docs/rfc/`](https://github.com/Arc-Future/Arc/blob/main/docs/rfc/index.md)（不进本发布包）。

## 目录

| 小节 | 内容 | 难度 |
|------|------|------|
| [Arc.UI](ui.md) | ARML、主题、wgpu 唯一后端 | 中级 |
| [Arc.Agent](ai-host.md) | 会话与工具 | 中级 |
| [Arc.AI](ai-inference.md) | Tensor 与推理 | 中级 |
| [Arc.Orm](orm.md) | DbContext 与查询翻译 | 中级 |
| [Arc.Web](web.md) | WebApplication 与 SSR | 中级 |
| [Arc.Net](networking-p2p.md) | HttpClient / WebSocket / P2P | 中级 |
| [Arc.DI](di.md) | 注册与生命周期 | 入门 |
| [Arc.Chord](chord.md) | 插件内核 | 进阶 |
| [拟真引擎（规划）](realism-engine.md) | 规划中能力 | 入门 |

## 建议阅读顺序

1. [di.md](di.md) → 宿主装配基础  
2. [ui.md](ui.md) / [web.md](web.md) → 界面或服务端  
3. [orm.md](orm.md) / [networking-p2p.md](networking-p2p.md) → 数据与网络  
4. [ai-host.md](ai-host.md) / [ai-inference.md](ai-inference.md) → AI 面  
5. [chord.md](chord.md) → 插件与热替换  

---

[工具链](../10-toolchain/INDEX.md) · [进阶与设计](../12-advanced/INDEX.md)
