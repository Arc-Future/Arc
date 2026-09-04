# ArcAgent — 真实编程智能体示例

用 **最少的 Arc 代码**实现一个**真实可用的编程智能体**：接入 DeepSeek 真实大模型，通过**声明式** **`[AITool]`** **工具系统**读写文件、执行命令，并借 HITL 人审机制保护敏感操作。

本项目作为 **Arc 标杆参考项目**：按职责划分目录、一个文件一个类、命名空间与目录对应、声明式能力注册，体现项目级编码规范。

对应的 Arc.AI 能力评估见 [RFC 038（AI 宿主，唯一权威）](../../docs/rfc/038-ai-host.md)。

## 一、为何能落地（Arc.AI 完整度结论）

Arc.AI 已具备构建真实编程智能体的全部要件，**结论：具备条件**。

| 智能体要件     | Arc.AI 支撑                                                      | 本示例使用                                      |
| --------- | -------------------------------------------------------------- | ------------------------------------------ |
| 真实 LLM 接入 | `DeepSeekChatClient`（HTTP + SSE 流式 + TLS 1.3 系统根证书校验）          | ✅ `DeepSeekOptions` / `DeepSeekChatClient` |
| 声明式工具     | `[AITool]` 属性（编译期生成元数据 + `__AIToolHost` 自动装配）                  | ✅ 声明式注册，零手写 handler                        |
| 能力门禁      | `AICapabilitySet` 白名单 + `AIToolSandbox` fail-closed            | ✅ `fs.Read`/`fs.Write`/`shell.Run` 白名单     |
| 多轮工具循环    | `AISession.RunAsync` 回合状态机 + 工具循环护栏                            | ✅ REPL 外层调用                                |
| 文件/目录操作   | `File` / `Directory` / `Path`（L2 Stable 面）                     | ✅ `read_file` / `list_dir` / `write_file`  |
| 命令执行      | `Process.RunCapture`（并发读 stdout/stderr 防死锁）                    | ✅ `run_command`                            |
| 命令/写入审批   | `AIHumanGate` + `RequireApproval` + `ApproveAsync/ResumeAsync` | ✅ HITL 门闩循环                                |
| 流式输出      | `AISession.TextDelta` 事件 + `IsStreaming`                       | ✅ 边生成边显示                                   |
| 上下文工程     | `AIContextEngine` + Instructions 前缀缓存（对齐 DeepSeek KV Cache）    | ✅ `opts.Instructions`                      |

## 二、项目结构

```
examples/ArcAgent/
├── arc.toml                 # 包清单：依赖 Arc.Agent / Arc.Agent.DeepSeek / Arc.Agent.Harness / Arc.Agent.Harness.Coding
├── Program.as               # 入口（无 namespace，C# Program.cs 惯例）：装配 + REPL + HITL 门闩
└── ArcAgent/                # 根命名空间 ArcAgent（= package.name）
    ├── Host/
    │   └── AgentHost.as     # 组合根 `ArcAgent.Host`：Provider + 工作区 + 记忆装配（单一组装点）
    ├── Context/
    │   ├── AgentContext.as  # 上下文工程：Wiki 记忆持久化 + 知识面注入 + 上下文源工厂
    │   └── ProjectConventionsProvider.as  # 自定义上下文源：.arcagent/conventions.md → Rules 层
    ├── Workspace/
    │   └── AgentWorkspace.as # 工作区：目标仓库根 + AIWorkspace 沙箱 + git 状态摘要
    └── Tools/               # 能力层 `ArcAgent.Tools`：按职责分类，一类一文件，声明式 [AITool]
        ├── FSTools.as       # read_file / list_dir / search_text / write_file / copy_file / delete_file / edit_file
        ├── RepoTools.as     # grep_search / git_status / git_diff（递归搜索 + git 只读查询）
        └── ShellTools.as    # run_command（shell.Run，RequireApproval）
```

> **Harness（RFC 043）**：依赖 `Arc.Agent.Harness` + `Arc.Agent.Harness.Coding`；`arc_build` / `arc_test` / `arc_check` / `arc_inspect` / `arcgr_query`（能力 `quality.Verify`，不进计划门闩）由标准库（`Arc.Agent.Harness.Coding`）声明式装配。`Program.as` 持有 `AIHarnessSession`；组合根只组装、不重复实现质量门。

