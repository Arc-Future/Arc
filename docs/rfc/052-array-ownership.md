# RFC 052: 运行时数组所有权（ArcHeader 化）与字典快照语义

状态：设计定案；**0.1 发布前置**（2026-09-07 升格）。**S1–S4 已落地**（runtime ArcHeader
化 · codegen drop/retain · 类字段/容器/async env · owned Keys/Values 快照 + class 键所有权）。
关联：[RFC 005](005-memory-model.md)（内存模型）· [RFC 006](006-object-model.md)（对象模型/ABI）·
[RFC 024](024-concurrent-collections.md)（并发集合快照）· [RFC 036](036-maturity.md)（冻结面流程 §3）·
[RFC 050](050-unified-object-header.md)（统一对象头）· [RFC 051](051-iface-value-lifetime.md) §5
（S3 家族残余登记）· 实施登记随 CHANGELOG 推进。

> 收口进度参照（RFC 051 S3a–S3d）：列表/字典/sorted/concurrent 的**条目值所有权**已
> 随 S3a–S3d 逐轮收口（2026-09-05/06，见 RFC 051 §5 与 CHANGELOG）；本 RFC 承接的
> 残余面 = **数组缓冲/元素所有权 + 字典快照（Keys/Values）逐元素引用 + class/string 键
> 所有权 + string 元素 char* 语义**——即 S3 家族在「数组表示层」上的延续。

## 1. 问题与取证基线（2026-09-05 测量登记，证据库 cprobe/arr-probe 家族）

运行时数组 = 裸 `header{length,elem_size}+payload`（rt_array.c）——**无 ArcHeader /
rc / vtable**：

- `rt_array_destroy` 全 codegen **零调用点**（grep 实证，与 dict/sorted 包装类死亡缺口
  同族：嵌套原生句柄/缓冲无人释放）；
- **arr-probe1**（`Payload[]` 16 元素 × 赋槽 `new` + 弃，20k 轮）：峰值 **39MB**
  （≈1.9KB/轮 = 缓冲 136B + 16×~90B 类元素——类元素永不被释放）；
- **arr-probe2**（`byte[4096]` 分配即弃，20k 轮）：峰值 **52MB**（≈2.6KB/轮——
  纯标量缓冲也全量泄漏）；
- **arr-probe3**（`byte[1]` ×20k）零损坏 → **数组局部 drop 为 no-op**（不存在对裸缓冲的
  误 rt_arc_dec；数组赋值 = **共享别名**，缓冲 rc 化前不可按拥有者逐点释放）；
- **snap-probe1**（Dictionary.Values 快照 → Clear → 堆复用 → 回读）：确定性
  **0xC0000374**（读臂对被释放元素 retain → 写已释放块）；对照 snap2（不回读）exit 0、
  snap3（字典存活时回读）exit 0。

**影响面**：一切 `T[]`（byte[] 缓冲、ToArray、List.ToArray、dict.Keys/Values、
string[] 拆分等）分配即泄漏至进程结束——**长期宿主按分配线性增长**；Keys/Values
与 .NET「快照副本」语义不符：当前为**借用视图**（条目存活期间有效；条目释放后读旧
快照为悬垂/损坏）。

**In-tree 用法审计**：std 内 Keys/Values 全部为「字典稳定期遍历」（ALC 依赖图/加载表、
AIModelRegistry、YamuxSession 流表等）；借用语义下安全（Dictionary.as 快照说明已改
「借用视图（数组所有权收口前）」，明示条目释放后旧快照失效）。

## 2. 语义目标与裁决

1. **数组 = 引用类型**（.NET 对标）：`T[]` 赋值共享同一数组对象；数组存活期 = 别名
   引用集合的存活期。收口后 drop 语义 = 元素 walk + 缓冲释放，且仅在 rc 1→0 恰一次。
2. **Keys/Values = 快照副本**（.NET 对标）：建快照时逐元素 +1（引用元素）；条目随后
   释放不影响快照。收口前保持借用视图（已文档化 + 用法审计）。
3. **string 元素 char* 语义**：string 元素的表示（raw char* vs 盒）决定元素 walker 是否
   inc/dec——string 元素释放语义先决（string 常驻 rodata / 堆串 / 盒串混合表示面需
   先收敛或按不可释放处理并文档化）。
