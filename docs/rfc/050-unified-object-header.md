# RFC 049: 统一对象头——runtime 句柄内存的身份物理化

状态：草案（评审中） · 关联：[RFC 005](005-memory-model.md)（ARC）· [RFC 009](009-async-concurrency.md)（调度）· [RFC 036](036-maturity.md)（冻结面流程）· Stability Review (internal record)（P1 模式全量归因）

## 1. 动机：豁免清单模式破产

ARC 引用计数要求所有被引用对象携带 `ArcHeader { refcount, weakcount, vtable }`。性能热路径上的 runtime 句柄（Socket/Mutex/Thread/Task/Pipe/Lock 等「模式 A：对象即裸 C 句柄」）不带此头——它们流入 ARC 世界后，**任何一次未经豁免的 inc/dec/虚调用都会把业务首字段当 refcount、把偏移 8 当 vtable**，即 0xC0000005。

历史修复选择了**字符串豁免清单**（`is_opaque_runtime_handle`）。全量归因（Stability Review (internal record)）证明该模式破产：

- 每新增一个模式 A 门面都要「记得」加豁免——NamedPipe 曾漏（批边界 0xC0000005）；
- 判定点散布全 codegen（协程 preamble、状态机 ctor、字段赋值、虚调用、方法调用），**逐点修复不可收敛**——channels `bounded_backpressure` 在一处豁免修复后即暴露下一处（`rt_arc_inc(0x2)`，整型值被当对象指针）；
- 豁免只覆盖 inc/dec，**不覆盖虚调用**（模式 A 裸块无 vtable，经基类引用调用即崩）。

**结论：类型身份必须物理化。** 判定从「名字形状启发式」迁到「对象内存自描述」。

## 2. 设计：16 字节统一头

### 2.1 头布局（两族统一，总宽 16B 不变）

```c
/* ARC 管理对象（现 ArcHeader 演化）：*/
typedef struct RtObjHead {
    uint32_t    magic;      /* RT_OBJ_MAGIC = 'A' | 'R'<<8 | 'C'<<16 | 'H'<<24 */
    uint32_t    kind;       /* RT_OBJKIND_ARC = 1（强计数对象） */
    _Atomic int32_t refcount;
    _Atomic int32_t weakcount;
    const void* vtable;     /* slot0 typeinfo / slot1 finalizer / slot2 walk / 3+ 虚方法 */
} RtObjHead;                /* 共 24B —— 见 2.3 兼容性 */
```

```c
/* Opaque runtime 句柄（模式 A，原「无头裸块」）：*/
typedef struct RtOpaqueHead {
    uint32_t magic;         /* RT_OBJ_MAGIC */
    uint32_t kind;          /* RT_OBJKIND_OPAQUE = 2（句柄，禁计数） */
    uint32_t reserved[2];   /* 对齐 + 未来扩展（如调试代数） */
} RtOpaqueHead;             /* 16B，业务结构随返回指针偏移 */
```

### 2.2 守卫语义（rt_arc.c 单点收口）

```c
void rt_arc_inc(void* ptr) {
    if (!ptr || (uintptr_t)ptr < RT_PTR_FLOOR) return;   /* 下界：整型当指针无害化 */
    RtObjHeadCommon* h = (RtObjHeadCommon*)ptr;          /* magic/kind 双字段公共前缀 */
    if (h->magic != RT_OBJ_MAGIC) return;                /* 非 runtime 堆块：无害 no-op */
    if (h->kind != RT_OBJKIND_ARC) return;               /* opaque 句柄：禁计数 */
    atomic_fetch_add(&h->refcount, 1, ...);
}
/* rt_arc_dec / rt_arc_collect_cycles / Weak 族同守卫。 */
```

