# RFC 051: 接口值（fat 盒）生命周期与字典条目值所有权

- 状态：**设计定案（草案）——实现前须按 [RFC 036](036-maturity.md) 冻结面流程评审**
- 关联：[RFC 005](005-memory-model.md)（ARC/收集器）· [RFC 006](006-object-model.md)
  （接口值 ABI）· [RFC 014](014-runtime-abi.md)（rt_* 面）· [RFC 050](050-unified-object-header.md)（草案：统一对象头）
- 证据：CHANGELOG 9/5 取证——MemProbe2（class→iface 装箱 3M 轮）**433MB** 私有内存
  （~144B/轮）；对照 MemProbe11（class-only 3M 轮）**恒 0.7MB**；热循环栈泄漏
  （非 entry alloca）已另案修复（086b2878），与本设计正交

## 1. 问题

1. **接口值无释放站点**：堆 fat 盒 `{ obj, itable }`（16B calloc）创建即
   `rt_arc_inc(obj)`，但全 codegen 不存在对盒/obj 的配对释放——接口类型局部/字段/
   参数无 drop、无覆写 dec、收集器不可见（无 ArcHeader/walker）、盒本身从不 free。
   结果：任何一次 class→iface 装箱的对象永久滞留（rc 恒 ≥1）。接口值在
   IDisposable 句柄、事件订阅、DI 容器、chord 账本中无处不在——长期宿主按操作数
   线性增长（433MB/3M 轮量化）。
2. **字典条目值所有权无配对 dec**：rt_dict/rt_sorted_dict/rt_concurrent_dict 无值
   析构回调（rt_list 有 inc/dec 回调可对标）；codegen Add 注入 +1 后，
   Remove/Clear/Destroy 均无配对 dec——条目值引用滞留。

## 2. 现状盘点（取证基线）

- 接口值表示：`ptr` 指向堆 `{ ptr obj; ptr itable }`（16B）；codegen 直建
  （`calloc(16)` + 手动 `rt_arc_inc(obj)` + 布局），非 rt 原语（对照：string→object
  走 **rt_string_box**：完整 ARC 对象 `{rc, vtable, char*}`，dec 即 header-only 释放）。
- 接口方法分派/转型仅 codegen 侧读写盒（itable @+8、obj @+0 经 UnboxIface），
  runtime 无盒结构依赖（字典只存不读）。
- 现有 codegen 对「接口值槽位」的 inc/dec 发射（闭包 env 捕获、Object 临时槽等）
  以盒为对象执行——在 16B 布局下写 `box[0]`（obj 指针）造成 obj+1 型损坏
  （round-8 已修同族 lower_return_value 路径）；若盒成为真 ARC 对象则全部合法化。
- 类值生命周期基线（对照，正确）：`new` rc=1 → 槽位赋值 retain/移交模板 → 局部
  epilogue `rt_arc_dec`；vtable slot1 finalizer / slot2 walker 契约（rt_arc.c）：
  dec 1→0 调 `vt[1]` 后 free；dec 1 且 `vt[2]` 非空 → 循环候选。

## 3. 设计（推荐 D2：fat 盒 = 真 ARC 对象，string-box 先例）

### 3.1 布局

```c
/* 堆 fat 盒（32B，对标 string ArcBox 先例）：
   rc@0 / weak@4（4B pad）/ vtable@8 / obj@16 / itable@24 */
typedef struct RtIfaceBox {
    _Atomic int32_t refcount;
    _Atomic int32_t weakcount;   /* 恒 0（弱引用面不覆盖盒） */
    const void*    vtable;       /* = __arc_iface_box_vtable */
    void*          obj;
    const void*    itable;
} RtIfaceBox;
```

### 3.2 语义

- **盒 rc = 接口值的引用计数**：任何持有接口值的槽位（局部/参数/字段/元素/闭包
  env/async spill）对**盒** inc/dec——与类值对对象 inc/dec 完全同构，现有 class
  模板零特化复用（arc_class_place 等判定扩展至接口即可）。
- **创建**：`rt_iface_box_create(obj, itable)`（新 rt 原语，**纯增量** ABI）：分配
  32B、rc=1、vt=共享 `__arc_iface_box_vtable`、`rt_arc_inc(obj)`（盒存活期持 obj
  一引用，保持当前模型）。
