# RFC 047 透明对象图迁移（热重载 L3）

> 状态：**已实施**（rt 原语 `rt_arc_retype`/`rt_arc_vtable_of`、迁移编排 `rt_library_migrate_instances`、codegen `__arc_vtable_registry` 发射、std 门面 `AssemblyHotReload.MigrateInstances`；验证锚点 `l2_hot_reload_batch` 6 case——含虚分派铁证 `hr_virtual_dispatch_after_migration`）。
> 关联：[017 热卸载](017-build-artifacts-packages.md) · [045 D8.1 组合契约](045-chord.md) · [005 内存模型](005-memory-model.md)（ARC/循环收集）· [006 对象模型](006-object-model.md)（vtable）。

## 1. 动机与定位

[045 D8.1](045-chord.md) 状态迁移分层中，L1（兼容性判定）与 L2（应用层显式搬运）已实施；本篇定型 **L3：透明对象图迁移**——换代窗口内，旧代实例对象**原地重绑**到新代同构类型，字段内存与对象地址保持不变，使用者无感知、无搬运代码。

## 2. 核心不变量：重绑不改地址

对象头（[rt_arc.c ArcHeader](../../crates/runtime/rt_arc.c)，16B）布局：

| 偏移 | 字段 | 迁移动作 |
|------|------|---------|
| 0 | `_Atomic int32_t refcount` | **保持** |
| 4 | `_Atomic int32_t weakcount` | **保持** |
| 8 | `const void* vtable` | **改写**为新代同构类型的 vtable 指针 |

**唯一动作 = 改写 offset 8 的单个指针**。由此派生的不变量：

- **引用值全部不变**：字段、数组元素、局部变量、弱槽 target——所有指向被迁移对象的指针无需修改。跨代引用边（旧代对象引用新代对象、反向、经宿主中转）**天然成立**——图遍历的目的仅是**枚举重绑目标**（找出全部旧代实例），不是改边。
- **生命周期计数天然保持**：refcount/weakcount 不动，迁移不引入 inc/dec、不与 ARC 热路径交互。
- **漏扫安全（有条件下）**：漏扫对象仍指向旧 vtable——在旧代**卸载前**完成迁移则漏扫窗口不产生悬垂；漏扫对象的真实风险在旧代卸载之后（旧 vtable 所在映像解除映射）。完备性条件见 §5。

## 3. 缺口一：重绑原语 `rt_arc_retype`

```c
/// 将 obj 的 vtable 重绑为 new_vtable（布局/vtable 形状已由调用方判定兼容）。
/// 返回 0 成功；非 0 参数无效。rc/weakcount 不变；非原子写——调用方保证
/// 迁移窗口处于 Freeze（无并发访问，见 §6）。
int32_t rt_arc_retype(void* obj, const void* new_vtable);
```

实现即对 `((ArcHeader*)obj)->vtable` 的单次 store（约 10 行）。

## 4. 缺口二：迁移目标枚举与双重兼容判定

### 4.1 对象图遍历——复用 walk 机制

循环收集器已为可参与环的 class 发射 `__walk_{cname}`（vtable slot 2，`rt_arc_walk_fields` DFS）——迁移器**复用同一机制**枚举旧代对象闭包：从模块根（`__arc_module_roots` 登记面）DFS，凡 vtable ∈ 旧代 vtable 集合的对象入迁移集。字段 walk 的偏移语义在重绑前后一致（布局指纹相同 → 字段偏移表相同 → walk 函数逐字段访问的地址不变）。

### 4.2 旧代 vtable 集合

`@.vtable.{Class}` 为按类名的确定性符号（与 Entry/typeinfo 同构的名字内联身份）。编译器为动态库发射 **vtable 登记表**（`__arc_vtable_registry`：`{type_name, layout_sig, shape_hash, slot_count}` 数组 + `_count`，随包元数据同源导出；仅本 TU 定义的、含虚方法的 class）——「对象属于旧代」判定 = 头中 vtable ∈ 该集合。