- **下界哨兵**（`RT_PTR_FLOOR = 0x10000`）拦截「小整数当指针」——本轮取证实证 `rt_arc_inc(0x2)`，哨兵使其无害化；
- **magic 拦截**「堆内非 ARC 块」；**kind 拦截**「opaque 被计数」；
- **三层守卫后，任何判定层（codegen/启发式/未来新代码）的漏判都物理无害**——豁免清单从「安全屏障」降级为「优化提示」（可渐进退役）。

### 2.3 兼容性裁定

- **ARC 对象头 16B → 24B**：`HEADER_SIZE`（layout.rs）同步 24，全部字段 offset = 24 起。**头宽变更影响**：对象尺寸 +8、字段 offset 平移——由 layout.rs 单点导出，codegen 无硬编码 16（存疑点需全仓 grep `i32 16` 复核，见 §4）。
- **兼容假头**（`rt_semaphore_obj{refcount, vtable, handle}` 等 offset-0 兼容 ArcHeader 的结构）：同步新布局（`{magic, kind, refcount, weakcount, ...}` 或改持 `RtObjHead`）。
- **代码地址 / Func_ / struct（无 ArcHeader 形态）**：`list_elem_is_ref` 等判定为非 ref 的元素**不进 inc/dec 路径**，无需头；若被误 inc，magic 守卫拦截（struct 首字段不是 magic → no-op）——**误判的爆炸半径从「数据损坏」降到「无害跳过」**。
- `/Brepro` 确定性、rt_cache 内容寻址不受影响（源码变化自然重编）。

## 3. 模式 A 创建点的单点改造

模式 A 的全部 `rt_*_create` 返回点统一经分配宏：

```c
/* rt_abi.h / rt_obj.h（新增）：*/
void* rt_obj_alloc_opaque(size_t biz_size, uint32_t kind_hint);
  /* = malloc(sizeof(RtOpaqueHead) + biz_size)，写 magic/kind，
     返回 (char*)head + sizeof(RtOpaqueHead)。 */
#define RT_OPAQUE_NEW(type) ((type*)rt_obj_alloc_opaque(sizeof(type), RT_OBJKIND_OPAQUE))
```

改造清单（`create` 返回点逐一换 `malloc` → `RT_OPAQUE_NEW`，C 内部字段访问不受影响——结构定义不变，只是分配多出头）：

- rt_net.c：`rt_socket_create`、`rt_socket_accept` 的 accept_sock、完成路径的 accept 包装
- rt_pipe.c：`rt_pipe_state_alloc`
- rt_thread.c：`rt_lock_create`、`rt_mutex_create`、`rt_semaphore_create`、`rt_thread_handle(_full)`、`rt_threadpool_create`、TLS worker ctx（如经对象面暴露）
- rt_task.c：`rt_task_alloc`/slab 复用路径（RtTask 即 PENDING Task 对象）
- rt_sync/rt_proc 等：审计 `is_opaque_runtime_handle` 清单逐一对应

**ArcHeader 创建点**（codegen 对象发射）：`calloc` 后的头初始化从「store refcount=1 @0 + vtable @8」改为「store magic/kind/refcount/vtable（RtObjHead 布局）」——单点在对象创建发射器。

## 4. 迁移分期与验收门

- **M-a（✅ 已落地，2026-09-02）**：头布局 + `rt_obj_alloc_opaque/rt_obj_free` 原语 + `rt_arc_inc/dec` 三层守卫（下界哨兵转正 + magic + kind）+ 试点创建点：`rt_lock_create`、`rt_socket_create`、`rt_socket_accept`（含 IO 完成路径 accept 包装）、`rt_pipe_state_alloc`。回归：net/pipe/contract/mono 全绿。
- **M-b**：其余创建点全量迁移（§3 清单余项：rt_mutex/rt_semaphore/rt_thread 系列/rt_task_alloc slab/rt_threadpool/rt_cts）+ `arc_drop.rs`/emit 的字段级 dec 路径复核 + **单态化边界系统审计**（泛型 async/coro 的参数与字段类型在单态化后 TypeId 上的判定全覆盖——channels case 2 第三层崩点 `WriteAsync resume` 内 reader-waiter 字段读属此范围）。
- **M-c**：`is_opaque_runtime_handle` 退役评估——inc/dec 交给守卫后，豁免仅剩「跳过冗余调用」的优化语义；虚调用面（#13）由 vtable 物理关闭。
- **验收门**：channels 四 case 去 `#[ignore]` 全绿 + `l2_net/l2_pipe/l2_channels/l2_mono` 回归 + **嵌套泛型容器回归批**（评审 D2 验收门）+ 全量 `cargo test --workspace`。
- **回归红线**：任何一批红即回退该创建点迁移（逐点小步，避免一次性大迁移的回归海啸）。