- **盒死亡**（rc 1→0，rt_arc_dec 既有路径）：`vt[1]` finalizer =
  `rt_iface_box_release`：`rt_arc_dec(box->obj)`；随后 free——obj 在无其它引用时
  归零释放。`vt[2]` = NULL（盒非循环候选——环由 obj 自身的 walker 承载，盒只是
  视图）。`vt[0]` typeinfo：诊断用最小静态条目（名 "Arc.IfaceBox"）或 NULL
  （runtime 释放/收集路径不读 slot0；与 string-box 对照中 string 需 slot0 仅因
  rt_string_unbox 校验——iface unbox 在 codegen 侧）。
- **codegen 改造清单**：
  1. MakeIface/MakeIfaceDyn/AdaptIface（heap=true）→ `rt_iface_box_create`；
  2. UnboxIface/接口 null 比较等读 obj/itable 处偏移 0/8 → 16/24；
  3. 接口类型槽位的 retain/drop 判定纳入 class 模板（arc_class_place 接口分支 +
     epilogue drop/覆写 dec/FieldSet/EH pad/async env——逐点对照类值模板）；
  4. MIR：接口类型局部加入 Drop 发射（与类局部同律）；
  5. 列表/字典接口元素：`list_elem_is_ref` 等判定覆盖接口 mangle（回调对盒
     inc/dec 合法化）；字典 Add 保留（盒 inc）+ Remove/Clear/Destroy 配对 dec
     （见 §5 S3b 收口登记）。

### 3.2a 转换源所有权（返回路径孤儿引用消除，实施要点）

class→iface 转换按**源所有权**区分引用记账，否则每次转换产生一个永不释放的
孤儿 +1（现模型下 obj rc 恒 ≥2 的根源之一）：

- **借引用源**（存活 class 局部/字段/形参：其类侧所有者日后自行 dec）→ 建盒
  `rt_arc_inc(obj)`（盒存活期独立持 obj 引用）；
- **owned 源**（`new`/Call 结果、返回物化路径的无 drop 临时槽）→ 建盒**不 inc**
  ——盒接管该唯一所有权；返回路径须在盒建好后 dec 临时槽（当前该临时以 Object
  登记禁 drop——改为显式 Drop 语句：盒已持引用，dec 不触底），盒死亡时
  finalizer dec obj 归零释放。

此规则与类值拷贝模板的 retain 判定（`assign_needs_arc_retain`：borrow 源 retain /
Call-new 移交不 retain）同构，可对照实现。
- **null 语义不变**：null 接口值 = 空盒指针；`(I)null` 不建盒（既有空等价比较）。

### 3.3 兼容与风险

- 纯增量 rt 面（create/release + 静态 vt），无既有 ABI 符号改动；codegen/运行时
  同轮发布自洽（rt_cache 自然重编）。盒尺寸 16→32B 影响面 = codegen 建盒点 +
  读偏移点（grep `calloc(i64 1, i64 16)` / `{ ptr, ptr }` 布局收敛）。
- 风险：槽位模板覆盖遗漏（async/EH/weak）→ 以 mem2/11 探针（泄漏对照）+
  新增 iface-值生命周期 L2 用例为验收锚；对象槽 iface→object 转换面复核
  （UnboxIface 后以 obj 入 object 槽，不受盒布局影响）。

### 3.4 验收

1. MemProbe2 3M 轮内存回到 class-only 基线（<2MB）；
2. iface 值覆写/出作用域/字段/列表/字典全路径 dec 配对（RC 轨迹或
   ARC_DBG_FREE 无泄漏增长）；
3. corpus 41/41、workspace、full-rt L2 批绿。

## 5. 实施登记（阶段落地随 CHANGELOG 9/5 节推进）

| 阶段 | 提交 | 内容 | 证据 |
|---|---|---|---|
| S1 | 8ddec6df | string-box calloc 修复（弱计数垃圾 → 盒永不释放） | MemProbe 家族对照 |
| S2 | db810512 | D2 整批：32B fat 盒 + 全部建/读站点迁移 + 模块级 thunk | MemProbe2 433MB→0.6MB |
| S3a | 7a1f5c53 | `list_elem_is_ref` 接口分支翻正（盒 = ARC 对象，容器须引用维护） | MemProbe12（List&lt;IDisposable&gt; 80 万次操作）内存平坦；corpus 41/41 |
| S3b | 本轮 | 见下 | dict-probe13 A/B/C |

