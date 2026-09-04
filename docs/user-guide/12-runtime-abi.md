# 12 运行时 ABI

Arc 生成的二进制链接 `crates/runtime` 与 `crates/runtime/platform/`，通过稳定的 C ABI 符号与宿主环境交互。

## 头文件与命名

- 头文件：`crates/runtime/rt_abi.h`
- 实现：`crates/runtime/rt_*.c`（按能力拆分的多文件；链接列表以 `codegen` 的 `rt_sources` 为准，例如 `rt_str.c`、`rt_dict.c`、`rt_list.c`、`rt_exc.c`、`rt_task.c`、`rt_file.c` 等）
- 符号前缀：`rt_`

## 控制台与 panic

```c
void rt_print(const char* msg);           /* 无换行 stdout */
void rt_println(const char* msg);         /* 带换行 stdout */
void rt_print_error(const char* msg);     /* 无换行 stderr */
void rt_println_error(const char* msg);   /* 带换行 stderr */
void rt_panic(const char* msg);
```

`Console.Write` / `WriteLine` / `ErrorWrite` / `ErrorWriteLine` 经 codegen 发射上述符号；`Window.Run(...)` 经 lowering 调用 `crates/runtime/platform/<os>/window.*`。不可恢复错误调用 `rt_panic`，默认向 stderr 输出并终止（策略可平台定制）。

## Environment（`rt_env_*` · Stable 最小面）

```c
void     rt_env_init(int argc, char** argv);
int32_t  rt_env_argc(void);
const char* rt_env_argv(int32_t index);
char*    rt_env_get_var(const char* name);   /* 未设置 → 空串（非 NULL） */
int32_t  rt_env_set_var(const char* name, const char* value); /* value=NULL/"" 删除 */
void     rt_env_exit(int32_t code);
int32_t  rt_env_get_exit_code(void);
void     rt_env_set_exit_code(int32_t code);
void     rt_env_fail_fast(const char* msg);
const char* rt_env_newline(void);
int32_t  rt_env_processor_count(void);
int32_t  rt_env_is_64bit_process(void);
char*    rt_env_get_cwd(void);
int32_t  rt_env_set_cwd(const char* path);
char*    rt_env_machine_name(void);
char*    rt_env_user_name(void);
const char* rt_env_platform(void);
int32_t  rt_env_is_windows / linux / macos / android / ios / ohos(void);
```

`Arc.Environment` 经 `try_emit_environment_static` 发射上述符号（与 `rt_abi.h` / `rt_env.c` 对齐）。**已撤面**（无头文件符号、无 codegen 臂、禁空串假 stub）：`ProcessId` / `ProcessPath` / `ExpandEnvironmentVariables` / `GetFolderPath`。计时单一惯用法 = `Stopwatch`（无 `TickCount*`）。

## 字符串

```c
char*   rt_str_concat(const char* a, const char* b);
int32_t rt_str_length(const char* s);
int32_t rt_str_char_at(const char* s, int32_t index); /* UTF-8 码元 → char；越界 0 */
int32_t rt_str_equals(const char* a, const char* b);
int32_t rt_str_compare(const char* a, const char* b);
```

`string` 的 `+`、`==`/`!=`、`.Length`、只读索引 `s[i]`（→ `get_Chars` / `rt_str_char_at`）、`string.Compare` lowering 至上述符号；`Length` 与 `s[i]` 均以 **UTF-8 码元（字节）** 为索引单位（非 UTF-16）；`rt_str_equals` 相等返回 `1`，不等返回 `0`；`rt_str_compare` 返回 `strcmp` 风格有符号差值。

Stable 加深面（非完备对等）：`IsNullOrEmpty`（codegen 内联 null/空）、`IsNullOrWhiteSpace`→`rt_str_is_null_or_white_space`；`IndexOf`/`LastIndexOf` 一参与起算位置二参（`rt_str_*_from` / `*_char_from`）；`FromCharCount` / `Concat`；`Split(char|string)`→`rt_str_split`/`rt_str_split_char`（`string[]`）；`string.Join(string|char, string[])`→`rt_str_join`/`rt_str_join_char`；`ToCharArray()`/`ToCharArray(start,length)`→`rt_str_to_char_array`/`rt_str_to_char_array_range`（`char[]`，UTF-8 码元；range 越界钳制同 Substring）；`PadLeft`/`PadRight(int, char)`→`rt_str_pad_*_char`；`Trim`/`TrimStart`/`TrimEnd(char)`→`rt_str_trim*_char`；`Trim(params char[])`/`Trim(char[])`→`rt_str_trim*_chars`（空集→空白 trim）；`Split(char|string, StringSplitOptions)`→`rt_str_split*_opts`（`None`/`RemoveEmptyEntries`/`TrimEntries` 可按位或）；`Split(params char|char[])`→`rt_str_split_chars*`；`Split(sep, count, options)`→`rt_str_split*_opts_count`（MIR 按实参类型改写方法名分派）；`StartsWith`/`EndsWith(char)`→`rt_str_*_char`；`string.Compare`/`CompareOrdinal`（两静态均 → `rt_str_compare`，**无文化面**，ordinal ≡ UTF-8 码元 strcmp）。后置：`Split(string[])` 多分隔符、两参 count、Invariant 文化。

## UTF-8 Encoding

```c
void*   rt_text_utf8_get_bytes(const char* s);      /* string → byte[]（rt_array，elem_size=1） */
char*   rt_text_utf8_get_string(void* bytes);       /* byte[] → malloc'd NUL-terminated string */
int32_t rt_text_utf8_get_byte_count(const char* s); /* UTF-8 码元数（strlen；null→0） */
```

`Arc.Text.Encoding.GetBytes` / `GetString` / `GetByteCount` 经 codegen 拦截发射上述符号。Arc `string` 已是 UTF-8；`GetBytes` 按 `strlen` 拷贝到 `rt_array_create` 负载；`GetString` 按 `rt_array_length` 拷贝并追加 NUL；`GetByteCount` 与 `GetBytes.Length` / `string.Length` 对齐。无内部 `0x00` 的文本可往返；含内部 NUL 时后续依赖 `strlen` 的 string 运算会截断（C-string 模型既有限制）。

## BitConverter / Buffer（字节缓冲）

```c
int32_t rt_bitconverter_is_little_endian(void);
void*   rt_bitconverter_get_bytes_i32(int32_t value);   /* → byte[4] */
void*   rt_bitconverter_get_bytes_i64(int64_t value);   /* → byte[8] */
int32_t rt_bitconverter_to_i32(void* bytes, int32_t start_index);
int64_t rt_bitconverter_to_i64(void* bytes, int32_t start_index);
/* Buffer.BlockCopy(byte[]) → rt_array_copy（elem_size=1；偏移=字节下标） */
```