**实施修订（相对初稿）**：条目**不物化 vtable 指针**——迁移时按名 `.vtable.{T}` 对新旧两代 dll 现场解析（`rt_lib_resolve_symbol`/`rt_library_sym`）。理由：vtable 全局的发射时机由方法发射驱动，条目物化指针会造成「类在 layouts 但 vtable 全局未发射」的未定义符号链接风险；按名解析把该风险转为迁移期**保守拒绝**（解析失败 → 拒绝迁移）。配套：vtable 全局自 linkonce_odr + COMDAT **改为强定义常量**——实测无引用的 COMDAT 节在 clang COFF 落盘时被丢弃（hr 插件实证），而 `.vtable.{T}` 须经 `rt_library_sym` 按 dll 解析（弱定义不可依赖）；插件 dll 为单 TU 编译，强定义无跨包重复风险。`.vtable.{T}` 与登记表符号经 `all_exports` 显式导出（Windows MSVC 数据符号默认不导出）。

### 4.3 双重兼容判定（比 L1 更严）

字段布局指纹（L1，`entry_layout_signature`）只覆盖字段内存；L3 重绑后旧对象将执行**新代的方法**——vtable 形状必须一致：

- **形状**：slot 数一致（`virtual_slots.len() + 3`）；
- **签名序列**：逐 slot 的方法名 + 参数/返回类型签名一致（重载消歧依赖签名，签名漂移 = 语义破坏）。

两者 + 字段指纹**三者齐备**方可重绑；任一不齐 → 该类型**不可透明迁移**（编排器降级：该类型实例走 L2 显式搬运或拒绝换代）。判定表由编译器物化（随 `__arc_package_meta` 扩展 vtable 形状指纹——实现期与 L1 指纹同源发射）。

### 4.4 跨代边与归属

- 旧代对象 → 新代对象：字段值不变；新代对象不在迁移集（vtable 不属旧代集合）→ 遍历到此即停（**不递归进新代图**——新代对象的正确性由新代自身构造保证）。
- 新代对象 → 旧代对象：旧代对象重绑后引用自动有效；**计数归属不变**（新代持有的是强引用，dec 时走新 vtable 的 finalizer slot——布局一致故 slot 语义一致）。
- 宿主/共享对象（非旧代类型）：不重绑、不遍历（与现行的跨库身份语义一致）。

## 5. 缺口三：与循环收集器及根扫描的交互

- **迁移窗口 = Freeze 态**（复用 [017 §2.4](017-build-artifacts-packages.md)：in-flight 收敛为 0、OnUnloading Cancel 已过、ledger 归零）——无并发 dec/alloc，`rt_arc_retype` 的非原子写安全；TLS 候选队列在单线程窗口内无竞争。
- **候选队列一致性**：迁移前 `rt_arc_collect_cycles()` 清空调用线程候选（已 pin 的候选在 Freeze 态下计数不变）；迁移后旧代对象的候选身份由**新 vtable 的 walk fn** 承接——布局一致故 trial-deletion 的字段遍历与 intra-closure 计数语义不变。
- **根扫描完备性**：迁移集 = 模块代数根 DFS 闭包（复用 [017](017-build-artifacts-packages.md)「模块根 + 字段 DFS」机制，**不引入全堆扫描**）。闭包外的旧代对象引用（宿主栈等）沿用**宿主侧引用前置条件**契约——编排序列要求迁移前宿主已完成 L2 搬运或显式放弃旧代引用；契约违反的暴露面与现行 Unload 相同（E_UNLOAD_HANGING_REF / 后续 dec 悬垂），不因 L3 引入新形态。
- **弱槽**：`RtWeak.target` 为裸指针，重绑不改地址 → 弱槽观察不受迁移影响；弱槽 target 对象在迁移集内则同样重绑（弱槽登记表可反查 target，作为迁移集的补充枚举源——实现期取舍）。