### S3b：Dictionary&lt;K,V&gt; 条目值所有权收口（rt_dict owned 变体 + 包装类 vtable finalizer）

**问题量化**（dict-probe13，20000 轮 × 64 值，~80B Payload）：
- A（建后即弃）：峰值 **~180MB**（≈9KB/轮：rt 表 ~4KB + 64×80B 值 +1 永留）；
- B（set_Item 覆盖，字典常驻）：峰值 **~116MB**（覆盖旧值从不释放）；
- C（Remove 全部后弃）：峰值 **~93MB**。
- 根因三通道同源：rt_dict 无值释放站点；且**包装类对象死亡无任何 destroy**
  （`Dictionary_K_V` 无 class 字段 → vtable slot1 finalizer 原为 null；`rt_dict_destroy`
  全 codegen 无引用——dumpbin /imports 实证）。

**设计（增量 ABI + codegen 收敛，无既有符号改动）**：
1. `rt_dict_create_owned(hash, eq)`（rt_dict.c / rt_abi.h 新增符号）：值 = ArcHeader
   对象（class / 接口 fat 盒）。**存储侧自持 +1**：set 新键 / try_add 命中时
   `rt_arc_inc`（codegen 写臂不再预 inc——移除旧「先 inc 后失败」孤儿 +1）；
   set 覆盖旧值、remove、clear、destroy 释放被移除条目值（先清槽再 dec，
   finalizer 重入安全）。legacy `rt_dict_create` 语义零改动（标量/string 值）。
2. codegen：ctor stub 按 `list_elem_is_ref(V)` 选 create 变体；emit_builtin /
   emit_stubs 的 Add / set_Item 写臂删除预 inc（与存储侧配对）。
3. **包装类 vtable finalizer**（mod.rs emit_vtables）：`Dictionary_K_V` 等容器句柄
   包装类 slot1 = `__finalize_{cname}`（加载 `_handle` @ offset 16 →
   `rt_dict_destroy`）。对象最终 drop（rc 1→0，任何路径：局部/字段/容器/异步帧）
   恰一次释放 rt 表 + owned 条目值——替代「局部 drop 特判」，无 rc 竞态判别。
4. 读臂（get_Item / TryGetValue / 枚举 / ContainsValue）零改动（返回侧 retain 与
   存储侧 +1 各自独立配对）。

**验证**：dict-probe13 A/B/C 修复后峰值恒 **~5.7MB**（exit 0）；功能探针 D
（覆盖/移除/清空/快照/复用交错 500 轮）exit 0 校验和一致；corpus 41/41、
workspace、full-rt L2 批绿（u5 并发载入竞态为独立缺陷——CHANGELOG 9/5
已知问题登记→已修复：ALC 注册表串行化 + Monitor 重入补齐，90/90 无崩溃）。

**残余登记（后继轮次范围）**：
- **concurrent 同族（部分落地，余 owned 值释放）**：已落地——包装类死亡
  destroy（rt_concurrent_dict_destroy + vtable finalizer；同轮修复
  rt_mutex_destroy 对 raw-malloc 句柄误用 rt_obj_free 的既有缺陷）、
  TryGetValue/GetOrAdd 读臂借用 retain 与分派补正（类值 GetOrAdd 误投
  factory 路径 0xC0000005）、int 键 0 墓碑冲突（dead 标记取代 NULL-key
  判活）。余项：owned 存储模型（create_owned + 存储侧 +1）使 destroy/remove
  能配对释放存活类值条目的 +1（当前该类值在包装类死亡时滞留——配合
  codegen 既有 retain 平衡）；
