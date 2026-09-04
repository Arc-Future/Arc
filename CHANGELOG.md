# Changelog

本文件按日期分节记录仓库重要变更（格式参考 Keep a Changelog；括号内为对应提交哈希）。更细粒度登记见 实现规划。

## 1.0.0（2026-09-04）

**Arc 1.0 —— 首个稳定版**。语言、编译器、标准库与运行时的首个正式发布：单一 `arc` 可执行文件 + 源码分发的标准库 + 随包 runtime C 源码（首次构建经内容寻址缓存按需编译），AOT 编译至原生机器码，无 JIT 运行时。

### 支持面

| 项     | 状态                                                                                              |
| ----- | ----------------------------------------------------------------------------------------------- |
| 平台    | Windows x86\_64（安装包交付）；Linux/macOS 构建门禁 CI 绿，安装脚本实机验收通过（harness），安装包产线后续交付                      |
| 工具链   | 捆绑瘦身版 LLVM（clang + lld 子集，完全离线构建）或外部 clang ≥ 22                                                 |
| 标准库   | 37 个子库源码分发（Arc / Collections / Threading / DI / Illusory / UI / Net / Orm / Web / AI / Chord 等） |
| 安装态自检 | `arc doctor` 九项检测（SDK 结构 / clang 基线 / MSVC 探测 / rt\_cache 完整性 / native DLL）                     |
| 发布分发  | `arc release`（签名发布清单）/ `arc self-update` / `arc publish`（`.aopkg` 源码分发包）随 1.0 交付                |
| 验收    | workspace 133 测试批全绿；运行时判据批 bisect 200 + channels 200 + u5 20 轮零失败；Illusory M1 门禁 5 case 全绿      |

### 本周期要点

- **开源准备（GitHub 公开发布）**：版权与署名统一落位 LUSIDA（Start）（LICENSE / Cargo authors / PE 版本资源 / 安装包 version.txt / README 中英双版）；文档清洁——修复 130+ 处相对链接层级错位与陈旧命名（RFC 045 更名残留等），公开文档与内部过程文档（plan / discuss / reviews / proposals）解链；新增 `scripts/release/github-export.ps1` 开源导出（跟踪文件 + 内部资产排除清单 + 发布前安全扫描：绝对路径/邮箱/密钥材料/大文件/占位签名密钥轮换门禁）；**发布签名密钥完成正式轮换**（开发占位密钥退役，新 seed 离线托管于发布者、不入库，编译期内嵌信任锚同步替换）

- **发布分发链补齐（1.0 门槛收口）**：`arc release`（keygen / manifest / verify——Ed25519 签名发布清单，信任锚内置 + `$ARC_RELEASE_PUBKEY` 覆盖）、`arc self-update`（验签 → staging → `--version` 自检 → 原子提交 → `--rollback`，指针 re-exec 与 AV 瞬时锁容忍）、`arc publish`（`.aopkg` 源码分发包：FILES 完整性清单 + 可选分离签名 + `--verify` 消费端校验）、`arc-install.sh` 补 `--ca` 与解压布局加固并以 harness 实机验收（WSL2 Ubuntu 端到端 10/10；`scripts/packaging/verify-arc-install.sh` CI 可复跑）——分发以**源码打包形态**回归，依赖求解体系维持裁撤（RFC 031 §13 / RFC 017 禁止项修订）

- **任务图竞态收敛**：八处协议级修复（follower 链全局锁、WhenAll 聚合器双竞态、poll\_inner 纳锁、NOTIFIED 双向验证、el 等待链心跳预算、AB-BA 破环、注册表幂等保护、Delay/WhenAll 任务 slab 注册）——收敛判据全量重验零失败

- **Illusory M1 门禁点亮**：首批系统性踩中标记接口与 struct 值语义盲区，八处编译器/运行时协议缺陷修复（标记接口 itable、Copy struct 字段悬垂、重载接口方法槽序、实参接口包裹、foreach 元素类型、struct 静态字段默认值、泛型型参 is 折叠、泛型转调单态化传播）

