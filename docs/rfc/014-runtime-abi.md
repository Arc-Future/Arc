# RFC 014 运行时 ABI

## 背景

Arc 生成的二进制链接 `crates/runtime` 与 `crates/runtime/platform/`，通过**稳定 C ABI 符号**（`rt_*` 前缀）与宿主环境交互。`std/` 中的 Arc 源码经 FFI 声明调用这些符号；不得绕过 ABI 直接嵌入平台 API，除非经 capability 标记。

## 设计决策

### 头文件与命名

- 头文件：`crates/runtime/rt_abi.h`
- 实现：`crates/runtime/rt_*.c`（按能力拆分的多文件；链接列表以 `codegen` 的 `rt_sources` 为准，如 `rt_str.c`、`rt_dict.c`、`rt_list.c`、`rt_exc.c`、`rt_task.c`、`rt_file.c` 等）
- 符号前缀：`rt_`

### 控制台与 panic

```c
void rt_print(const char* msg);          /* 无换行 stdout */
void rt_println(const char* msg);        /* 带换行 stdout */
void rt_print_error(const char* msg);    /* 无换行 stderr */
void rt_println_error(const char* msg);  /* 带换行 stderr */
void rt_panic(const char* msg);
```

`Console.Write` / `WriteLine` / `ErrorWrite` / `ErrorWriteLine` 经 codegen 发射上述符号；`Window.Run(...)` 经 lowering 调用 `crates/runtime/platform/<os>/window.*`。不可恢复错误调用 `rt_panic`，默认向 stderr 输出并终止（策略可平台定制）。

### Environment（`rt_env_*` · Stable 最小面）

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

`Arc.Environment` 经 `try_emit_environment_static` 发射上述符号（与 `rt_abi.h` / `rt_env.c` 对齐）。不在设计面（无头文件符号、无 codegen 臂、禁空串假 stub）：`ProcessId` / `ProcessPath` / `ExpandEnvironmentVariables` / `GetFolderPath`。计时单一惯用法 = `Stopwatch`（无 `TickCount*`）。

### 字符串（`rt_str_*`）

```c
char*   rt_str_concat(const char* a, const char* b);
int32_t rt_str_length(const char* s);
int32_t rt_str_char_at(const char* s, int32_t index); /* UTF-8 码元 → char；越界 0 */
int32_t rt_str_equals(const char* a, const char* b);
int32_t rt_str_compare(const char* a, const char* b);
```

`string` 的 `+`、`==`/`!=`、`.Length`、只读索引 `s[i]`（→ `get_Chars` / `rt_str_char_at`）、`string.Compare` lowering 至上述符号；`Length` 与 `s[i]` 均以 **UTF-8 码元（字节）** 为索引单位；`rt_str_equals` 相同返回 `1`，不同返回 `0`；`rt_str_compare` 返回 `strcmp` 风格有符号差值。

Stable 加深面：`IsNullOrEmpty`（codegen 内联 null/空）、`IsNullOrWhiteSpace`→`rt_str_is_null_or_white_space`；`IndexOf`/`LastIndexOf` 一参与起算位置二参（`rt_str_*_from` / `*_char_from`）；`FromCharCount` / `Concat`；`Split(char|string)`→`rt_str_split`/`rt_str_split_char`（`string[]`）；`string.Join(string|char, string[])`→`rt_str_join`/`rt_str_join_char`；`ToCharArray()`/`ToCharArray(start,length)`→`rt_str_to_char_array`/`rt_str_to_char_array_range`（`char[]`，UTF-8 码元；range 越界钳制同 Substring）；`PadLeft`/`PadRight(int, char)`→`rt_str_pad_*_char`；`Trim`/`TrimStart`/`TrimEnd(char)`→`rt_str_trim*_char`；`Trim(params char[])`/`Trim(char[])`→`rt_str_trim*_chars`（空集→空白 trim）；`Split(char|string, StringSplitOptions)`→`rt_str_split*_opts`（`None`/`RemoveEmptyEntries`/`TrimEntries` 可按位或）；`Split(params char|char[])`→`rt_str_split_chars*`；`Split(sep, count, options)`→`rt_str_split*_opts_count`（MIR 按实参类型改写方法名分派）；`StartsWith`/`EndsWith(char)`→`rt_str_*_char`；`string.Compare`/`CompareOrdinal`（两静态均 → `rt_str_compare`，**无文化面**，ordinal ≡ UTF-8 码元 strcmp）。

### UTF-8 Encoding（`rt_text_utf8_*`）

```c
void*   rt_text_utf8_get_bytes(const char* s);      /* string → byte[]（rt_array，elem_size=1） */
char*   rt_text_utf8_get_string(void* bytes);       /* byte[] → malloc'd NUL-terminated string */
int32_t rt_text_utf8_get_byte_count(const char* s); /* UTF-8 码元数（strlen；null→0） */
```