`Arc.BitConverter`：主机端序（与 C# `System.BitConverter` 一致）；`IsLittleEndian()` 为方法（Arc 无静态属性）。Stable 面：`GetBytes(int|long|float|double)` / `ToInt32` / `ToInt64` / `ToSingle` / `ToDouble`。float/double 位型重释（`SingleToInt32Bits` 等）为 codegen `bitcast` 内建，字节编解码复用既有 i32/i64 ABI（见 [RFC 014 §BitConverter](../../docs/rfc/014-runtime-abi.md)）。`Arc.Buffer.BlockCopy` 仅 `byte[]`（任意 `Array` 字节级拷贝后置）。

## 关联表（`Dictionary<K,V>`）

```c
typedef uint32_t (*rt_hash_fn)(void*);
typedef int32_t  (*rt_eq_fn)(void*, void*);

void*   rt_dict_create(rt_hash_fn hash, rt_eq_fn eq);
void    rt_dict_set(void* dict, void* key, void* value);
void*   rt_dict_get(void* dict, void* key);
int32_t rt_dict_contains(void* dict, void* key);
int32_t rt_dict_contains_value(void* dict, void* value, rt_eq_fn eq); /* 按值相等扫表 */
int32_t rt_dict_try_add(void* dict, void* key, void* value);
int32_t rt_dict_try_get_value(void* dict, void* key, void** out_value);
int32_t rt_dict_count(void* dict);
int32_t rt_dict_remove(void* dict, void* key);
void    rt_dict_clear(void* dict);
void    rt_dict_destroy(void* dict);
void*   rt_dict_keys(void* dict);
void*   rt_dict_values(void* dict);
void*   rt_dict_get_enumerator(void* dict);
int32_t rt_dict_enumerator_move_next(void* handle);
void*   rt_dict_enumerator_get_key(void* handle);
void*   rt_dict_enumerator_get_value(void* handle);

uint32_t rt_hash_str(void* key);
int32_t  rt_eq_str(void* a, void* b);
uint32_t rt_hash_int(void* key);   /* 标量键：哈希指针位 */
int32_t  rt_eq_int(void* a, void* b);
```

`Arc.Collections.Dictionary<K,V>`（`where K : IEquatable<K>`）经单态化与 codegen stub 调用上述符号；`_handle` 保存堆上 `RtDict*`。实现：`rt_dict.c`（**SoA 开放寻址**：`hashes|keys|values` 单块；负载因子超过 0.75 时 2× 扩容；`rt_hash_int`/`rt_eq_int` 整键快路径）。键/值在 ABI 层均为 `void*`：标量经 codegen 装箱为指针位；`string`/引用类型直传指针。

**`TryGetValue` / `ContainsValue` / Stable 链接 stub 契约**：`TryGetValue` 签名为 `(self, key, out ptr) → i1`，体走 `rt_dict_try_get_value`；`ContainsValue` 走 `rt_dict_contains_value`（注入值 eq）；`Add`/`Keys`/`Values` 链接 stub 分别走 `rt_dict_try_add` / `rt_dict_keys` / `rt_dict_values`。**禁止**缺 out 槽或静默 `ret i1 false`（IndirectCall / 未 inline 路径会假绿或 `0xc0000005`）。`GetEnumerator` 热路径由 `emit_builtin` 装配 itable；链接兜底 `rt_panic`（禁静默假绿）。ConcurrentDictionary Stable 面链接 stub **真转发** `rt_concurrent_dict_*`（与 Dictionary 同构）；未挂面的 `Values`/`ToArray`/`AddOrUpdate` → `rt_panic`（禁静默 `0`/`null`）。`rt_concurrent_dict_try_get` / `try_remove`：**miss 须写 `*out_value = NULL`**（对齐 `rt_dict_try_get_value`；标量 out 经 `ptrtoint` 得 `default(V)`；禁未初始化泄漏）。

**键哈希/相等**（`rt_dict_create` 注入；同一 C 实现服务全部单态化）：

| 键类型 | hash | eq |
|--------|------|-----|
| `string` | `rt_hash_str` | `rt_eq_str` |
| 标量（`int`/`long`/…） | `rt_hash_int` | `rt_eq_int` |
| 用户类型（含 record；`IHashable`/`IEquatable`） | `@__dict_hash_{K}` → `K.GetHashCode` | `@__dict_eq_{K}` → `K.Equals` |

用户类型键路径零装箱、零运行时类型分派。

## out/ref 形参 byref 转发不变量

对齐 C#/CLI byref 语义（ECMA-335 I 11.4.1.5 / §12.4.1.5）：`out`/`ref` 形参在调用约定中为**指针传递**（managed pointer），callee 写入指针指向的值，调用方在调用后读取同一存储。由此确立编译期不变量：

- **形参表示**：`out T v` / `ref T v` 的 MIR 局部槽类型为 `TypeId::Ref`，槽内**存储调用方变量的指针**；函数入口把 ABI `ptr` 实参写入该槽，读/写形参经「load 槽内指针 → load/store 目标」两级解引用。
- **转发接线**：把 `out v`/`ref v` 作为实参转发给被调方法时，传递的必须是**槽内存储的指针**（`load ptr, ptr %vN`），而非槽地址 `%vN` 本身——否则被调方把值写进指针槽（覆写指针），调用方变量永远收不到值。codegen 统一经 `byref_arg_ptr(id)`（普通值局部 → 槽地址；`Ref` 局部 → 转发存储指针）计算 byref 目标，普通函数调用、builtin 内联、native FFI 三条路径共用同一规则。
- **确定性赋值**：typeck 的 out 形参赋值检查在 **return 表达式求值之后**执行——`return dict.TryGetValue(k, out v);` 的 `v` 由 `RefArg` 求值路径标记已定值，先检查后求值会把尚未定值的 `v` 误判为未赋值。

## 动态数组

```c
void*   rt_list_create(int32_t elem_size, ...);
void    rt_list_destroy(void* handle);
void    rt_list_push(void* handle, const void* elem_ptr);
void    rt_list_get(void* handle, int32_t idx, void* out_ptr);
void    rt_list_set(void* handle, int32_t idx, const void* elem_ptr);
void*   rt_list_at(void* handle, int32_t idx);  /* 越界 panic；返回元素槽指针 */
void    rt_list_ensure_capacity(void* handle, int32_t needed); /* 冷路径扩容 */
int32_t rt_list_size(void* handle);
```