- **接口相等语义**：接口元素 List 的 Remove/Contains/IndexOf 与 `==` 按底层对象身份比较（内联 + stub 双路径）

### 已知限制

- 官方发布端点（`manifest.json` 托管，现占位 `static.arc.dev`）定版与 Linux/macOS 安装包（tar.xz 产线）为外部依赖待交付；`arc self-update` 分发容器统一 zip

- 标准库以源码分发：项目首次构建按需编译 runtime C（内容寻址缓存后增量）

## 2026-09-02

### RFC 045 D14：Chord 类型化服务与 DI 融合（以终为始·API 面以 C# 惯性收敛）

- **类型即契约**：`Provide<T>(T)` / `Provide<T>(Func<T>)`（工厂按需构造，首次解析构造并缓存，MEDI 工厂语义同构）/ `GetService<T>()` / `HasService<T>()` / `Inject<T>((ctx, value) => …)` / `InjectReactive<T>` 以 `typeof(T).FullName` 派生键——零魔法字符串、零强转、值直入回调；字符串名形态全保留（运行期动态名场景），单一分工律：契约按类型、通道按语义名

- **DI 融合**：`ChordContext` 持有 `IServiceProvider`（`IChordContext` 契约注册进 `ServiceCollection`）——类型化解析动态阴影链优先（可逆层）、DI 容器兜底（静态层）；DI 可解析注入依赖恒就绪（无挂起语义）

- **词系**：`Context` → `ChordContext`（实现）+ `IChordContext`（DI 契约），`ContextExtensions` → `ChordContextExtensions`；std/Chord 全程零 Plugin 字眼

- **贡献机制四件套（D11 修订，插件容器热插拔）**：`IContribute`（贡献项，Id 标识）/ `ContributeOptions`（**结构体值类型**：GroupId/Order/ParentId，getter-only + 完整构造，`Group` 命中保留字更名 `GroupId`）/ `IContributeHost`（容器 = 扩展点宿主，Register/Unregister 严格对称）/ `IContributeRegistry`（统一注册表，`Add`/`Remove` 容器热插拔 + `Register`/`Unregister` 按 hostId 定向；实现 `ContributeRegistry`）——纯库契约剥离语言核心，可逆性经 Effect 账本在扩展层组合；`IContributionPoint<TEntry>` 退场。**融合分层**：`IContributeRegistry` 直注 `ServiceCollection` 即组装基座（无需上下文直接拆装 = 热插拔），`IChordContext` 体系为编排增强层（账本可逆/事务/回滚/准入/热替换）；命名家族化：Contribute / Chord / Tone 三族前缀自洽

- **codegen：`Type.FullName`** **唯一限定名落地（RFC 018 M2）**：layout 层新增 `type_full_names`（HIR namespace 经 `type_fqn` 拼接，键与各布局表同源），`emit_typeinfos` 四循环（interface/class/struct/enum）发射 `name`/`full_name`/`ns` 三常量——full\_name/ns 为真实点分限定名；`name` 与 `type_id` 哈希输入不变（RFC 026 `type_name_to_id` 勿动共识）；RuntimeType 注释对齐

- **验证**：`arc build std/Chord --dynamic` 全绿（Arc/Arc.DI/Arc.Chord 三库构建）；typeck/mir/codegen 测试全绿（含新增 `ProgramLayouts.type_full_names` 面）；QIF 语料扩至类型键/工厂/DI 兜底/贡献四件套用例（语料 typeck 仍受既有 lambda unify 缺口阻断，见 plan.md 登记）

## 2026-09-01

### 语言核心洁净度：贡献机制残留清收（核心裁决收尾）

- codegen 载体发射全删：`ContributionsMeta`、`@__arc_contributions`(+count) IR 嵌入与导出面注入、单测 `contribution_carrier_emit.rs`——收集器 `contributions.rs` 已删后的发射端孤儿清零，编译器核心不再残留任何贡献机制面

- std 记账面退场：`std/Arc` 四件套（`ContributionAttribute`/`ContributionDescriptor`/`ContributionArg`/`IContributionRegistry`）与 `std/DI/InjectRegistry`、`std/AI/Agent/Tools/AIToolRegistry` 删除；`InjectAttribute`/`AIToolAttribute` 改直接派生 `Attribute`（`[Inject]`/`[AITool]` 静态绑定合成不受影响）