`Arc.Text.Encoding.GetBytes` / `GetString` / `GetByteCount` 经 codegen 拦截发射上述符号。Arc `string` 已是 UTF-8；`GetBytes` 按 `strlen` 拷贝到 `rt_array_create` 负载；`GetString` 按 `rt_array_length` 拷贝并追加 NUL；`GetByteCount` 与 `GetBytes.Length` / `string.Length` 对齐。无内部 `0x00` 的文本可往返；含内部 NUL 时后续依赖 `strlen` 的 string 运算会截断（C-string 模型既有限制）。

### BitConverter / Buffer（字节缓冲）

```c
int32_t rt_bitconverter_is_little_endian(void);
void*   rt_bitconverter_get_bytes_i32(int32_t value);   /* → byte[4] */
void*   rt_bitconverter_get_bytes_i64(int64_t value);   /* → byte[8] */
int32_t rt_bitconverter_to_i32(void* bytes, int32_t start_index);
int64_t rt_bitconverter_to_i64(void* bytes, int32_t start_index);
/* Buffer.BlockCopy(byte[]) → rt_array_copy（elem_size=1；偏移=字节下标） */
```

`Arc.BitConverter`：主机端序（`IsLittleEndian()` 为方法，Arc 无静态属性）；Stable 面 `GetBytes(int|long)` / `ToInt32` / `ToInt64`。`Arc.Buffer.BlockCopy` 仅 `byte[]`。

**float/double 位型面**：`SingleToInt32Bits` / `Int32BitsToSingle` / `DoubleToInt64Bits` / `Int64BitsToDouble` 为 **编译期内建**——codegen 直射 LLVM `bitcast`（float↔i32 · double↔i64），零运行时开销、NaN/Inf/-0 位型精确保留，**无新增 `rt_*` 符号**（Math 直射 LLVM intrinsic 同路径）。`GetBytes(float|double)` / `ToSingle` / `ToDouble` 在 `bitcast` 之上**复用既有 i32/i64 ABI**（`GetBytes(float)` = bitcast→i32 + `rt_bitconverter_get_bytes_i32`，余类推），端序行为与 int/long 完全一致。

### 关联表（`Dictionary<K,V>`）

