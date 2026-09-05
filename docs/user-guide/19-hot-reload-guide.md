# 热重载编排指南

> 面向**插件宿主开发者**与**编译器/运行时协作者**。设计权威见 [RFC 017](../rfc/017-build-artifacts-packages.md)（二进制热卸载）、[RFC 045 D8.1](../rfc/045-chord.md)（组合契约）、[RFC 047](../rfc/047-object-graph-migration.md)（透明对象图迁移）；本篇是三者的**操作化教学**——正确序列、判定原语、以及实施期实证的坑清单。测试锚点全部在 `crates/arc-tests/tests/l2_hot_reload_batch.rs`（6 case）与 `l2_dynamic_load_batch.rs`（9 case）。

> **平台实测面（1.0）**：本篇的 DLL 文件锁、COMDAT/COFF 语义与测试锚点均以 **Windows/COFF** 为实测面；POSIX（`dlopen`/`.so`）侧契约相同，但运行中覆盖/卸载锁定、符号解析与弱定义语义不同，且未随 1.0 验收（平台能力边界见 [11 编译模型](11-compilation-model.md)）——跨平台编排请以 Windows 实测面为基准。

## 1. 三层迁移模型——何时用哪层

| 层 | API | 适用 | 语义 |
|----|-----|------|------|
| **L1 兼容性判定** | `AssemblyHotReload.IsLayoutCompatible(old, new, typeName)` | 换代决策的前置门 | 同名类型**字段布局指纹 + vtable 形状**全等 → 结构兼容；任一未物化或指纹异 → **保守拒绝**（未知 ≠ 兼容） |
| **L2 显式状态搬运** | 宿主代码（读旧对象字段 → 写新对象字段） | 状态需要**映射/变换**的场景 | 迁移路径对使用者可见、可审计——锚点 `hr_state_handover` |
| **L3 透明对象图迁移** | `AssemblyHotReload.MigrateInstances(old, new)` | 同构类型的长驻状态对象（已登记模块根） | vtable 头重绑，**字段内存/对象地址/引用计数全部保持**——零搬运代码；锚点 `hr_transparent_migration` |

**决策序**：L1 判定通过 → 优先 L3（零代码）；判定失败但业务允许映射 → L2；两者皆不可 → 拒绝换代（旧代持续服务）。

## 2. 正确编排序列（D8.1 六步）

```text
1. 编译新代 dll 至独立路径        —— 旧代卸载前被 OS 锁定（Windows/COFF 语义），不可覆盖
2. alc.Load(newPath)             —— Load 不校验类型身份
3. Entry 烟测（指纹门禁）          —— newGen.Entry<T>()；同名异构在此显式失败
4. 内核 Reload / 服务面切换        —— 新代 apply 成功后进入卸旧
5. MigrateInstances + Retire      —— 状态迁移 + 旧代退役（见下）
6. in-flight 收敛由 Freeze 承接    —— rt_library_unload_hot 内建
```

**第 5 步的完整形态**（状态对象为长驻根的正确编排）：

```arc
alc.RegisterModuleRoot(v1, state);            // 长驻状态登记为模块根（进入迁移闭包）
int migrated = AssemblyHotReload.MigrateInstances(v1, v2);
// 迁移后 state 指向同一地址，vtable 已重绑——字段原值、方法走 v2 实现
alc.UnregisterModuleRoot(v1, state);          // 旧代解绑
alc.RegisterModuleRoot(v2, state);            // 新代接管卸载释放责任
AssemblyHotReload.RetireGeneration(alc, v1);  // 旧代退役（三道护栏内建）
```

**三条不可违反的前置/收尾规则**（违反即 AV 或双重释放，均有用例锚定）：

1. **宿主跨界引用在 `Unload` 前必须归零**（`state = null` 或移交给根登记）——根扫描闭包不含宿主栈，残留引用的 dec 会在卸载后命中已解除映射的内存（`hr` 系用例的收尾顺序即此规则的展开）；
2. **模块根登记即移交释放责任**——`release_roots` 在卸载时对已登记根 dec，故收尾必须**先 `UnregisterModuleRoot` 再置 null**（顺序颠倒 = 双重释放）；
3. **`MigrateInstances`/`Retire` 失败抛异常时旧代原样保持**——回滚零成本，禁止吞异常后强行继续换代。

## 3. 判定原语与符号协议

- **类型身份**：`type_name_to_id`（FNV-1a-32 名字哈希）——`typeof(T)`/DI/弱槽共用，**勿动**（RFC 026 三端共识）；
- **布局指纹**：`entry_layout_signature`（FNV-1a-64，沿继承链递归展开字段类型 + 偏移）——物化于 `__arc_package_meta` 的 `#layouts:` 字段，运行时经 `Assembly.GetLayoutSignature(typeName)` 读取；返回 0 = 未物化（旧产物），判定按拒绝处理；
- **vtable 形状指纹**：逐虚槽 `name(params):ret` 序列的 FNV-1a-64，物化于 `__arc_vtable_registry`——**方法签名漂移是字段指纹的盲区**，L3 迁移必须三元全等（layout_sig + shape_hash + slot_count）；
- **vtable 全局为强定义常量**（非 linkonce_odr，见 §4 坑 C3），`.vtable.{T}` 经 `all_exports` 显式导出。