- **数组所有权缺口（测量登记，独立 RFC 流程）**：运行时数组 = 裸 header+
  payload（rt_array.c），`rt_array_destroy` 全 codegen 无调用点——缓冲与类
  元素永不释放（arr-probe1 `Payload[]` 39MB/20k 轮；arr-probe2 `byte[4096]`
  52MB/20k 轮；arr-probe3 `byte[1]` ×20k 无损坏 → 数组局部 drop = no-op，
  无对裸缓冲的误 dec；赋值 = 共享别名）；Keys/Values 快照无逐元素 +1 →
  借用视图语义（Dictionary.as 已注；快照在条目释放后回读为悬垂/损坏：
  snap-probe1 Clear 后回读 0xC0000374——读臂对被释放元素 retain 推断）。
  收口设计：数组对象 ArcHeader 化（表示层变更，别名共享安全）→ drop 经
  统一 release（元素 walk + 缓冲释放）铺设局部 epilogue / 类字段 finalizer /
  容器元素 / async env drop 站点；owned 字典快照逐元素 +1（.NET 快照语义）；
- **键所有权 + string 元素 char* 语义**：class/string 键 gen 槽存裸指针从不
  retain——键对象在外围所有者消亡后字典侧悬垂（hash/eq 解引用存储键）；并入
  数组所有权同一登记（string 元素 char* 释放语义先决）。

**S3c 落地**（本日 CHANGELOG）：SortedDictionary 值所有权收口
（rt_sorted_dict_create_owned + 读臂 retain + 包装类 finalizer destroy）——
sort-probe1a 39MB→5.6MB、sort-probe1f 悬垂垃圾回读→精确校验和、L2 回归
ng_sorted_dict_class_value 绿；corpus/workspace/full-rt 全绿。

**S3d 落地**（本日 CHANGELOG）：ConcurrentDictionary 值所有权收口——concurrent
同族残余项（owned 存储模型配对释放）闭合，余「数组所有权 + 键所有权」仍独立
登记（见上）。

- **运行时（增量 ABI）**：`rt_concurrent_dict_create_owned` /
  `_level_owned` / `_level_cap_owned`——值 = ArcHeader 对象（class / 接口 fat
  盒），存储侧在 **stripe 锁内**自持 +1（TryAdd 新键 / set 新键 / GetOrAdd miss /
  AddOrUpdate 插入；dup-fail 不 inc）；覆盖旧值 / TryUpdate 换槽后**锁外**
  `rt_arc_dec` 旧值（槽先指向新值、锁不持有时释放——finalizer 重入安全）；
  clear / destroy 遍历收集存活条目值后**锁外**统一释放（`clearing` 守卫使值
  finalizer 重入 clear/destroy 为 no-op，防嵌套 teardown）。**TryRemove 不移除
  侧 dec**：存储 +1 随 out 槽**所有权移交**调用方（调用方局部 epilogue dec 配对）。
- **读臂借用 retain 内移**（关键并发正确性点）：TryGetValue / get_or_default /
  GetOrAdd 命中返回在 **stripe 锁内** `rt_arc_inc`（借用），codegen 并发类值臂
  **移除调用侧 post-inc**——消除「lock-free 读返回后、调用侧 inc 前」被并发
  remove/覆盖释放的悬垂窗口（既有 cprobe2 同族缺口的并发形态）。代价：owned
  （class/接口 V）实例的读臂由 lock-free 变为**锁内借用**；legacy（标量/string）
  维持 lock-free 读与无 ARC 语义（H2 门禁面不变，宣称面按此如实划分）。
- **验证**：L2 新增 `ng_concurrent_dict_class_value`（TryAdd/dup/读回校验和/
  GetOrAdd 命中与 miss/覆盖/TryUpdate CAS/二次 TryRemove false/clear/包装死亡
  ×200 轮 + 复用潮）；corpus/workspace/full-rt 全绿（见当日 CHANGELOG 门禁行）。
- **残余登记（与 S3b/S3c 同面收敛）**：字典快照（Keys/Values）借用视图语义与
  逐元素 +1、class/string 键所有权、数组所有权（ArcHeader 化）——并入数组
  所有权独立 RFC 流程（见上），不在本阶段滚动。

## 6. 备选与边界

- **D1 借引用 + 全值拷贝再装箱**：不动 ABI，但每次 iface 赋值/传参须重装箱+配对
  dec——覆盖点与 D2 同量级且更多转换开销，不推荐。
- **D0 维持现状（登记泄漏）**：长期宿主不可接受。
- 字典条目所有权（登记①）：随 D2 的 codegen 配对 dec 一并收口；sorted/concurrent
  同族。
- RFC 050（统一对象头）若先行落地，本设计改为在其头部之上叠加（盒头 24B 统一
  化），两文档交叉引用。