4. **class/string 键所有权**（S3b/S3c 残余同面）：gen 槽存裸指针从不 retain——键对象在
   外围所有者消亡后字典侧悬垂（hash/eq 解引用存储键）；键所有权方案并入本 RFC 分期
   （class 键 retain/释放与值同律；string 键按元素 char* 语义一并裁决）。

## 3. 设计：数组对象 ArcHeader 化（表示层变更，分期实施）

### 布局与运行时（增量 ABI）

- 数组对象：`ArcHeader{rc@0, weak@4, vtable@8} + length@16 + elem_size@20 + payload@24`；
  payload 语义与现 rt_array 完全一致（既有消费方零改动）；共享别名安全（rc 化后
  赋值 inc/dec 与 class 同律）。
- 每数组类型一份静态 vtable（codegen 按元素类型发射）：slot1 finalizer =
  `__finalize_{T}arr`（元素 walk + 缓冲释放）；slot2 = walker（循环收集候选，元素为
  class/接口盒时参与环检测；标量/string 元素 NULL）。**逐元素释放语义按元素类型**：
  class/接口盒 rt_arc_dec、嵌套数组递归 release、标量 no-op、string 按 §2.3 裁决。
- 新 rt 原语 `rt_array_create_owned`（或统一改造 `rt_array_create` 加头）；所有 runtime
  数组产出点（rt_*_keys/values/to_array、list_to_array、split 等）统一走带头创建。
  legacy 无头形态仅存于 C ABI 白盒/内部过渡期，随后收敛（同 D2 fat 盒先例：纯增量
  起步、同轮 codegen/运行时自洽）。

### codegen drop 站点铺设（与 class 局部 drop 管线同源扩展）

1. 局部 epilogue（数组型局部纳入既有 drop 判定：`list_elem_is_ref`/`arc_class_place`
   对应数组后缀翻真——含 `{T}_arr` 名义判定细化）；
2. 类字段 vtable finalizer（field-walk 扩展含数组型字段）；
3. 容器元素（rt_list 数组回调变体：push/remove/clear 对元素数组 inc/dec）；
4. async env / EH pad 路径；
5. 返回路径 owned-temp 显式 Drop（与 RFC 051 §3.2a 同构）。

### 快照与键所有权

- owned 字典 Keys/Values 建快照时逐元素 +1 → .NET 快照语义；快照数组消亡走数组
  release 释放元素 +1；
- class 键存储侧 retain/释放与值同律（S3b/S3c owned 模型扩展至键）；
- string 键/元素按 §2.3 裁决（表示面收敛前维持现状 + 文档化）。

### 分期与验收锚（独立专项 Sprint，每期保绿）

| 期 | 内容 | 验收锚 |
|---|---|---|
| S1 ✅ | 运行时头化 + 创建点统一（增量 ABI） | `rt_array_create` = ArcHeader@0 + length@16 + payload@24；`rt_list_to_array` 统一走 create |
| S2 ✅ | codegen 建头/局部 epilogue drop + 返回路径 | `rt_array_retain/release`；`TypeId::Array` 入 epilogue；`create_refs/nested` |
| S3 ✅ | 类字段 finalizer / 容器元素 / async env | `__finalize_*` 数组字段 `rt_array_release`；List `_arr` 走 `rt_array_arc_*`；SM dtor |
| S4 ✅ | owned 快照逐元素 +1、class 键所有权、string 裁决 | `rt_dict_values/keys` owned→create_refs+inc；class 键 retain；string 键维持借用（§2.3） |

### 备选

- **D1（仅收 Keys/Values +1 + 文档化借用边界）**：成本低但数组缓冲/类元素泄漏
  （arr-probe1/2）不解决——长期宿主主内存项仍敞口；
- **D0（维持现状）**：长期宿主线性增长 + 快照悬垂 UB 面，不可宣称 .NET 对等；
- **采纳 D2（ArcHeader 化）**：S1–S4 已落地（2026-09-07）；string 元素/键表示面按 §2.3
  维持借用并文档化，待表示收敛后另排。

## 4. 发布线结论（0.1 发布前置 · 已收口）

- **结论**：RFC 052 升格为 **0.1 发布前置** 且 **S1–S4 已实施**（本变更集）。数组对象
  ArcHeader 化 + 全谱 drop 站点 + owned Keys/Values 快照 + class 键所有权已接线；
  string 键/元素按 §2.3 维持借用（非 ArcHeader）。
- **宣称纪律**：owned 字典 Keys/Values 可宣称 .NET 快照对等；legacy 字典仍为借用视图；
  string 键/元素不作释放承诺直至表示面收敛。