`Arc.Collections.List<T>` 经单态化 `List_int`/`List_string`；`_handle` 指向 `RtList`。索引器 `list[i]`（`get_Item`/`set_Item`）与**值类型** `Add` 的 **热路径由 codegen 直访** `RtList`（布局契约：`data@0`、`size@8`、`capacity@12`、`elem_size@16`）：`Add` 满容时调 `rt_list_ensure_capacity`，否则 GEP+store+size++（无 `rt_list_push`/alloca/memcpy）；索引越界 `rt_panic`。`rt_list_at` / `rt_list_push` 供 stub 与**引用元素**（ARC）回退。`rt_list_get`/`rt_list_set` 内部经 `rt_list_at`；引用元素 `set`/`Add` 仍走 `rt_list_*` 维护 ARC。

## 原生数组工具（`Arc.Array` · Stable 最小面）

堆数组带 `RtArrayHeader`（`rt_array_create` / `rt_array_length` / `rt_array_destroy`）。静态工具面经 codegen `Array.*` →：

```c
void    rt_array_copy / clear / reverse(...);
int32_t rt_array_index_of_int / last_index_of_int(void* payload, int32_t value);
void    rt_array_resize(void** slot, int32_t new_size);  /* null → elem_size=sizeof(int32) */
int32_t rt_array_exists / find_index / find_last_index / true_for_all(void*, rt_list_pred_fn);
int32_t rt_array_find_int / find_last_int(void*, rt_list_pred_fn);
void    rt_array_for_each(void*, rt_list_pred_fn);
void    rt_array_sort_int(void*);
int32_t rt_array_binary_search_int(void*, int32_t);
void*   rt_array_find_all_int(void*, rt_list_pred_fn);
void*   rt_array_convert_all_int(void*, rt_list_pred_fn);  /* converter 返回映射 int */
/* Empty → rt_array_create(0, 4) */
```

`std/Arc/Array.as` Stable 公开面（均为 `[Builtin]`；Array 为 stub facade）：

| API | 落地 |
|-----|------|
| `Copy` / `Clear` / `Reverse` | `rt_array_*`（header `elem_size`；泛型 + `int[]`） |
| `IndexOf` / `LastIndexOf` / `Empty` / `Resize` | 仅 `int[]`（`Empty`→`rt_array_create(0,4)`） |
| `Exists` / `Find` / `FindLast` / `FindIndex` / `FindLastIndex` / `TrueForAll` / `ForEach` | 仅 `int[]` + `Func`/`Action` trampoline（同 `rt_list_pred_fn`） |
| `Sort` / `BinarySearch` | 仅 `int[]`；升序；未命中返回 `~insertionPoint` |
| `FindAll` / `ConvertAll` | 仅 `int[]`；`FindAll`→新建匹配数组；`ConvertAll`→`Func<int,int>` 映射（跨类型后置） |

后置：`Join`（C# `System.Array` 无此成员，勿发明；`string.Join` 另轨）/ 泛型 `Empty<T>` / 定制比较器 / 跨类型 `ConvertAll`——**禁止**空 stub 挂 Stable。

## 双向链表（LinkedList · Stable 最小面）

```c
void*  rt_linked_list_create(int32_t elem_size, rt_list_eq_fn, rt_list_arc_fn, rt_list_arc_fn);
void*  rt_linked_list_add_last / add_first / first / last / find(…);  /* → RtLinkedListNode* */
void   rt_linked_list_node_value(void* node, void* out);
void*  rt_linked_list_node_prev / next(void* node);
```

`LinkedListNode<T>` 为 **不透明节点句柄透传**（identity）：`Add*`/`First`/`Find` 返回的即 `RtLinkedListNode*`，属性访问走 `rt_linked_list_node_*`。codegen 禁止把节点当 Arc 对象 `rt_arc_inc`/`dec`（会写坏 `value` 槽）。`node.List` 返回 runtime 链表句柄，非 Arc 包装——Stable 面不依赖。

## 排序集合（SortedSet · Stable 最小面）

```c
void*   rt_sorted_set_create(rt_cmp_fn cmp);   /* 标量 @rt_cmp_int；string @rt_cmp_str */
int32_t rt_sorted_set_add / contains / remove(void* handle, void* key);
int32_t rt_sorted_set_min / max(void* handle, void* out_ptr);  /* out 写 void*；空集返回 0 */
int32_t rt_sorted_set_count(void* handle);
void    rt_sorted_set_clear / destroy(void* handle);
```

标量元素与 SortedDictionary 同为 **inttoptr 装箱**（`rt_cmp_int` 比较指针位）；禁止栈 `alloca` 假指针。Stable 面：ctor / Add / Contains / Remove / Min / Max / Count / Clear。比较器 ctor、Reverse、GetViewBetween、集合运算未进公开面。空集 Min/Max 未定义。

## 排序字典（SortedDictionary · Stable 最小面）

```c
void*   rt_sorted_dict_create(rt_cmp_fn cmp);   /* 标量 @rt_cmp_int；string @rt_cmp_str */
void*   rt_sorted_dict_get(void* handle, void* key);           /* NULL if missing */
void    rt_sorted_dict_set(void* handle, void* key, void* value);
int32_t rt_sorted_dict_add / try_get / remove / contains(…);
int32_t rt_sorted_dict_count(void* handle);
void    rt_sorted_dict_clear / destroy(void* handle);
```

标量键/值为 **inttoptr 装箱**（`rt_cmp_int` 比较指针位）；禁止栈 `alloca` 假指针。Stable 面：ctor / 索引器 / Add / TryGetValue / ContainsKey / Remove / Count / Clear。比较器 ctor、Keys/Values 未进公开面。

## 异常（最小面）

```c
void rt_throw(void* ex);           /* native raise (Win: _CxxThrowException) */
void* rt_get_exception(void);      /* TLS slot bound by the catch site */
char* rt_format_stacktrace(void);  /* malloc'd multiline stack string for Exception.StackTrace */
```