> **项目级规范（标杆）**：
>
> - **一个文件一个类**：每个源文件声明一个公开类型，职责单一；工具按**职责范围**分类（FS 一组、shell 一组），每类一文件。
>
> - **目录即命名空间**：`ArcAgent/Host/` ↔ `namespace ArcAgent.Host`，`ArcAgent/Tools/` ↔ `namespace ArcAgent.Tools`；根命名空间 = `package.name`。
>
> - **声明式能力**：工具方法用 `[AITool]` 标记（名称/能力/审批），描述由 `[Description]` 组合（方法级=工具描述、参数级、模型字段级）；编译期生成描述符 + 包装 handler，实例化 `AIHost` 即自动装配——**零手写** **`AIToolHandler`** **/ 零手动** **`tools.Add`** **/ 零显式 registry 调用**。
>
> - **组合根收敛**：`Program.as` 只做装配与交互编排，领域能力下沉到按职责命名的类。

## 三、工具清单

| 工具            | 能力               | 审批    | 参数 (JSON)                  | 说明                                                           |
| ------------- | ---------------- | ----- | -------------------------- | ------------------------------------------------------------ |
| `read_file`   | `fs.Read`        | 否     | `{path}`                   | 读取文件全文（异步）                                                   |
| `list_dir`    | `fs.Read`        | 否     | `{path}`                   | 枚举文件与子目录（非递归，异步）                                             |
| `search_text` | `fs.Read`        | 否     | `{path, pattern}`          | 关键词搜索，返回 `行号: 内容`（异步）                                        |
| `write_file`  | `fs.Write`       | **是** | `{path, content}`          | 覆盖写入文件（异步）                                                   |
| `copy_file`   | `fs.Write`       | **是** | `{src, dst}`               | 复制文件（异步）                                                     |
| `delete_file` | `fs.Write`       | **是** | `{path}`                   | 删除文件（异步）                                                     |
| `edit_file`   | `fs.Write`       | **是** | `{path, oldText, newText}` | 定点替换唯一文本片段（异步；`oldText` 须唯一出现）                               |
| `grep_search` | `fs.Read`        | 否     | `{root, keyword}`          | 目录递归关键词搜索，返回 `path:行号: 内容`（上限 200）                           |
| `git_status`  | `fs.Read`        | 否     | `{repo}`                   | git 工作区状态（short 格式，只读）                                       |
| `git_diff`    | `fs.Read`        | 否     | `{repo}`                   | git 未暂存差异（只读）                                                |
| `run_command` | `shell.Run`      | **是** | `{command}`                | 执行 shell 命令并捕获输出（异步）                                         |
| `arc_build`   | `quality.Verify` | 否     | `{project}`                | Harness：`arc build`（D0；来自 `Arc.Agent.Harness`）               |
| `arc_test`    | `quality.Verify` | 否     | `{project}`                | Harness：`arc test`（D3）                                       |
| `arc_check`   | `quality.Verify` | 否     | `{file}`                   | Harness：`arc check`（快速 typeck）                               |
| `arc_inspect` | `quality.Verify` | 否     | `{file, format}`           | Harness：`arc inspect`（D1 `.arcgr` 语义索引，`--format json` 机器可读） |
| `arcgr_query` | `quality.Verify` | 否     | `{kind,arcgr,symbol}`      | Harness：`.arcgr` 语义查询                                        |

> 所有工具统一遵循 **异步契约**（RFC 038 §3.1.1）：方法签名返回 `Task<string>`，内部调用 `File.ReadAllTextAsync` / `Process.RunCaptureAsync` 等异步 API；无原生异步的 `Directory`/`Delete` 以 `Task.Run` 后台线程包装。工具执行不阻塞 Agent 主流程。

审批工具（`write_file` / `copy_file` / `delete_file` / `run_command`）在描述符上置 `RequireApproval = true`，执行前触发 HITL 门闩；只读工具（`read_file` / `list_dir` / `search_text`）即时执行。

## 四、运行

```bash
# 1. 设置 DeepSeek API 密钥（真实模式）
set ARC_DEEPSEEK_API_KEY=sk-xxx

# 2. 编译并运行
cargo run -p arc -- run examples/ArcAgent/Program.as
```

> 必须设置 `ARC_DEEPSEEK_API_KEY`——Agent 真实接入 DeepSeek，无假 Provider 离线模式。

交互示例：