## 4. 实施期坑清单（每条都有失败实证——违反即编译断/运行崩/静默错配）

### C1. std 解析外部数据**禁用 Parse 家族**

`long.Parse` 失败走 **rt_panic（不可捕获，中止整个进程）**；`char` 索引同理高危。解析外部字符串一律 **Substring 比较 + 非 panic 的 `long.TryParse`（ref 参数，支持负号）**，非法段宽容跳过。先例：YamlParser、AIPerfMonitor、Assembly.ParsePackageMeta（三处同源）。**实测**：布局指纹以 u64 十进制发射（超 i64 域）→ TryParse 失败 → 表空 → 判定恒拒绝。

### C2. `__arc_package_meta` 字段扩展必须**自描述前缀**

meta 的依赖循环「读到空段为止」——无标记的后续字段会被**当作依赖名吞掉**（实测 `Dependency not found: HrPayload:5961...`）。新增字段一律带 magic 前缀（如 `#layouts:`），解析循环识别前缀即转入对应解析并终止依赖读取。旧产物无该字段 → 向后兼容（表空 = 保守拒绝）。

### C3. 数字序列化的**域匹配**

发射端 `as u64` 可产生超 i64 域的十进制串，接收端 `long.TryParse`（i64 域）解析失败——**发射端用 i64 表示**（可负号，TryParse 支持负号）。域不匹配的表现是**静默**（解析宽容跳过 → 表空 → 判定恒拒绝），极难归因。

### C4. vtable 全局**强定义**，勿回退 linkonce_odr

`linkonce_odr` + COMDAT 的 vtable 全局在**无引用时 clang COFF 落盘被丢弃**（实测 hr 插件：IR 有定义、.o 符号缺席）——RFC 047 迁移按 dll 解析 `.vtable.{T}`，弱定义不可依赖。强定义在插件 dll 单 TU 编译下无跨包重复风险。

### C5. 纯基类插件的 **typeinfo↔vtable 发射耦合**

typeinfo 恒发射且引用 `.vtable.{T}`，而 vtable 全局仅类被实例化/分派时发射——**「只有虚方法、无实例化点」的纯基类插件会 link 断**（`undefined symbol: .vtable.{T}`）。规避：插件内保留一个实例化哨兵（static Probe）。**emit 层发射对齐缺陷已立案**。

### C6. 插件编译为**单文件直通**——跨插件源码级继承不可见

`compile_plugin_library` 的 deps 只写运行时 meta（自动加载），**编译期**依赖源码不参与——插件继承插件基类必须**每插件内嵌基类副本**（迁移按 vtable slot 语义对齐，同名同签名副本无碍）。源码级共享挂 arc.toml path 依赖课题。

### C7. 未限定名跨命名空间 = 编译容忍、运行时炸

`File` 实际在 `Arc.IO`——缺 `using Arc.IO;` 时编译通过但运行时调用崩。跨命名空间类型调用前**核对 using**。

### C8. 卸载顺序护栏（E_UNLOAD_DEPENDED）与拓扑序

依赖方在载时卸载被依赖方 → 护栏拒载（依赖方名单随异常报告）；`UnloadAll` 已按依赖拓扑序编排。**自写卸载循环者**必须遵守「依赖方先于被依赖方」，或直接使用 `UnloadAll`。

## 5. 测试锚点索引

| 用例 | 验证语义 |
|------|---------|
| `hr_seamless_generation_swap` | 正序换代全链路（多代共存 → 烟测 → 切面 → 卸旧 → 新代持续） |
| `hr_fingerprint_gate_rollback` | 指纹门禁：同名异构 → 显式失败 → 旧代持续服务 |
| `hr_orchestrated_swap` | 编排 API 形态（IsLayoutCompatible + RetireGeneration） |
| `hr_state_handover` | L1 判定 + L2 显式搬运（mutant 保守拒绝对照） |
| `hr_transparent_migration` | L3 透明迁移（模块根登记 → 重绑 → 旧代退役后对象存活） |
| `hr_virtual_dispatch_after_migration` | 迁移后基类虚分派命中新代实现（三段铁证） |
| `u5_unload_order_guard` | 卸载顺序护栏（E_UNLOAD_DEPENDED 拒载 + 名单报告） |
| `u5_entry_call_roundtrip` | Entry 全链路 + Unload 前置条件（引用清理） |
| `u5_entry_layout_drift_detected` | 同名异构 → 加载期显式失败（指纹哨兵） |

## 6. 已立案遗留（推进时先读）

- **L3 方法分派的 itable 深度验证**：接口分派（itable 跨代）的取证在虚分派（vtable slot）之外，随接口迁移场景补；
- **emit 层 typeinfo↔vtable 发射耦合**（C5 的根修）：typeinfo 恒发射而 vtable 按需发射的不对称；
- **跨插件源码级继承**（C6 的根修）：arc.toml path 依赖的编译期展开；
- **net 批 accept-null flaky**：IOCP accept 完成路径竞态（ARC_DEBUG_NET 埋点已留存，已移交独立任务）。