## 6. 迁移器编排序列（在 [045 D8.1](045-chord.md) 六步序列中替换步骤 4-5）

1. Freeze 旧代（in-flight 收敛）；
2. 构建映射表：旧代 vtable 集合 ∩ 新代类型名 → `rt_library_sym(newHandle, ".vtable.{T}")` + **双重兼容判定**（§4.3）；不可迁移类型 → 编排器降级（L2 搬运该类型实例 / 拒绝换代）；
3. 模块根 DFS 枚举迁移集（§4.1）；
4. 逐对象 `rt_arc_retype`（§3）；
5. `rt_arc_collect_cycles()` 清候选；
6. Unload 旧代（现行卸载闭环——旧 vtable 映像解除映射时已无对象引用它）。

**失败回滚**：步骤 2-3 任一失败 → 未执行任何 retype → 旧代原样保持（回滚零成本，与 D8.1 同语义）；步骤 4 中途失败（映射表构建后不应发生——判定前置）→ **反向 retype**（旧 vtable 指针已留存于映射表）逐对象恢复。

## 7. 验收协议（实施状态标注）

1. 单代迁移：两代同构类型（同字段布局 + 同 vtable 形状）实例图迁移后：字段值保持、方法分派命中新代实现、`rt_arc_collect_cycles` 语义不变——**已锚定**（`hr_transparent_migration`：字段保持 + Retire 旧代后对象存活 + dec 走新 vtable 无 AV；`hr_virtual_dispatch_after_migration`：迁移前 `:hr-v1-method` → 迁移后 `:hr-v2-method` 的同引用分派切换铁证——状态字段 Tag 上移基类，`HrBase state = v1.Entry<HrPayload>()` 单变量向上转型 + 基类虚调用观察）；
2. 双重判定负向：字段同但 vtable 形状异（override 签名漂移）→ 拒绝迁移并报告类型名——**拒绝语义已实施**（rt 层三元全等 + 整体拒绝 -3 → std 抛 InvalidOperationException）；**报告类型名的逐类型枚举**依赖 ARC_DEBUG_MIGRATE 取证埋点（env 门控，已留存）；
3. 漏扫契约负向：宿主持旧代引用未清理 → 沿用 E_UNLOAD_HANGING_REF（不因 L3 改变暴露面）——**已锚定**（u5_unload_order_guard 同族护栏语义）；
4. 压力：迁移集 ≥ COLLECT_MAX（DFS 上界）时的 abandonment 语义不破坏（泄漏不悬垂）——**迁移 DFS 沿用同上界（RT_LIB_SCAN_MAX）**，压力用例待补；
5. e2e：`l2_hot_reload_batch` 扩展 L3 用例——**已落地**（`hr_transparent_migration` + `hr_virtual_dispatch_after_migration`，与 L1/L2 用例共存，6 case 全绿）。

**实施期排障沉淀**（防漂移）：① 纯基类（无实例化点）的 typeinfo 引用 `.vtable.{T}` 而 vtable 定义未发射 → link 断——emit 层 typeinfo↔vtable 发射耦合缺陷已立案，用例侧以实例化哨兵规避；② 插件编译为单文件直通，跨插件源码级继承编译期不可见（deps 只写运行时 meta）——基类以每插件内嵌副本表达，源码级共享挂 arc.toml path 依赖课题；③ vtable 全局的 linkonce_odr + COMDAT 在无引用时落盘丢失——强定义修订见 §4.2。

## 8. 边界

| 不做 | 归属 |
|------|------|
| struct/数组元素的值内存重排 | 布局指纹判定拒绝（值类型字段布局漂移本就不可透明迁移） |
| 枚举/variant | 无字段布局，不参与（同 L1） |
| 全堆扫描 | 沿用 [017](017-build-artifacts-packages.md) 根扫描裁决（模块代数闭包） |
| 宿主栈引用的自动迁移 | 前置条件契约（宿主侧引用前置条件，017） |