`try`/`catch`/`finally` lowering：**已切换为** LLVM `invoke`/`landingpad`（Windows SEH 主平台：`catchswitch`/`catchpad`/`cleanuppad`，`__CxxFrameHandler3` personality；未抛出路径零开销、finally 深层 unwind 恒执行、catch 类型过滤 C# 对齐、`rt_exception` TLS）。**async 状态机协作**：await 提取点 faulted Task 经 `rt_task_is_faulted`/`rt_task_get_exception` rethrow → 外层 catch（try 跨 await 语义正确）；cleanup funclet 内 `call` 携带 `"funclet"("token")` 操作数（LLVM WinEH 强制），已知 nounwind 外部与 facade 方法按 `RT_MAY_THROW` 镜像标注；**并发与性能**：`rt_exception` TLS 多线程隔离（4 线程并发 throw/catch 确定性输出），H2 热路径门禁 exit 0——未抛出路径零开销；POSIX Itanium 仍后置。`finally` 已落地。catch 的运行时类型过滤（`when` / 精确类型匹配）未覆盖路径须硬错误而非静默吞异常。

**`Exception.StackTrace`**：`throw` 降级在 `rt_throw` 前调用 `rt_format_stacktrace()`，写入 `Exception.StackTrace`（仅当槽位仍为 null）。构造后为 `null`。捕获真实返回地址；**嵌入 `__arc_dbg_table` 默认发射**（与 DWARF `-g` 解耦；Windows MSVC/MinGW 与 POSIX 同路径）→ 函数名 + 可行时 file:line；POSIX `backtrace_symbols` 次级；仍无符号时 `at <0x…>`；极端无帧时 `at <throw>`。**不**宣称 PDB 级完美还原。

**`Exception.ToString`（附栈）**：`Message`（+ `" ---> "` 内层 `ToString`）；若 `StackTrace != null` 则换行追加栈串。构造未 throw 时不附栈。

**`nounwind` 与嵌套虚分派**：当前异常模型为 LLVM `invoke`/`landingpad`（Windows SEH 主平台）。用户函数的 `nounwind` 由**模块内 call-graph 不动点**推断：无局部 `Throw`/`TryCatch`，且每个调用均解析到已知 `nounwind` 被调方时才标注。已知 `nounwind` 被调方包括：

1. **模块内**已推断为 `nounwind` 的用户函数；
2. **`rt_*` 白名单**：闭世界审计下，除 may-throw 表外的全部 `rt_*` 视为不 unwind 的 leaf（含 `rt_get_exception`/`rt_panic*`——它们不向外传播异常；`TryCatch` 仍由 MIR 局部语句置 may-throw）。**不得**进白名单的符号见下表。
3. 常用 libc leaf（`malloc`/`free`/`memcpy`/…）与 `llvm.*` intrinsic。

虚分派 / 接口 / 间接 / **未知外部**（native FFI 等）一律视为 may-throw——中间帧若误标 `nounwind`，unwind 会穿栈导致 `STATUS_BAD_STACK`（Windows `0xc00000ff`）。zero-cost EH（`invoke`/`landingpad`）已落地；`RT_MAY_THROW` 表同时作为 invoke 转换判据；已知 nounwind `declare` 统一补 `nounwind`，facade 方法（`File.AppendAllText`/`Console.WriteLine` 等）按 `RT_MAY_THROW` 镜像判 nounwind。

**`rt_*` may-throw 表**（codegen `attr.rs` · `RT_MAY_THROW`；新增同栈回调或 unwind 的 `rt_*` 必须追加）：

| 类别 | 符号 |
|------|------|
| EH（native raise） | `rt_throw` |
| List 同栈谓词/比较回调 | `rt_list_find_get` / `find_all` / `exists` / `for_each` / `remove_all` / `sort` / `binary_search_cmp` |
| 并行同栈 body | `rt_parallel_for` / `rt_parallel_foreach` |
| QIF 同栈跑测（未来可 setjmp） | `rt_qif_try_run` / `rt_qif_run_all` |
| CTS 可能同步点火 | `rt_cts_register` / `rt_cts_register_lf` / `rt_cts_node_try_fire` |
| ConcurrentDict 工厂回调 | `rt_concurrent_dict_get_or_add` / `get_or_add_val` / `add_or_update` / `add_or_update_pf` |
| ContinueWith 可能同步续体 | `rt_task_continue_with` |

边界：异步投递（`rt_thread_create` / `rt_threadpool_spawn*` / `rt_task_run*` 等）当前**允许**进白名单——回调不在调用方同栈展开；若未来改为同栈同步执行，须移入 may-throw 表。

## 文件 I/O（基础文件操作 + 目录与路径）

```c
/* 文件读写与基础操作（rt_file.c） */
char* rt_read_file(const char* path);
int32_t rt_write_file(const char* path, const char* content);
int32_t rt_file_exists(const char* path);
int32_t rt_file_delete(const char* path);
int32_t rt_file_append(const char* path, const char* content);
int32_t rt_file_copy(const char* src, const char* dst);
int32_t rt_file_move(const char* src, const char* dst);

/* 目录操作 */
int32_t rt_dir_create(const char* path);
int32_t rt_dir_exists(const char* path);
int32_t rt_dir_delete(const char* path);
void*   rt_dir_list_files(const char* path); /* → string[] 完整路径；失败/空 Length 0 */

/* 路径字符串操作（纯计算，无 I/O 副作用） */
char* rt_path_combine(const char* a, const char* b);
char* rt_path_get_dir_name(const char* path);
char* rt_path_get_file_name(const char* path);
char* rt_path_get_file_name_without_ext(const char* path);
char* rt_path_get_extension(const char* path);

/* FileStream（rt_file_stream.c） */
void*   rt_file_stream_open(const char* path, int32_t mode); /* 0=read 1=write 2=create */
void    rt_file_stream_close(void* handle);
int32_t rt_file_stream_read(void* handle, void* buffer, int32_t offset, int32_t count);
void    rt_file_stream_write(void* handle, void* buffer, int32_t offset, int32_t count);
int64_t rt_file_stream_seek(void* handle, int64_t offset, int32_t origin);
int64_t rt_file_stream_get_length(void* handle);
int64_t rt_file_stream_get_position(void* handle);
void    rt_file_stream_set_position(void* handle, int64_t value);
void    rt_file_stream_set_length(void* handle, int64_t value);
void    rt_file_stream_flush(void* handle);
int32_t rt_file_stream_can_read(void* handle);
int32_t rt_file_stream_can_write(void* handle);
int32_t rt_file_stream_can_seek(void* handle);

/* Memory-mapped file + CodeEditor buffer（rt_file_mmap.c / rt_editor.c） */
void*       rt_file_mmap_open(const char* path);
void        rt_file_mmap_close(void* handle);
int64_t     rt_file_mmap_length(void* handle);
const char* rt_file_mmap_data(void* handle);
void*    rt_editor_create_empty(void);
void*    rt_editor_open_path(const char* path);
void     rt_editor_destroy(void* handle);
int64_t  rt_editor_length(void* handle);
int32_t  rt_editor_line_count(void* handle);
int32_t  rt_editor_ensure_lines(void* handle, int32_t first_line, int32_t last_line);
char*    rt_editor_line_text(void* handle, int32_t line_no);
int32_t  rt_editor_set_text(void* handle, const char* text);
int32_t  rt_editor_insert(void* handle, int64_t offset, const char* text);
int32_t  rt_editor_delete(void* handle, int64_t offset, int64_t length);
int32_t  rt_editor_is_mmap_backed(void* handle);
```