```c
typedef uint32_t (*rt_hash_fn)(void*);
typedef int32_t  (*rt_eq_fn)(void*, void*);

void*   rt_dict_create(rt_hash_fn hash, rt_eq_fn eq);
void    rt_dict_set(void* dict, void* key, void* value);
void*   rt_dict_get(void* dict, void* key);
int32_t rt_dict_contains(void* dict, void* key);
int32_t rt_dict_contains_value(void* dict, void* value, rt_eq_fn eq);
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

Stable 链接 stub 契约：`TryGetValue` 走 `rt_dict_try_get_value`（`out` 槽必写，miss 置 `NULL`）；`ContainsValue` 走 `rt_dict_contains_value`（注入值 eq）；`Add`/`Keys`/`Values` 分别走 `rt_dict_try_add` / `rt_dict_keys` / `rt_dict_values`。**禁止**缺 out 槽或静默 `ret i1 false`（IndirectCall / 未 inline 路径会假绿或崩溃）。`GetEnumerator` 热路径由 `emit_builtin` 装配 itable；链接兜底 `rt_panic`（禁静默假绿）。ConcurrentDictionary 链接 stub **真转发** `rt_concurrent_dict_*`；未挂面的 `Values`/`ToArray`/`AddOrUpdate` → `rt_panic`（禁静默 `0`/`null`）。`rt_concurrent_dict_try_get` / `try_remove` miss 须写 `*out_value = NULL`（禁未初始化泄漏）。

**键哈希/相等**（`rt_dict_create` 注入；同一 C 实现服务全部单态化）：

| 键类型 | hash | eq |
|--------|------|-----|
| `string` | `rt_hash_str` | `rt_eq_str` |
| 标量（`int`/`long`/…） | `rt_hash_int` | `rt_eq_int` |
| 用户类型（含 record；`IHashable`/`IEquatable`） | `@__dict_hash_{K}` → `K.GetHashCode` | `@__dict_eq_{K}` → `K.Equals` |

用户类型键路径零装箱、零运行时类型分派。

### out/ref 形参 byref 转发不变量

对齐 C#/CLI byref 语义（ECMA-335 I 11.4.1.5 / §12.4.1.5）：`out`/`ref` 形参在调用约定中为**指针传递**（managed pointer），callee 写入指针指向的值，调用方在调用后读取同一存储。由此确立编译期不变量：

- **形参表示**：`out T v` / `ref T v` 的 MIR 局部槽类型为 `TypeId::Ref`，槽内**存储调用方变量的指针**；函数入口把 ABI `ptr` 实参写入该槽，读/写形参经「load 槽内指针 → load/store 目标」两级解引用。
- **转发接线**：把 `out v`/`ref v` 作为实参转发给被调方法时，传递的必须是**槽内存储的指针**（`load ptr, ptr %vN`），而非槽地址 `%vN` 本身——否则被调方把值写进指针槽（覆写指针），调用方变量永远收不到值。codegen 统一经 `byref_arg_ptr(id)`（普通值局部 → 槽地址；`Ref` 局部 → 转发存储指针）计算 byref 目标，普通函数调用、builtin 内联（`emit_builtin`）、native FFI（`emit_native_byref_arg`）三条路径共用同一规则。
- **确定性赋值**：typeck 的 out 形参赋值检查在 **return 表达式求值之后**执行——`return dict.TryGetValue(k, out v);` 的 `v` 由 `RefArg` 求值路径标记已定值，先检查后求值会把尚未定值的 `v` 误判为未赋值。

### 动态数组（`List<T>`）

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

`Arc.Collections.List<T>` 经单态化；`_handle` 指向 `RtList`。索引器 `list[i]`（`get_Item`/`set_Item`）与**值类型** `Add` 的**热路径由 codegen 直访** `RtList`（布局契约：`data@0`、`size@8`、`capacity@12`、`elem_size@16`）：`Add` 满容时调 `rt_list_ensure_capacity`，否则 GEP+store+size++（无 `rt_list_push`/alloca/memcpy）；索引越界 `rt_panic`。`rt_list_at` / `rt_list_push` 供 stub 与**引用元素**（ARC）回退。`rt_list_get`/`rt_list_set` 内部经 `rt_list_at`；引用元素 `set`/`Add` 仍走 `rt_list_*` 维护 ARC。

### 原生数组工具（`Array` · Stable 最小面）

堆数组带 `RtArrayHeader`（`rt_array_create` / `rt_array_length` / `rt_array_destroy`）。静态工具面经 codegen →：

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

`std/Arc/Array.as` Stable 公开面（均为 `[Builtin]`；Array 为 stub facade）：`Copy`/`Clear`/`Reverse` → `rt_array_*`（泛型 + `int[]`）；`IndexOf`/`LastIndexOf`/`Empty`/`Resize` 仅 `int[]`；`Exists`/`Find`/`FindLast`/`FindIndex`/`FindLastIndex`/`TrueForAll`/`ForEach` 仅 `int[]` + `Func`/`Action` trampoline；`Sort`/`BinarySearch` 仅 `int[]` 升序（未命中返回 `~insertionPoint`）；`FindAll`/`ConvertAll` 仅 `int[]`（`FindAll`→新建匹配数组；`ConvertAll`→`Func<int,int>` 映射）。**禁止**空 stub 挂 Stable；`Join`（C# `System.Array` 无此成员，勿发明）勿引入。

### 双向链表（LinkedList · Stable 最小面）

```c
void*  rt_linked_list_create(int32_t elem_size, rt_list_eq_fn, rt_list_arc_fn, rt_list_arc_fn);
void*  rt_linked_list_add_last / add_first / first / last / find(…);  /* → RtLinkedListNode* */
void   rt_linked_list_node_value(void* node, void* out);
void*  rt_linked_list_node_prev / next(void* node);
```

`LinkedListNode<T>` 为**不透明节点句柄透传**（identity）：`Add*`/`First`/`Find` 返回的即 `RtLinkedListNode*`，属性访问走 `rt_linked_list_node_*`。codegen 禁止把节点当 Arc 对象 `rt_arc_inc`/`dec`（会写坏 `value` 槽）。`node.List` 返回 runtime 链表句柄，非 Arc 包装——Stable 面不依赖。

### 排序集合（SortedSet）

```c
void*   rt_sorted_set_create(rt_cmp_fn cmp);   /* 标量 @rt_cmp_int；string @rt_cmp_str */
int32_t rt_sorted_set_add / contains / remove(void* handle, void* key);
int32_t rt_sorted_set_min / max(void* handle, void* out_ptr);  /* out 写 void*；空集返回 0 */
int32_t rt_sorted_set_count(void* handle);
void    rt_sorted_set_clear / destroy(void* handle);
```

标量元素为 **inttoptr 装箱**（`rt_cmp_int` 比较指针位）；禁止栈 `alloca` 假指针。Stable 面：ctor / Add / Contains / Remove / Min / Max / Count / Clear。空集 Min/Max 未定义。

### 排序字典（SortedDictionary）

```c
void*   rt_sorted_dict_create(rt_cmp_fn cmp);   /* 标量 @rt_cmp_int；string @rt_cmp_str */
void*   rt_sorted_dict_get(void* handle, void* key);           /* NULL if missing */
void    rt_sorted_dict_set(void* handle, void* key, void* value);
int32_t rt_sorted_dict_add / try_get / remove / contains(…);
int32_t rt_sorted_dict_count(void* handle);
void    rt_sorted_dict_clear / destroy(void* handle);
```

标量键/值为 **inttoptr 装箱**；禁止栈 `alloca` 假指针。Stable 面：ctor / 索引器 / Add / TryGetValue / ContainsKey / Remove / Count / Clear。

### 异常

```c
void rt_throw(void* ex);           /* native raise (Win: _CxxThrowException) */
void* rt_get_exception(void);      /* TLS slot bound by the catch site */
char* rt_format_stacktrace(void);  /* malloc'd multiline stack string for Exception.StackTrace */
```

`try`/`catch`/`finally` lowering 采用 **LLVM `invoke`/`landingpad`**（Windows SEH 主平台：`catchswitch`/`catchpad`/`cleanuppad`，`__CxxFrameHandler3` personality；未抛出路径零开销、finally 深层 unwind 恒执行、catch 类型过滤 C# 对齐、`rt_exception` TLS）。async 状态机协作：await 提取点 faulted Task 经 `rt_task_is_faulted`/`rt_task_get_exception` rethrow → 外层 catch（try 跨 await 语义正确）。cleanup funclet 内 `call` 携带 `"funclet"("token")` 操作数（LLVM WinEH 强制）；已知 nounwind 外部与 facade 方法按 `RT_MAY_THROW` 镜像标注。catch 的运行时类型过滤（`when` / 精确类型匹配）未覆盖路径须硬错误而非静默吞异常。

**`Exception.StackTrace`**：`throw` 降级在 `rt_throw` 前调用 `rt_format_stacktrace()`，写入 `Exception.StackTrace`（仅当槽位仍为 null），构造后为 null。捕获真实返回地址；**内嵌 `__arc_dbg_table` 默认发射**（与 DWARF `-g` 解耦）→ 函数名 + 可行时 file:line；POSIX `backtrace_symbols` 次级；仍无符号时 `at <0x…>`；极端无帧时 `at <throw>`。

**`Exception.ToString`**：`Message`（+ `" ---> "` 内层 `ToString`）；若 `StackTrace != null` 则换行追加栈串。构造未 throw 时不附栈。

**`nounwind` 与嵌套虚分派**：用户函数的 `nounwind` 由**模块内 call-graph 不动点**推断（见 [015 LLVM 原生后端](015-llvm-backend.md)）：无局部 `Throw`/`TryCatch`，且每个调用均解析到已知 `nounwind` 被调方时才标注。已知 `nounwind` 被调方包括：模块内已推断用户函数、`rt_*` 白名单（closed-world 审计下除 may-throw 表外的全部 `rt_*`，含 `rt_get_exception`/`rt_panic*`）、常用 libc leaf 与 `llvm.*` intrinsic。虚分派 / 接口 / 间接 / **未知外部**（native FFI 等）一律视为 may-throw——中间帧若误标 `nounwind`，unwind 会穿栈导致 `STATUS_BAD_STACK`（Windows `0xc00000ff`）。

**`rt_*` may-throw 表**（codegen `attr.rs` · `RT_MAY_THROW`；新增同栈回调或 unwind 的 `rt_*` 必须追加）：

| 类别 | 符号 |
|------|------|
| EH（native raise） | `rt_throw` |
| List 同栈谓词/比较回调 | `rt_list_find_get` / `find_all` / `exists` / `for_each` / `remove_all` / `sort` / `binary_search_cmp` |
| 并行同栈 body | `rt_parallel_for` / `rt_parallel_foreach` |
| QIF 同栈跑测 | `rt_qif_try_run` / `rt_qif_run_all` |
| CTS 可能同步点火 | `rt_cts_register` / `rt_cts_register_lf` / `rt_cts_node_try_fire` |
| ConcurrentDict 工厂回调 | `rt_concurrent_dict_get_or_add` / `get_or_add_val` / `add_or_update` / `add_or_update_pf` |
| ContinueWith 可能同步续体 | `rt_task_continue_with` |

边界：异步投递（`rt_thread_create` / `rt_threadpool_spawn*` / `rt_task_run*` 等）允许进白名单——回调不在调用方同栈展开；若改为同栈同步执行，须移入 may-throw 表。

### 文件 I/O

```c
/* 文件读写与基础操作 */
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