**M-a 实证**：D1 守卫使 channels 取证第三层的 `rt_arc_inc(0x2)` 从**崩溃**降级为**无害跳过**（inc 物理免疫验证通过）；剩余崩溃为「字段读」数据流（守卫不可达——数据流正确性归 D2 单态化边界审计）。

**M-b 进展（2026-09-02 深夜）**：channels case 2 第三层崩点已闭环——真因非单态化边界，而是 **#15 值槽 ABI 违例**（`rt_queue_*` 的 `elem_ptr` 为「元素值槽地址」，`Queue<T>.Enqueue` 非标量分支直传对象指针，C 侧 memcpy 把对象头 refcount 快照当元素存入；`Stack<T>.Push/Contains` 同型）。修复后 `l2_channels_batch` case1-5 全绿（backpressure 直付/Wait 背压/完成信号实证），case6（ReadAllAsync 流式）暴露新形态「指针低 32 位截断撕裂」，转 stability 评审账本。验收门相应更新为 channels 八 case（case6/7/8 待取证）。

**M-b 第一批迁移（同日）**：`RT_OPAQUE_NEW` 宏落地（rt_abi.h §3 形态），创建点迁移 rt_thread.c（mutex/semaphore/thread_handle/handle_full）、rt_threadpool.c（pool）、rt_cts.c（cts）；顺带修复 `rt_semaphore_destroy` 句柄泄漏（H1 缺陷实证）。`rt_semaphore_obj` refcount@0 确认为死字段（豁免清单后仅 create 写 1）。余项：rt_task slab 路径（slab 内头写入需独立设计，非独立 malloc 块）、TLS worker ctx 审计。**D3 契约收敛同步落地**：`rt_task_poll(NULL)` 由「返 READY 静默成功」改 fail-fast（取证打印 + abort，全部 runtime 调用方已核具 NULL 预防）。回归：workspace 全绿 + 新增 D2 验收门 `l2_nested_generics_batch`（类元素 Queue/Stack/Dict + 嵌套泛型 Queue）全绿。

## 5. 与 D2/D3 的协同（承上启下）

- **D2（判定布局化）**：统一头是「判定错误的最后防线」；D2 的单态化边界判定（泛型 async 参数、嵌套泛型容器解析）仍是第一道正确性关卡——本轮 `is_generic_template_name` 守卫与 `parse_queue_elem` 放宽属 D2，先行落地。
- **D3（唤醒协议收敛）**：`rt_task_poll(NULL)==READY` 静默语义在 D1 头落地后重新评估（NULL task 仍需显式失败语义，与头无关）。
- **不做清单**：不改 GC 算法（试删循环收集维持）；不引入标记阶段；不做分代。

## 6. 冻结面流程

本 RFC 触碰 `rt_*` ABI 分配语义与 ArcHeader 布局——按 RFC 036 基础面冻结流程评审：H1（底层稳定）要求破坏性变更先 RFC；本文档即评审载体。M-a 试点三创建点属**增量防御**（旧布局对象在过渡期经 `is_opaque_runtime_handle` 豁免与 magic 守卫双保险），不破坏既有二进制契约。