- 过期注释与文档同步：合成宿主唯一为 `__AIToolHost`（`generate_ai_tool_host`/`maybe_inject_ai_tool_host`/`maybe_inject_di_bindings` 更名对齐）；RFC 012 历史注记、RFC 045 索引边界、领域文档与 ArcAgent 示例改指「显式静态注册」

### 同批在途工作落地（工作树既有 WIP，组合全量验收）

- `CLAUDE.md` → `AGENTS.md` 权威迁移：CI spec-guard 与 `arc-language` 规则引用同步

- `--emit-llvm`（keep\_ir）产物域贯通：CLI/pipeline/equipment/codegen/arc-tests 全链 + runtime debug/ABI 配套、mir/parse 局部修复

- arc-ui/ARML 样式体系增强：`arc-ui` codegen/verify/ast 扩面、`arml_style` 测试扩量、`std/UI/Core/Styling` 与 Markup/Rendering 配套重构

### 插件内核更名与 RFC 045 修订（Arc.Chord）

- `Arc.Plugins` → `Arc.Chord`：chord 是 arc 上两点的连线（内核即连接物）+ 心弦词根与 Cordis 同族；`std/Plugins/` → `std/Chord/`（git mv 保历史）、15 文件 namespace 与 arc.toml 同步、入列 std workspace members（30 子库）；docs 全链更名（RFC 045/017、SUMMARY、domain 索引、plugins.md → chord.md）

- RFC 045 修订：新增架构分层（显式静态注册 / 内核 / 贡献点四层单轨）、D11 贡献点（`IContributionPoint<TEntry>` + `Contribute` 副作用语义，VSCode contributes 运行期化）、D12 依赖声明（`IPluginDependencies.Requires` 挂起准入/启动序推导）、D5.1 瀑布事件（`OnWaterfall`/`Waterfall` 同步 next 委托）、D13 组合即数据（另立 RFC）；验收要点扩至 11 项

### Chord 内核实现与编译器缺口登记

- **去 plugin 化**：`IPlugin`→`ITone`（音）、`Plugin(...)`→`Context.Tone(...)`、`IPluginDependencies`→`IToneRequirements`、作用域名 plugin→tone、RFC 文件 045-plugin-kernel.md→045-chord.md——文件名/类名/方法零 plugin 字眼

- **Context 门面落码（D1–D12 全语义）**：副作用账本、动态服务阴影注册、注入就绪/挂起/丢弃/反应式回滚重跑、事件三级广播、瀑布管道、副作用事务、失败回滚、热替换（原位保序）、依赖准入挂起唤醒、贡献点扩展；QIF 语料 `examples/UnitTest/Chord/` 三用例类 33 Fact

- **编译器修复**：`parse_program_in_file` 剥离 UTF-8 BOM（外部编辑器产物容忍）

- **编译器缺口登记**（阻断 Chord QIF 语料运行，详见 plan.md）：`string[]?` parse 静默错位 / 赋值表达式 lambda 体 / bare `throw;` / lambda→`Action<object?>` unify / 库模式 `arc build std/Arc` MIR panic / `AIWriteAuditEntry`（并发 WIP）

### 编译器缺口架构级修正（第一梯次）

- **统一类型后缀文法**（`ty.rs`）：`?` 升级为每层复合类型的后缀运算符——`string[]?`/`string?[]?` 均合法（此前 `?` 仅基类型级消费，遗留 `?` 被语句层误吞为三元，产生静默解析错位）；**库模式 MIR** **`unresolved ident 'int'`** **panic 随之根除**（确证为 parse 错位下游），`arc build std/Chord --dynamic` 327 文件 parse 零错误实证

- **bare** **`throw;`** **语言级支持**：Parser 引入 catch 绑定栈，裸重抛脱糖为 `throw <绑定名>`（合成名/实名一视同仁），rt\_\* 零改动；非 catch 上下文显式报错（对齐 C# CS0156 家族）