/* FileStream */
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

/* FileStream 真异步（文件 I/O 线程池卸载 + EventLoop 完成投递；rt_file_stream_async.c）*/
void* rt_file_stream_read_async(void* handle, void* buffer, int32_t offset, int32_t count, void* ct);
void* rt_file_stream_write_async(void* handle, void* buffer, int32_t offset, int32_t count, void* ct);
void* rt_file_stream_flush_async(void* handle, void* ct);
void  rt_file_stream_async_shutdown(void);

/* Memory-mapped file + CodeEditor buffer */
void*       rt_file_mmap_open(const char* path);
void        rt_file_mmap_close(void* handle);
int64_t     rt_file_mmap_length(void* handle);
const char* rt_file_mmap_data(void* handle);
```

`File.ReadAllText` / `WriteAllText` / `Exists` / `Delete` / `AppendAllText` / `Copy` / `Move`（及对应 `*Async`）、`Directory.CreateDirectory` / `Exists` / `Delete` / `GetFiles`(含 `searchPattern`) / `GetDirectories`、`Path.Combine`（二元）/ `GetDirectoryName` / `GetFileName` / `GetFileNameWithoutExtension` / `GetExtension` / `GetTempPath` 分别 lowering 到同名符号（`rt_file_*` / `rt_dir_*` / `rt_path_*`）。`File.OpenRead` / `OpenWrite` / `OpenText` / `Create` 与 `FileStream` 走 `rt_file_stream_*`。`File.ReadAllLines`→`rt_file_read_all_lines`；`Directory.GetFiles`→`rt_dir_list_files`；`GetFiles(path, searchPattern)`→`rt_dir_list_files_pattern`（`*`/`?` 在 C 侧匹配；非 codegen filter）；`GetDirectories`→`rt_dir_list_dirs`（跳过 `.`/`..`）。完整路径 `string[]`；失败/空 Length 0。

**FileStream 真异步（`rt_file_stream_*_async`）**：`FileStream.ReadAsync` / `WriteAsync` / `FlushAsync` 走**文件 I/O 线程池卸载 + 完成投递**模型（对标 .NET FileStream 默认路径的语义层）——阻塞读写/刷新提交到文件 I/O **专用**线程池（复用 `rt_threadpool` 池内核的独立实例，worker 数 min(4, hardware)，与 `Task.Run` 默认池隔离：阻塞文件操作不侵占 CPU 密集 worker）；池线程完成后 `rt_task_complete` → `g_rt_wake_fn` → `rt_event_loop_spawn`（mutex + 就绪队列 + condvar signal）唤醒 EventLoop，与 `Task.Run` 同构的跨线程完成投递。调用线程提交后立即返回 Pending Task——真异步，非 sync-over-async。**为什么不是 Reactor overlapped（`File.*Async` 数据面路线）**：FileStream 同步面（Stable）持有 CRT `FILE*`（内部缓冲 + 文件位置）；Reactor 路线要求 `FILE_FLAG_OVERLAPPED` 专用句柄，会与 `FILE*` 形成双句柄双位置，破坏 Stream 单一 `Position` 语义，而替换同步面句柄模型属破坏性变更（RFC 036 冻结面禁止）。且 Windows 文件句柄无 readiness（epoll/select）语义，socket Reactor 模型不适用于文件。trampoline 直接复用同步面 ABI（同一 `FILE*`），同步/异步混用时 `Position` 同源（CRT per-handle 锁保证并发安全）。取消语义：仅提交前预检 `ct`（已取消 → 返回已取消 Task，不占池线程；先例 `rt_task_run_func_ct`）；进入池线程后不可中止（CRT I/O 无取消原语，与 .NET 非重叠路径一致）。`buffer` 归调用方所有，Task 完成前保持有效（async 状态机跨 await 存活）。

**错误模型**：与 C# `System.IO` 对齐，返回值模型而非异常——`bool` 返回类型用 `i32`（0/1）；`string` 返回类型用 `ptr`（malloc'd NUL-terminated，失败返回空串）；操作失败统一返回 `0`/空串，不引入异常机制。`Path.GetFileName` 遵循 C# 语义：路径以分隔符结尾时返回空串（目录而非文件）。

**跨平台**：路径分隔符统一使用 `/`；`rt_file.c` 在 Windows 下 `typedef long long ssize_t` 补齐 POSIX 类型缺失。

### 接口 fat pointer（itable）

接口值不是裸对象指针，而是指向 `{ ptr obj, ptr itable }` 的 `ptr`（见对象模型）。

- itable 全局：`@.itable.{Class}_{Iface}`——类直接声明该接口或其基类链继承该接口（派生类继承接口实现时亦发射**自己的** itable，槽位经 override 链解析命中派生实现；基类静态类型赋值不再复用祖先 itable）
- 方法调用：`fn = itable[slot]; call fn(obj, …)`
- 槽位布局：接口继承时父接口方法槽位在前、子接口自身方法在后；槽位身份 = 名 + 形参类型（重载各占其槽），发射与调用点查找共享同一扁平布局
- `is` 绑定：接口静态类型 scrutinee 的类型测试先取 fat pointer 首槽对象指针（`UnboxIface`）再经 `rt_obj_isa`；绑定目标为子接口时按源 itable 重绑定（父→子 `AdaptIface`），`obj` 槽复用同一对象指针
- 禁止在调用点把已有 fat pointer 再包一层（会把 fat 地址误当作 `obj`）

### ARC 与循环收集

```c
void rt_arc_inc(void* ptr);
void rt_arc_dec(void* ptr);
```

`class` 句柄复制、赋值、传参及作用域结束处，codegen 插入 inc/dec。循环收集**默认 always-on**（编译进每个二进制、阈值触发、用户无感）：rc→0 且可成环对象**延迟释放**（滞留至下次收集），finalizer **不再在 rc→0 同步执行**——确定性时序依赖 `Weak<T>` 或显式 `Dispose`。弱引用与循环收集细节见 [005 内存模型与资源安全](005-memory-model.md)。

### Stopwatch（高精度计时）

```c
int64_t rt_stopwatch_get_timestamp(void);      /* 单调时钟原始 ticks */
int64_t rt_stopwatch_frequency(void);          /* 每秒 ticks */
int32_t rt_stopwatch_is_high_resolution(void); /* 1 = 高精度（恒为 1） */
```

`Arc.Diagnostics.Stopwatch` 经 `crates/arc/native/rt_resources.ani` 调用上述符号；实现于 `rt_resources.c`。

| 平台 | 时间源 | Frequency |
|------|--------|-----------|
| Windows | `QueryPerformanceCounter` | `QueryPerformanceFrequency` |
| POSIX | `CLOCK_MONOTONIC` 纳秒 | `1_000_000_000` |

`ElapsedTicks` 为计时器原始 ticks；`Elapsed` / `ElapsedMilliseconds` 换算为 TimeSpan 刻度（每秒 10_000_000）。**计时单一惯用法**：间隔测量走 Stopwatch；`Environment` **无** `TickCount` / `TickCount64`。

### 异步 Task

```c
#define RT_TASK_READY    0
#define RT_TASK_PENDING  1
#define RT_TASK_FAULTED  2
#define RT_TASK_CANCELED 3