`File.ReadAllText` / `WriteAllText` / `Exists` / `Delete` / `AppendAllText` / `Copy` / `Move`（及对应 `*Async`）、`Directory.CreateDirectory` / `Exists` / `Delete` / **`GetFiles`** / **`GetFiles(searchPattern)`** / **`GetDirectories`**、`Path.Combine`（二元）/ `GetDirectoryName` / `GetFileName` / `GetFileNameWithoutExtension` / `GetExtension` 分别 lowering 到同名符号（`rt_file_*` / `rt_dir_*` / `rt_path_*`）。`File.OpenRead` / `OpenWrite` / `OpenText` / `Create` 与 `FileStream` 走 `rt_file_stream_*`。

**诚实面收窄**：`std/Arc/IO` 仅保留已接线方法；未接线扩展禁止 stub 静默 `null`/`false`。`Open*` 经 `FileStream` 重新挂回。**已加深**：`File.ReadAllLines`→`rt_file_read_all_lines`；`Path.GetTempPath`→`rt_path_get_temp_path`（尾部分隔符）；**`Directory.GetFiles`→`rt_dir_list_files`**；**`GetFiles(path, searchPattern)`→`rt_dir_list_files_pattern`（`*`/`?` 在 C 侧匹配；非 codegen filter）**；**`GetDirectories`→`rt_dir_list_dirs`（跳过 `.`/`..`）**；完整路径 `string[]`；失败/空 Length 0；`MemoryStream.ToArray`（纯 Arc）。**仍后置**：`SearchOption` / `GetDirectories(searchPattern)` / Move / 当前目录。

**Guid 字节面**：`ToByteArray` / `FromByteArray`（.NET 混合端序；纯 Arc）。**无** `Guid(byte[])` ctor（string/byte[] 同载荷会吞字符串 ctor）。

**错误模型**：与 C# `System.IO` 对齐，返回值模型而非异常——`bool` 返回类型用 `i32`（0/1）表示；`string` 返回类型用 `ptr`（malloc'd NUL-terminated，失败返回空串）；操作失败统一返回 `0`/空串，不引入异常机制。`Path.GetFileName` 遵循 C# 语义：路径以分隔符结尾时返回空串（表示目录而非文件）。

**跨平台**：路径分隔符统一使用 `/`（现代 Windows 接受正斜杠）；`rt_file.c` 在 Windows 下 `typedef long long ssize_t` 补齐 POSIX 类型缺失。

## 接口 fat pointer（itable）