- **UTF-8 BOM 容忍**：`parse_program_in_file` 入口剥离 `\u{FEFF}`

- **待专项登记**（plan.md ④⑤⑥）：库模式 typeck 依赖解析缺陷（`--dynamic` 泛型实参丢失/using 解析失败，exe 模式同代码全绿）、lambda→`Action<T>` unify、赋值表达式四层落码

## 2026-08-29

### 语言核心与诊断

- P1/P12：`BlockingCollection` 构造第一实参约束前移 typeck 诊断，`emit_call` 用户可达 panic 清零（c2f5895e）

- 泛型体系加固：field\_check 验证器、诊断去重管道、UI Markup 框架（3908f3c7）

- LSP 兜底：arc-server dispatch 级 panic 兜底 + 全部锁中毒恢复（1ae17573）

### 工具链收敛

- 锁定工具链 + rustfmt 全库收敛 + correctness 级 lint 真修复（231c6e20）

- workspace.lints 统一基线 + 全库 clippy 真修复（da692318）

- P0-3b/c clippy 长尾清零 + rustfmt 全库收敛（8283c90c），并登记 `.git-blame-ignore-revs`（162be055）

## 2026-08-27

- 修复泛型接口转换与运行时服务注入崩溃（409c4d80）

## 2026-08-26

- QIF 框架量产收口合并里程碑 + RFC 009 M6 多线程 Executor（ae314b95）

- 实现 GAP #5 delegate 委托类型支持（be286d4a）——plan.md 基础面缺漏 #5 关闭

- RFC 引用编号统一至 037 标准（f08530c7、514c6796）

- codegen/eh 平台属性辅助函数提取（85a8b32b）

## 2026-08-25

### 测试重构：进程内快测 + 批量门控分层

- 新增 `arc-tests` 进程内快测框架与首批批量测试用例（e695fc58）：L1 `arc::compile_file`（256MB 大栈线程 + 全局锁串行，`assert_compiles` / `assert_rejected`）+ L2 `build_and_run_batch`（`full-rt` feature 门控）

- `arc-integration` 集成测试包退场，workspace 移除成员（a2627a0f）；验证矩阵切换为 `cargo test --workspace` / `cargo test -p arc-tests`（运行时面 `--features full-rt`）

### 语言面

- 浮点字面量双精度/单精度后缀 + 多字段声明支持（d864d366）——基础面缺漏 #3/#4 关闭；新增 L1 工件布局批量回归 `l1_artifact_layout_batch.rs`

## 2026-08-24

- 修复批量同名类型匹配与接口返回值悬垂指针（ebe41de2）

- 整理修复多类编译与运行时问题（96ab8b55）

## 2026-08-23

### 拟真引擎与文档

- 拟真引擎 P0 离屏 3D 渲染探针 + wgpu 回读对齐缺陷修复（1fde9d2c）

- Arc.UI 拟真引擎文档与领域导航（365eecc9）

### 类型系统与代码生成

- comdat 前置：跨命名空间同名类在 layout/typeck 层 FQN 物化与路由（704ed6e1）；推进现状与验收教训登记（98561c0c）

- 闭包重构：单块捕获簇 + 定形 capture struct，2×malloc→1×malloc（af872ddd）

- await-in-loop→coro 推广：While/LinqForeach/CFG backedge 分支放开，循环内 await 走 pre-split 协程（890b6665）

- 阶段 3/4 缺陷校准：null\_flow 跨函数泄漏修复 + reachability 契约对齐（1f198a7f）

### 测试批迁移（arc-integration 内部，一次编译一次运行）

- text 域 6 case（c81bc40a）、ternary 域 10 case（87500a90）、nullable\_boxing 6 case（82d4b863）入批，删除对应旧 e2e 文件

### 底座收敛

- 落实双层架构裁决：`Task.ContinueWith` 残面全面删除（std stub + typeck 分支 + codegen 发射 + runtime 实现），合法表面 WhenAll/WhenAny/Run/Delay 不动（c72d9fde）；ContinueWith 消除与 TCS 保留机制登记（bc7a1c69）

<br />