typedef struct RtTask RtTask;

/* Waker：外部事件唤醒 Pending Task */
typedef struct rt_waker {
    void (*wake)(void* data);
    void* data;
} rt_waker;

RtTask* rt_task_alloc(void);
void*   rt_task_from_int(int32_t value);       /* int 结果 → 已完成 Task */
void*   rt_task_void(void);                    /* void 结果 → 已完成 Task */
int32_t rt_task_poll(void* state);             /* 推进状态机；返回 RT_TASK_* */
int32_t rt_task_result_int(void* state);
void*   rt_task_from_ptr(void* value);               /* 指针结果（string/class/array） */
void*   rt_task_from_value(void* data, int32_t size); /* 值类型结果（double/long/Vector） */
int32_t rt_task_status(void* state);                 /* 仅查询状态，不推进 */
void*   rt_task_result_ptr(void* state);
void    rt_task_result_value(void* state, void* dst, int32_t size);
void    rt_task_cancel(void* state);
int32_t rt_task_is_canceled(void* state);
void*   rt_task_from_state_machine(void* env, void* resume_fn); /* resume_fn: int32_t (*)(void* env, rt_waker* waker) */
void    rt_task_set_result_int(void* state, int32_t value);
void    rt_task_set_result_ptr(void* state, void* value);
void    rt_task_set_result_value(void* state, void* data, int32_t size);
void    rt_task_set_waker(void* state, rt_waker* waker);
void    rt_waker_wake(rt_waker* waker);
void    rt_task_complete(void* state);                 /* 状态=READY + 触发 waker */
void    rt_task_register_waker(void* inner, void* outer);
void*   rt_task_delay(int32_t milliseconds);           /* Pending Task + 定时器；无 EventLoop 时 fallback Ready */