接口值不是裸对象指针，而是指向 `{ ptr obj, ptr itable }` 的 `ptr`（详见 [07 对象模型 · 接口值 ABI](07-object-model.md#接口值-abifat-pointer)）。

- itable 全局：`@.itable.{Class}_{Iface}`（仅对**直接声明**该接口的类发射；派生类赋值复用祖先 itable）
- 方法调用：`fn = itable[slot]; call fn(obj, …)`
- 禁止在调用点把已有 fat pointer 再包一层（会把 fat 地址误当作 `obj`）

## ARC

```c
void rt_arc_inc(void* ptr);
void rt_arc_dec(void* ptr);
```

`class` 句柄复制、赋值、传参及作用域结束处，codegen 插入 inc/dec。语义见[内存与资源](06-memory-resources.md)。

> **循环收集指针**：循环收集默认 **always-on**（编译进每个二进制、阈值触发、用户无感）已立宪；语义变化：rc→0 且可成环对象**延迟释放**（滞留至下次收集），finalizer **不再在 rc→0 同步执行**——确定性时序依赖 `Weak<T>` 或显式 `Dispose`。

## Stopwatch（高精度计时）

```c
int64_t rt_stopwatch_get_timestamp(void);      /* 单调时钟原始 ticks */
int64_t rt_stopwatch_frequency(void);          /* 每秒 ticks */
int32_t rt_stopwatch_is_high_resolution(void); /* 1 = 高精度（当前恒为 1） */
```

`Arc.Diagnostics.Stopwatch`（对标 C# `System.Diagnostics.Stopwatch`）经 `crates/arc/native/rt_resources.ani` 调用上述符号；实现于 `rt_resources.c`。

| 平台 | 时间源 | Frequency |
|------|--------|-----------|
| Windows | `QueryPerformanceCounter` | `QueryPerformanceFrequency` |
| POSIX | `CLOCK_MONOTONIC` 纳秒 | `1_000_000_000` |

`ElapsedTicks` 为计时器原始 ticks；`Elapsed` / `ElapsedMilliseconds` 换算为 TimeSpan 刻度（每秒 10_000_000）。**计时单一惯用法**：间隔测量走 Stopwatch；`Environment` **无** `TickCount` / `TickCount64`。

## 进程（`rt_proc_*` · 子进程生命周期 + 资源统计）

`Arc.Diagnostics.Process` 经 `crates/arc/native/rt_process.ani` 契约调用（实现 `crates/runtime/rt_proc.c`）：

```c
void*    rt_proc_spawn(const char* exe, const char* args, const char* wd, int32_t rstdin, int32_t rstdout, int32_t rstderr, int32_t no_window, int32_t* in_fd, int32_t* out_fd, int32_t* err_fd);
int32_t  rt_proc_wait(void* handle, int64_t timeout_ms);   /* 0 退出 / 1 超时 / -1 失败 */
int32_t  rt_proc_kill(void* handle);
int32_t  rt_proc_close(void* handle);
int32_t  rt_proc_get_pid(void* handle);
int32_t  rt_proc_get_exit_code(void* handle);              /* 运行中：Windows STILL_ACTIVE(259) */
int32_t  rt_proc_get_current_pid(void);
/* 管道 I/O / PTY（rt_pty_*）…… */
```

**资源统计（RFC 043 P1 新增 additive 符号）**：

```c
int32_t rt_proc_get_stats(void* handle, int64_t* out_user_ms, int64_t* out_kernel_ms,
                          int64_t* out_peak_mem_bytes, int32_t* out_exit_reason);
```

- 平台实现：Windows `GetProcessTimes`（UserTime/KernelTime，100ns→ms）+ `GetProcessMemoryInfo`（`K32GetProcessMemoryInfo` 动态加载免链 psapi.lib，PeakWorkingSetSize）；POSIX `getrusage`（ru_utime/ru_stime→ms；ru_maxrss 归一字节：macOS 直接字节、其余 ×1024）+ `exit_reason` 暴露 `WIFSIGNALED/WTERMSIG` 信号号（`wait_status` 经 `waitpid` 保留，补齐「信号终止丢信号号」缺口）。
- `exit_reason` 编码：`0` = 正常退出；`>0` = 被信号终止（POSIX 信号号）；`<0` = 尚未退出。Windows 无信号语义：崩溃码经 `exit_code` 暴露（如 `0xC0000005`）。
- Arc 消费面：`ProcessRunStats`（`ElapsedMs` / `PeakMemoryBytes` / `CpuUserMs` / `CpuKernelMs` / `ExitReason` / `ExitSignal`）+ `ProcessRunResult.Stats` / `TimedOut`；`Process.RunCaptureAsync(si, timeoutMs, ct)` 超时捕获（`WaitForExit(timeoutMs)` → 超时 `Kill` → `TimedOut`）。additive：不改变既有 `rt_proc_*` 语义。

## 异步 Task

```c
#define RT_TASK_READY    0
#define RT_TASK_PENDING  1
#define RT_TASK_FAULTED  2
#define RT_TASK_CANCELED 3

typedef struct RtTask RtTask;

/* Waker：外部事件唤醒 Pending Task。 */
typedef struct rt_waker {
    void (*wake)(void* data);
    void* data;
} rt_waker;

/* ---- Phase A（async 状态机基础） ---- */
RtTask* rt_task_alloc(void);
void*   rt_task_from_int(int32_t value);       /* int 结果 → 已完成 Task */
void*   rt_task_void(void);                    /* void 结果 → 已完成 Task */
int32_t rt_task_poll(void* state);             /* 推进状态机；返回 RT_TASK_* */
int32_t rt_task_result_int(void* state);       /* 读取 int 结果 */

/* ---- 泛型结果提取 + 取消 + 状态查询 ---- */
void*   rt_task_from_ptr(void* value);               /* 指针结果（string/class/array）→ 已完成 Task */
void*   rt_task_from_value(void* data, int32_t size); /* 值类型结果（double/long/Vector）→ 已完成 Task */
int32_t rt_task_status(void* state);                 /* 仅查询状态，不推进（vs poll 推进） */
void*   rt_task_result_ptr(void* state);             /* 读取指针结果 */
void    rt_task_result_value(void* state, void* dst, int32_t size); /* 读取值类型结果到 dst */
void    rt_task_cancel(void* state);                 /* 标记取消：状态 → CANCELED */
int32_t rt_task_is_canceled(void* state);            /* 查询取消标志 */

/* ---- 状态机 result 写回 + resume 签名升级 ---- */
void*   rt_task_from_state_machine(void* env, void* resume_fn); /* resume_fn: int32_t (*)(void* env, rt_waker* waker) */
void    rt_task_set_result_int(void* state, int32_t value);    /* resume 完成时写 int 结果到 Task 句柄 */
void    rt_task_set_result_ptr(void* state, void* value);      /* resume 完成时写指针结果到 Task 句柄 */
void    rt_task_set_result_value(void* state, void* data, int32_t size); /* resume 完成时写值类型结果 */

/* ---- waker 真实唤醒 ---- */
void    rt_task_set_waker(void* state, rt_waker* waker);
void    rt_waker_wake(rt_waker* waker);

/* ---- EventLoop 真实 suspend/resume ---- */
void    rt_task_complete(void* state);                 /* 设置 status=READY + 触发 waker（定时器到期/IO 完成调用） */
void    rt_task_register_waker(void* inner, void* outer); /* inner 完成时通过 waker 将 outer 移入就绪队列 */
void*   rt_task_delay(int32_t milliseconds);           /* 创建 Pending Task + 定时器；无 EventLoop 时 fallback Ready */

/* EventLoop ABI */
void*   rt_event_loop_create(void);                    /* 初始化 + 设置 g_current_loop + g_rt_wake_fn */
void    rt_event_loop_destroy(void* loop);
void    rt_event_loop_run(void* loop);                 /* 主循环：tick → fire_expired → 退出判断 → condvar wait */
void    rt_event_loop_stop(void* loop);
int32_t rt_event_loop_tick(void* loop);                /* 快照就绪队列 + 逐个 poll，返回处理数量 */
void    rt_event_loop_spawn(void* loop, void* task);   /* 线程安全：mutex lock + push 就绪队列 + condvar signal */
void    rt_event_loop_set_current(void* loop);
void*   rt_event_loop_current(void);                   /* 返回 g_current_loop（单线程 MVP） */
void    rt_event_loop_inc_pending(void* loop);         /* pending_count++（跟踪未完成 Task 数量） */
void    rt_event_loop_dec_pending(void* loop);         /* pending_count--（Task 完成时调用） */

/* 全局函数指针：解耦 rt_task.c → rt_event_loop.c 反向调用 */
typedef void (*rt_waker_fn_ptr)(void* data);
extern rt_waker_fn_ptr g_rt_wake_fn;                   /* rt_event_loop_create 时初始化为 rt_task_default_wake */
```

**EventLoop 调度器**（`runtime/rt_event_loop.c`）：单线程调度器驱动状态机真实 suspend/resume。

- **就绪队列**：ring buffer（容量 256），mutex 保护 push（`rt_event_loop_spawn`），condvar signal 唤醒 EventLoop 线程。
- **定时器堆**：有序单链表（按 deadline_ms 升序），`fire_expired` 扫描到期定时器并调用 `rt_task_complete` 触发 waker。
- **跨线程唤醒**：`g_rt_wake_fn` 全局函数指针让 `rt_task.c` 反向引用 `rt_event_loop.c` 的 `rt_task_default_wake`，避免循环依赖。`rt_task_default_wake` 可从任意线程调用（mutex 保护就绪队列 push）。
- **waker 内嵌槽**：`RtTask` 内嵌 `_waker_slot` 字段（避免堆分配 binding）。`rt_task_register_waker(inner, outer)` 设置 inner task 的 `_waker_slot.wake = g_rt_wake_fn` + `_waker_slot.data = outer_task`。inner 完成时通过 `rt_task_complete` → `rt_waker_wake` → `rt_task_default_wake` → `rt_event_loop_spawn(loop, outer_task)` 将 outer 移入就绪队列。
- **EventLoop run 循环**：`tick`（快照就绪队列 + 逐个 poll）→ `fire_expired`（处理到期定时器）→ 判断退出（无 pending + 无 ready + 无 timer）→ condvar wait 到下一定时器 deadline。
- **Task.Delay**：`rt_task_delay(ms)` 创建 Pending Task + 定时器节点 + 注册到 EventLoop。codegen `try_emit_task_static` 拦截 `Task.Delay(int)` → `call ptr @rt_task_delay(i32)`。
- **状态机 await waker 集成**：`emit_sm_await` suspend 块新增 waker 注册——从 `env->task_ptr` 加载 outer task，调用 `rt_task_register_waker(inner, outer)`。inner task 完成时通过 waker 将 outer task 移入就绪队列，EventLoop 重新调度 resume。
- **main entry wrapper**：`emit_async_main_entry` 从 busy-wait poll 升级为 EventLoop 驱动（create + set_current + spawn + run + destroy）。
- **平台抽象**：Windows (CRITICAL_SECTION + CONDITION_VARIABLE + QueryPerformanceCounter) vs POSIX (pthread_mutex_t + pthread_cond_t + clock_gettime)。

**Facade 拦截链路**（`std/Arc/Tasks/Task.as` stub + typeck + MIR lower + codegen）：

- `Task.FromResult(value)` → typeck `check_builtin_static_method` 返回 `Task<T>`；MIR lower `builtin_static_method` 转 `MirRvalue::Call { func: "Task.FromResult" }`；codegen `try_emit_task_static` 按 `expected` 的 inner 类型分派 `rt_task_from_int` / `rt_task_from_ptr` / `rt_task_from_value`。
- `t.Result` / `t.GetResult()` → MIR lower 将 `Expr::Field` 转为 `MethodCall { method: "get_Result" }`；codegen `try_emit_task_method` 按 `expected` 分派 `rt_task_result_int` / `rt_task_result_ptr` / `rt_task_result_value`。
- `t.Status` / `t.IsCompleted` / `t.IsCanceled` / `t.IsFaulted` / `t.Exception` → `rt_task_status` / `rt_task_status+icmp eq 0` / `rt_task_is_canceled` / `rt_task_is_faulted` / `rt_task_get_exception`。
- `t.Wait()` / `t.Cancel()` → `rt_task_poll` / `rt_task_cancel`。
- `Task.FromCanceled(ct)` → `rt_task_from_canceled`；`Task.FromException(ex)` → `rt_task_from_exception`（FAULTED；异常存 `ptr_result`）。
- `Task.WhenAll(t1, t2, …)` / `Task.WhenAll()` / `Task.WhenAny(…)` / `Task.WaitAll` / `Task.WaitAny`（`params ReadOnlySpan<Task>`）/ `Task.CompletedTask` → 组合子解包 Span 后调 `rt_task_when_*` / `rt_task_wait_*`；空参 → `(null, 0)`。
- `Task.Yield` **已撤面**（调度器让步 ABI 后置；禁 null stub）。

`async main` 与顶层 async 函数共用状态机 lowering；C 入口 `main` 分配根任务并 poll 至 `RT_TASK_READY`。

**状态机 lowering**（`codegen/emit_async_sm.rs`）：含 await 的 async 函数编译为整图 CFG 状态机：

- **env struct**：`{ i32 state, ptr awaiter, ptr task_ptr, <params>, <locals> }`——state 驱动 switch，task_ptr 反向指向 RtTask 句柄用于 result 写回。
- **resume 函数**：`int32_t (*)(void* env, rt_waker* waker)`——state 0 发射完整 MIR CFG；每个 await 就地 poll，Pending 则保存 locals + 登记 waker + ret PENDING；Ready/唤醒后提取 result 并继续同块后续；完成时 state=-1 + `rt_task_set_result_*` + ret READY。覆盖多块 await 链与循环内 await。
- **构造函数**：calloc env + 初始化 params + `rt_task_from_state_machine` 构造 Task + 设置 env->task_ptr 反向指针。

无 await 的 async 回退 M1 同步构造路径。

## FFI 装箱 ABI

FFI 边界值类型 ↔ `object` marshal 的运行时支持。ArcBox 共享 ArcHeader 布局，可由 `rt_arc_inc`/`rt_arc_dec` 直接管理生命周期。

```c
/* ArcBox 内存布局（v2 简化版，反射永久剔除）：
 *   ┌────────────────┐  offset 0
 *   │ ArcHeader       │   _Atomic int32_t refcount  (4B)
 *   │                 │   const void* vtable        (8B, 4B padding 在前)
 *   ├────────────────┤  offset 16
 *   │ payload_size    │   int32_t (4B) — 装箱时记录的 payload 字节数
 *   ├────────────────┤  offset 20
 *   │ _padding        │   int32_t (4B) — 保证 payload 8B 对齐
 *   ├────────────────┤  offset 24
 *   │ payload[N]      │   实际负载数据
 *   └────────────────┘
 *
 * - rt_box_destroy 是 rt_arc_dec 的 alias（ArcBox 共享 ArcHeader 布局）
 * - unboxing 通过 expected_size 与 payload_size 比较校验（替代 v1 type_id 校验，反射剔除）
 * - 失败调用 rt_panic/rt_panic_at */
void*    rt_box_create(int32_t payload_size, int32_t payload_align);
void     rt_box_destroy(void* box_ptr);                   /* alias of rt_arc_dec */
int32_t  rt_box_unbox(void* box_ptr, int32_t expected_size,
                      void* out_ptr, int32_t out_size);   /* 0 成功；非 0 失败（panic） */

/* Arc closure → C 回调 trampoline（CancellationToken.Register 用）。
 * ct.Register(callback) 时 callback 是 arc_closure { void* fn_ptr; void* env; }，
 * rt_cts_register 存储 fn=rt_cts_callback_trampoline, data=closure_ptr，
 * ct 取消时调用 trampoline(closure) → closure->fn_ptr(closure->env)。
 * Action 的 lifted lambda 签名为 void(ptr env)，故 trampoline 仅转发 env。 */
void    rt_cts_callback_trampoline(void* data);
```

**ABI 语义**：

- `rt_box_create(size, align)`：分配 `ArcBoxHeader + payload`，refcount 初始化为 1，`vtable = NULL`（v2 移除 type_id，反射永久剔除），`payload_size` 记录负载字节数。返回 ArcHeader 起始指针，与 `rt_arc_inc`/`dec` 兼容。
- `rt_box_destroy(box_ptr)`：`rt_arc_dec` 的 alias，dec refcount 至 0 时 `free`。
- `rt_box_unbox(box_ptr, expected_size, out_ptr, out_size)`：`expected_size != payload_size` 时 `rt_panic("InvalidCastException: unboxing size mismatch")`；否则 `memcpy(out_ptr, payload, min(expected_size, out_size))`。`box_ptr == NULL || out_ptr == NULL` 时返回 `-1`（不 panic），由 codegen 在调用前插入 null 检查决定 fallback 路径（与可空引用类型规则一致）。
- `rt_cts_callback_trampoline(data)`：Arc closure 转发器，`ct.Register(callback)` 注册后由 `rt_cts_cancel` 触发。

**装箱点自动插入策略**（typeck 实现）：仅在 FFI `extern` 函数 `void*`（`object`）形参/返回值处自动插入 `Expr::Box`/`Expr::Unbox`；通用赋值/参数/返回值装箱已永久剔除。

**codegen 发射**（`emit_box.rs`）：

- 装箱：`%box = call ptr @rt_box_create(i32 %size, i32 %align)` + `call void @llvm.memcpy.p0.p0.i64(ptr %payload, ptr %src, i64 %size, i1 false)` + `call void @rt_arc_inc(ptr %box)`。
- 拆箱：`%rc = call i32 @rt_box_unbox(ptr %box, i32 %expected, ptr %out, i32 %out_size)` + `icmp ne i32 %rc, 0` → 失败路径调用 `rt_panic_at`。

**已知限制**：`object` 类型 local 不参与 MIR Drop（`is_class_type(TypeId::Object)` 返回 `false`），ArcBox 内存泄漏。FFI 边界装箱场景下 ArcBox 生命周期由 C 侧或显式 `rt_box_destroy` 管理。命名类型（struct）deep-copy 装箱未实现——`llvm_size_of` 对 `TypeId::Named(_)` 返回默认 8。

## 窗口与事件

原生窗口示例链接 `crates/runtime/platform/<os>/window.*`，提供：

- 窗口创建与销毁
- 事件泵（键盘、关闭）
- 与 ARML 窗口示例（`examples/ArmlDemo`）配套

窗口 ABI 与 `rt_*` 并列，文档随 platform 演进。

## 链接顺序

典型链接单元：

1. codegen 输出（`.o` 或 `.c` 编译结果）
2. `runtime.c`
3. `crates/runtime/platform/<os>/window.*`（若需要）
4. 系统库（如 Windows `user32`）

`arc build` 在 `-o` 指定路径后调用宿主链接器（如 `clang`）。

## 移植清单

移植到新平台时须实现：

| 符号 | 必需 |
|------|------|
| `rt_println` / `rt_print` / `rt_*_error` | 是（控制台） |
| `rt_panic` | 是 |
| `rt_env_*` | 否（仅用 `Environment` 时） |
| `rt_arc_inc/dec` | 是（class 程序） |
| `rt_dict_*`（含 `rt_hash_*`/`rt_eq_*`/`rt_dict_contains_value`） | 否（仅 `Dictionary<K,V>` 程序） |
| `rt_list_*` | 否（仅 `List<T>` 程序） |
| `rt_task_*` | 是（async 程序） |
| 窗口 API | 否（仅 GUI 示例） |

## 与标准库关系

`std/` 中的 Arc 源码通过 FFI 声明调用上述符号；不得绕过 ABI 直接嵌入平台 API，除非经 capability 标记。

> **用户级 FFI 契约**：用户通过 `.ani` 契约文件声明外部 C 库接口，编译器解析为 `NativeModule` AST，typeck 注册为 `StaticClass` 复用 OOP 静态方法分派，codegen 直接发射 `call @<symbol>` 并注入 `-l<module>` 链接标志。支持基元类型 + `string`/`string?`/`void` 白名单；符号存在性验证、`out`/`ref` 参数与复杂类型 marshal 待后续里程碑。
>
> **库文件解析**：按平台命名约定在 `[native].ani-native-lib` 搜索列表（始终以主程序根目录为隐式第一项）中查找——Windows MSVC `<module>.lib`；MinGW `lib<module>.dll.a`/`lib<module>.a`；Linux `lib<module>.so`/`.a`；macOS `lib<module>.dylib`/`.a`。契约内可声明 per-module `library = "dir"`（相对主程序根目录）指定模块专属库目录，优先级高于全局列表（多库体系隔离）。完整规则见 [17 arc.toml 项目清单](17-arc-toml-reference.md)。

## 动态库加载与热卸载

动态库加载 ABI（`rt_library_load` / `rt_library_sym` / `rt_library_unload`）。**热卸载闭环**（可回收 ALC + ARC 根扫描 + 模块级代数引用计数）立宪为必做能力。

热卸载 ABI 符号清单（`crates/runtime/rt_library.c` + `rt_abi.h`）：

| ABI | 语义 |
|-----|------|
| `rt_library_unload_hot(handle)` | 热卸载闭环：Freeze → 在途收敛 → ledger 归零检测 → **中和边界弱槽位** → 释放模块根 → dlclose → tombstone；返回 1=成功 0=悬挂拒载 -1=在途未收敛 -2=无效 |
| `rt_library_generation(handle)` | 查询模块代数（1..256；tombstone/未知返回 0） |
| `rt_library_ref_register/unregister/count(gen)` | 跨模块外部强引用 ledger（方案 B 边界变体；`rt_arc_inc/dec` 热路径零改动） |
| `rt_library_call_enter/leave(gen)` | 模块代码在途调用计数（Freeze 等待收敛） |
| `rt_library_root_add/remove/scan(gen)` | ARC 根扫描（模块根可达闭包 + ledger 一致性复核；无全堆扫描） |
| `rt_library_weak_register/unregister/untrack(gen, slot)` | 宿主侧弱登记表：模块边界 `Weak<T>` 登记；卸载时中和（`rt_arc_weak_neutralize` 置空 target）→ 卸载后 `TryGet()` 确定性返回 NULL |
| `E_UNLOAD_HANGING_REF` | 卸载后访问已卸载符号（`rt_library_sym` / `get_meta` tombstone 检测）→ `rt_panic` 硬错误，禁静默 |

`arc build --dynamic` 共享库经 `EmitRole::DynamicLibrary` 发射内嵌 `__arc_dbg_table`/`__arc_dbg_count`（`rt_debug.o` 硬引用，Windows PE 链接须就地解析）+ Entry wrapper + 资源导出符号。

---

上一节：[11 编译模型](11-compilation-model.md) · 下一节：[13 标准库架构](13-standard-library.md)