```
> 列出当前目录
[tool] list_dir
[reply] D C:\...\ArcAgent
        F C:\...\arc.toml
        ...
> 在 hello.txt 中写入 "Hello, Arc!"
[tool] write_file
[approval] tool=write_file
[approval] args={"path":"hello.txt","content":"Hello, Arc!"}
approve? [a]pprove / [e]dit args / [r]eject: a
[reply] 已写入 hello.txt
```

### `/run` —— 一句话需求 → AIRfc → 计划树 → 子代理 → 汇总门（实战 P1-3）

```bash
> /run 在项目里写一个打印 hello 的 Program.as 并 arc build 通过
[run] 立项 RUN-1 v1
[run] 计划树已装配并批准（1 步）
[run] 子代理 W1 Completed (2/4 步)
[run] 汇总门 绿（全 Passed）
[run] 完成：/dod 复核或 /summary 小结可继续
```

`/run` 走通「一句话需求 → `AIRfc` 立项（`SetRfc`）→ 计划树（`AIPlanGate.SetPlan` + 批准）→ 单子代理（`AIParallelCoordinator`，并行度 1、`MaxStepsPerSubAgent` 小值）→ 汇总门（`RunAggregatedGatesAsync` D0–D7）」。**务实范围**：先接「单子代理」路径可跑；多子代理并行与子代理写工具 HITL 回环留后续。实现见 `ArcAgent/Repl/RunOrchestrator.as`（薄组装，不重复实现 PM/DoD）。

### 真实连通冒烟（scripts/arcagent-smoke.ps1）

```bash
# 先验证「真实 API 可连通」再进 REPL——key 只读环境变量，不落盘
$env:ARC_AGENT_API_KEY = "sk-xxx"
powershell -File scripts/arcagent-smoke.ps1 --provider deepseek   # 或 agnes / openai
```

可选环境变量 `ARC_AGENT_BASE_URL` / `ARC_AGENT_MODEL` 覆盖 provider 默认；脚本以一句话断言返回非错误（退出码 0 = 通过）。

## 五、代码结构要点

- **组合根**（`Host/AgentHost.as`）：`CreateProvider` 读 `ARC_DEEPSEEK_API_KEY` 环境变量，缺密钥抛错；`BuildOptions` 集中系统指令 + 能力白名单 + 流式。

- **声明式工具**（`Tools/*.as`）：按职责范围分类（`FSTools` / `ShellTools`），`[AITool]` 声明名称/能力/审批，`[Description]` 提供描述（方法级工具描述、参数级、模型字段级）；编译期生成 `AIToolDescriptor`（含参数 Schema，模型参数由字段 `[Description]` 驱动嵌套 schema）+ 包装 handler。**实例化** **`AIHost`** **即自动获得全部工具**（编译器注入 `AIToolRegistry.__RegisterGlobal()` 注册为默认工具源），**零手写** **`AIToolHandler`** **/ 零手动** **`tools.Add`** **/ 零显式 registry 调用**。

- **能力白名单**（`AgentHost.BuildOptions`）：`AICapabilitySet` 显式授权 `ai.Tool` + `fs.Read` + `fs.Write` + `shell.Run`，未授权工具 fail-closed 拒绝（工具自动装配但**真实生效仍靠白名单授权**）。

- **HITL 门闩**（`Program.as`）：`RunAsync` 在需确认工具前返回 `NeedsHuman`，主循环提示审批/编辑/拒绝，`ApproveAsync` + `ResumeAsync` 续回合。

- **`/run`** **编排**（`Repl/RunOrchestrator.as`）：一句话需求 → AIRfc 立项 → 计划树 → 单子代理 → 汇总门（实战 P1-3；薄组装，AIRfc/任务图/并行协调器/汇总门均为框架既有面）。

- **流式输出**：`session.TextDelta` 实时打印生成文本；`ToolInvoked`/`ToolCompleted` 观察工具生命周期。

## 六、扩展方向（最小成本放大能力）

- **更多工具**：在 `Tools/` 下新增一个类 + 一个 `[AITool]` 方法即注册（如 `grep_search`、`read_secret`），无需改动组合根。

- **能力细分**：拆 `fs.Write` 为 `fs.Write{allow}` 等，配合 `AICapabilitySet` 精细授权。

- **模型参数**：工具方法可用 `[Description] UserModel model` 接收复杂参数，schema 与反序列化由模型字段 `[Description]` 自动生成（原 `arc-integration` 声明式 e2e `create_user` 已随该 crate 退场，a2627a0f）。

- **上下文记忆**：利用 `AISession.Wiki` / `WikiPathsToAttach` 注入知识库面。