/* EventLoop ABI */
void*   rt_event_loop_create(void);                    /* 初始化 + 设置 g_current_loop + g_rt_wake_fn */
void    rt_event_loop_destroy(void* loop);
void    rt_event_loop_run(void* loop);
void    rt_event_loop_stop(void* loop);
int32_t rt_event_loop_tick(void* loop);                /* 快照就绪队列 + 逐个 poll */
void    rt_event_loop_spawn(void* loop, void* task);   /* 线程安全：mutex lock + push + condvar signal */
void    rt_event_loop_set_current(void* loop);
void*   rt_event_loop_current(void*);
void    rt_event_loop_inc_pending(void* loop);
void    rt_event_loop_dec_pending(void* loop);

typedef void (*rt_waker_fn_ptr)(void* data);
extern rt_waker_fn_ptr g_rt_wake_fn;                   /* rt_event_loop_create 时初始化为 rt_task_default_wake */
```

**EventLoop 调度器**（`runtime/rt_event_loop.c`）：单线程调度器驱动状态机真实 suspend/resume。

- **就绪队列**：ring buffer（容量 256），mutex 保护 push（`rt_event_loop_spawn`），condvar signal 唤醒 EventLoop 线程。
- **定时器堆**：有序单链表（按 deadline_ms 升序），`fire_expired` 扫描到期定时器并调用 `rt_task_complete` 触发 waker。
- **跨线程唤醒**：`g_rt_wake_fn` 全局函数指针让 `rt_task.c` 反向引用 `rt_event_loop.c` 的 `rt_task_default_wake`，避免循环依赖；`rt_task_default_wake` 可从任意线程调用（mutex 保护就绪队列 push）。
- **waker 内嵌槽**：`RtTask` 内嵌 `_waker_slot` 字段（避免堆分配 binding）。`rt_task_register_waker(inner, outer)` 设置 inner task 的 `_waker_slot.wake = g_rt_wake_fn` + `_waker_slot.data = outer_task`。inner 完成时经 `rt_task_complete` → `rt_waker_wake` → `rt_task_default_wake` → `rt_event_loop_spawn(loop, outer_task)` 将 outer 移入就绪队列。
- **run 循环**：`tick`（快照就绪队列 + 逐个 poll）→ `fire_expired`（处理到期定时器）→ 判断退出（无 pending + 无 ready + 无 timer）→ condvar wait 到下一定时器 deadline。
- **Task.Delay**：codegen `try_emit_task_static` 拦截 `Task.Delay(int)` → `call ptr @rt_task_delay(i32)`。
- **状态机 await waker 集成**：`emit_sm_await` suspend 块新增 waker 注册——加载 `env->task_ptr` 的 outer task，调用 `rt_task_register_waker(inner, outer)`。
- **main entry wrapper**：`emit_async_main_entry` 从 create + set_current + spawn + run + destroy 驱动。
- **平台抽象**：Windows (CRITICAL_SECTION + CONDITION_VARIABLE + QueryPerformanceCounter) vs POSIX (pthread_mutex_t + pthread_cond_t + clock_gettime)。

**Facade 拦截链路**（`std/Arc/Tasks/Task.as` stub + typeck + MIR lower + codegen）：

- `Task.FromResult(value)` → codegen `try_emit_task_static` 按 inner 类型分派 `rt_task_from_int` / `rt_task_from_ptr` / `rt_task_from_value`。
- `t.Result` / `t.GetResult()` → `try_emit_task_method` 分派 `rt_task_result_int` / `rt_task_result_ptr` / `rt_task_result_value`。
- `t.Status` / `t.IsCompleted` / `t.IsCanceled` / `t.IsFaulted` / `t.Exception` → `rt_task_status` / `rt_task_status+icmp eq 0` / `rt_task_is_canceled` / `rt_task_is_faulted` / `rt_task_get_exception`。
- `t.Wait()` / `t.Cancel()` → `rt_task_poll` / `rt_task_cancel`。
- `Task.FromCanceled(ct)` → `rt_task_from_canceled`；`Task.FromException(ex)` → `rt_task_from_exception`（FAULTED；异常存 `ptr_result`）。
- `Task.WhenAll` / `Task.WhenAny` / `Task.WaitAll` / `Task.WaitAny`（`params ReadOnlySpan<Task>`）/ `Task.CompletedTask` → 组合子解包 Span 后调 `rt_task_when_*` / `rt_task_wait_*`；空参 → `(null, 0)`。
- `Task.Yield` 不在设计面（调度器让步 ABI 未立；禁 null stub）。

**状态机 lowering**（`codegen/emit_async_sm.rs`）：含 await 的 async 函数编译为整图 CFG 状态机。

- **env struct**：`{ i32 state, ptr awaiter, ptr task_ptr, <params>, <locals> }`——state 驱动 switch，task_ptr 反向指向 RtTask 句柄用于 result 写回。
- **resume 函数**：`int32_t (*)(void* env, rt_waker* waker)`——state 0 发射完整 MIR CFG；每个 await 就地 poll，Pending 则保存 locals + 登记 waker + ret PENDING；Ready/唤醒后提取 result 并继续同块后续；完成时 state=-1 + `rt_task_set_result_*` + ret READY。覆盖多块 await 链与循环内 await。
- **构造函数**：calloc env + 初始化 params + `rt_task_from_state_machine` 构造 Task + 设置 env->task_ptr 反向指针。

无 await 的 async 回退同步构造路径。async 状态机局部变量所有权由 **env 唯一 owner + resume 级 EH cleanup pad** 收敛（AST 局部跨 await 存活经编译期 liveness 分析只提升存活者；任何 unwind 路径由 personality 驱动的 cleanup 释放恰一次，删除散落路径上的手工 inc/dec 配对）。

### FFI 装箱 ABI

FFI 边界值类型 ↔ `object` marshal 的运行时支持。ArcBox 共享 ArcHeader 布局，可由 `rt_arc_inc`/`rt_arc_dec` 直接管理生命周期。

```c
/* ArcBox 内存布局：
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
 * - unboxing 通过 expected_size 与 payload_size 比较校验
 * - 失败调用 rt_panic/rt_panic_at */
void*    rt_box_create(int32_t payload_size, int32_t payload_align);
void     rt_box_destroy(void* box_ptr);                   /* alias of rt_arc_dec */
int32_t  rt_box_unbox(void* box_ptr, int32_t expected_size,
                      void* out_ptr, int32_t out_size);   /* 0 成功；非 0 失败（panic） */

/* Arc closure → C 回调 trampoline */
void    rt_cts_callback_trampoline(void* data);
```

**ABI 语义**：

- `rt_box_create(size, align)`：分配 `ArcBoxHeader + payload`，refcount 初始化为 1，`vtable = NULL`，`payload_size` 记录负载字节数。返回 ArcHeader 起始指针，与 `rt_arc_inc`/`dec` 兼容。
- `rt_box_destroy(box_ptr)`：`rt_arc_dec` 的 alias，dec refcount 至 0 时 `free`。
- `rt_box_unbox(box_ptr, expected_size, out_ptr, out_size)`：`expected_size != payload_size` 时 `rt_panic`；否则 `memcpy(out_ptr, payload, min(expected_size, out_size))`。`box_ptr == NULL || out_ptr == NULL` 时返回 `-1`（不 panic），由 codegen 在调用前插入 null 检查决定 fallback 路径。
- `rt_cts_callback_trampoline(data)`：Arc closure 转发器，`ct.Register(callback)` 注册后由取消触发。

**装箱点自动插入策略**（typeck 实现）：仅在 FFI `extern` 函数 `void*`（`object`）形参/返回值处自动插入 `Expr::Box`/`Expr::Unbox`；通用赋值/参数/返回值装箱不引入。

**codegen 发射**（`emit_box.rs`）：装箱 = `rt_box_create(size, align)` + `memcpy(payload, src, size)` + `rt_arc_inc`；拆箱 = `rt_box_unbox` + 失败路径调 `rt_panic_at`。

### 窗口与事件

原生窗口示例链接 `crates/runtime/platform/<os>/window.*`，提供窗口创建与销毁、事件泵（键盘、关闭），与 ARML 窗口示例配套。窗口 ABI 与 `rt_*` 并列，文档随 platform 演进。

### 链接顺序

典型链接单元：

1. codegen 输出（`.o` 或 `.c` 编译结果）
2. `runtime.c`
3. `crates/runtime/platform/<os>/window.*`（若需要）
4. 系统库（如 Windows `user32`）

`arc build` 在 `-o` 指定路径后调用宿主链接器（如 `clang`）。

### 移植清单

| 符号 | 必需 |
|------|------|
| `rt_println` / `rt_print` / `rt_*_error` | 是（控制台） |
| `rt_panic` | 是 |
| `rt_env_*` | 否（仅用 `Environment` 时） |
| `rt_arc_inc/dec` | 是（class 程序） |
| `rt_dict_*`（含 `rt_hash_*`/`rt_eq_*`） | 否（仅 `Dictionary<K,V>` 程序） |
| `rt_list_*` | 否（仅 `List<T>` 程序） |
| `rt_task_*` | 是（async 程序） |
| 窗口 API | 否（仅 GUI 示例） |

### 动态库加载与热卸载

动态库加载 ABI（`rt_library_load` / `rt_library_sym` / `rt_library_unload`）与热卸载闭环见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)。

## 边界

- 本篇只讲**运行时 `rt_*` ABI 符号面**；语言级内存语义（ARC 语义、借用、Span、循环收集、弱引用、资源确定性）见 [005 内存模型与资源安全](005-memory-model.md)。
- `rt_*` may-throw 表与 `nounwind` 推断的编译实现见 [015 LLVM 原生后端](015-llvm-backend.md)。
- 用户级 FFI 契约（`.ani`）见 [016 验证式 FFI 与 Native 加载](016-verified-ffi.md)。

---
上一节：[013 编译管线架构](013-compiler-pipeline.md) · 下一节：[015 LLVM 原生后端](015-llvm-backend.md)