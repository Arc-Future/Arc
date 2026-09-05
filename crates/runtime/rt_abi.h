#ifndef DLANG_RT_ABI_H
#define DLANG_RT_ABI_H

#include <stdint.h>

#define RT_ABI_VERSION 1

#define RT_TASK_READY    0
#define RT_TASK_PENDING  1
#define RT_TASK_FAULTED  2
#define RT_TASK_CANCELED 3

#define RT_EVENT_NONE  0
#define RT_EVENT_CLOSE 1
#define RT_EVENT_KEY   2

#ifdef __cplusplus
extern "C" {
#endif

/* Environment ABI
 * Phase 1 (2026-07-20): 命令行参数访问
 * Phase 2 (2026-07-21): 环境变量、进程控制、系统信息、当前目录、机器/用户名
 * 与 C# System.Environment 核心子集对齐。所有字符串返回值为 malloc 出的 NUL 终止串，
 * 调用方（ARC runtime）拥有所有权；查询失败返回空串（非 NULL）。 */
void     rt_env_init(int argc, char** argv); /* 进程启动时由 @main 调用一次，填充 argc/argv */
int32_t  rt_env_argc(void);                  /* 命令行参数总数（含程序名） */
const char* rt_env_argv(int32_t index);       /* 按索引获取参数（0 = 程序名） */
char*    rt_env_get_var(const char* name);     /* getenv；未设置返回空串 */
int32_t  rt_env_set_var(const char* name, const char* value); /* setenv；value=NULL/"" 删除 */
void     rt_env_exit(int32_t code);           /* exit(code) */
int32_t  rt_env_get_exit_code(void);          /* 进程退出码（默认 0） */
void     rt_env_set_exit_code(int32_t code);  /* 设置退出码 */
void     rt_env_fail_fast(const char* msg);   /* 输出到 stderr 后 abort() */
const char* rt_env_newline(void);             /* "\r\n" (Windows) 或 "\n" (POSIX)；静态常量 */
int32_t  rt_env_processor_count(void);        /* CPU 核数 */
int32_t  rt_env_is_64bit_process(void);       /* 1=64 位进程，0=32 位 */
char*    rt_env_get_cwd(void);                /* getcwd；失败返回空串 */
int32_t  rt_env_set_cwd(const char* path);    /* chdir；1=成功，0=失败 */
char*    rt_env_machine_name(void);           /* 机器名；失败返回空串 */
char*    rt_env_self_exe(void);               /* 自身可执行文件绝对路径；失败返回空串（malloc 串，调用方借引用） */
char*    rt_env_user_name(void);              /* 当前用户名；失败返回空串 */
const char* rt_env_platform(void);            /* "Windows" / "Linux" / "macOS" / "Android" / "iOS" / "OHOS"；静态常量 */
int32_t  rt_env_is_windows(void);            /* 1=Windows，0=否；编译期常量 */
int32_t  rt_env_is_linux(void);              /* 1=Linux（非 Android/OHOS），0=否；编译期常量 */
int32_t  rt_env_is_macos(void);              /* 1=macOS，0=否；编译期常量 */
int32_t  rt_env_is_android(void);            /* 1=Android，0=否；编译期常量 */
int32_t  rt_env_is_ios(void);                /* 1=iOS，0=否；编译期常量 */
int32_t  rt_env_is_ohos(void);               /* 1=OpenHarmony，0=否；编译期常量 */

/* Console ABI（Phase 1+2：核心 I/O + 颜色控制，与 C# System.Console 对齐） */
void    rt_print(const char* msg);             /* 无换行输出 */
void    rt_println(const char* msg);          /* 带换行输出 */
void    rt_print_error(const char* msg);      /* Phase 3: 无换行输出到 stderr */
void    rt_println_error(const char* msg);    /* Phase 3: 带换行输出到 stderr */
char*   rt_read_line(void);                    /* 行输入；EOF 返回 NULL */
int32_t rt_read_char(void);                    /* 字符输入；EOF 返回 -1 */
void    rt_console_set_fg(int32_t color);      /* 设置前景色 (ConsoleColor 0-15) */
void    rt_console_set_bg(int32_t color);      /* 设置背景色 (ConsoleColor 0-15) */
void    rt_console_reset_color(void);          /* 重置为默认色 */
int32_t rt_console_get_fg(void);               /* 获取前景色（返回默认 Gray=7） */
int32_t rt_console_get_bg(void);               /* 获取背景色（返回默认 Black=0） */

char* rt_str_concat(const char* a, const char* b);
int32_t rt_str_length(const char* s);
int32_t rt_str_equals(const char* a, const char* b);
int32_t rt_str_compare(const char* a, const char* b);

/* String method ABI (P2): instance methods backed by rt_str.c helpers. */
void*    rt_str_split(const char* s, const char* sep);          /* → char*[] */
void*    rt_str_split_char(const char* s, int32_t c);            /* → char*[] */
/* options: bit0=RemoveEmptyEntries, bit1=TrimEntries（与 Arc.StringSplitOptions 对齐） */
void*    rt_str_split_opts(const char* s, const char* sep, int32_t options);
void*    rt_str_split_char_opts(const char* s, int32_t c, int32_t options);
/* 多分隔符：seps 为 int32_t[]（char[]）；空集 → 整串单段 */
void*    rt_str_split_chars(const char* s, void* seps);
void*    rt_str_split_chars_opts(const char* s, void* seps, int32_t options);
/* count：最大段数（C# 语义；count<=0 → 不限制）；末段含剩余 */
void*    rt_str_split_opts_count(const char* s, const char* sep, int32_t count, int32_t options);
void*    rt_str_split_char_opts_count(const char* s, int32_t c, int32_t count, int32_t options);
void*    rt_str_split_chars_opts_count(const char* s, void* seps, int32_t count, int32_t options);
void*    rt_str_to_char_array(const char* s);                    /* → int32_t[] */
void*    rt_str_to_char_array_range(const char* s, int32_t start, int32_t length); /* → int32_t[]；越界钳制同 Substring */
int32_t  rt_str_char_at(const char* s, int32_t index);           /* UTF-8 code unit → char */
char*    rt_str_join(const char* sep, void* arr);               /* arr: char*[] */
char*    rt_str_join_char(int32_t sep, void* arr);              /* sep: UTF-8 码元；arr: char*[] */
char*    rt_str_replace(const char* s, const char* old, const char* neu);
char*    rt_str_substring(const char* s, int32_t start, int32_t length); /* length<0 → to end */
int32_t  rt_str_contains(const char* s, const char* sub);
int32_t  rt_str_index_of(const char* s, const char* sub);
int32_t  rt_str_index_of_char(const char* s, int32_t c);
int32_t  rt_str_index_of_from(const char* s, const char* sub, int32_t start);
int32_t  rt_str_index_of_char_from(const char* s, int32_t c, int32_t start);
int32_t  rt_str_last_index_of(const char* s, const char* sub);
int32_t  rt_str_last_index_of_char(const char* s, int32_t c);
int32_t  rt_str_last_index_of_from(const char* s, const char* sub, int32_t start);
int32_t  rt_str_last_index_of_char_from(const char* s, int32_t c, int32_t start);
char*    rt_str_insert(const char* s, int32_t index, const char* value);
char*    rt_str_remove(const char* s, int32_t start, int32_t count);
char*    rt_str_trim_start(const char* s);
char*    rt_str_trim_end(const char* s);
char*    rt_str_trim_char(const char* s, int32_t c);
char*    rt_str_trim_start_char(const char* s, int32_t c);
char*    rt_str_trim_end_char(const char* s, int32_t c);
/* chars: int32_t[]（char[]）；空/null → 空白 Trim（对齐 C# params char[]） */
char*    rt_str_trim_chars(const char* s, void* chars);
char*    rt_str_trim_start_chars(const char* s, void* chars);
char*    rt_str_trim_end_chars(const char* s, void* chars);
char*    rt_str_pad_left(const char* s, int32_t total_width);
char*    rt_str_pad_right(const char* s, int32_t total_width);
char*    rt_str_pad_left_char(const char* s, int32_t total_width, int32_t c);
char*    rt_str_pad_right_char(const char* s, int32_t total_width, int32_t c);
char*    rt_str_from_char_count(int32_t c, int32_t count);
char*    rt_str_format(const char* fmt, const char* a0, const char* a1,
                       const char* a2, const char* a3);
int32_t  rt_str_starts_with(const char* s, const char* prefix);
int32_t  rt_str_ends_with(const char* s, const char* suffix);
int32_t  rt_str_starts_with_char(const char* s, int32_t c);
int32_t  rt_str_ends_with_char(const char* s, int32_t c);
int32_t  rt_str_is_null_or_white_space(const char* s);
char*    rt_str_trim(const char* s);
char*    rt_str_to_upper(const char* s);
char*    rt_str_to_lower(const char* s);
char*    rt_str_from_codepoint(int32_t code);         /* Unicode codepoint → UTF-8 string */
void* rt_dict_create(uint32_t (*hash)(void*), int32_t (*eq)(void*, void*));
void rt_dict_set(void* dict, void* key, void* value);
void rt_dict_ensure_capacity(void* dict, int32_t capacity);
void* rt_dict_get(void* dict, void* key);
int32_t rt_dict_contains(void* dict, void* key);
int32_t rt_dict_contains_value(void* dict, void* value, int32_t (*eq)(void* a, void* b));
int32_t rt_dict_try_add(void* dict, void* key, void* value);
int32_t rt_dict_try_get_value(void* dict, void* key, void** out_value);
int32_t rt_dict_count(void* dict);
int32_t rt_dict_remove(void* dict, void* key);
void rt_dict_clear(void* dict);
void rt_dict_destroy(void* dict);
void* rt_dict_keys(void* dict);
void* rt_dict_values(void* dict);
void* rt_dict_get_enumerator(void* dict);
int32_t rt_dict_enumerator_move_next(void* handle);
void* rt_dict_enumerator_get_key(void* handle);
void* rt_dict_enumerator_get_value(void* handle);
uint32_t rt_hash_str(void* key);
int32_t rt_eq_str(void* a, void* b);
uint32_t rt_hash_int(void* key);
uint32_t rt_hash_long(void* key);
int32_t rt_eq_int(void* a, void* b);
int32_t rt_cmp_int(void* a, void* b);
int32_t rt_cmp_str(void* a, void* b);

/* Common hash/eq/cmp function pointer typedefs used by dict, set, sorted, etc. */
typedef uint32_t (*rt_hash_fn)(void*);
typedef int32_t (*rt_eq_fn)(void*, void*);
typedef rt_hash_fn rt_set_hash_fn;
typedef rt_eq_fn rt_set_eq_fn;

/* HashSet<T> — contiguous entry table + bucket index chaining (.NET shape).
   int keys: rt_hash_int/rt_eq_int inlined (int_keys); SoA OA tried+withdrawn (RFC 005 §3.5).
   Reuses rt_hash_fn / rt_eq_fn; add/remove return bool. */
void*    rt_set_create(uint32_t (*hash)(void*), int32_t (*eq)(void*, void*));
void     rt_set_destroy(void* handle);
void     rt_set_ensure_capacity(void* handle, int32_t capacity);
int32_t  rt_set_add(void* handle, const void* elem_ptr);
int32_t  rt_set_contains(void* handle, const void* elem_ptr);
int32_t  rt_set_remove(void* handle, const void* elem_ptr);
int32_t  rt_set_count(void* handle);
void     rt_set_clear(void* handle);
void     rt_set_union_with(void* handle, void* other_handle);
void     rt_set_intersect_with(void* handle, void* other_handle);
void     rt_set_except_with(void* handle, void* other_handle);
void     rt_set_symmetric_except_with(void* handle, void* other_handle);
int32_t  rt_set_is_subset_of(void* handle, void* other_handle);
int32_t  rt_set_is_superset_of(void* handle, void* other_handle);
int32_t  rt_set_is_proper_subset_of(void* handle, void* other_handle);
int32_t  rt_set_is_proper_superset_of(void* handle, void* other_handle);
int32_t  rt_set_overlaps(void* handle, void* other_handle);
int32_t  rt_set_set_equals(void* handle, void* other_handle);
void*    rt_set_to_array(void* handle);
int32_t  rt_set_get(void* handle, int32_t index, void* out_elem);
void*    rt_set_get_enumerator(void* handle);
int32_t  rt_set_enumerator_move_next(void* handle);
void*    rt_set_enumerator_current(void* handle);

/* 集合容器共享的回调类型签名 —— List<T> / LinkedList<T> / SortedDictionary<K,V> /
 * SortedSet<T> 等通用容器在 create 时接收这些回调以解耦元素类型。
 *
 * 设计：回调签名统一在文件头部定义，确保所有容器 ABI 声明可见，避免前向引用。
 * 同一翻译单元内不得重复 typedef（C11 §6.7§3）；此处为唯一定义点。 */

/* 元素相等判定 — 用于 List<T> / LinkedList<T> 的 contains / find / remove。
 * 值类型默认 memcmp；string 使用 strcmp；其他类型由 codegen 单态化生成。 */
typedef int32_t (*rt_list_eq_fn)(const void* a, const void* b);

/* ARC 槽位回调 — 用于 List<T> / LinkedList<T> 维护引用类型元素的生命周期。
 * 接收元素槽指针（非元素值本身），由回调内部解引用并调用 rt_arc_inc/dec。
 * 值类型传 NULL（无引用计数维护）。 */
typedef void (*rt_list_arc_fn)(void* slot);

/* 比较函数签名 — 用于 SortedDictionary / SortedSet 等按键排序的容器。
 * 返回值：<0 表示 a<b，0 表示相等，>0 表示 a>b。
 * 与 IComparable<T>.CompareTo 的语义直接对齐；codegen 在单态化时
 * 将 T.CompareTo 编译为 rt_cmp_fn 传给 create。 */
typedef int32_t (*rt_cmp_fn)(void* a, void* b);

/* LinkedList<T> (Phase 3) — 双向链表 + 哨兵节点。
 *
 * 与 List<T> 共享 eq_fn / arc_inc / arc_dec 模式：
 *   - eq_fn: 元素相等判定（const void* 签名，与 rt_list_eq_fn 一致）；
 *   - arc_inc/arc_dec: 引用类型元素的引用计数维护，add 时 inc、
 *     remove/clear/destroy 时 dec；值类型传 NULL。
 *
 * 节点句柄：rt_linked_list_add_* / first / last / find / find_last 返回
 * 不透明 RtLinkedListNode* 指针；Arc 侧 LinkedListNode<T> 将该指针透传
 * （identity，非再包一层 Arc 对象）。节点内存由 runtime 拥有（free 发生在
 * remove_node / clear / destroy 时）。
 *
 * 重要：调用方在通过 first/last/find 获取节点后，不可越过 ABI 直接修改
 * 节点字段；所有变更须走 rt_linked_list_* ABI。 */
void*    rt_linked_list_create(int32_t elem_size, rt_list_eq_fn eq,
                               rt_list_arc_fn arc_inc, rt_list_arc_fn arc_dec);
void     rt_linked_list_destroy(void* handle);
void     rt_linked_list_clear(void* handle);
int32_t  rt_linked_list_count(void* handle);
void*    rt_linked_list_first(void* handle);              /* NULL=空 */
void*    rt_linked_list_last(void* handle);               /* NULL=空 */
void*    rt_linked_list_add_last(void* handle, const void* elem_ptr);
void*    rt_linked_list_add_first(void* handle, const void* elem_ptr);
void*    rt_linked_list_add_after(void* handle, void* node_handle, const void* elem_ptr);
void*    rt_linked_list_add_before(void* handle, void* node_handle, const void* elem_ptr);
void     rt_linked_list_remove_node(void* handle, void* node_handle);
int32_t  rt_linked_list_remove(void* handle, const void* elem_ptr);  /* 1=removed, 0=not found */
void*    rt_linked_list_find(void* handle, const void* elem_ptr);
void*    rt_linked_list_find_last(void* handle, const void* elem_ptr);
int32_t  rt_linked_list_contains(void* handle, const void* elem_ptr);

/* 节点访问器 — 供 Arc 侧 LinkedListNode<T> facade 调用 */
void     rt_linked_list_node_value(void* node_handle, void* out_ptr);
void     rt_linked_list_node_set_value(void* node_handle, const void* value_ptr);
void*    rt_linked_list_node_prev(void* node_handle);     /* NULL=首节点 */
void*    rt_linked_list_node_next(void* node_handle);     /* NULL=末节点 */
void*    rt_linked_list_node_list(void* node_handle);     /* 所属 LinkedList handle */

/* SortedDictionary<K, V> (Phase 3) — 红黑树实现的有序映射。
 *
 * 比较函数 cmp_fn 由 codegen 在单态化时根据 K : IComparable<K> 约束
 * 生成；返回值同 rt_cmp_fn 语义。
 *
 * key/value 通过 void* 传递，runtime 不维护其 ARC（与 rt_dict 一致：
 * 由 Arc 侧 facade 负责引用计数）。Keys/Values 返回 rt_array payload
 * （void** 数组），按中序遍历产出有序序列。 */
void*    rt_sorted_dict_create(rt_cmp_fn cmp);
void     rt_sorted_dict_destroy(void* handle);
void     rt_sorted_dict_clear(void* handle);
int32_t  rt_sorted_dict_count(void* handle);
int32_t  rt_sorted_dict_contains(void* handle, void* key);
void*    rt_sorted_dict_get(void* handle, void* key);             /* NULL=未找到 */
int32_t  rt_sorted_dict_try_get(void* handle, void* key, void** out_value);
void     rt_sorted_dict_set(void* handle, void* key, void* value);  /* 覆盖或插入 */
int32_t  rt_sorted_dict_add(void* handle, void* key, void* value);  /* 1=added, 0=dup */
int32_t  rt_sorted_dict_remove(void* handle, void* key);            /* 1=removed, 0=missing */
void*    rt_sorted_dict_keys(void* handle);                         /* → rt_array payload */
void*    rt_sorted_dict_values(void* handle);                       /* → rt_array payload */

/* SortedSet<T> (Phase 3) — 红黑树实现的有序集合。 */
void*    rt_sorted_set_create(rt_cmp_fn cmp);
void     rt_sorted_set_destroy(void* handle);
void     rt_sorted_set_clear(void* handle);
int32_t  rt_sorted_set_count(void* handle);
int32_t  rt_sorted_set_add(void* handle, void* key);               /* 1=added, 0=dup */
int32_t  rt_sorted_set_contains(void* handle, void* key);
int32_t  rt_sorted_set_remove(void* handle, void* key);
int32_t  rt_sorted_set_min(void* handle, void* out_ptr);           /* 1=ok, 0=empty */
int32_t  rt_sorted_set_max(void* handle, void* out_ptr);
void*    rt_sorted_set_to_array(void* handle);                     /* → rt_array payload，中序有序 */
void*    rt_sorted_set_reverse(void* handle);                      /* → RtSortedSetEnumerator* */
void*    rt_sorted_set_view_between(void* handle, void* lower, void* upper);
void     rt_sorted_set_union(void* handle, void* other_handle);
void     rt_sorted_set_intersect(void* handle, void* other_handle);
void     rt_sorted_set_except(void* handle, void* other_handle);

/* Queue<T> — RFC Phase 5: circular buffer, 2x grow.
   elem_size fixed at create time; dequeue/peek write to out_ptr. */
void*    rt_queue_create(int32_t elem_size);
void     rt_queue_destroy(void* handle);
void     rt_queue_enqueue(void* handle, const void* elem_ptr);
int32_t  rt_queue_dequeue(void* handle, void* out_ptr);
int32_t  rt_queue_peek(void* handle, void* out_ptr);
int32_t  rt_queue_count(void* handle);
void     rt_queue_clear(void* handle);
int32_t  rt_queue_contains(void* handle, const void* elem_ptr);
void*    rt_queue_to_array(void* handle);

/* ConcurrentDictionary<K,V> — RFC 024 M1 per-bucket lock + lock-free read + deferred-free reclamation.
   Keys/values passed as void* (scalar inttoptr'd). hash_fn/eq_fn work like
   rt_dict_create. resize auto-triggers at load_factor 0.75.
   Removed nodes are marked deleted (key=NULL) and deferred-free'd during
   clear()/resize() when all bucket locks are held. */
void*   rt_concurrent_dict_create(uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t bucket_count);
void*   rt_concurrent_dict_create_level(uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t concurrency_level);
void*   rt_concurrent_dict_create_level_cap(uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t concurrency_level, int32_t capacity);
int32_t rt_concurrent_dict_try_add(void* dict, void* key, void* value);
int32_t rt_concurrent_dict_try_get(void* dict, void* key, void** out_value);
int32_t rt_concurrent_dict_try_update(void* dict, void* key, void* newValue, void* comparisonValue);
void    rt_concurrent_dict_set(void* dict, void* key, void* value);
void*   rt_concurrent_dict_get_or_default(void* dict, void* key);
int32_t rt_concurrent_dict_try_remove(void* dict, void* key, void** out_value);
void*   rt_concurrent_dict_get_or_add(void* dict, void* key, void* (*factory)(void*));
void*   rt_concurrent_dict_get_or_add_val(void* dict, void* key, void* value);
void*   rt_concurrent_dict_add_or_update(void* dict, void* key, void* addValue, void* (*updateFactory)(void*, void*));
void*   rt_concurrent_dict_add_or_update_pf(void* dict, void* key, void* (*addFactory)(void*), void* (*updateFactory)(void*, void*));
int32_t rt_concurrent_dict_contains(void* dict, void* key);
int32_t rt_concurrent_dict_count(void* dict);
void    rt_concurrent_dict_clear(void* dict);
void*   rt_concurrent_dict_keys(void* dict);
void*   rt_concurrent_dict_values(void* dict);
void*   rt_concurrent_dict_to_array(void* dict);

/* ConcurrentQueue<T> — RFC 024 M2 Michael-Scott lock-free queue */
void*   rt_concurrent_queue_create(void);
void    rt_concurrent_queue_enqueue(void* queue, void* value);
int32_t rt_concurrent_queue_try_dequeue(void* queue, void** out_value);
int32_t rt_concurrent_queue_try_peek(void* queue, void** out_value);
int32_t rt_concurrent_queue_count(void* queue);
int32_t rt_concurrent_queue_is_empty(void* queue);
void    rt_concurrent_queue_clear(void* queue);
void*   rt_concurrent_queue_to_array(void* queue);
/* RFC 024 M7 IConcurrentCollection surface */
int32_t rt_concurrent_queue_try_add(void* queue, void* value);
int32_t rt_concurrent_queue_try_take(void* queue, void** out_value);
void    rt_concurrent_queue_copy_to(void* queue, void* dst, int32_t start_idx);

/* ConcurrentBag<T> — RFC 024 M3 per-worker local list + steal */
void*   rt_concurrent_bag_create(void);
void    rt_concurrent_bag_add(void* bag, void* value);
int32_t rt_concurrent_bag_try_take(void* bag, void** out_value);
int32_t rt_concurrent_bag_try_peek(void* bag, void** out_value);
int32_t rt_concurrent_bag_count(void* bag);
int32_t rt_concurrent_bag_is_empty(void* bag);
void    rt_concurrent_bag_clear(void* bag);
void*   rt_concurrent_bag_to_array(void* bag);
/* RFC 024 M7 IConcurrentCollection surface */
int32_t rt_concurrent_bag_try_add(void* bag, void* value);
void    rt_concurrent_bag_copy_to(void* bag, void* dst, int32_t start_idx);

/* ConcurrentStack<T> — RFC 024 M4 Treiber lock-free stack */
void*   rt_concurrent_stack_create(void);
void    rt_concurrent_stack_push(void* stack, void* value);
int32_t rt_concurrent_stack_try_pop(void* stack, void** out_value);
int32_t rt_concurrent_stack_try_peek(void* stack, void** out_value);
void    rt_concurrent_stack_push_range(void* stack, void* items, int32_t n);
int32_t rt_concurrent_stack_try_pop_range(void* stack, void* out_array, int32_t max_n);
int32_t rt_concurrent_stack_count(void* stack);
int32_t rt_concurrent_stack_is_empty(void* stack);
void    rt_concurrent_stack_clear(void* stack);
void*   rt_concurrent_stack_to_array(void* stack);
/* RFC 024 M7 IConcurrentCollection surface */
int32_t rt_concurrent_stack_try_add(void* stack, void* value);
int32_t rt_concurrent_stack_try_take(void* stack, void** out_value);
void    rt_concurrent_stack_copy_to(void* stack, void* dst, int32_t start_idx);

/* BlockingCollection<T> — RFC 024 M5/M7 Semaphore-based bounded producer-consumer */
void*   rt_blocking_collection_create(int32_t capacity, int32_t strategy);
/* M7: kind 0=Queue, 1=Bag, 2=Stack；inner 由调用方创建，BC 接管使用权 */
void*   rt_blocking_collection_create_with(void* inner, int32_t kind, int32_t capacity, int32_t strategy);
/* M7: 固定 kind 的薄包装（供 facade Builtin ABI 分派） */
void*   rt_blocking_collection_create_with_queue(void* inner, int32_t capacity, int32_t strategy);
void*   rt_blocking_collection_create_with_bag(void* inner, int32_t capacity, int32_t strategy);
void*   rt_blocking_collection_create_with_stack(void* inner, int32_t capacity, int32_t strategy);
void    rt_blocking_collection_add(void* bc, void* value);
void*   rt_blocking_collection_take(void* bc);
int32_t rt_blocking_collection_try_add(void* bc, void* value);
int32_t rt_blocking_collection_try_take(void* bc, void** out_value);
void    rt_blocking_collection_complete(void* bc);
int32_t rt_blocking_collection_is_completed(void* bc);
int32_t rt_blocking_collection_is_adding_completed(void* bc);
int32_t rt_blocking_collection_count(void* bc);
int32_t rt_blocking_collection_bounded_capacity(void* bc);
void*   rt_blocking_collection_to_array(void* bc);
int32_t rt_blocking_collection_try_add_to(void* bc, void* value, uint64_t timeout_ms);
int32_t rt_blocking_collection_try_take_to(void* bc, void** out_value, uint64_t timeout_ms);
void    rt_blocking_collection_copy_to(void* bc, void* dst, int32_t start_idx);

/* Stack<T> — sequential LIFO stack backed by rt_list_* ABI */
void*   rt_stack_create(int32_t elem_size, void* eq_fn, void* arc_inc, void* arc_dec);
void    rt_stack_push(void* stack, const void* elem_ptr);
void    rt_stack_pop(void* stack, void* out_ptr);
int32_t rt_stack_try_pop(void* stack, void* out_ptr);
void    rt_stack_peek(void* stack, void* out_ptr);
int32_t rt_stack_try_peek(void* stack, void* out_ptr);
int32_t rt_stack_count(void* stack);
int32_t rt_stack_contains(void* stack, const void* elem_ptr);
void*   rt_stack_to_array(void* stack);
void    rt_stack_clear(void* stack);
void    rt_stack_destroy(void* stack);
/* File & Directory ABI (M1 + M3: 基础文件操作 + 目录与路径，与 C# System.IO 对齐).
 * 所有错误返回 0/NULL，不引入异常。路径分隔符统一使用 '/'，跨平台兼容。
 * ReadAllText 失败返回空串（malloc(1) 的 '\0'）；其余 bool 方法失败返回 0。 */
char*    rt_read_file(const char* path);                       /* 读全文；失败返回空串 */
int32_t  rt_write_file(const char* path, const char* content); /* 覆盖写入 */
int32_t  rt_file_exists(const char* path);
int32_t  rt_file_delete(const char* path);
int32_t  rt_file_append(const char* path, const char* content); /* 追加写入 */
int32_t  rt_file_copy(const char* src, const char* dst);
int32_t  rt_file_move(const char* src, const char* dst);
int32_t  rt_dir_create(const char* path);                     /* 单层；EEXIST 视为成功 */
int32_t  rt_dir_exists(const char* path);
int32_t  rt_dir_delete(const char* path);                      /* 仅删空目录 */
void*    rt_dir_list_files(const char* path); /* → string[] 完整路径；失败/空 → Length 0 */
void*    rt_dir_list_files_pattern(const char* path, const char* search_pattern); /* * / ?；失败/空 → Length 0 */
void*    rt_dir_list_dirs(const char* path);  /* 直接子目录；跳过 . / ..；失败/空 → Length 0 */
char*    rt_path_combine(const char* a, const char* b);        /* 智能拼接路径 */
char*    rt_path_get_dir_name(const char* path);
char*    rt_path_get_file_name(const char* path);
char*    rt_path_get_file_name_without_ext(const char* path);  /* 不含扩展名 */
char*    rt_path_get_extension(const char* path);              /* 含前导点 */
char*    rt_path_change_extension(const char* path, const char* ext); /* 换扩展名；ext 空/NULL 去扩展 */
int32_t  rt_path_has_extension(const char* path);              /* 1=有扩展名（含 "."） */
char*    rt_path_get_temp_path(void);                          /* 临时目录；带尾部分隔符 */
void*    rt_file_read_all_bytes(const char* path);             /* → byte[] payload；失败 Length 0 */
int32_t  rt_file_write_all_bytes(const char* path, void* bytes); /* byte[] → 文件；成功 1 */
void*    rt_file_read_all_lines(const char* path);             /* → string[]；失败 Length 0 */

/* FileStream ABI（标准库就绪 P0）：rt_file_stream_* —— FILE* 句柄流 */
void*    rt_file_stream_open(const char* path, int32_t mode); /* 0=read 1=write 2=create；失败 NULL */
void     rt_file_stream_close(void* handle);
int32_t  rt_file_stream_read(void* handle, void* buffer, int32_t offset, int32_t count);
void     rt_file_stream_write(void* handle, void* buffer, int32_t offset, int32_t count);
int64_t  rt_file_stream_seek(void* handle, int64_t offset, int32_t origin);
int64_t  rt_file_stream_get_length(void* handle);
int64_t  rt_file_stream_get_position(void* handle);
void     rt_file_stream_set_position(void* handle, int64_t value);
void     rt_file_stream_set_length(void* handle, int64_t value);
void     rt_file_stream_flush(void* handle);
int32_t  rt_file_stream_can_read(void* handle);
int32_t  rt_file_stream_can_write(void* handle);
int32_t  rt_file_stream_can_seek(void* handle);

/* FileStream 真异步（文件 I/O 线程池卸载 + 完成投递；实现见 rt_file_stream_async.c）。
 * 阻塞读写/刷新卸载到文件 I/O 专用池（独立于 Task.Run 默认池）；worker 完成后
 * rt_task_complete → g_rt_wake_fn → rt_event_loop_spawn 唤醒 await（与 Task.Run
 * 同构的跨线程完成投递）。返回 malloc'd RtTask*（Pending）；失败 NULL。
 * ct 为 CancellationToken 对象指针（RtCts* 视图），NULL 视为不可取消；仅提交前
 * 预检取消（先例 rt_task_run_func_ct），进入池线程后不可中止。
 * buffer 归调用方所有，须在 Task 完成前保持有效（async 状态机跨 await 存活）。 */
void*    rt_file_stream_read_async(void* handle, void* buffer, int32_t offset,
                                   int32_t count, void* ct); /* int_result = 实际字节数；0=EOF */
void*    rt_file_stream_write_async(void* handle, void* buffer, int32_t offset,
                                    int32_t count, void* ct); /* Task（无结果面） */
void*    rt_file_stream_flush_async(void* handle, void* ct);   /* Task（无结果面） */
void     rt_file_stream_async_shutdown(void); /* 显式收尾文件 I/O 池（Shutdown + join，不 free） */

/* SQLite ABI（L3 Orm execute MVP）：rt_sqlite_* —— 1-based slot 句柄；0 = 无效。
 * 包装 vendored amalgamation（crates/runtime-sqlite）。step：100=ROW / 101=DONE。 */
int32_t  rt_sqlite_open(const char* path);              /* ":memory:" / 文件路径；失败 0 */
void     rt_sqlite_close(int32_t db_handle);
int32_t  rt_sqlite_exec(int32_t db_handle, const char* sql); /* 受影响行数；失败 -1 */
int32_t  rt_sqlite_prepare(int32_t db_handle, const char* sql); /* stmt 句柄；失败 0 */
int32_t  rt_sqlite_step(int32_t stmt_handle);           /* 100=ROW, 101=DONE */
int32_t  rt_sqlite_column_count(int32_t stmt_handle);
int32_t  rt_sqlite_column_int(int32_t stmt_handle, int32_t col);
char*    rt_sqlite_column_text(int32_t stmt_handle, int32_t col); /* malloc；调用方拥有 */
char*    rt_sqlite_column_name(int32_t stmt_handle, int32_t col); /* malloc；调用方拥有 */
void     rt_sqlite_finalize(int32_t stmt_handle);
char*    rt_sqlite_errmsg(int32_t db_handle);           /* malloc；调用方拥有 */
int32_t  rt_sqlite_bind_text(int32_t stmt_handle, int32_t idx, const char* text); /* 1-based；0 ok / -1 fail */
int32_t  rt_sqlite_bind_int(int32_t stmt_handle, int32_t idx, int32_t value);     /* 1-based；0 ok / -1 fail */
int32_t  rt_sqlite_changes(int32_t db_handle);          /* 最近语句受影响行数；无效句柄 -1 */

/* Memory-mapped file ABI (RFC 037 M-CE1 · rt_file_mmap.c).
 * Read-only zero-copy mapping for large document open. */
void*       rt_file_mmap_open(const char* path);   /* failure NULL */
void        rt_file_mmap_close(void* handle);
int64_t     rt_file_mmap_length(void* handle);
const char* rt_file_mmap_data(void* handle);         /* valid until close */

/* CodeEditor piece-table buffer ABI (RFC 037 M-CE1 · rt_editor.c).
 * Original via mmap; edits via add buffer. No full-file string materialize. */
void*    rt_editor_create_empty(void);
void*    rt_editor_open_path(const char* path);    /* mmap; NOT rt_read_file */
void     rt_editor_destroy(void* handle);
int64_t  rt_editor_length(void* handle);
int32_t  rt_editor_line_count(void* handle);
int32_t  rt_editor_ensure_lines(void* handle, int32_t first_line, int32_t last_line);
char*    rt_editor_line_text(void* handle, int32_t line_no); /* single line malloc */
int32_t  rt_editor_set_text(void* handle, const char* text);
int32_t  rt_editor_insert(void* handle, int64_t offset, const char* text);
int32_t  rt_editor_delete(void* handle, int64_t offset, int64_t length);
int32_t  rt_editor_is_mmap_backed(void* handle);

/* Resource/culture ABI (RFC 027 M1: localization & resources).
 *
 * 仅保留 OS API 调用（rt_os_current_uilocale/locale）。资源查找已由
 * ResX CodeGen（RFC 027 强类型访问器）在编译期内联为字面量，运行时
 * 零解析零查找；Culture 数据表与字符串处理已迁移至
 * std/Arc/Globalization/（CultureData.as / CultureHelper.as）纯 Arc 实现。
 *
 * All string returns are freshly malloc'd NUL-terminated (caller-owned).
 * NULL return = allocation failure or invalid input. */
char*    rt_os_current_uilocale(void);
char*    rt_os_current_locale(void);
int64_t  rt_os_now_ticks(void);        /* 本地时间 ticks (100ns since 0001-01-01) */
int64_t  rt_os_now_utc_ticks(void);    /* UTC 时间 ticks */
char*    rt_guid_new_string(void);         /* UUID v4 ← GUID string (malloc'd) */
void*    rt_guid_to_byte_array(const char* s); /* D/N/B/P → byte[16] .NET 混合端序；失败 Length 0 */
char*    rt_guid_from_byte_array(void* bytes); /* byte[16] → D 小写；失败空串 */

/* Stopwatch ABI（Arc.Diagnostics.Stopwatch；高精度间隔测量）
 * Windows: QueryPerformanceCounter / Frequency
 * POSIX:   CLOCK_MONOTONIC 纳秒（frequency = 1e9）
 * IsHighResolution 恒为 1（两平台均高精度，无低精度降级）。 */
int64_t  rt_stopwatch_get_timestamp(void);
int64_t  rt_stopwatch_frequency(void);
int32_t  rt_stopwatch_is_high_resolution(void);

/* Cryptographic ABI (RFC 026 M3: arc-security).
 * All entry points return a freshly malloc'd, NUL-terminated lowercase-hex
 * string; the caller (ARC runtime) owns it. NULL input is treated as "". */
char* rt_crypto_md5(const char* data);
char* rt_crypto_sha1(const char* data);
char* rt_crypto_sha256(const char* data);
char* rt_crypto_sha512(const char* data);
char* rt_crypto_sha384(const char* data);
char* rt_crypto_sha3_256(const char* data);
char* rt_crypto_sha3_512(const char* data);
char* rt_crypto_hmac_sha256(const char* key, const char* msg);
char* rt_crypto_hmac_sha384(const char* key, const char* msg);
char* rt_crypto_hmac_sha512(const char* key, const char* msg);
char* rt_crypto_random_bytes(int32_t count);

/* ---- byte[] 变体（RFC 026 M3 修订：绕过 hex-string 中转，字节进字节出）----
 * 入参为 RtArray byte[] 的 payload 指针（布局见 rt_array.c：8 字节 header +
 * payload；长度经 rt_array_length 读取），NULL 输入按空数组处理。
 * 返回新建 RtArray byte[]（elem_size=1），失败返回 NULL。失败源仅限：
 *   - CSPRNG 失败（rt_crypto_random_bytes_arr）；
 *   - 违规输入（count < 0 / seed 长度不符，见各自注释）。
 * oom 由 rt_array_create 内 rt_panic 处理（全 runtime 惯例，不返回 NULL）。
 * 调用方（ARC 门面）负责将 NULL 转译为 CryptographicException。 */
void* rt_crypto_md5_arr(void* data);            /* → byte[16] */
void* rt_crypto_sha1_arr(void* data);           /* → byte[20] */
void* rt_crypto_sha256_arr(void* data);         /* → byte[32] */
void* rt_crypto_sha384_arr(void* data);         /* → byte[48] */
void* rt_crypto_sha512_arr(void* data);         /* → byte[64] */
void* rt_crypto_sha3_256_arr(void* data);       /* → byte[32]（实现在 rt_sha3.c） */
void* rt_crypto_sha3_512_arr(void* data);       /* → byte[64]（实现在 rt_sha3.c） */
void* rt_crypto_hmac_sha256_arr(void* key, void* msg); /* → byte[32] */
void* rt_crypto_hmac_sha384_arr(void* key, void* msg); /* → byte[48] */
void* rt_crypto_hmac_sha512_arr(void* key, void* msg); /* → byte[64] */
void* rt_crypto_random_bytes_arr(int32_t count);       /* → byte[count]；count<0 → NULL */

/* X.509 证书 ABI（RFC 026 M3；实现见 shim/rt_crypto_native.c）。
 * 句柄为不透明 `mbedtls_x509_crt*`（malloc'd），由本函数释放（幂等：NULL 直接返回）。 */
void rt_crypto_x509_free(void* handle);

/* Fill `buf` with `len` cryptographically-secure random bytes.
 * Returns 0 on success, -1 on failure. Shared CSPRNG helper for all
 * crypto modules (rt_crypto.c / rt_ed25519.c / rt_noise.c).
 * Windows: BCryptGenRandom / RtlGenRandom; POSIX: /dev/urandom.
 * Thread-safe, no global state. */
int rt_crypto_csprng_bytes(uint8_t* buf, size_t len);

/* P2P Cryptographic ABI (RFC 042 M1: Noise Protocol + Ed25519 identity).
 * Ed25519 / X25519 / ChaCha20-Poly1305 primitives.  All operate on raw byte
 * buffers; caller owns all output buffers.  Thread-safe. */
void    rt_crypto_ed25519_keygen(uint8_t out_sk[32], uint8_t out_pk[32]);
void    rt_crypto_ed25519_seed_keygen(const uint8_t seed[32],
                                       uint8_t out_sk[32], uint8_t out_pk[32]);
void    rt_crypto_ed25519_sign(const uint8_t* msg, uint32_t msg_len,
                                const uint8_t sk[32], uint8_t out_sig[64]);
int32_t rt_crypto_ed25519_verify(const uint8_t* msg, uint32_t msg_len,
                                  const uint8_t sig[64], const uint8_t pk[32]);

/* ---- Ed25519 RtArray byte[] 包装（P0 修复：PeerKey 门面去假面化）----
 * 返回值均为新建 RtArray byte[]（elem_size=1），失败返回 NULL（CSPRNG 失败
 * 或输入长度违规），调用方负责转译异常。
 *   keygen_arr        → byte[64] = sk(32, seed) || pk(32)
 *   seed_keygen_arr   → byte[64] = sk(32, seed) || pk(32)；seed 须 byte[32]，否则 NULL
 *   sign_arr          → byte[64] 签名；sk 须 byte[32]（seed），否则 NULL
 *   verify_arr        → 1 有效 / 0 无效 / -1 输入违规（sig 须 64、pk 须 32 字节） */
void*   rt_crypto_ed25519_keygen_arr(void);
void*   rt_crypto_ed25519_seed_keygen_arr(void* seed);
void*   rt_crypto_ed25519_sign_arr(void* msg, void* sk);
int32_t rt_crypto_ed25519_verify_arr(void* msg, void* sig, void* pk);

void    rt_crypto_x25519_dh(const uint8_t sk[32], const uint8_t pk[32],
                             uint8_t out_shared[32]);
int32_t rt_crypto_aead_encrypt(const uint8_t* plaintext, uint32_t pt_len,
                                const uint8_t key[32], const uint8_t nonce[12],
                                const uint8_t* aad, uint32_t aad_len,
                                uint8_t* out_ciphertext, uint8_t out_tag[16]);
int32_t rt_crypto_aead_decrypt(const uint8_t* ciphertext, uint32_t ct_len,
                                const uint8_t key[32], const uint8_t nonce[12],
                                const uint8_t* aad, uint32_t aad_len,
                                const uint8_t tag[16], uint8_t* out_plaintext);

/* Noise Protocol ABI (RFC 042 M5: Noise_XK handshake + transport session).
 * Session is an opaque handle (void*) allocated by rt_noise_session_create.
 * All buffers are caller-owned.  Thread-safe per-session. */
void* rt_noise_session_create(const uint8_t local_sk[32], const uint8_t remote_pk[32],
                               int initiator);
void  rt_noise_session_destroy(void* session);
int   rt_noise_initiate_handshake(void* session, uint8_t* out_msg, int out_max);
int   rt_noise_respond_handshake(void* session, const uint8_t* in_msg, int in_len,
                                  uint8_t* out_msg, int out_max);
int   rt_noise_initiate_finalize(void* session, const uint8_t* in_msg, int in_len);
int   rt_noise_session_encrypt(void* session,
                                const uint8_t* plaintext, int pt_len,
                                uint8_t* out_ciphertext, uint8_t out_tag[16]);
int   rt_noise_session_decrypt(void* session,
                                const uint8_t* ciphertext, int ct_len,
                                const uint8_t tag[16], uint8_t* out_plaintext);
int   rt_noise_respond_finalize(void* session, const uint8_t* in_msg, int in_len);

/* Noise Protocol vector-support ABI (new symbols — official-vector testing;
 * not part of the frozen surface).  Deterministic ephemeral injection,
 * generic spec-correct message processing with payloads, last-message
 * retrieval and handshake-hash inspection. */
int   rt_noise_session_set_ephemeral(void* session, const uint8_t sk[32]);
int   rt_noise_session_set_prologue(void* session, const uint8_t* prologue, int len);
int   rt_noise_handshake_write(void* session, const uint8_t* payload, int payload_len,
                               uint8_t* out, int out_max);
int   rt_noise_handshake_read(void* session, const uint8_t* in_msg, int in_len,
                              uint8_t* out_payload, int out_payload_max);
int   rt_noise_session_last_msg(void* session, uint8_t* out, int out_max);
int   rt_noise_session_handshake_hash(void* session, uint8_t out[32]);

/* ---- Noise RtArray byte[] 包装（P0-2 修复：Noise 门面去假面化）----
 * 入参均为 RtArray byte[]（NULL 按空数组）；数组出参均为新建 RtArray byte[]
 * （elem_size=1），失败返回 NULL。create_arr 返回 opaque 会话句柄（非
 * RtArray，经 Arc string 句柄机制传递）；respond_finalize_arr 返回 Arc
 * bool 语义（成功 1 / 失败 0，C 层 0/-1 惯例在 wrapper 归一化）。
 * encrypt_arr 出参为「密文‖tag」合并数组（pt_len+16）；decrypt_arr 显式
 * 分离收 16 字节 tag（镜像 AesGcm 语义）。 */
void*   rt_noise_session_create_arr(void* sk, void* pk, int32_t initiator);
void*   rt_noise_initiate_handshake_arr(void* session);
void*   rt_noise_respond_handshake_arr(void* session, void* in_msg);
void*   rt_noise_initiate_finalize_arr(void* session, void* in_msg);
int32_t rt_noise_respond_finalize_arr(void* session, void* in_msg);
void*   rt_noise_session_encrypt_arr(void* session, void* pt);
void*   rt_noise_session_decrypt_arr(void* session, void* ct, void* tag);
void*   rt_noise_session_last_msg_arr(void* session);
void*   rt_noise_session_handshake_hash_arr(void* session);

/* Kademlia DHT ABI (RFC 042 M8: distributed hash table routing). */
void* rt_kad_table_create(void);
void  rt_kad_table_destroy(void* table);
void  rt_kad_table_set_local(void* table, const uint8_t local_id[32]);
int   rt_kad_table_add(void* table, const uint8_t peer_id[32], const char* addr);
int   rt_kad_table_remove(void* table, const uint8_t peer_id[32]);
int   rt_kad_table_find_nearest(void* table, const uint8_t target[32], int k);

/* Network ABI (RFC 025 M4: Arc.Net facade).
 * Socket handle is an opaque RtSocket* pointer; create returns ptr, methods
 * take ptr as implicit first argument (via codegen receiver dispatch).
 * All data transfer returns malloc'd NUL-terminated strings (caller-owned).
 * Boolean results: 1 = success, 0 = failure. */
void*   rt_socket_create(int32_t addressFamily, int32_t socketType, int32_t protocolType);
void    rt_socket_close(void* handle);
int32_t rt_socket_connect(void* handle, const char* host, int32_t port);
int32_t rt_socket_bind(void* handle, int32_t port);
int32_t rt_socket_listen(void* handle, int32_t backlog);
void*   rt_socket_accept(void* handle);
int32_t rt_socket_send(void* handle, const void* data, int32_t length);
void*   rt_socket_receive(void* handle, int32_t bufferSize);
/* RFC 025 §1.2.g / RFC 025 M0（2026-08-05 数据报级 byte[] 升级 · 逐项独立立宪）：
 * UdpClient 数据报级收发（显式长度，内部 0x00 完整往返，无 NUL 截断）。
 *   - sendto_bytes：向远端 host:port 发一个数据报（sendto 语义，不 connect）。
 *     返回实际发送字节数（≤ length；失败 0）。
 *   - recvfrom_bytes：收一个数据报到调用方 buffer（recvfrom 语义，忽略源地址）。
 *     返回实际收到的数据报字节数（≤ bufferSize；失败/超时 0）；
 *     数据报大于 bufferSize 时按 UDP 语义截断。
 * 既有 rt_socket_send/receive（string 面 · NUL 终止）语义不变。 */
int32_t rt_socket_sendto_bytes(void* handle, const void* data, int32_t length,
                               const char* host, int32_t port);
int32_t rt_socket_recvfrom_bytes(void* handle, void* buffer, int32_t bufferSize);
int32_t rt_socket_available(void* handle);
int32_t rt_socket_connected(void* handle);
void    rt_socket_shutdown(void* handle, int32_t how);
int32_t rt_socket_poll(void* handle, int32_t microSeconds, int32_t mode);
void    rt_socket_set_recv_timeout(void* handle, int32_t ms);
void    rt_socket_set_send_timeout(void* handle, int32_t ms);
void    rt_socket_set_no_delay(void* handle, int32_t noDelay);
void    rt_socket_set_send_buf_size(void* handle, int32_t size);
void    rt_socket_set_recv_buf_size(void* handle, int32_t size);

/* RFC 009 M2: 异步网络 IO facade。
 *
 * 这些 ABI 是 std/Net/ facade async 方法的入口点。内部流程：
 *   1. 从 handle 取出底层 fd
 *   2. 设置 fd 为非阻塞（首次 async 调用时惰性切换）
 *   3. 获取当前 EventLoop 绑定的 Reactor（rt_event_loop_get_reactor）
 *   4. 创建 Pending Task 作为 completion token
 *   5. 调用 rt_reactor_submit_* 提交 IO 操作，user_data = Task 指针
 *   6. 返回 Task 给调用方（async 状态机 await 挂起）
 *   7. Reactor 完成后 EventLoop tick 调用 g_rt_wake_fn(task) → Task 转 Ready
 *
 * 返回值：malloc'd RtTask* 指针（Pending 状态），由 Arc 侧 Task facade 持有。
 * 失败时返回 NULL（极端场景如 Reactor 未绑定、fd 无效）。 */
void*   rt_socket_connect_async(void* handle, const char* host, int32_t port);
void*   rt_socket_accept_async(void* handle);
void*   rt_socket_send_async(void* handle, const void* data, int32_t length);
void*   rt_socket_receive_async(void* handle, int32_t bufferSize);
/* 字节面异步接收（RFC 009 异步为主 · WebSocket wss TLS 密文含 0x00，不能走
 * string 面）：读取至多 bufferSize 字节写入调用方 buffer，返回 Task<int>
 * （int_result = 实际字节数；0=EOF；<0=错误）。buffer 归调用方所有，须在
 * Task 完成前保持有效（async 状态机跨 await 存活）。 */
void*   rt_socket_receive_bytes_async(void* handle, void* buffer, int32_t bufferSize);

/* RFC 048 §3: Named pipe facade (本机 IPC, 双后端: Windows `\\.\pipe\name` / POSIX FIFO).
 * Handle is an opaque RtPipe* pointer; create returns ptr, methods take ptr as
 * implicit first argument (via codegen receiver dispatch). Byte-stream semantics:
 * no message boundaries; Read returns 0 on ordered peer close (unified EOF);
 * Write performs short-write completion internally and returns 0 when the peer's
 * read side is closed (SIGPIPE suppressed on POSIX — RFC 048 §3.1-1).
 * Boolean results: 1 = success, 0 = failure. Buffer default 64KB (§3.1-2). */
void*   rt_pipe_server_create(const char* name, int32_t maxInstances);
int32_t rt_pipe_server_wait_connect(void* handle);
void*   rt_pipe_client_create(const char* name);
int32_t rt_pipe_client_connect(void* handle, int32_t timeoutMs);
int32_t rt_pipe_read(void* handle, void* buffer, int32_t length);
int32_t rt_pipe_write(void* handle, const void* data, int32_t length);
int32_t rt_pipe_server_disconnect(void* handle);
int32_t rt_pipe_is_connected(void* handle);
void    rt_pipe_close(void* handle);


/* RFC 009 M2: IO 完成上下文（网络/文件 async 共享）。
 *
 * 跨域共享的完成 token：EventLoop reactor_poll 把完成事件交给
 * rt_io_completion_complete 分发，各域（rt_net.c 网络 / rt_file.c 文件）按
 * op_type 把 result 写回 Task 的 int_result/ptr_result，再 rt_task_complete
 * 标记 READY + 触发 waker，最后释放 completion 上下文。
 *
 * 网络 op_type ∈ [0,3]；文件 op_type >= RT_IO_OP_FILE_BASE（rt_file.c 定义
 * 以 RtIoCompletion 为 embedded base 的 RtFileIoCompletion 派生结构）。 */
typedef struct RtTask RtTask;   /* fwd（唯一前置声明）：RtTask 完整定义见本头后文
                                   * `struct RtTask`（RFC 009 M3，依赖 rt_waker /
                                   * rt_resume_fn 类型）。本前置使 RtIoCompletion、
                                   * 文件 async 派生结构能安全持有 RtTask* 指针。 */

typedef enum {
    RT_IO_OP_CONNECT = 0,
    RT_IO_OP_ACCEPT  = 1,
    RT_IO_OP_READ    = 2,
    RT_IO_OP_WRITE   = 3,
    RT_IO_OP_READ_BYTES = 4, /* 字节面读（写入调用方 buffer，int_result=字节数） */
    /* ---- 文件 async（RFC 009 异步为主；rt_file.c 真异步实现）---- */
    RT_IO_OP_FILE_BASE       = 100,
    RT_IO_OP_FILE_READ_TEXT  = 100, /* read → NUL 终止 string（ptr_result） */
    RT_IO_OP_FILE_READ_BYTES = 101, /* read → byte[]（ptr_result） */
    RT_IO_OP_FILE_READ_LINES = 102, /* read → string[]（ptr_result） */
    RT_IO_OP_FILE_WRITE_TEXT = 103, /* write → int_result(1/0) */
    RT_IO_OP_FILE_WRITE_BYTES= 104, /* write → int_result(1/0) */
    RT_IO_OP_FILE_APPEND     = 105, /* write → int_result(1/0) */
} RtIoOpType;

typedef struct RtIoCompletion {
    RtTask*    task;       /* 关联的 Pending Task */
    RtIoOpType op_type;    /* 操作类型（网络/文件） */
    void*      buf;        /* read buffer / write data */
    int32_t    buf_size;   /* buffer 容量 */
} RtIoCompletion;

/* RFC 009 M2: IO 完成事件处理器（分发表）。
 * EventLoop tick 对每个完成事件调用此函数；user_data 为提交时传入的
 * completion 上下文。op_type >= RT_IO_OP_FILE_BASE 时转发到
 * rt_file_io_completion_complete（rt_file.c），否则按网络 op_type 处理。
 * result 是 RtIoEvent.result（字节数 / accept fd / 0=EOF / -errno=错误）。 */
void    rt_io_completion_complete(void* user_data, int32_t result);

/* 文件 async 完成处理器（rt_file.c 定义；rt_file_io_completion_complete）。*/
void    rt_file_io_completion_complete(void* user_data, int32_t result);

/* 文件 async 完成处理器指针（可插拔，解耦 rt_net.c↔rt_file.c 跨域硬链）。
 * rt_file.c 在构造期（main 前）注册为 rt_file_io_completion_complete；
 * 未注册（rt_file.c 未链接）时 rt_io_completion_complete 走安全 no-op。 */
extern void (*g_rt_io_file_completion)(void* user_data, int32_t result);

/* DNS ABI (RFC 025 M4): hostname resolution.
 * rt_dns_resolve returns a malloc'd presentation-format IP string (e.g. "93.184.216.34")
 * or NULL if resolution fails. rt_dns_get_host_name returns the local hostname. */
void*   rt_dns_resolve(const char* host);
void*   rt_dns_get_host_name(void);
void*   rt_dns_resolve_all(const char* host);

/* Text-processing ABI (RFC 021 §4.3 M4: Arc.Text facade).
 * Base64/Hex entry points return a freshly malloc'd, NUL-terminated string; the
 * caller (ARC runtime) owns it. NULL input is treated as "". */
char* rt_text_base64_encode(const char* data);   /* raw bytes → base64 */
char* rt_text_base64_decode(const char* data);   /* base64 → raw bytes */
char* rt_text_hex_encode(const char* data);      /* raw bytes → lowercase hex */
char* rt_text_hex_decode(const char* data);      /* hex → raw bytes */

/* RFC 037 M1 §1.2 ⑥: byte[] ↔ hex (Arc.Text.Hex.ToHexString / FromHexString). */
char* rt_text_hex_bytes_encode(void* bytes);     /* byte[] payload → lowercase hex */
void* rt_text_hex_bytes_decode(const char* data); /* hex → byte[] payload (elem_size 1) */

/* RFC 037 M1 §1.2 ⑥: byte[] ↔ base64 (Arc.Text.Base64.ToBase64String / FromBase64String).
 * - bytes_encode: byte[] payload → base64（rt_array_length 计长，内嵌 0x00 不截断）。
 * - bytes_decode: base64 → byte[] payload（elem_size 1，二进制安全）。 */
char* rt_text_base64_bytes_encode(void* bytes);   /* byte[] payload → base64 */
void* rt_text_base64_bytes_decode(const char* data); /* base64 → byte[] payload (elem_size 1) */

/* Arc.Text.Url percent-encoding (WebUtility.UrlEncode/UrlDecode 对齐).
 * - encode: unreserved (A-Za-z0-9-._~) 原样；' ' → '+'; 其余字节 → %HH（大写）。
 * - decode: '+' → ' '; %HH → byte（大小写十六进制均可）；孤立 '%' 原样保留。 */
char* rt_text_url_encode(const char* value);     /* raw bytes → percent-encoded */
char* rt_text_url_decode(const char* value);     /* percent-encoded → raw bytes */

/* UTF-8 Encoding (std readiness P0: Encoding.GetBytes / GetString).
 * Arc `string` is already UTF-8 NUL-terminated. GetBytes copies payload bytes
 * into an `rt_array_create` byte[] (elem_size=1). GetString copies byte[]
 * payload into a fresh NUL-terminated string (interior 0x00 truncates under
 * Arc's C-string Length model — text without embedded NUL round-trips). */
void* rt_text_utf8_get_bytes(const char* s);     /* string → byte[] payload */
char* rt_text_utf8_get_string(void* bytes);      /* byte[] payload → string */
int32_t rt_text_utf8_get_byte_count(const char* s); /* UTF-8 码元数（strlen；null→0） */

/* Encoding 变体：UTF-16LE / Latin-1（对齐 System.Text.Encoding）。
 * Arc string 为 UTF-8 字节流；UTF-16LE GetBytes 无 BOM；Latin-1 将码点
 * >0xFF 映射为 '?'（同 .NET Latin1）。byte[] = rt_array elem_size=1，
 * 内嵌 0x00 以 rt_array_length 计数（不截断）。 */
void* rt_text_utf16_get_bytes(const char* s);    /* string(UTF-8) → UTF-16LE byte[] */
char* rt_text_utf16_get_string(void* bytes);     /* UTF-16LE byte[] → string(UTF-8) */
void* rt_text_latin1_get_bytes(const char* s);   /* string(UTF-8) → Latin-1 byte[] */
char* rt_text_latin1_get_string(void* bytes);    /* Latin-1 byte[] → string(UTF-8) */

/* Regex（rt_regex.c）：Arc.Text.Regex facade 的 runtime。byte-oriented 子集，
 * 见 rt_regex.c 头部「诚实子集」说明。string[] = rt_array elem_size=sizeof(char*)，
 * 元素为 strdup 产物（ARC string 收编）。 */
int32_t rt_regex_is_match(const char* pattern, const char* input);           /* 是否命中 */
char*   rt_regex_match(const char* pattern, const char* input);              /* 首个匹配子串（无→空串） */
char*   rt_regex_match_group(const char* pattern, const char* input, int32_t group); /* 捕获组（无→空串） */
void*   rt_regex_matches(const char* pattern, const char* input);            /* → char*[] 非重叠匹配 */
char*   rt_regex_replace(const char* pattern, const char* input, const char* replacement); /* $0..$9 / $$ */
void*   rt_regex_split(const char* pattern, const char* input);              /* → char*[] 按匹配切割 */

/* 带 RegexOptions（int32 flags，见 rt_regex.c RX_ICASE/MLINE/SLINE/EXPLI）。
 * Linux 环境位：IgnoreCase=1 Multiline=2 Singleline=4 ExplicitCapture=8。
 * 无 options 的同名 ABI 等价于传 0，保持不变。 */
int32_t rt_regex_is_match_opt(const char* pattern, const char* input, int32_t options);
char*   rt_regex_match_opt(const char* pattern, const char* input, int32_t options);
char*   rt_regex_match_group_opt(const char* pattern, const char* input, int32_t group, int32_t options);
void*   rt_regex_matches_opt(const char* pattern, const char* input, int32_t options);
char*   rt_regex_replace_opt(const char* pattern, const char* input, const char* replacement, int32_t options);
void*   rt_regex_split_opt(const char* pattern, const char* input, int32_t options);

/* BitConverter（主机端序；byte[] = rt_array elem_size=1）。
 * IsLittleEndian / GetBytes(int|long) / ToInt32|ToInt64(byte[], startIndex)。
 * Buffer.BlockCopy(byte[]) → rt_array_copy（元素偏移 = 字节偏移）。 */
int32_t rt_bitconverter_is_little_endian(void);
void*   rt_bitconverter_get_bytes_i32(int32_t value);
void*   rt_bitconverter_get_bytes_i64(int64_t value);
int32_t rt_bitconverter_to_i32(void* bytes, int32_t start_index);
int64_t rt_bitconverter_to_i64(void* bytes, int32_t start_index);

/* StringBuilder ABI (RFC 021 §4.3 M4: Arc.Text.StringBuilder facade).
 * The handle is an opaque `rt_sb_t*` stored at offset 16 of the Arc object.
 * Append entry points return the handle unchanged for fluent chaining. */
void* rt_text_sb_new(void);                      /* create empty builder */
void* rt_text_sb_new_with_str(const char* initial);  /* create with initial string */
void* rt_text_sb_new_with_capacity(int32_t capacity); /* create with capacity */
void* rt_text_sb_append(void* handle, const char* s);         /* append s, return handle */
void* rt_text_sb_append_int(void* handle, int32_t value);     /* append int, return handle */
void* rt_text_sb_append_long(void* handle, int64_t value);    /* append long, return handle */
void* rt_text_sb_append_bool(void* handle, int8_t value);     /* append bool, return handle */
void* rt_text_sb_append_char(void* handle, int32_t value);    /* append char, return handle */
void* rt_text_sb_append_float(void* handle, float value);     /* append float, return handle */
void* rt_text_sb_append_double(void* handle, double value);   /* append double, return handle */
void* rt_text_sb_append_line(void* handle, const char* s);    /* append s + "\n", return handle */
char* rt_text_sb_to_string(void* handle);        /* copy out to fresh string */
char* rt_text_sb_to_string_range(void* handle, int32_t start_index, int32_t length); /* substring */
int32_t rt_text_sb_length(void* handle);         /* current length */
int32_t rt_text_sb_get_capacity(void* handle);   /* current capacity */
void   rt_text_sb_clear(void* handle);           /* reset to empty */
void   rt_text_sb_ensure_capacity(void* handle, int32_t capacity); /* pre-allocate */
void* rt_text_sb_insert(void* handle, int32_t index, const char* s);     /* insert at index */
void* rt_text_sb_remove(void* handle, int32_t start_index, int32_t length); /* remove range */
void* rt_text_sb_replace(void* handle, const char* old_val, const char* new_val); /* replace all */
int32_t rt_text_sb_get_char(void* handle, int32_t index);  /* get char at index */
void    rt_text_sb_set_char(void* handle, int32_t index, int32_t value); /* set char at index */

/* 运行时可观测性（RFC 017 M1） */
typedef struct ArcStackFrame {
    const char* symbol;  /* 符号名（未修饰）；M1 为 NULL，M2 .arcdbg 解析 */
    const char* file;    /* 源文件；M1 为 NULL，M2 .arcdbg 解析 */
    int32_t line;        /* 源码行；M1 为 0 */
} ArcStackFrame;

void rt_panic(const char* msg);                                      /* 兼容入口 */
void rt_panic_at(const char* msg, const char* file, int32_t line, int32_t col);  /* 携带源位置 */
int32_t rt_backtrace(ArcStackFrame* frames, int32_t max_frames);    /* 符号化栈回溯 */
void rt_print_backtrace(void);                                       /* 便捷输出 */

/* .arcdbg 调试符号包（RFC 017 M2 / D5.2） */
int32_t rt_debug_lookup(uint64_t addr, const char** symbol, const char** file,
                        int32_t* line, int32_t* col);  /* 地址 → 符号化 */
int32_t rt_debug_is_arc_frame(const char* symbol);                /* ARC 帧折叠判定 */

/* 模块 dbg 表 registry（RFC 017 阶段一：runtime 单副本共享）——插件 dll 改为
 * 导入引用 arc_runtime 后，模块 dbg 表不再被 rt_debug.o 链接期就地解析，
 * rt_library_load 在加载期登记 / 卸载期注销。table 指向 codegen 发射的
 * ArcDbgEntry 数组（{fn_ptr, name, file, line, col} × count）。 */
int32_t rt_debug_module_register(void* handle, const void* table, int32_t count);
void rt_debug_module_unregister(void* handle);

/* List<T> (RFC 007 Phase 1 + Phase 2 + Phase 4 ARC) —
 * rt_list_eq_fn / rt_list_arc_fn typedef 已在文件头部统一定义。 */

/// Built-in ARC callbacks for class-type elements.
/// codegen passes these to `rt_list_create` when `T` is a reference type.
void rt_list_arc_inc_ref(void* slot);
void rt_list_arc_dec_ref(void* slot);

void* rt_list_create(int32_t elem_size, rt_list_eq_fn eq,
                     rt_list_arc_fn arc_inc, rt_list_arc_fn arc_dec);
void* rt_list_create_with_capacity(int32_t elem_size, int32_t capacity,
                                    rt_list_eq_fn eq,
                                    rt_list_arc_fn arc_inc,
                                    rt_list_arc_fn arc_dec);
void rt_list_destroy(void* handle);
void rt_list_push(void* handle, const void* elem_ptr);
void rt_list_get(void* handle, int32_t idx, void* out_ptr);
void rt_list_set(void* handle, int32_t idx, const void* elem_ptr);
/// 索引器快路径：越界 panic；返回第 idx 个元素槽指针（供 codegen 直接 load/store）。
/// RtList 布局契约（codegen 亦可直访）：data@0, size@8, capacity@12, elem_size@16。
void* rt_list_at(void* handle, int32_t idx);
/// 冷路径扩容（RFC 005）：值类型 `Add` 热路径由 codegen 直降；仅 size≥capacity 时调用。
void rt_list_ensure_capacity(void* handle, int32_t needed);
int32_t rt_list_size(void* handle);
/// RFC 016 M3 §3.3: 零拷贝 List<T> marshal — 获取内部 buffer 指针和元素数量。
/// 供 FFI 边界直接传递给 C 函数（T* + size_t），无拷贝。O(1) 复杂度。
void rt_list_buffer_and_size(void* handle, void** out_buf, int32_t* out_size);
int32_t rt_list_contains(void* handle, const void* elem_ptr);
int32_t rt_list_index_of(void* handle, const void* elem_ptr);
void rt_list_insert(void* handle, int32_t idx, const void* elem_ptr);
void rt_list_remove_at(void* handle, int32_t idx);
void rt_list_clear(void* handle);
int32_t rt_list_remove(void* handle, const void* elem_ptr);
void rt_list_reverse(void* handle);
int32_t rt_list_eq_str(const void* a, const void* b);

/* List<T> (RFC 007 Phase 3: predicate/comparison/array callbacks) */
typedef int32_t (*rt_list_pred_fn)(const void* elem);
typedef int32_t (*rt_list_cmp_fn)(const void* a, const void* b);
int32_t rt_list_find_get(void* handle, rt_list_pred_fn pred, void* out_ptr);
void*   rt_list_find_all(void* handle, rt_list_pred_fn pred);
int32_t rt_list_exists(void* handle, rt_list_pred_fn pred);
int32_t rt_list_find_index(void* handle, rt_list_pred_fn pred);
int32_t rt_list_find_last_index(void* handle, rt_list_pred_fn pred);
int32_t rt_list_true_for_all(void* handle, rt_list_pred_fn pred);
int32_t rt_list_last_index_of(void* handle, const void* elem_ptr);
void    rt_list_for_each(void* handle, rt_list_pred_fn action);
int32_t rt_list_remove_all(void* handle, rt_list_pred_fn pred);
void    rt_list_sort(void* handle, rt_list_cmp_fn cmp);
void    rt_list_sort_default(void* handle);
int32_t rt_list_cmp_str(const void* a, const void* b);
void* rt_list_to_array(void* handle);
void rt_array_destroy(void* ptr);
void    rt_list_copy_to(void* handle, void* dst, int32_t start_idx);
void    rt_list_add_range_list(void* dst, void* src);
int32_t rt_list_capacity(void* handle);
void    rt_list_set_capacity(void* handle, int32_t new_cap);
int32_t rt_list_is_read_only(void* handle);
void    rt_list_remove_range(void* handle, int32_t index, int32_t count);
void    rt_list_trim_excess(void* handle);
void    rt_list_insert_range(void* handle, int32_t idx, void* src, int32_t n);
void*   rt_list_get_range(void* handle, int32_t idx, int32_t count);
int32_t rt_list_binary_search(void* handle, const void* key);
int32_t rt_list_binary_search_cmp(void* handle, const void* key, rt_list_cmp_fn cmp);

/* Runtime-length array ABI (RFC 015 Phase B) */
void*    rt_array_create(int32_t cap, int32_t elem_size);
int32_t  rt_array_length(void* payload);
/* rt_array_destroy is shared with the legacy List::ToArray path above. */

/* P5-F: Array utility methods */
void     rt_array_copy(void* src, int32_t src_offset, void* dst, int32_t dst_offset, int32_t length);
void     rt_array_clear(void* payload, int32_t offset, int32_t length);
void     rt_array_reverse(void* payload);
int32_t  rt_array_index_of_int(void* payload, int32_t value);
int32_t  rt_array_last_index_of_int(void* payload, int32_t value);
void     rt_array_resize(void** slot, int32_t new_size);
int32_t  rt_array_exists(void* payload, rt_list_pred_fn pred);
int32_t  rt_array_find_int(void* payload, rt_list_pred_fn pred);
int32_t  rt_array_find_last_int(void* payload, rt_list_pred_fn pred);
int32_t  rt_array_find_index(void* payload, rt_list_pred_fn pred);
int32_t  rt_array_find_last_index(void* payload, rt_list_pred_fn pred);
int32_t  rt_array_true_for_all(void* payload, rt_list_pred_fn pred);
void     rt_array_for_each(void* payload, rt_list_pred_fn action);
void     rt_array_sort_int(void* payload);
int32_t  rt_array_binary_search_int(void* payload, int32_t value);
/* FindAll / ConvertAll：返回新建 int[]；converter 复用 rt_list_pred_fn 签名（返回映射值）。 */
void*    rt_array_find_all_int(void* payload, rt_list_pred_fn pred);
void*    rt_array_convert_all_int(void* payload, rt_list_pred_fn converter);

/* Exception unwinding (zero-cost EH; Windows SEH native raise via
 * `_CxxThrowException` — see rt_exc.c). Milestone ⑥ removed the legacy
 * try-registry; POSIX Itanium is milestone ⑨. */
void rt_throw(void* exception_obj);
void* rt_get_exception(void);
/* L2 StackTrace：捕获当前调用栈为多行字符串（malloc；调用方写入 Exception.StackTrace）。
 * 主路径嵌入 __arc_dbg_table（与 -g 解耦）→ 函数名 + 可行时 file:line；
 * POSIX backtrace_symbols 次级；否则 `at <0x…>`；极端无帧 `at <throw>`。 */
char* rt_format_stacktrace(void);

void rt_arc_inc(void* ptr);
void rt_arc_dec(void* ptr);
int32_t rt_arc_count(void* ptr);
/* RFC 005 M2: 循环收集器字段遍历。obj 的 vtable slot 2 为 __walk_{cname}
 *（有 class 字段的 class）或 null；对每个 class 类型字段调 visit(ctx, field)。 */
void rt_arc_walk_fields(void* obj, void (*visit)(void* ctx, void* field), void* ctx);

/* ---- RFC 050 统一对象头：runtime 句柄内存的身份物理化 ----
 *
 * 模式 A 句柄（对象即裸 C 结构，无 ArcHeader）原靠 codegen 侧
 * `is_opaque_runtime_handle` 字符串豁免清单防 ARC 误计数——清单是隐式约定，
 * 每新增门面都需「记得」登记（NamedPipe 曾漏 → 批边界 0xC0000005）。
 * 本头把身份物理化进内存：任何 inc/dec 先验 magic/kind，判定层的漏判
 * 物理无害化（豁免清单降级为优化语义）。
 *
 * - `rt_obj_alloc_opaque(biz_size)`：malloc(头 + biz) 并写 magic/kind，
 *   返回业务区指针（C 结构定义与字段访问不变）；配对 `rt_obj_free`
 *   （回退头后 free）。
 * - `rt_arc_inc/dec` 内建三层守卫：下界哨兵（小整数指针无害化）→
 *   magic（非 runtime 堆块 no-op）→ kind（opaque 禁计数）。
 * - 迁移分期见 RFC 050 §4（M-a 三创建点试点 → M-b 全量 → M-c 豁免退役）。 */
#define RT_OBJ_MAGIC 0x48435241u /* 'ARCH' */
#define RT_OBJKIND_ARC 1
#define RT_OBJKIND_OPAQUE 2
#define RT_PTR_FLOOR ((uintptr_t)0x10000)
typedef struct RtOpaqueHead {
    uint32_t magic;
    uint32_t kind;
    uint32_t reserved[2]; /* 16B 对齐 + 未来扩展（调试代数等） */
} RtOpaqueHead;
void* rt_obj_alloc_opaque(size_t biz_size);
void  rt_obj_free(void* biz_ptr);
/* RFC 050 §3：模式 A 创建点统一入口——create 返回点 malloc → RT_OPAQUE_NEW，
 * destroy/析构点 free → rt_obj_free。结构定义与字段访问不变，只是分配多出 16B 头。 */
#define RT_OPAQUE_NEW(type) ((type*)rt_obj_alloc_opaque(sizeof(type)))

/* RFC 005 M3/M4: Nim ORC 试删循环收集器（默认关闭，G8 不劣化）。
 *
 * - rt_arc_set_cycle_collection(enabled)：运行时开关，返回先前状态。关闭时
 *   rt_arc_dec 与 RFC 005 之前完全一致（rc 归零立即 free）。
 * - rt_arc_collect_cycles()：对候选队列做试删——经 rt_arc_walk_fields DFS
 *   闭包，逐对象计算（真实 rc − 环内入引用）；全为 0 则 fire finalizer + free，
 *   否则保留（可能泄漏，绝不悬垂）。
 *
 * 并发姿态：首版单线程安全，仅收集本线程可独占的对象（对齐延迟释放姿势）；
 * 跨线程环不收集（文档化限制）。
 */
int32_t rt_arc_set_cycle_collection(int32_t enabled);
void    rt_arc_collect_cycles(void);

/* RFC 005 §2.2: Weak<T> 运行时支持。
 *
 * ArcHeader 在 refcount 与 vtable 之间增加 _Atomic int32_t weakcount 字段
 *（复用原 4B padding，header 保持 16B）。rt_arc_dec 在 strong refcount 归零
 * 时检查 weakcount，>0 则保留 header 供 Weak<T>.TryGet 原子观察 0 状态。
 *
 * RtWeak 为不透明槽位结构（C runtime 内部 void* target），由 rt_arc_weak_create
 * 在堆上分配，作为 Weak<T> Arc 对象偏移 16 处的 _target 字段存储（codegen
 * 直接发射 store ptr，不走 FieldSet 的 ARC 维护路径——slot 不是 ArcHeader 对象）。
 *
 * 语义：
 *   - rt_arc_weak_create(target)：target.weakcount +1；分配 RtWeak 槽持有 target
 *   - rt_arc_weak_try_get(slot)：CAS-inc target.refcount（仅当 >0）；返回目标
 *     指针（已 strong-retained）或 NULL（目标已回收）
 *   - rt_arc_weak_destroy(slot)：target.weakcount -1；若归零且 refcount==0 则
 *     释放 target header；释放槽位（析构前自动从模块弱登记表 untrack）
 *
 * RFC 017 §2.6（热卸载 Weak<T> 边界语义 · 宿主侧弱登记表）：
 *   RtWeak 槽位带可选模块代数标签（0 = 未关联）。模块边界 Weak<T> 经
 *   rt_library_weak_register(gen, slot) 登记进 ALC 宿主内存并盖上代数；
 *   模块卸载时 rt_library_unload_hot 对已登记槽位调用 rt_arc_weak_neutralize
 *   —— target 置空（幂等）→ 卸载后 TryGet() 确定性返回 NULL（观察 tombstone
 *   头语义，禁悬垂复活）。Weak<T> 不阻止卸载（ledger 不计弱引用）。
 *
 * Arc 语言层约束（typeck 强制）：`new Weak<T>(null)` 拒绝（E_WEAK_NULL_TARGET）；
 * `Weak<T>` 的 T 必须为引用类型（E_WEAK_VALUE_TYPE）。C ABI 仅做防御性 null 检查。 */
void* rt_arc_weak_create(void* target);
void* rt_arc_weak_try_get(void* weakslot);
void  rt_arc_weak_destroy(void* weakslot);
int32_t rt_arc_weak_generation(void* weakslot);
void    rt_arc_weak_set_generation(void* weakslot, int32_t generation);
void    rt_arc_weak_neutralize(void* weakslot);

/* RFC 018 M1 + RFC 018 M1: 运行时类型判断 + 完整反射元数据 ABI
 *
 * RtTypeInfo 是每个 class/struct/interface/enum 关联的全局常量结构，
 * 由 codegen 发射为 `@.typeinfo.{Type}` 全局符号；vtable slot 0 持有指向它的指针。
 *
 * RFC 018 M1（2026-07-18）引入最小结构：type_id + parent 链，支持 rt_obj_isa。
 * RFC 018 M1（2026-07-19）扩展为完整元数据描述——新增 name/full_name/ns/kind/flags
 * + declared_methods/fields/properties/events/constructors + implemented_interfaces
 * + element_type + declared_nested_types + attributes 字段。
 *
 * **二分边界**（RFC 018 §3.2）：
 *   - 反射元数据描述（保留）：所有 *Info 结构体仅含只读元数据，由 codegen 发射为
 *     rodata 全局常量，运行时零成本（指针 + 偏移读取）
 *   - 反射动态操作（永久剔除）：所有 *Info 结构体**不含函数指针 / 字段偏移**，
 *     从 ABI 物理层面保证无法 Invoke/GetValue/SetValue/CreateInstance
 *
 * **物理边界**（RFC 018 §3.3）：
 *   - RtMethodInfo/RtConstructorInfo 无函数指针字段
 *   - RtFieldInfo 无字段偏移字段
 *   - RtPropertyInfo 的 get_method/set_method 是 RtMethodInfo* 签名指针，无函数指针
 *   - 这是元数据描述 vs 反射调用的物理隔离
 *
 * rt_obj_isa 沿用 RFC 018 M1 语义：遍历 parent 链比对 type_id，实现 class 层级的
 * `is` 测试。RFC 018 M1 扩展字段不影响该语义（向后兼容）。
 *
 * vtable 布局（RFC 018 D5 修订，协同 RFC 006 vtable 槽位修订）：
 *   slot 0: const RtTypeInfo* typeinfo   ← RFC 018 M1 新增，RFC 018 M1 扩展内容
 *   slot 1: dtor placeholder (ptr null)  ← RFC 006 原槽 0，语义保留
 *   slot 2+: virtual methods             ← RFC 006 原 slot 1+ 平移
 *
 * 编译期 is 折叠（RFC 018 D8）由 typeck 在静态类型已知时直接产出常量，
 * rt_obj_isa 仅在运行时无法折叠时调用（基类指针测试子类）。 */

/* 类型分类（对齐 Arc.Reflection.TypeKind） */
typedef enum {
    RT_TYPE_KIND_PRIMITIVE = 0,
    RT_TYPE_KIND_CLASS = 1,
    RT_TYPE_KIND_STRUCT = 2,
    RT_TYPE_KIND_INTERFACE = 3,
    RT_TYPE_KIND_ENUM = 4,
    RT_TYPE_KIND_ARRAY = 5,
    RT_TYPE_KIND_NULLABLE = 6,
    RT_TYPE_KIND_TASK = 7,
    RT_TYPE_KIND_FUNC = 8,
    RT_TYPE_KIND_OTHER = 9,
} RtTypeKind;

/* 成员类型（对齐 Arc.Reflection.MemberTypes） */
typedef enum {
    RT_MEMBER_TYPE_INFO = 1,
    RT_MEMBER_TYPE_METHOD = 2,
    RT_MEMBER_TYPE_FIELD = 4,
    RT_MEMBER_TYPE_PROPERTY = 8,
    RT_MEMBER_TYPE_EVENT = 16,
    RT_MEMBER_TYPE_CONSTRUCTOR = 32,
    RT_MEMBER_TYPE_NESTED_TYPE = 64,
} RtMemberType;

/* 前向声明 */
typedef struct RtTypeInfo RtTypeInfo;
typedef struct RtMethodInfo RtMethodInfo;
typedef struct RtFieldInfo RtFieldInfo;
typedef struct RtPropertyInfo RtPropertyInfo;
typedef struct RtEventInfo RtEventInfo;
typedef struct RtConstructorInfo RtConstructorInfo;
typedef struct RtParameterInfo RtParameterInfo;
typedef struct RtCustomAttributeData RtCustomAttributeData;

/* 自定义属性数据（不含 Attribute 实例，仅元数据描述）
 * 这是元数据描述 vs 反射调用的物理边界——不持有 Attribute 实例，无法触发其逻辑。 */
struct RtCustomAttributeData {
    const RtTypeInfo* attribute_type;          /* 属性类型 */
    /* 构造参数（按类型分槽，简化为 string/int/typeof 四类）
     * M1 阶段仅支持 string + int + typeof 三类，bool 用 int 0/1 表示 */
    const char* const* ctor_str_args;          /* string 参数数组 */
    int32_t ctor_str_args_count;
    const int64_t* ctor_int_args;              /* int/long/bool 参数数组（bool 用 0/1） */
    int32_t ctor_int_args_count;
    const RtTypeInfo* const* ctor_type_args;   /* typeof(T) 参数数组 */
    int32_t ctor_type_args_count;
    /* 命名参数 M2 阶段细化 ABI */
};

/* 参数信息（独立结构，非 MemberInfo 派生，对齐 C# ParameterInfo） */
struct RtParameterInfo {
    const char* name;                          /* 参数名 */
    const RtTypeInfo* parameter_type;          /* 参数类型 */
    int32_t position;                          /* 0 起始位置 */
    int32_t flags;                             /* ParameterAttributes 位掩码 */
    int32_t has_default_value;                 /* 0 = 无默认值，1 = 有默认值 */
    int64_t default_value_int;                 /* int/long/bool 默认值 */
    const char* default_value_str;             /* string 默认值（NULL 表示无） */
    const RtCustomAttributeData* attributes;   /* 参数上声明的属性 */
    int32_t attribute_count;
};

/* 方法签名信息（不含函数指针 — 物理边界，对齐 RFC 018 §3.3） */
struct RtMethodInfo {
    const char* name;                          /* 方法名 */
    const RtTypeInfo* declaring_type;          /* 声明此方法的类型 */
    const RtTypeInfo* return_type;             /* 返回类型 */
    const RtParameterInfo* parameters;         /* 形参列表 */
    int32_t parameter_count;
    int32_t flags;                             /* MethodAttributes 位掩码 */
    const RtCustomAttributeData* attributes;   /* 方法上声明的属性 */
    int32_t attribute_count;
    /* !! 无函数指针字段 !! —— 物理边界，杜绝 Invoke */
};

/* 构造函数签名信息（不含函数指针 — 物理边界） */
struct RtConstructorInfo {
    const RtTypeInfo* declaring_type;          /* 声明此构造函数的类型 */
    const RtParameterInfo* parameters;         /* 形参列表 */
    int32_t parameter_count;
    int32_t flags;                             /* MethodAttributes 位掩码 */
    const RtCustomAttributeData* attributes;   /* 构造函数上声明的属性 */
    int32_t attribute_count;
    /* !! 无函数指针字段 !! —— 物理边界，杜绝 Invoke */
};

/* 字段信息（不含字段偏移 — 物理边界） */
struct RtFieldInfo {
    const char* name;                          /* 字段名 */
    const RtTypeInfo* declaring_type;          /* 声明此字段的类型 */
    const RtTypeInfo* field_type;              /* 字段类型 */
    int32_t flags;                             /* FieldAttributes 位掩码 */
    const RtCustomAttributeData* attributes;   /* 字段上声明的属性 */
    int32_t attribute_count;
    /* !! 无字段偏移字段 !! —— 物理边界，杜绝 GetValue/SetValue */
};

/* 属性信息（不含 getter/setter 函数指针 — 物理边界）
 * get_method/set_method 是 RtMethodInfo* 签名指针，本身无函数指针 */
struct RtPropertyInfo {
    const char* name;                          /* 属性名 */
    const RtTypeInfo* declaring_type;          /* 声明此属性的类型 */
    const RtTypeInfo* property_type;           /* 属性类型 */
    int32_t can_read;                          /* 0 = 不可读，1 = 可读 */
    int32_t can_write;                         /* 0 = 不可写，1 = 可写 */
    const RtMethodInfo* get_method;            /* getter 签名（无函数指针） */
    const RtMethodInfo* set_method;            /* setter 签名（无函数指针） */
    int32_t flags;                             /* PropertyAttributes 位掩码 */
    const RtCustomAttributeData* attributes;   /* 属性上声明的属性 */
    int32_t attribute_count;
};

/* 事件信息 */
struct RtEventInfo {
    const char* name;                          /* 事件名 */
    const RtTypeInfo* declaring_type;          /* 声明此事件的类型 */
    const RtTypeInfo* event_handler_type;      /* 事件处理器类型 */
    const RtMethodInfo* add_method;            /* 订阅方法签名 */
    const RtMethodInfo* remove_method;         /* 取消订阅方法签名 */
    const RtMethodInfo* raise_method;          /* 触发方法签名（可为 NULL） */
    int32_t flags;                             /* EventAttributes 位掩码 */
    const RtCustomAttributeData* attributes;   /* 事件上声明的属性 */
    int32_t attribute_count;
};

/* 类型信息（扩展 RFC 018 v1 的 type_id + parent 链）
 * RFC 018 M1 扩展为完整元数据描述，向后兼容 RFC 018 v1。 */
struct RtTypeInfo {
    /* RFC 018 v1 字段（保留，向后兼容 rt_obj_isa） */
    int32_t type_id;                           /* FNV-1a hash，跨编译单元稳定 */
    const struct RtTypeInfo* parent;           /* 直接基类（NULL = 无基类） */

    /* RFC 018 M1 扩展字段 */
    const char* name;                          /* 类型名（不含命名空间） */
    const char* full_name;                     /* 完整限定名（含命名空间） */
    const char* ns;                            /* 命名空间 */
    int32_t kind;                              /* RtTypeKind 枚举 */
    int32_t flags;                             /* TypeAttributes 位掩码 */

    /* 本类型声明的成员列表（不含继承） */
    const RtMethodInfo* declared_methods;
    int32_t declared_method_count;
    const RtFieldInfo* declared_fields;
    int32_t declared_field_count;
    const RtPropertyInfo* declared_properties;
    int32_t declared_property_count;
    const RtEventInfo* declared_events;
    int32_t declared_event_count;
    const RtConstructorInfo* declared_constructors;
    int32_t declared_ctor_count;

    /* 实现的接口（仅 class/struct，按声明顺序） */
    const RtTypeInfo* const* implemented_interfaces;
    int32_t interface_count;

    /* 元素类型（数组/可空/Task — NULL 表示非泛型） */
    const RtTypeInfo* element_type;

    /* 嵌套类型 */
    const RtTypeInfo* const* declared_nested_types;
    int32_t nested_type_count;

    /* 自定义属性 */
    const RtCustomAttributeData* attributes;
    int32_t attribute_count;

    /* 与 implemented_interfaces 同索引平行：每个接口对应的 itable 指针
     * （class 为 @.itable.{Class}_{Iface}，struct 为 @.itable.{Struct}_Box_{Iface}）。
     * 标记接口（无方法/属性，codegen 不发射 itable）对应 null。
     * 供 rt_obj_to_iface 动态 downcast（object → 接口）返回分派 itable。
     * 追加式字段（置于结构体末尾），不改动既有 24 字段偏移。 */
    const void* const* interface_itables;
};

/* RFC 018 M1: class 层级 is 测试（语义不变，向后兼容） */
int32_t rt_obj_isa(void* obj, const RtTypeInfo* target);

/* 动态 downcast（RFC 004 P0 后续 Sprint）：返回 obj 实际类型实现 target 接口的
 * itable 指针；未实现 / obj 为 null / target 非接口 返回 NULL。
 * 与 rt_obj_isa 同源：读 obj vtable slot0 typeinfo，沿 typeinfo（含 parent 链）
 * 在 implemented_interfaces 中比对 type_id，命中返回平行 interface_itables[i]。 */
const void* rt_obj_to_iface(void* obj, const RtTypeInfo* target_iface);

/* RFC 018 M1: 完整反射元数据查询 ABI */

/* 按全名查找类型，返回 typeinfo 指针；未找到返回 NULL。
 * name 例："Arc.Collections.List"、"Foo"、"int"。
 * 内部维护 codegen 发射的全局 typeinfo 表 + 基元 typeinfo 静态表。 */
const RtTypeInfo* rt_type_by_name(const char* full_name);

/* 按类型 ID 查找（用于 DI 容器 typeof(T) TypeId 反查 Type）。
 * M2 阶段 DI 迁移到 Type 后此 ABI 仍保留供内部使用。 */
const RtTypeInfo* rt_type_by_id(int32_t type_id);

/* 遍历基类链查找方法（含继承），返回方法签名或 NULL。
 * 用于 Type.GetMethods() 实现。param_count = -1 表示不校验参数数量。 */
const RtMethodInfo* rt_type_find_method(
    const RtTypeInfo* type, const char* name, int32_t param_count);

/* 遍历基类链查找字段（含继承）。 */
const RtFieldInfo* rt_type_find_field(
    const RtTypeInfo* type, const char* name);

/* 遍历基类链查找属性（含继承）。 */
const RtPropertyInfo* rt_type_find_property(
    const RtTypeInfo* type, const char* name);

/* 判断 type 是否是 base 的子类型（复用 RFC 018 rt_obj_isa parent 链遍历路径）。
 * 与 rt_obj_isa 区别：rt_obj_isa 接收对象指针，rt_type_is_subtype 直接接收 typeinfo。 */
int32_t rt_type_is_subtype(const RtTypeInfo* type, const RtTypeInfo* base);

/* RFC 018 M2: RtTypeInfo 字段直接查询 ABI。
 * 这些函数直接从 codegen 发射的 RtTypeInfo* rodata 中读取指定字段，
 * 零堆分配，O(1) 复杂度。codegen 在拦截 RuntimeType 的 [Builtin] getter
 * 时发射对这些函数的调用。 */
const char* rt_type_get_name(const RtTypeInfo* ti);
const char* rt_type_get_full_name(const RtTypeInfo* ti);
int32_t    rt_type_get_kind(const RtTypeInfo* ti);
const RtTypeInfo* rt_type_get_base(const RtTypeInfo* ti);

/* 注册用户类型 typeinfo（由 codegen 在 .ctor 段调用）。
 * 用于将 @.typeinfo.{Type} 全局注册到运行时查询表。
 * 幂等：重复注册同一指针返回 1；type_id 冲突返回 0。
 * 返回 1 = 成功注册，0 = 表满或 type_id 冲突。 */
int32_t rt_type_register(const RtTypeInfo* ti);

/* RFC 017 阶段一（ALC 共享 dll）：基元 typeinfo 经**函数符号**暴露。
 *
 * 共享库边界上数据符号的导入 thunk 是「指向数据的指针」而非数据本身，
 * codegen 直接引用数据符号会别名 thunk 导致语义错误；函数符号经 thunk
 * 解析天然正确，故基元 typeinfo 改为按 id 索引查询。基元 typeinfo 全局
 * 在 rt_type.c 中 static 化（TU 局部），不再出现在导出面。
 *
 * id 序对齐 rt_primitive_table：int=0 long=1 short=2 byte=3 char=4
 * float=5 double=6 bool=7 string=8 void=9 object=10。
 * 越界 id 返回 NULL；首次调用自动触发 rt_type_init()。 */
const RtTypeInfo* rt_typeinfo_prim(int32_t id);

/* 基元装箱 vtable 查询：返回 runtime 内静态 [3 x ptr] 表
 * { &rt_typeinfo_<prim>, NULL, NULL }（slot0 供 `o is T` 判别）。
 * 仅接受可装箱基元 id（int/long/short/byte/char/float/double/bool），
 * 其余 id 返回 NULL。与 rt_typeinfo_prim 同 TU 定义（rt_type.c），
 * 表内嵌的 typeinfo 地址仅同 TU 可见。 */
const void* rt_box_vtable(int32_t id);

/* 一次性初始化所有基元 typeinfo（幂等）。由 rt_env_init 在进程启动时调用，
 * 确保反射代码直接读取基元 typeinfo 字段（如 RuntimeType.Name）前已初始化。
 * 暴露为公开符号，替代缺失的 GCC/Clang constructor 属性（跨平台可移植）。 */
void rt_type_init(void);

/* FFI Marshal 装箱 ABI（RFC 016 v2 M2 / RFC 016 M3 同期推进）.
 *
 * ArcBox 内存布局（v2 简化版，反射永久剔除）：
 *   [ArcHeader 16B][payload_size 4B][padding 4B][payload N B]
 * - ArcHeader 共享 refcount+vtable 布局，rt_arc_inc/dec 可直接管理生命周期
 * - rt_box_destroy 是 rt_arc_dec 的 alias，无独立实现
 * - unboxing 通过 expected_size 与 payload_size 比较校验（替代 v1 type_id 校验）
 * - 失败调用 rt_panic/rt_panic_at（与 RFC 017 D4.1 一致）
 *
 * FFI 边界装箱点由 typeck 自动插入（仅在 extern 函数 void* 形参/返回值处）；
 * 通用赋值/参数/返回值装箱已永久剔除（RFC 016 v2 §6，由 RFC 004 variant 承担）。 */
void*    rt_box_create(int32_t payload_size, int32_t payload_align);
void     rt_box_destroy(void* box_ptr);                   /* alias of rt_arc_dec */
int32_t  rt_box_unbox(void* box_ptr, int32_t expected_size,
                      void* out_ptr, int32_t out_size);   /* 0 成功；非 0 失败（panic） */

/* RFC 006 M3: string→object 装箱（rt_type.c）。object 槽持有 string 时由
 * codegen 调用 rt_string_box 包装（box 带 vtable→rt_typeinfo_string，使
 * `o is string` 可识别且其它类型判别安全）；rt_string_unbox 从 object 槽
 * 提取 char*（非 string box 返回 NULL）。 */
void*       rt_string_box(const char* s);
const char* rt_string_unbox(void* obj);

/* Waker (RFC 009 §5.3): invoked by external events to move a task to ready.
 * M1 stores the waker pointer on the task; M3 implements real wake chaining. */
typedef struct rt_waker {
    void (*wake)(void* data);
    void* data;
} rt_waker;

/* resume 函数签名（M2 状态机）：推进状态机到下一 suspend 点或完成。
 *   env_ptr : 状态机 env 结构指针
 *   waker   : 调度器传入的 waker（M3 启用；M2 可为 NULL）
 *   返回值   : 新 status（RT_TASK_READY / RT_TASK_PENDING / RT_TASK_FAULTED） */
typedef int32_t (*rt_resume_fn)(void* env_ptr, rt_waker* waker);

/* RtTask 完整定义（runtime 内部，rt_task.c + rt_event_loop.c 共享）。
 * M3 扩展：内嵌 _waker_slot 避免堆分配 waker binding。
 * M5.2 扩展：from_slab 标记分配来源，slab 路径释放时归还 free_list。
 * M6 扩展：poll_flags 多线程 poll 并发守卫（多线程 executor 默认启用）。 */
/* RtTask poll 并发守卫位（RFC 009 M6：多线程 executor）。
 * 多 worker 可能对同一 Task 并发 poll（wake 风暴 / await 竞争），必须防重入：
 *   - POLLING ：某 worker 已 CAS 抢占，正在 poll/resume 该 Task；
 *               其他 poll 调用 CAS 失败 → 置 NOTIFIED → 返回 PENDING（不重入）。
 *   - NOTIFIED：POLLING 持有期间收到新唤醒（complete/waker），
 *               持锁线程释放 POLLING 时见位 → 清位并重 poll 一次（闭合丢失唤醒）。 */
#define RT_TASK_PF_POLLING  (1u << 0)
#define RT_TASK_PF_NOTIFIED (1u << 1)
struct RtTask {
    int32_t    status;        /* RT_TASK_* */
    int32_t    canceled;      /* 取消标志（0/1） */
    uint32_t   poll_flags;    /* M6: poll 并发守卫位（原子访问；POLLING/NOTIFIED） */
    /* waker 交接自旋锁（case2/case8 第三处丢失唤醒根治）：register_waker 的
     * waker 安装与 complete/fault 的 waker snapshot 原为两侧普通读写，check-
     * after-register 仅闭合「complete 先行」分支；「snapshot 与 register 并发」
     * 交错（snapshot 漏读 → outer 挂起 → READY 后至 → 零唤醒源）无保护。
     * 0=unlocked / 1=held；位于 ZERO_PREFIX 内，slab 复用 memset 归零。 */
    uint32_t   wk_lock;
    /* M6.2 协程暖启动守卫：async 函数调用点被「暖启动」（autostart 首 poll 驱动
     * body 同步前缀）后，本 Task 将以 PENDING 状态暴露给父 await；父 await 的
     * 「直达提取」（零 re-poll）在暖启动路径下可能越过 PENDING 越界 resume ——
     * 违反复乐园非重入假设 → 二次 resume 误判 final → 子成孤儿 → 0xC0000005。
     * 守卫协议（await_waiting ∈ {0,1}，原子访问）：
     *   - await 挂起：rt_task_register_waker 置 1 ——「我正依赖 outer 唤醒才续行」。
     *   - 父 await poll：见位（非 0）即返 RT_TASK_PENDING（绝不异地 resume）。
     *   - waker 触发：rt_task_coro_wake 先清 0 再投递 outer —— 此后外层 poll 才
     *     越过守卫，经 NOTIFIED 重 poll 闭合「wake 先于置位」竞态（恒 waker 驱动）。
     *   - 非 await 场景（首 poll 直接越守），见位先清 0 再推进（一次性守卫）。
     *   - autostart 不做 post-poll 重置位：PENDING 返回时位必为 1（挂起于
     *     await）或 0 且投递在途（coro_wake 已清位投递），重置位会吞投递
     *     致永挂（6b 修复，详见 rt_task.c rt_task_autostart）。 */
    uint32_t   await_waiting; /* M6.2: 暖启动守卫位（0/1；原子访问） */
    int32_t    int_result;    /* int 结果槽 */
    void*      ptr_result;    /* 指针结果槽（string/class/array） */
    int32_t    ptr_is_class;  /* RFC 009 §结果所有权：ptr_result 是否为 ArcHeader class。
                               * 1=class（task 强持有 +1，release 统一 dec）；
                               * 0=string/array/Task/Func（无 ArcHeader，借用，release 不 dec）。 */
    void*      value_result;  /* 值类型结果槽（malloc'd copy） */
    int32_t    value_size;    /* value_result 字节数 */
    rt_resume_fn resume;      /* resume 函数（M2 状态机；NULL=已完成同步 Task） */
    void*      resume_data;   /* env 指针（M2 状态机字段） */
    rt_waker*  waker;         /* waker（M3 调度器填充）；free_list 复用此槽位 */
    rt_waker   _waker_slot;   /* M3: 内嵌 waker 槽（避免堆分配 binding） */
    int32_t    from_slab;     /* M5.2: 1=slab 分配，0=malloc 分配；waker 复用为 next_free */
    void (*dtor_fn)(void* env); /* RFC 009 M3: env 析构函数指针（codegen 设置）；
                                     NULL 表示无 spilled locals，直接 free(env) */
    struct RtTask* follower_head; /* RFC 008: 扇出链头（TCS get_Task 副本挂此；
                                     leader 完成时级联传播结果并唤醒） */
    struct RtTask* follower_next; /* follower 链表 next（自身作为 follower 时用） */
    /* 丢失唤醒取证：活任务双向注册表（临时，定位后随诊断计数器整体回收）。
     * alloc 登记 / slab_free 摘除；普查遍历统计 (status, bit, waker) 形态分布。 */
    struct RtTask* diag_prev;
    struct RtTask* diag_next;
    uint8_t diag_linked;   /* 取证：任务当前在活任务注册表链上（幂等 unregister 用） */
    uint8_t diag_freed;    /* 取证：任务已归还内存池（double-free 检测；复用时清零） */
};

/* ---- Task ABI (RFC 015 Phase A + RFC 009 M1/M2/M3 扩展) ----
 * Phase A 同步占位：rt_task_poll 内联调用 resume，无真实调度。
 * RFC 009 M1 扩展：泛型结果提取（ptr/value）、取消标记、状态查询、
 *                  状态机 env 句柄（M2 消费）、waker 注册（M3 消费）。
 * RFC 009 M2 升级：resume 签名 int32_t (*)(void* env, rt_waker* waker)，
 *                  返回新 status；rt_task_poll 用返回值更新 Task 状态。
 * RFC 009 M3 扩展：rt_task_complete（完成通知+触发 waker）、
 *                  rt_task_register_waker（默认 waker 注册）、rt_task_delay。 */

/* 已有（Phase A） */
RtTask* rt_task_alloc(void);
void*   rt_task_from_int(int32_t value);       /* int 结果 → 已完成 Task */
void*   rt_task_void(void);                    /* void 结果 → 已完成 Task */
int32_t rt_task_poll(void* state);             /* 推进状态机；返回 RT_TASK_* */
int32_t rt_task_result_int(void* state);       /* 读取 int 结果 */

/* RFC 009 M5.2：Task 释放统一入口。
 * 先释放 value_result/resume_data，再将 Task 推入当前线程 slab free_list
 * （受 RT_TASK_SLAB_CAP 限制；超 cap 则 free）。稳态零 malloc 关键路径。
 * 调用方需确保 Task 已完成且不再被引用。 */
void    rt_task_release(void* state);

/* RFC 009 M1 新增 */
void*   rt_task_from_ptr(void* value);         /* 指针结果（string/class/array）→ 已完成 Task */
void*   rt_task_from_class(void* value);       /* 强持有：class 结果专用（置 ptr_is_class=1；调用方先 inc 授予 +1） */
void*   rt_task_from_value(void* data, int32_t size); /* 值类型结果（struct/double/Vector）→ 已完成 Task */

int32_t rt_task_status(void* state);           /* 仅查询状态，不推进（vs poll 推进） */
void*   rt_task_result_ptr(void* state);       /* 读取指针结果 */
void    rt_task_result_value(void* state, void* dst, int32_t size); /* 读取值类型结果到 dst */

void    rt_task_cancel(void* state);           /* 标记取消：状态 → CANCELED */
int32_t rt_task_is_canceled(void* state);      /* 查询取消标志 */

/* 状态机句柄（M2 消费）：env + resume_fn → Pending Task。
 * resume_fn 签名：int32_t (*)(void* env, rt_waker* waker) → 新 status。 */
void*   rt_task_from_state_machine(void* env, void* resume_fn);

/* I2：协程 Task ABI（CoroSplit 单帧所有权）。codegen 直线体 async 走 LLVM
 * 协程路径时以单次 ABI 调用创建 Task——运行时直接持帧所有权：resume_data=帧、
 * resume=thunk（桥接 rt_resume_fn）、dtor_fn=destroy thunk（coro.destroy 释放帧）。
 * 与通用 rt_task_from_state_machine 解耦（去除「先建再 set_dtor_fn」两步间接），
 * 使 coro 路径不复依赖旧状态机路径（plan.md 阶段 3 I3 删该路径的前置）。 */
void*   rt_task_from_coroutine(void* frame, void* resume_fn, void (*destroy_fn)(void* frame));

/* RFC 008 AsyncStream：TaskCompletionSource 支撑。 */
void*   rt_task_create_pending(void);          /* PENDING 态 Task（外部事件完成） */
void    rt_task_adopt(void* dst, void* src);   /* 已完成源结果转移到待完成目标并唤醒 */
void    rt_task_add_follower(void* leader, void* follower); /* get_Task 扇出：leader 完成时级联传播结果到 follower（leader 已完成则立即同步） */
/* RFC 009 M3：为状态机 Task 设置 env 析构函数指针（codegen 在 ctor 中调用）。
 * dtor_fn = NULL（默认）表示 env 无 spilled locals，rt_task_release 直接 free(env)。
 * dtor_fn != NULL 时，rt_task_release 调用 dtor_fn(env) 释放 spilled 指针 + free(env)。 */
void    rt_task_set_dtor_fn(void* state, void (*dtor_fn)(void* env));
/* M2: resume 完成时将 result 写入 Task 句柄（由 codegen 生成的 resume 调用）。 */
void    rt_task_set_result_int(void* state, int32_t value);
void    rt_task_set_result_ptr(void* state, void* value);
void    rt_task_set_result_class(void* state, void* value); /* 强持有：class 结果专用（置 ptr_is_class=1） */
void    rt_task_set_result_value(void* state, void* data, int32_t size);
/* Waker 注册（M3 消费）：调度器为 Pending Task 注册唤醒回调 */
void    rt_task_set_waker(void* state, rt_waker* waker);
/* waker 交接自旋锁（case2/case8 丢失唤醒根治）：waker 安装/快照与 status 终态
 * 迁移的临界区互斥。使用协议——安装侧：锁内 {读 status；终态则回收 waker 不装，
 * 否则安装}；完成侧（complete/fault）：锁内 {snapshot waker + 终态 store}。
 * 两侧互斥保证 register/complete 任一交错均不丢唤醒。rt_task.c 定义。 */
void    rt_task_wk_lock(struct RtTask* t);
void    rt_task_wk_unlock(struct RtTask* t);

void    rt_waker_wake(rt_waker* waker);        /* M1 占位；M3 实现真实唤醒 */

/* RFC 009 M3 新增 */
void    rt_task_complete(void* state);         /* 标记 READY + 触发 waker（定时器/IO 完成时调用） */
void    rt_task_fault(void* state, void* exception); /* 标记 FAULTED + 存异常 + 触发 waker（异步边界捕获异常后调用） */
void    rt_task_register_waker(void* inner, void* outer); /* 注册默认 waker：wake 时将 outer 移入就绪队列 */
/* M6.2 协程暖启动：
 *   rt_task_autostart   : 异步函数调用点发射——首 poll 驱动 body 同步前缀至首个未完成
 *                         await，打破「create N → await each」串行化（对标 .NET async
 *                         同步前缀）。暖启动后 Task 以 PENDING 暴露，由 await_waiting
 *                         守卫阻止父 await 越界 resume。
 *   rt_task_coro_wake   : 协程 waker 专用——先清 await_waiting 再投递 outer（闭合
 *                         「wake 先于置位」竞态）。经 rt_task_register_waker 挂为
 *                         `_waker_slot` 的 wake 回调（与默认 dispatch 的差异仅在清位）。 */
void    rt_task_autostart(void* state);        /* 协程暖启动：首 poll 推进 body（一次性，不做 post-poll 置位） */
void*   rt_task_delay(int32_t milliseconds);   /* 创建 Delay Task（Pending + 定时器） */

/* M5.7: Wait / WaitAll / WaitAny / FromCanceled 新增 */
int32_t rt_task_wait_timeout(void* state, int32_t timeout_ms); /* 同步轮询等待 Task 完成，带超时（0=无限）。返回 1=完成，0=超时，-1=取消 */
int32_t rt_task_wait_ct(void* state, void* ct);                 /* 同步轮询等待 Task 完成，可被 ct 中断。返回 1=完成，0=中断 */
void    rt_task_wait_all(void** tasks, int32_t count);          /* 同步阻塞等待全部 Task 完成 */
int32_t rt_task_wait_any(void** tasks, int32_t count);          /* 同步阻塞等待任一 Task 完成，返回索引 */
void*   rt_task_from_canceled(void);                             /* 创建已取消的 Task（CANCELED，无结果） */
void*   rt_task_from_exception(void* exception);                 /* 创建已失败的 Task（FAULTED + 异常指针） */
int32_t rt_task_is_faulted(void* state);                         /* 查询 Status == FAULTED */
void*   rt_task_get_exception(void* state);                      /* FAULTED 时返回异常对象；否则 null */

/* M5.7 Async: File.*Async 线程池包装（Task 返回） */
void*   rt_file_read_all_text_async(const char* path);                     /* → Task* (ptr result) */
void*   rt_file_write_all_text_async(const char* path, const char* content); /* → Task* (int result) */
void*   rt_file_append_all_text_async(const char* path, const char* content); /* → Task* (int result) */
void*   rt_file_copy_async(const char* src, const char* dst);               /* → Task* (int result) */
void*   rt_file_move_async(const char* src, const char* dst);               /* → Task* (int result) */

/* IO Async 补全（AI 领域异步优先）：File/Directory 线程池包装（Task 返回） */
void*   rt_file_read_all_lines_async(const char* path);                    /* → Task* (string[] result) */
void*   rt_file_read_all_bytes_async(const char* path);                    /* → Task* (byte[] result) */
void*   rt_file_write_all_bytes_async(const char* path, void* bytes);      /* → Task* (int result) */
void*   rt_file_delete_async(const char* path);                            /* → Task* (int result) */
void*   rt_file_exists_async(const char* path);                            /* → Task* (int result) */
void*   rt_dir_create_async(const char* path);                             /* → Task* (int result) */
void*   rt_dir_exists_async(const char* path);                             /* → Task* (int result) */
void*   rt_dir_delete_async(const char* path);                             /* → Task* (int result) */
void*   rt_dir_list_files_async(const char* path);                         /* → Task* (string[] result) */
void*   rt_dir_list_files_pattern_async(const char* path, const char* search_pattern); /* → Task* (string[] result) */
void*   rt_dir_list_dirs_async(const char* path);                          /* → Task* (string[] result) */

/* M3: 全局 waker 回调函数指针。由 rt_event_loop.c 在 create 时设置。
 * rt_task_register_waker 用此指针设置 Task 的 waker.wake 回调。
 * 设计原因：rt_task.c 不能直接引用 rt_event_loop.c 的 static 函数，
 * 通过全局函数指针解耦，同时避免堆分配 waker binding。 */
typedef void (*rt_waker_fn_ptr)(void* data);
extern rt_waker_fn_ptr g_rt_wake_fn;

/* ---- Task Slab ABI (RFC 009 M5.2) ----
 * Per-worker 无锁 free-list，消除 Task 创建/释放热路径 malloc。
 * 调用方在线程入口/出口调用 thread_init/destroy；Task 创建走 rt_task_alloc
 * 自动优先使用 slab；释放走 rt_task_release（统一推入 free_list，超 cap 则 free）。 */
void    rt_task_slab_thread_init(void);   /* worker/main 线程入口调用 */
void    rt_task_slab_thread_destroy(void); /* 线程出口调用 */
RtTask* rt_task_slab_alloc(void);          /* 优先 free_list pop，空则 malloc fallback */
void    rt_task_slab_free(RtTask* t);      /* 推入 free_list（受 RT_TASK_SLAB_CAP 限制） */
int32_t rt_task_slab_free_count(void);     /* 监控：free_list 当前节点数 */
int32_t rt_task_slab_in_use(void);         /* 监控：当前线程已分配未释放数 */
uint64_t rt_diag_live_count(void);         /* 取证：注册表 register/unregister 净计数（对账链断） */
int32_t rt_diag_pool_count(void);          /* 取证：全局池 free 节点数 */
int32_t rt_task_slab_total_alloc(void);    /* 监控：累计 malloc fallback 次数 */

/* ---- Timer Wheel ABI (RFC 009 M5.3) ----
 * 3 级分级时间轮（1ms/256ms/16s 精度），替代 M3 有序单链表 O(n) 定时器。
 * 插入/到期均 O(1)；EventLoop 委托 add/tick/next_timeout 给时间轮。 */

/* 定时器节点（EventLoop + TimerWheel 共享）。
 * 与 M3 RtTimer 完全兼容（同字段布局），M5.3 提升为 ABI 公共类型。 */
typedef struct rt_timer_node {
    uint64_t              deadline_ms;  /* 到期时间（ms） */
    void                (*fn)(void*);   /* 到期回调（M4 通用化） */
    void*                 data;         /* 回调数据（Task* 或 CTS*） */
    int32_t               canceled;     /* 1=已取消（惰性删除） */
    struct rt_timer_node* next;         /* 同槽链表 */
} rt_timer_node;

typedef struct rt_timer_wheel rt_timer_wheel;

rt_timer_wheel* rt_timer_wheel_create(void);
void            rt_timer_wheel_destroy(rt_timer_wheel* tw);
void            rt_timer_wheel_add(rt_timer_wheel* tw, rt_timer_node* node);
void            rt_timer_wheel_tick(rt_timer_wheel* tw, uint64_t now_ms);
uint64_t        rt_timer_wheel_next_timeout(rt_timer_wheel* tw);  /* 距下一到期 ms 数；UINT64_MAX=无 */
int32_t         rt_timer_wheel_count(rt_timer_wheel* tw);         /* 活跃定时器数 */

/* ---- EventLoop ABI (RFC 009 M3) ----
 * 单线程 EventLoop 调度器：就绪队列 + 定时器 + waker 唤醒。
 * main entry wrapper 创建 EventLoop → spawn root task → run。 */
void*   rt_event_loop_create(void);
void    rt_event_loop_destroy(void* loop);
void    rt_event_loop_run(void* loop);         /* 阻塞运行直到无 pending task */
void    rt_event_loop_stop(void* loop);
int32_t rt_event_loop_tick(void* loop);        /* 处理一轮就绪任务，返回处理数 */
void    rt_event_loop_pump(void* loop);        /* M4：单轮驱动（tick + fire_expired），供 busy-wait await 调用 */
void    rt_event_loop_spawn(void* loop, void* task); /* 加入就绪队列（线程安全） */
void    rt_event_loop_set_current(void* loop);
void*   rt_event_loop_current(void);
void    rt_event_loop_inc_pending(void* loop); /* 增加未完成 Task 计数 */
void    rt_event_loop_dec_pending(void* loop); /* 减少未完成 Task 计数 */
void    rt_event_loop_set_root(void* loop, void* task); /* 标记根任务：仅其对 pending_count 递减 */

/* RFC 009 M2: Reactor 集成 —— 绑定 IO 后端到 EventLoop。
 * 绑定后 tick 末尾调用 rt_reactor_poll（非阻塞）处理就绪 IO 事件；
 * run 用 reactor_poll 替代 condvar_wait 阻塞等待 IO 完成。
 * 传 NULL 解绑。线程安全。 */
void    rt_event_loop_set_reactor(void* loop, void* reactor);
void*   rt_event_loop_get_reactor(void* loop);

/* RFC 009 M6: 多线程 Executor —— 绑定线程池作为续体执行器。
 * 绑定后：
 *   - g_rt_wake_fn 重定向到线程池（wake → 投递 poll-task 工作项）
 *   - EventLoop 不再自 poll 就绪 Task，改为向线程池投递
 *   - EventLoop 线程仅驱动 Reactor(IO) + 定时器 + 退出检测
 * 传 NULL 解绑（回退单线程：EventLoop 自 poll）。线程安全（须在 run 前绑定）。 */
void    rt_event_loop_set_threadpool(void* loop, void* pool);

/* RFC 009 M4: 通用定时器回调注册。
 * delay_ms 后调用 fn(data)。用于 CTS.CancelAfter + 未来 IO 异步等。
 * 复用 EventLoop 的 RtTimer 链表；fn=NULL 时等价于 M3 的 task-only 语义。 */
void    rt_event_loop_schedule(void* loop, void(*fn)(void*), void* data, uint64_t delay_ms);

/* Closure ABI (RFC 008: Lambda Capture) */
typedef struct arc_closure {
    void* fn_ptr;  /* pointer to lifted lambda function */
    void* env;     /* pointer to capture environment struct, or NULL */
} arc_closure;

/* ---- RFC 016 M2: native callback TLS 回调表 ----
 * 有捕获 lambda 传给 native callback 参数时，codegen 在调用前把
 * arc_closure 指针存入当前线程的 TLS slot，trampoline 从 slot 取 closure
 * 再间接调用（qsort/sqlite3_exec 等 env-less C 回调）。静态 TLS 直接内嵌，
 * 线程启动零初始化。 */
#define RT_FFI_MAX_CALLBACK_SLOTS 16
void*  rt_ffi_get_callback(int32_t slot);
void   rt_ffi_set_callback(int32_t slot, void* closure);
void   rt_ffi_clear_callback(int32_t slot);

/* ---- CancellationTokenSource ABI (RFC 009 M4 + M5.4) ----
 * 协作式取消的控制端：atomic canceled 标志 + Treiber stack 回调 + CancelAfter 定时器。
 * CT（CancellationToken）与 CTS 共享同一 RtCts* 指针（只读视图）。
 *
 * M4：mutex + 链表（已被 M5.4 取代）。
 * M5.4：无锁 Treiber stack（LIFO CAS push）+ atomic flag 取消检查。
 *       M4 ABI 保留为兼容入口，内部委托 M5.4 无锁实现。 */
typedef struct RtCts RtCts;

/* M5.4：Treiber stack 节点。调用方填充 cb/data 后传给 rt_cts_register_lf。
 * 节点池化（slab）避免 Register 热路径 malloc；生命周期与 CTS 绑定消除 ABA。 */
typedef struct rt_cts_node {
    void                  (*cb)(void*);   /* 回调函数 */
    void*                  data;          /* 回调数据 */
    struct rt_cts_node*    next;          /* Treiber stack 链 */
    _Atomic(int32_t)       registered;    /* 1=已注册未触发, 0=已触发/已取消 */
    int32_t                _pad[13];      /* cache line 对齐 */
} rt_cts_node;

/* M4 ABI（兼容入口，内部委托 M5.4 无锁实现） */
void*   rt_cts_create(void);                                       /* 创建 CTS（canceled=0） */
int32_t rt_cts_is_canceled(void* cts);                             /* 查询 atomic canceled 标志 */
int32_t rt_cts_can_be_canceled(void* cts);                         /* .NET 语义：null=None 恒不可取消 */
void    rt_cts_cancel(void* cts);                                  /* 设 canceled=1 + 触发 stack 全部回调 */
void    rt_cts_register(void* cts, void(*fn)(void*), void* data);  /* 注册取消回调（已取消则立即调用） */
void    rt_cts_cancel_after(void* cts, int32_t ms);                /* 延迟 ms 后触发 cancel */
void    rt_cts_destroy(void* cts);                                 /* 释放 CTS + 剩余回调 */
void    rt_cts_throw_if_canceled(void* cts);                       /* ThrowIfCancellationRequested 封装：已取消则 rt_panic */
void    rt_cts_callback_trampoline(void* data);                    /* Arc closure → C 回调 trampoline（ct.Register 用） */

/* M5.4 无锁 ABI（直接 node 注册路径，高性能场景使用） */
void    rt_cts_register_lf(RtCts* cts, rt_cts_node* node);         /* Treiber push（无锁） */
void    rt_cts_node_try_fire(rt_cts_node* node);                   /* CAS 防重复触发单节点 */
rt_cts_node* rt_cts_node_alloc(void);                              /* 从 per-thread slab 取节点 */
void    rt_cts_node_free(rt_cts_node* node);                       /* 还回 slab */

/* ---- Work-Stealing Deque + ThreadPool ABI (RFC 009 M5.1) ----
 *
 * Chase-Lev lock-free work-stealing deque（owner LIFO + stealer FIFO）+
 * ThreadPoolScheduler（N workers + 全局注入队列 + park/wake）。
 *
 * rt_work_t 解耦 ThreadPool 与 Arc Task/RtTask 状态机：
 *   - fn  : 工作函数指针（任意 C 函数，签名 void(*)(void*)）
 *   - data: 工作参数（由 fn 解释，可为 NULL）
 * ThreadPool 调度时不感知 Arc 语义；上层（M5.2 Task 池化）将 RtTask*
 * 包装为 rt_work_t 后投递，下层执行时 fn 回调推进 Task 状态机。
 *
 * ABI 边界：
 *   - rt_threadpool_spawn       : 外部线程投递（进全局队列）
 *   - rt_threadpool_spawn_local : worker 自投递（进 own deque，LIFO 缓存友好）
 *   - rt_threadpool_worker_id   : TLS 查询当前 worker 编号（-1=非 worker 线程）
 *   - rt_threadpool_wait_idle   : 测试用阻塞等待 pending=0（M5.1 MVP，M5.2 改 condvar） */
typedef struct rt_ws_deque     rt_ws_deque;
typedef struct rt_threadpool   rt_threadpool;

/* RFC 009 M1: per-worker 上下文前向声明，供 rt_event_loop.c 等查询当前 worker。
 * 完整定义在下方（rt_preempt.c 等独立翻译单元需访问 preempt 字段）。 */
typedef struct rt_worker_ctx   rt_worker_ctx;

typedef struct rt_work {
    void  (*fn)(void* data);
    void*   data;
} rt_work_t;

/* RFC 009 §3：rt_work_node —— 统一 work 节点类型（消除 rt_rw_pool，单 work_pool 分配）。
 *
 * 原设计：rt_task_run_work（rt_rw_pool）+ rt_work_node（work_pool）双池分配，
 * 每次 Task.Run 经历 3 次 CAS（slab + rt_rw_pool + work_pool）。.NET Task.Run 仅 1 次
 * GC bump pointer 分配。双池 CAS 是 Arc task_spawn_wait 慢于 .NET ~2.5× 的主因之一。
 *
 * 统一方案：rt_work_node 扩展字段（task/action_fn/action_data/ct），rt_task_run 直接
 * 从 work_pool 分配 node，消除 rt_rw_pool。普通 work 不使用扩展字段，但 node 大小
 * 统一（56B），work_pool 不区分类型。
 *
 * 生命周期（关键变更）：
 *   - spawn 侧（rt_task_run）：work_node_alloc → 设置 work + 扩展字段 → spawn_node
 *   - worker 侧：pop node → 执行 work.fn(work.data) → work_pool_push(node)
 *   - **worker 在 work.fn 执行后 push node**（原设计在执行前 push，若 work.data
 *     指向 node 内部则 UAF；统一后 rt_task_run 的 work.data = node，必须延迟 push）
 *
 * 布局兼容：rt_task_run_work 不再单独定义，直接用 rt_work_node 的扩展字段。 */
typedef struct rt_work_node rt_work_node;
struct rt_work_node {
    rt_work_t                      work;          /* fn + data（16B） */
    _Atomic(struct rt_work_node*)  next;          /* Treiber stack next（8B） */
    /* Task.Run 扩展字段（仅当 work.fn 是 trampoline 时有效；普通 work 不使用） */
    void*                          task;          /* RtTask* 句柄 */
    void                         (*action_fn)(void* data);  /* 用户 Action fn */
    void*                          action_data;   /* 用户委托 data */
    void*                          ct;            /* CancellationToken */
};

/* 预分配 node 的 spawn（rt_task_run 路径，消除内部 work_node_alloc）。
 * node 须来自 rt_work_node_alloc；worker 执行完后自动回收。 */
void           rt_threadpool_spawn_node(rt_threadpool* pool, rt_work_node* node);
void           rt_threadpool_spawn_node_batched(rt_threadpool* pool, rt_work_node* node);
void           rt_threadpool_flush_local(void);
rt_work_node*  rt_work_node_alloc(rt_threadpool* pool);
void           rt_work_node_recycle(rt_threadpool* pool, rt_work_node* node);

rt_ws_deque* rt_ws_deque_create(int32_t worker_id, int32_t cap_log2);
void         rt_ws_deque_destroy(rt_ws_deque* q);
void         rt_ws_push(rt_ws_deque* q, void* item);    /* owner 推入；deque 满时走 overflow handler */
void*        rt_ws_pop(rt_ws_deque* q);                  /* owner 弹出（LIFO）；空返回 NULL */
void*        rt_ws_steal(rt_ws_deque* q);                /* stealer 偷取（FIFO，CAS）；空返回 NULL */
int32_t      rt_ws_deque_size(rt_ws_deque* q);
int32_t      rt_ws_deque_worker_id(rt_ws_deque* q);
void         rt_ws_deque_set_overflow_handler(void (*handler)(void* ctx, void* item)); /* handler 全局；ctx 见 set_overflow_ctx */
void         rt_ws_deque_set_overflow_ctx(rt_ws_deque* q, void* ctx);                 /* per-deque 池指针，防多池 UAF */

rt_threadpool* rt_threadpool_create(int32_t n_workers, int32_t numa_aware); /* n_workers<=0 → hardware_concurrency；numa_aware=1 启用绑定（单 node 为 no-op） */
void           rt_threadpool_destroy(rt_threadpool* pool);                  /* safe destroy：wait_idle + join(跳过已 Shutdown) + free；禁止重复 */
void           rt_threadpool_spawn(rt_threadpool* pool, rt_work_t work);       /* 外部 → 全局队列 */
void           rt_threadpool_spawn_local(rt_threadpool* pool, rt_work_t work); /* worker → own deque（M1 优先 push LIFO slot） */
int32_t        rt_threadpool_worker_id(void);                                  /* TLS：当前 worker 编号；非 worker 返回 -1 */
int32_t        rt_threadpool_pending_count(rt_threadpool* pool);
int32_t        rt_threadpool_worker_count(rt_threadpool* pool);                /* 池中 worker 数（用于 Parallel.For 分区估算） */
void           rt_threadpool_wait_idle(rt_threadpool* pool);                   /* 测试用：忙等 pending=0 */
void           rt_threadpool_shutdown(rt_threadpool* pool);                    /* 关闭，等待所有 worker 完成 */

/* RFC 009 M1: LIFO slot 优化 ABI —— per-worker 单 slot 缓存提升缓存局部性，
 * continuation 派发延迟从 ~30ns 降至 <15ns（vs Tokio 15ns / C# 120ns）。
 *
 * 设计要点：
 *   - lifo_slot 仅 owner worker 可读写（atomic_exchange 无锁）
 *   - LIFO slot 已占用时 push 自动溢出到本地 deque
 *   - needs_wakeup 快路径：LIFO slot 空时才 signal（避免无效唤醒）
 *   - 不可偷：stealer 仅偷本地 deque 的 FIFO 端，LIFO slot 不参与 steal
 *
 * 完整背景见 RFC 009 §2.2 LIFO Slot 设计。 */
rt_worker_ctx* rt_threadpool_current_worker_ctx(void);   /* TLS 查询当前 worker；非 worker 返回 NULL */
void           rt_worker_push_lifo(rt_worker_ctx* w, rt_work_t work); /* push LIFO slot；已占用则溢出到 deque */
int32_t        rt_worker_needs_wakeup(rt_worker_ctx* w); /* 1=LIFO slot 空，需 signal；0=已有活干 */
void           rt_worker_mark_parked(rt_worker_ctx* w);  /* 标记 worker 进入 park（is_parked=1） */
void           rt_worker_mark_unparked(rt_worker_ctx* w); /* 标记 worker 唤醒（is_parked=0） */

/* RFC 009 M2: 异步抢占 ABI。
 *
 * 抢占状态结构嵌入 rt_worker_ctx（见 rt_threadpool.c 内部定义）。
 * 公共 ABI 通过 rt_worker_ctx* 间接操作 preempt 字段。
 *
 * 平台：
 *   - Linux: SIGURG（Go 1.14 方案；`rt_preempt.c` 现仅 __linux__ 启用，
 *     macOS/BSD 定义 SIGURG 但无 sigqueue(2)，走降级——平台审计 S2 #5）
 *   - Windows: QueueUserAPC
 *   - 降级：rt_preempt_is_supported() == 0 时协作式
 *
 * 现状注记（平台审计 S2 #6）：**注入侧尚未接线**——rt_preempt_signal_impl /
 * rt_preempt_init 当前零调用方（定时器轮未触发），await 点的
 * rt_worker_preempt_check/clear 面已由 codegen 发射；实际语义为协作式
 * 检测钩子预留。宣称的「1ms 定时抢占」须在调度器接线并协议验证后生效。
 *
 * 设计要点：
 *   - rt_preempt_check: await 边界主动检查（codegen 生成在 __async_resume_* 内）
 *   - rt_preempt_was_triggered: rt_task_poll 外层查询（与 check 分离避免误清）
 *   - rt_preempt_clear: 消费抢占标志
 *   - rt_preempt_record_start: 记录当前 task 开始执行时间（worker 主循环调用）
 *   - rt_preempt_signal_impl: 发送信号/APC（定时器轮 tick 调用，内部使用）
 *   - rt_preempt_is_supported: 查询平台能力（编译期常量，测试用） */
typedef struct rt_preempt_state {
    _Atomic(int32_t) preempt_requested;  /* 信号 handler 设置，await 点检查 */
    _Atomic(int64_t) exec_start_ms;      /* 当前 task 开始执行时间（ms） */
} rt_preempt_state;

/* RFC 009 M1/M2: per-worker 上下文。
 * 完整定义移入 rt_abi.h（原在 rt_threadpool.c），使 rt_preempt.c 等
 * 独立翻译单元可访问 preempt 字段（避免前向声明 incomplete type 错误）。
 * 注意：typedef 已在前方声明（line 1053），此处仅补全 struct 体。 */
struct rt_worker_ctx {
    int32_t             worker_id;
    rt_ws_deque*        deque;
    struct rt_threadpool* pool;
    _Atomic(void*)      lifo_slot;      /* M1: 单 slot 缓存（rt_work_node*，A5②） */
    _Atomic(int32_t)    is_parked;      /* M1+044: worker park 状态机（三态）：
                                           0=RUNNING（未 park）/ 1=PARKED（已 park 等 condvar，
                                           未被 notify）/ 2=NOTIFIED（已被 spawner notify，worker
                                           未醒）。参照 tokio notify 状态机：spawner CAS
                                           PARKED→NOTIFIED 成功才 signal，消除 redundant signal
                                           （burst spawn 场景 N 次 signal → 1 次）。 */
    void*               park_sync;      /* RFC 009 M5（2026-08-03）：per-worker park 同步原语
                                           （rt_sync_t*，由 rt_threadpool_create 分配）。
                                           每个 worker 在**自己的** condvar 上等待；spawner 只
                                           signal 目标 worker → 无 thundering herd（原先 pool 级
                                           broadcast 每次 spawn 唤醒全部 worker，task_spawn_wait
                                           4  worker 场景 3 个空醒 + 重 park ≈ 2200ns/op）。 */
    rt_preempt_state    preempt;        /* M2: 异步抢占状态 */
    int32_t             numa_node;      /* M5: 绑定的 NUMA node（-1=未启用） */
    int32_t             pending_batch;  /* pending_count 批量递减累积器（start-based）：
                                           每个任务开始执行前累积 1，链表末节点执行前批量
                                           fetch_sub 落账（消除每任务 fetch_sub 的 cache
                                           line 争用）。单线程读写（仅 owner worker），
                                           无需原子。 */
    _Atomic(int32_t)    busy;           /* 1=正在执行 work。wait_idle 以「pending=0 且全部
                                           worker busy=0 且无排队未取」判定空闲——busy 闭合
                                           「递减先于完成落账」后的执行窗口（见 rt_run_work_list
                                           start-based 语义）。 */
};

void    rt_preempt_init(void);                                            /* 注册 SIGURG handler（主线程初始化） */
int32_t rt_preempt_check(rt_preempt_state* s);                            /* await 边界检查；1=需抢占 */
int32_t rt_preempt_was_triggered(rt_preempt_state* s);                    /* rt_task_poll 外层查询；1=抢占触发 PENDING */
void    rt_preempt_clear(rt_preempt_state* s);                            /* 清除抢占标志 */
void    rt_preempt_record_start(rt_preempt_state* s, int64_t now_ms);     /* 记录当前 task 执行开始时间 */
void    rt_preempt_signal_impl(_Atomic(int32_t)* preempt_requested);      /* 发送抢占信号（内部使用） */
int32_t rt_preempt_is_supported(void);                                    /* 1=平台支持抢占，0=降级协作式 */
int32_t rt_worker_preempt_check(rt_worker_ctx* w);                        /* worker ctx 便捷封装：await 边界检查（codegen 调用） */
void    rt_worker_preempt_clear(rt_worker_ctx* w);                        /* worker ctx 便捷封装：清除抢占标志（codegen 调用） */

/* Task.Run ABI：将 Action 调度到线程池并返回 Task 句柄。
 *   1. rt_task_run(action_fn, action_data) → 默认线程池，返回 Task*
 *   2. rt_task_run_on_pool(pool, action_fn, action_data) → 指定池，返回 Task* */
void*          rt_task_run(void* action_fn, void* action_data);
void*          rt_task_run_on_pool(void* pool, void* action_fn, void* action_data);
void*          rt_task_run_func(void* func_fn, void* func_data);             /* Run<T>(Func<T>) */
void*          rt_task_run_func_ct(void* func_fn, void* func_data, void* ct); /* Run<T>(Func<T>, CT) */
/** Join 默认 Task.Run 池 worker；Join 未收尾 Thread。不 free 池结构 / 不回收 trampoline w。 */
void           rt_default_pool_shutdown(void);
/** Join 所有仍存活、未 Join 的 Thread（报告前由 ShutdownDefaultPool 调用）。 */
void           rt_thread_join_live(void);

/* Parallel.For ABI (RFC 009 M2 / RFC 009 M5.7)：
 * 将 [from, to) 分区，分发到 ThreadPool 并行执行 body(i, env)。
 * pool=NULL 使用默认线程池；cts=NULL 不启用取消。
 * max_degree<=0 表示不限制并发度。返回值 = 分区数。 */
int32_t        rt_parallel_for(int32_t from, int32_t to,
                               void (*body)(int32_t i, void* env),
                               void* env,
                               rt_threadpool* pool,
                               void* cts,
                               int32_t max_degree);

/* RFC 009 M7 优化：Parallel.For 局部累加 + 最终合并模式。
 *
 * 解决纯 atomic_fetch_add 工作负载的负扩展问题：每分区维护独立 local 累加器，
 * 避免跨 worker 原子竞争；分区完成后 merge_local 将所有 local 合并到 result。
 *
 * 典型用法（并行求和）：
 *   void body_sum(int32_t i, void* local, void* env) {
 *       int64_t* sum = (int64_t*)local;
 *       *sum += i;  // 无原子，无竞争
 *   }
 *   void init_zero(void* local) { *(int64_t*)local = 0; }
 *   void merge_add(void* dst, const void* src) {
 *       *(int64_t*)dst += *(const int64_t*)src;
 *   }
 *   int64_t total = 0;
 *   rt_parallel_for_reduce(0, N, body_sum, init_zero, merge_add,
 *                          &total, sizeof(int64_t), NULL, pool, 0);
 *
 * 参数：
 *   local_size：local 累加器大小（字节）；runtime 按 local_size 分配每分区 local
 *   init_local：分区开始前初始化 local（如清零）
 *   merge_local：分区完成后将 local 合并到 result（最终单线程串行合并）
 *   result：最终合并结果（调用方预分配）
 *
 * 返回值 = 分区数。 */
typedef void (*rt_parallel_body_local)(int32_t i, void* local, void* env);
typedef void (*rt_parallel_local_init)(void* local);
typedef void (*rt_parallel_local_merge)(void* dst, const void* src);

int32_t        rt_parallel_for_reduce(int32_t from, int32_t to,
                                      rt_parallel_body_local body,
                                      rt_parallel_local_init init_local,
                                      rt_parallel_local_merge merge_local,
                                      void* result, size_t local_size,
                                      void* env,
                                      rt_threadpool* pool,
                                      int32_t max_degree);

/* RFC 009 M6：Parallel.ForEach —— 数组源并行遍历。
 *
 * 将 [0, array_len) 分区，分发到 ThreadPool 并行执行
 * body(i, (char*)array_ptr + i*elem_size, env)。
 * elem_size 从数组 header 自动读取（RtArrayHeader.elem_size），codegen 无需传递。
 *
 * 多平台说明：平台无关的分区调度；底层线程池/信号量由 rt_threadpool
 * 提供（Windows Semaphore / Linux macOS sem_t）。元素指针通过字节偏移
 * 计算，支持任意元素类型与对齐。
 *
 * 当前限制（M6 MVP）：body 传递元素指针（void*）。对引用类型完全正确；
 * 对值类型，codegen trampoline 需做类型感知 load——当前 MVP 仅传指针。
 *
 * 参数：
 *   array_ptr  - 数组首元素指针（payload，header 之后）
 *   array_len  - 元素数量
 *   body       - 回调函数 void(*)(int32_t index, void* elem_ptr, void* env)
 *   env        - 闭包环境指针
 *   pool       - 线程池；NULL 表示同步执行
 *   cts        - 取消令牌；NULL 不启用取消（MVP 忽略）
 *   max_degree - 最大并发度；<=0 不限制
 * 返回值 = 分区数。 */
int32_t        rt_parallel_foreach(void* array_ptr,
                                   int32_t array_len,
                                   void (*body)(int32_t index, void* elem_ptr, void* env),
                                   void* env,
                                   rt_threadpool* pool,
                                   void* cts,
                                   int32_t max_degree);

/* ---- SIMD 运行时检测 ABI (RFC 009 M3) ----
 *
 * 运行时查询当前 CPU 的 SIMD 能力，供 codegen auto-vectorization 路径
 * 选择最优指令集（AVX-512 → AVX2 → SSE2 → NEON → 标量降级）。
 *
 * 返回值在进程生命周期内不变（CPU 特性在启动时检测一次）。 */
int32_t rt_simd_width_bytes(void);   /* SIMD 寄存器宽度（字节）：64=AVX-512, 32=AVX2, 16=SSE2/NEON, 0=无 SIMD */
int32_t rt_simd_supports_fma(void);  /* 1=支持 FMA（乘加融合） */
int32_t rt_simd_supports_avx512(void); /* 1=支持 AVX-512 */
int32_t rt_simd_supports_gather(void); /* 1=支持 gather/scatter 指令 */

/* ---- NUMA 感知调度 ABI (RFC 009 M5) ----
 *
 * 查询 NUMA 拓扑 + worker 绑定 + NUMA 感知内存分配。
 * 不依赖 libnuma（D-1 决策：全部手写，100% 独立）。
 *
 * Linux:   sysfs + mbind/set_mempolicy（无 libnuma 依赖）
 * Windows: GetNumaHighestNodeNumber + SetThreadGroupAffinity + VirtualAllocExNuma
 * 降级：   非 NUMA 平台返回 node_count=1，bind_worker/alloc_on_node 为 no-op */
int32_t rt_numa_node_count(void);                                    /* NUMA node 数；非 NUMA 平台返回 1 */
int32_t rt_numa_cpu_to_node(int32_t cpu);                            /* CPU 逻辑核 → NUMA node 映射 */
void    rt_numa_bind_worker(int32_t worker_id, int32_t node);        /* 绑定 worker 线程到指定 NUMA node */
void*   rt_numa_alloc_on_node(uint64_t size, int32_t node);         /* 在指定 node 上分配内存（first-touch） */
void    rt_numa_free(void* ptr);                                     /* 释放 NUMA 感知分配的内存 */

/* ---- SoA 数据布局 ABI (RFC 009 M4) ----
 *
 * 为 [SoA] struct 数组提供 Structure-of-Arrays 内存布局：
 *   - rt_soa_array_create: 分配 SoA 描述符 + num_fields 个独立字段数组
 *   - rt_soa_field_ptr:    取第 field_idx 个字段数组首指针
 *   - rt_soa_length:       取元素数（兼容 arr.Length 语义）
 *   - rt_soa_free:         释放 SoA 数组（含所有字段数组）
 *
 * 内存布局（SoA 描述符）：
 *   { int32_t length; int32_t num_fields; void* field_arrays[num_fields]; }
 * 每个字段数组为连续存储（length × field_size 字节），cache-line 对齐（64B）。
 *
 * 多平台说明：平台无关的内存分配（malloc + memset）；无 SIMD/线程依赖。
 * 性能收益由 codegen 的 GEP 重排 + LLVM auto-vectorize 实现（平台相关）。
 *
 * 设计要点：
 *   - 字段数组按 field_index 顺序排列（typeck StructLayout.fields 顺序）
 *   - 字段大小由 codegen 传入（field_sizes[]），支持任意类型（double/int/ptr/...）
 *   - 释放时只需一次 rt_soa_free，内部遍历释放所有字段数组 */
typedef struct {
    int32_t length;       /* 元素数 N */
    int32_t num_fields;   /* 字段数 F */
    void**  field_arrays; /* [F] 个字段数组指针，每个数组 N × field_size[f] 字节 */
} rt_soa_array;

rt_soa_array* rt_soa_array_create(int32_t length, int32_t num_fields, const int32_t* field_sizes);
void*         rt_soa_field_ptr(rt_soa_array* arr, int32_t field_idx);
int32_t       rt_soa_length(rt_soa_array* arr);
void          rt_soa_free(rt_soa_array* arr);

/* ---- Thread + 同步原语 ABI (RFC 009 M5.5) ----
 *
 * 平台抽象：Thread / Mutex / Semaphore / Monitor。
 * Windows: CreateThread + CRITICAL_SECTION + Semaphore + CONDITION_VARIABLE
 * POSIX:   pthread_create + pthread_mutex_t + sem_t + pthread_cond_t
 *
 * Monitor 作用于 Lock 类实例（RFC 009 §7.2 D6 双轨设计：专用 Lock 类，
 * 非任意 object，避免为所有 class 实例追加 sync-block 头开销）。
 * lock 语句糖由 codegen 脱糖为 Monitor.Enter/Exit + try/finally。 */
typedef void (*rt_thread_fn)(void* data);

void*    rt_thread_create(rt_thread_fn fn, void* data);    /* 创建并启动线程；返回不透明句柄 */
void     rt_thread_join(void* thread);                     /* 等待线程结束 + 释放句柄 */
void     rt_thread_detach(void* thread);                   /* 分离线程（结束自动回收） */
void     rt_thread_sleep(uint64_t milliseconds);           /* 当前线程睡眠 */
void*    rt_thread_current(void);                          /* 当前线程句柄（不可 join/detach） */
int64_t  rt_thread_current_id(void);                       /* 当前线程 ID（调试用） */

void*    rt_mutex_create(void);                            /* 创建互斥锁 */
void     rt_mutex_lock(void* mutex);
int32_t  rt_mutex_try_lock(void* mutex);                   /* 1=成功, 0=已被占用 */
void     rt_mutex_unlock(void* mutex);
void     rt_mutex_destroy(void* mutex);

void*    rt_semaphore_create(int32_t initial, int32_t maximum);
void     rt_semaphore_wait(void* sem);
int32_t  rt_semaphore_wait_timeout(void* sem, uint64_t ms); /* 1=获取, 0=超时 */
void     rt_semaphore_release(void* sem);
void     rt_semaphore_release_n(void* sem, int32_t count); /* std P3: 批量释放 */
void     rt_semaphore_destroy(void* sem);

/* Monitor：作用于 Lock 类实例（rt_monitor_obj 内部布局） */
void     rt_monitor_enter(void* obj);
void     rt_monitor_exit(void* obj);
int32_t  rt_monitor_try_enter(void* obj);
void     rt_monitor_wait(void* obj);                        /* 释放锁 + 等待 Pulse */
void     rt_monitor_pulse(void* obj);                       /* 唤醒一个等待者 */
void     rt_monitor_pulse_all(void* obj);                   /* 唤醒所有等待者 */

/* Lock 类构造/析构：codegen 将 `new Lock()` 拦截为 rt_lock_create() */
void*    rt_lock_create(void);
void     rt_lock_destroy(void* obj);

/* RFC 006 A3 S2：`static readonly` 惰性初始化 guard（类级状态机）。
 * state 为 codegen 发射的类级全局 i32：0=未初始化，1=初始化中，2=已初始化。
 * 快速路径 rt_lazy_is_initialized 为单原子 acquire 读（对齐 C# beforefieldinit）；
 * 慢路径由 codegen 在 begin 与 commit 之间发射字段初始化器（store 到
 * @__static_<Class>_<field>），commit 以 release 发布 state=2（无部分可见）。
 * 零动态分配、零急切设置——无需运行时互斥量创建。 */
int32_t  rt_lazy_is_initialized(int32_t* state);   /* 快速路径：==2 → 1 */
int32_t  rt_lazy_init_begin(int32_t* state);       /* 1=本线程执行初始化；0=已由他线程完成（等待至就绪） */
void     rt_lazy_init_commit(int32_t* state);      /* release 发布 state=2 */

/* Thread 类 facade 支持（Arc 侧 Thread 类需延迟 Start 模型）：
 *   1. new Thread(action) → rt_thread_handle_create(fn, data) 存储 Action
 *   2. t.Start()          → rt_thread_handle_start(th) 创建 OS 线程
 *   3. t.Join()           → rt_thread_handle_join(th) 等待并释放 OS 句柄
 *   4. t.IsAlive          → rt_thread_handle_is_alive(th) */
void*    rt_thread_handle_create(rt_thread_fn fn, void* data);
void     rt_thread_handle_start(void* th);
void     rt_thread_handle_join(void* th);
int32_t  rt_thread_handle_is_alive(void* th);
void     rt_thread_handle_destroy(void* th);

/* ---- DI Container ABI (RFC 023 M1 / RFC 018 M5) ----
 * 编译期工厂生成 + 运行时描述符表分派，零反射（RFC 023 v2）。
 * ServiceCollection 收集描述符 → Build 固化为 ServiceProvider →
 * GetService 按 Type.TypeId（int，FNV-1a）查表调用工厂函数。
 *
 * 工厂函数签名 rt_di_factory_fn：接收 IServiceProvider* (void*)，返回服务实例 (void*)。
 * 方式 1（实现类型）：codegen 编译期生成 __di_factory_<TImpl>，内部 ctor 注入依赖
 *   （M5：工厂内联 RuntimeType + itable GetService(Type)，不再依赖 TypeId struct）。
 * 方式 2（工厂委托）：用户 lambda 编译为函数指针，直接传递。
 *
 * key 比较：M1 使用指针相等（string 字面量在 codegen 中为全局常量，相同字面量同址）。 */
typedef enum {
    RT_DI_SINGLETON = 0,
    RT_DI_SCOPED    = 1,
    RT_DI_TRANSIENT = 2,
} rt_di_lifetime;

typedef void* (*rt_di_factory_fn)(void* sp);

/* ServiceCollection: 收集服务描述符（构建期可变） */
void*   rt_di_collection_create(void);
void    rt_di_collection_add(void* collection, int32_t service_type, int32_t impl_type,
                             int32_t lifetime, void* key, rt_di_factory_fn factory);
void    rt_di_collection_destroy(void* collection);

/* ServiceProvider: 固化描述符表 + 解析服务（构建后不可变） */
void*   rt_di_provider_build(void* collection);
void*   rt_di_resolve(void* provider, int32_t service_type);
void*   rt_di_resolve_keyed(void* provider, int32_t service_type, void* key);
void    rt_di_provider_destroy(void* provider);

/* ServiceScope: Scoped 生命周期隔离（同一 scope 内 Scoped 实例缓存） */
void*   rt_di_scope_create(void* provider);
void*   rt_di_scope_resolve(void* scope, int32_t service_type);
void*   rt_di_scope_resolve_keyed(void* scope, int32_t service_type, void* key);
void    rt_di_scope_dispose(void* scope);

/* RFC 009 M4: Task.Delay(ms, ct) 取消传播 ABI。
 * 创建 Pending Delay Task + 定时器 + 注册 ct 取消回调。
 * ct 取消时 Delay Task 转 CANCELED + 触发 waker；定时器到期时转 READY。
 * ct=NULL 时等价于 rt_task_delay(ms)。 */
void*   rt_task_delay_ct(int32_t milliseconds, void* ct);

/* RFC 009 M4: WhenAll/WhenAny 真实异步组合子 ABI。
 * N 个 inner 共享 aggregator + per-inner waker binding。
 * WhenAll：所有 inner 完成后 outer 转 READY；WhenAny：首个完成的 inner 唤醒 outer。
 * count==0 或所有 inner 已完成 → outer 立即 READY。
 * tasks 是 Arc 数组 payload（已剥除 RtArrayHeader）。 */
void*   rt_task_when_all(void** tasks, int32_t count);
void*   rt_task_when_any(void** tasks, int32_t count);

/* ---- QIF (Quality Inspection Framework) ABI (RFC 032 M1) ----
 * 测试注册表 + Assert 断言 + 结果记录。
 *
 * 编译期测试发现（RFC 032 D2.2，Phase B2 GenerateTo 注入）生成调用
 * rt_qif_registry_add 的代码构造 rt_qif_registry 堆结构；Runner
 * （std/QIF/Runner.as，Phase B3）读取 registry 串行执行测试方法，
 * Assert 失败/异常通过结果记录族收集。
 *
 * M1 行为：Assert 失败 → rt_qif_record_result(failed) + rt_panic_at
 * 终止进程；Phase B3 引入 rt_qif_try_run setjmp/longjmp 捕获 panic
 * 使 Runner 可继续下一测试。
 *
 * 数据结构定义见 rt_qif.c（rt_qif_entry / rt_qif_registry /
 * rt_qif_result / rt_qif_result_set）；ABI 层仅暴露 void* 不透明句柄。
 */

/* registry 族：构造与查询注册表 */
void*       rt_qif_registry_create(void);
void        rt_qif_registry_add(void* registry, void* fn_ptr, const char* name,
                                int32_t kind, const char* class_name,
                                const void* data, int32_t data_len);
int32_t     rt_qif_registry_count(void* registry);
const void* rt_qif_registry_entry(void* registry, int32_t idx);
void        rt_qif_registry_destroy(void* registry);

/* Assert 族：失败即 record_result(failed) + rt_panic_at 终止 */
void rt_qif_assert_equal_int(int64_t expected, int64_t actual,
                             const char* file, int32_t line);
void rt_qif_assert_equal_str(const char* expected, const char* actual,
                             const char* file, int32_t line);
void rt_qif_assert_true(int32_t cond, const char* expr_str,
                        const char* file, int32_t line);
void rt_qif_assert_false(int32_t cond, const char* expr_str,
                         const char* file, int32_t line);
void rt_qif_assert_null(void* ptr, const char* file, int32_t line);
void rt_qif_assert_not_null(void* ptr, const char* file, int32_t line);
void rt_qif_fail(const char* msg, const char* file, int32_t line);
void rt_qif_skip(const char* reason, const char* file, int32_t line);

/* result 族：结果收集与输出（进程级全局结果集） */
void rt_qif_record_result(const char* name, int32_t kind, int32_t status,
                          double duration_ms, const char* error_msg);
void rt_qif_report_human(void);
void rt_qif_report_json(void);
void rt_qif_result_set_destroy(void);

/* panic 捕获边界（Phase B3 实现，M1 占位直接执行不捕获）。
 * fn 为被测方法函数指针（void(void*)），ctx 为 Runner 传递的上下文。
 * 返回 rt_qif_status 枚举值（0=passed, 1=failed, 2=skipped）。
 * Phase B3 实现 setjmp/longjmp 捕获 panic，M1 直接执行不捕获。 */
int32_t rt_qif_try_run(void (*fn)(void*), void* ctx, const char* name);

/* ---- QIFRuntime ABI (RFC 032 D3.3 v1.0, 2026-07-19 增补) ----
 * host 二进制（arc-test-host.exe）构造的 QIFRuntime 上下文。
 *
 * 设计原则（RFC 032 D3.3 v1.0）：
 *   - QIFRuntime 是 C struct 不透明句柄（对外暴露 QIFRuntime*）
 *   - 内部持有 rt_qif_registry* + 测试动态库句柄 + 统计计数器
 *   - rt_qif_register_test_case 转发 rt_qif_registry_add
 *   - rt_qif_run_all 串行调用 registry 中的测试方法函数指针
 *
 * 生命周期：
 *   1. host main: rt = rt_qif_runtime_create(test_lib_handle)
 *   2. host: __qif_init(rt) → 测试动态库注册测试用例
 *   3. host: rt_qif_run_all(rt) → 串行执行所有测试
 *   4. host: rt_qif_runtime_report(rt, format) → 输出报告
 *   5. host: rt_qif_runtime_destroy(rt)
 *
 * M1 行为约束：
 *   - 串行执行（不并行）
 *   - 不捕获 panic——测试方法 Assert 失败 → rt_panic 终止 host 进程
 *   - Phase B3 将引入 rt_qif_try_run 捕获 panic，允许继续下一测试
 */

typedef struct QIFRuntime QIFRuntime;

/* 构造 QIFRuntime 上下文。
 * test_lib_handle: rt_library_load 返回的测试动态库句柄（可为 NULL）。
 * 返回 NULL 表示 OOM。 */
QIFRuntime* rt_qif_runtime_create(void* test_lib_handle);

/* 销毁 QIFRuntime 上下文（不释放 test_lib_handle——由 host 负责 unload）。 */
void rt_qif_runtime_destroy(QIFRuntime* rt);

/* 注册测试用例（由 __qif_init 函数体调用，每个测试用例一次）。
 * 转发 rt_qif_registry_add——参数语义一致。 */
void rt_qif_register_test_case(QIFRuntime* rt, void* fn_ptr, const char* name,
                                int32_t kind, const char* class_name,
                                const void* data, int32_t data_len);

/* 查询已注册测试用例数。 */
int32_t rt_qif_runtime_test_count(QIFRuntime* rt);

/* 执行所有注册的测试用例（M1 串行执行，不捕获 panic）。
 * 返回 0=全部通过，1=有失败（M1 永远返回 0，因为 panic 终止进程；
 * Phase B3 捕获 panic 后才会返回 1）。 */
int32_t rt_qif_run_all(QIFRuntime* rt);

/* 输出测试报告。
 * format: 0=human, 1=json。 */
void rt_qif_runtime_report(QIFRuntime* rt, int32_t format);

/* 销毁进程级全局结果集（host 退出前调用）。 */
void rt_qif_runtime_cleanup(QIFRuntime* rt);

/* ---- Dynamic Library Loading ABI (RFC 017 D8 v1.0) ----
 * 动态库加载三 ABI：刻意最小化，仅 dlopen/dlsym/dlclose 对应物。
 *
 * 设计原则（RFC 017 D8 v1.0）：Arc 动态库对齐 C# 程序集（Assembly）模型——
 * 动态库 = 干净的库逻辑 + 引用链接信息，不引入 Rust 风格的复杂插件机制。
 * 框架无 plugin 概念，仅有「动态库」实体。删除的机制（v1.0）：
 *   - 通用插件契约（arc_plugin_register/arc_plugin_info）
 *   - capabilities 声明（能力系统由契约文件 .ani 独立管理，见 RFC 016）
 *   - 稳定 ABI 版本契约（abi="stable"/abi_version）
 *   - 热卸载（动态库生命周期 = 进程生命周期；进程退出 OS 自动回收，
 *     冷卸载 only——C# 同样废弃 AppDomain 卸载）
 *
 * 领域约定符号范式（RFC 017 D8.1 v1.0）：领域 host 通过 rt_library_sym 按
 * 名字查找领域约定符号（如 QIF 的 __qif_init），编译器核心零领域感知。
 *
 * 跨平台映射：
 *   - Linux / OHos：dlopen/dlsym/dlclose（libdl，需链接 -ldl）
 *   - macOS：dlopen/dlsym/dlclose（libc 内置，无需额外链接）
 *   - Windows：LoadLibraryA/GetProcAddress/FreeLibrary（kernel32 内置）
 *
 * 错误语义：失败返回 NULL；具体错误信息由平台 errno/GetLastError 维护，
 * v1.0 不引入错误消息提取 ABI（保持三 ABI 刻意最小化）。 */

void* rt_library_load(const char* path);              /* 加载动态库；失败返回 NULL */
void* rt_library_sym(void* handle, const char* name); /* 查找符号；失败返回 NULL */
void  rt_library_unload(void* handle);                /* 卸载动态库；NULL 安全 */

/* Assembly ABI (RFC 017 M1): 程序集执行上下文。
 * rt_assembly_set_executing 由 AssemblyLoadContext 在 Load 时设置，
 * rt_assembly_get_executing 由 codegen 拦截 GetExecutingAssembly() 时调用。 */
void  rt_assembly_set_executing(void* assembly_ptr);
void* rt_assembly_get_executing(void);              /* 返回当前执行的 Assembly*；NULL 表示未设置 */

/* RFC 017 M4: 读取动态库的包元数据。
 * 返回 "name\0version\0edition\0" 格式字符串；无元数据时返回 NULL。
 * 调用方无需释放返回指针——它指向已加载库的只读内存映射。 */
const char* rt_library_get_meta(void* handle);
/* RFC 017 M4 修复（additive ABI）：按索引读取包元数据字段。
 * 0=name、1=version、2=edition（未来依赖列表追加更高索引）；索引越界返回 NULL。
 * 返回指针指向库只读内存映射，调用方无需 free（同 rt_library_get_meta）。 */
const char* rt_library_get_meta_field(void* handle, int32_t index);

/* RFC 017 热卸载闭环：模块代数 + 跨模块引用登记 + 根扫描 + 在途调用计数。
 * 破坏性 ABI 扩展经 RFC 017 §0 立宪；Assembly.Dispose() 冷路径保留。 */
int32_t rt_library_unload_hot(void* handle);   /* 热卸载闭环：1=成功 0=悬挂拒载 -1=在途未收敛 -2=无效/已并发卸载 */
int32_t rt_library_generation(void* handle);   /* 查询模块代数；tombstone/未知句柄返回 0 */
int32_t rt_library_ref_register(int32_t generation);   /* 登记跨模块外部强引用（ledger++）；无效代数返回 0 */
int32_t rt_library_ref_unregister(int32_t generation); /* 释放跨模块外部强引用（ledger--）；已为 0 返回 0 */
int32_t rt_library_ref_count(int32_t generation);      /* 查询模块外部强引用计数（0 = 可卸载） */
int32_t rt_library_call_enter(int32_t generation);     /* 模块代码在途调用进入 +1；Freeze/失效代数返回 0 */
int32_t rt_library_call_leave(int32_t generation);     /* 模块代码在途调用返回 -1（下限 0） */
int32_t rt_library_root_add(int32_t generation, void* root);    /* 登记模块根（模块静态 class 引用） */
int32_t rt_library_root_remove(int32_t generation, void* root); /* 移除模块根 */
int32_t rt_library_root_scan(int32_t generation);      /* 根扫描：1=可卸载 0=外部引用非零/无效 */

/* RFC 017 §2.6: 模块边界弱登记表（宿主侧弱登记表）。
 * 宿主经 register 声明「Weak<T> 槽位指向本模块对象」；卸载路径中和已登记
 * 槽位（TryGet 卸载后返回 NULL）。Weak<T> 不阻止卸载。 */
int32_t rt_library_weak_register(int32_t generation, void* weakslot);   /* 登记 + 盖代数；1=成功 */
int32_t rt_library_weak_unregister(int32_t generation, void* weakslot); /* 显式解除；1=已解除 */
void    rt_library_weak_untrack(void* weakslot); /* 析构路径代数无关解除；幂等 no-op */

/* RFC 047（透明对象图迁移 · 热重载 L3）：
 * rt_arc_retype 将 obj 头中 vtable 指针重绑为 new_vtable（字段布局/vtable
 * 形状兼容性由调用方经 __arc_vtable_registry 双重判定先行保证）；refcount/
 * weakcount 不变、对象地址不变。非原子写——调用方保证迁移窗口处于 Freeze
 * （无并发访问）。返回 0=成功，-1=参数无效。 */
int32_t rt_arc_retype(void* obj, const void* new_vtable);
/* 读取 obj 头中当前 vtable 指针（迁移遍历的成员判定用；rodata 字面量等
 * 非 ARC 对象返回 NULL）。obj 为 NULL 返回 NULL。 */
const void* rt_arc_vtable_of(void* obj);
/* 透明对象图迁移：对 old_gen 模块代数的根可达闭包内、vtable 属于旧代
 * registry 的对象，按 new_gen registry 同名条目重绑（双重判定：slot_count
 * + shape_hash + layout_sig 全等；任一类型判定失败 → 整体拒绝，返回 -3）。
 * 迁移窗口须处于 Freeze（无在途调用）。返回迁移对象数；
 * -1=参数无效 -2=registry 缺失/形态非法 -3=存在不可迁移类型。 */
int32_t rt_library_migrate_instances(int32_t old_generation, int32_t new_generation);

/* Window + events (implemented in platform static library) */
void* rt_window_create(const char* title, int32_t width, int32_t height);
void rt_window_destroy(void* window);
int32_t rt_window_should_close(void* window);
void rt_window_invalidate(void* window);
int32_t rt_event_poll(void* window);
int32_t rt_event_wait(void* window, int32_t timeout_ms); /* 空闲阻塞等待：负超时=无限；1=有事件 */
void rt_window_close(void* window);
void rt_ui_wake_ui_thread(void); /* 跨线程唤醒 UI 泵（后台 Post 入队后调用） */

/* Parse ABI (RFC 007 M1): 数值类型字符串解析。
 * 对齐 C# int.Parse/TryParse 语义：支持正负号、前导/尾随空白、十进制格式。
 * TryParse 返回 1=成功, 0=失败；成功时写入 out 参数。
 * Parse 返回值或调用 rt_panic（失败时）。 */
int32_t rt_parse_int32(const char* s);
int32_t rt_parse_int32_try(const char* s, int32_t* out);
int64_t rt_parse_int64(const char* s);
int32_t rt_parse_int64_try(const char* s, int64_t* out);
double rt_parse_double(const char* s);
int32_t rt_parse_double_try(const char* s, double* out);
float rt_parse_float(const char* s);
int32_t rt_parse_float_try(const char* s, float* out);
int32_t rt_parse_bool(const char* s);
int32_t rt_parse_bool_try(const char* s, int32_t* out);
int32_t rt_parse_char(const char* s);
int32_t rt_parse_char_try(const char* s, int32_t* out);

/* Char classification/conversion ABI (P3-1b + P3-1c).
 * 对齐 C# System.Char 静态方法：IsDigit/IsLetter/IsWhiteSpace/IsUpper/IsLower +
 * ToUpper/ToLower。接受 int32_t 参数（Arc char 为 Unicode codepoint），
 * 返回 int32_t（分类方法返回 1/0，转换方法返回新 codepoint）。
 * 实现委托 C 标准库 <ctype.h>（ASCII 子集；扩展 Unicode 需后续升级）。 */
int32_t rt_char_is_digit(int32_t c);
int32_t rt_char_is_letter(int32_t c);
int32_t rt_char_is_white_space(int32_t c);
int32_t rt_char_is_upper(int32_t c);
int32_t rt_char_is_lower(int32_t c);
int32_t rt_char_to_upper(int32_t c);
int32_t rt_char_to_lower(int32_t c);

uint32_t rt_parse_uint32(const char* s);
int32_t rt_parse_uint32_try(const char* s, uint32_t* out);
uint64_t rt_parse_uint64(const char* s);
int32_t rt_parse_uint64_try(const char* s, uint64_t* out);

/* ToString ABI: 数值类型 → 字符串转换。
 * 返回 freshly malloc'd NUL-terminated string（调用方拥有所有权）。
 * 对齐 C# ToString() 语义。 */
char* rt_int_to_string(int32_t value);
char* rt_long_to_string(int64_t value);
char* rt_short_to_string(int16_t value);
char* rt_byte_to_string(int8_t value);
char* rt_float_to_string(float value);
char* rt_double_to_string(double value);
char* rt_bool_to_string(int32_t value);
char* rt_char_to_string(int32_t value);
char* rt_uint_to_string(uint32_t value);
char* rt_ulong_to_string(uint64_t value);
char* rt_ushort_to_string(uint16_t value);
char* rt_sbyte_to_string(int8_t value);

/* RFC 007 M2a: 带标准格式说明符的 ToString（D/X/x/F/G + 可选精度）。
 * format 非法时 rt_panic（插值路径由 typeck 先拦截）。 */
char* rt_int_to_string_fmt(int32_t value, const char* format);
char* rt_long_to_string_fmt(int64_t value, const char* format);
char* rt_short_to_string_fmt(int16_t value, const char* format);
char* rt_byte_to_string_fmt(uint8_t value, const char* format);
char* rt_sbyte_to_string_fmt(int8_t value, const char* format);
char* rt_uint_to_string_fmt(uint32_t value, const char* format);
char* rt_ulong_to_string_fmt(uint64_t value, const char* format);
char* rt_ushort_to_string_fmt(uint16_t value, const char* format);
char* rt_float_to_string_fmt(float value, const char* format);
char* rt_double_to_string_fmt(double value, const char* format);
/* RFC 027 M5: culture-aware ToString(format, provider) */
char* rt_int_to_string_fmt_p(int32_t value, const char* format, void* provider);
char* rt_long_to_string_fmt_p(int64_t value, const char* format, void* provider);
char* rt_short_to_string_fmt_p(int16_t value, const char* format, void* provider);
char* rt_byte_to_string_fmt_p(uint8_t value, const char* format, void* provider);
char* rt_sbyte_to_string_fmt_p(int8_t value, const char* format, void* provider);
char* rt_uint_to_string_fmt_p(uint32_t value, const char* format, void* provider);
char* rt_ulong_to_string_fmt_p(uint64_t value, const char* format, void* provider);
char* rt_ushort_to_string_fmt_p(uint16_t value, const char* format, void* provider);
char* rt_float_to_string_fmt_p(float value, const char* format, void* provider);
char* rt_double_to_string_fmt_p(double value, const char* format, void* provider);

/* ============================================================
 * Reactor ABI (RFC 009 M1) —— 统一 IO 多路复用抽象
 *
 * 跨平台 API 表面，后端可插拔：
 *   - Linux: io_uring（批量提交 + 零拷贝，主推）
 *   - Windows: IOCP（IO Completion Port）
 *   - macOS/FreeBSD: kqueue
 *   - 嵌入式回退: poll
 *
 * 设计原则（RFC 009 §0.4 + §4.1）：
 *   1. API 表面不变性——所有平台暴露相同 ABI
 *   2. 分层降级——无 io_uring 降级 epoll/poll，功能不丢
 *   3. 零分配热路径——Reactor 内部 SQE/CQE 环形缓冲预分配
 *   4. 批量化——N 个 IO 操作从 N syscalls 降为 1 syscall
 *
 * 与 EventLoop 集成（RFC 009 §4.3）：
 *   EventLoop tick 末尾调用 rt_reactor_poll 处理就绪 IO 事件，
 *   Reactor 不另起线程。
 * ============================================================ */

/* Reactor 事件类型（bitmask，rt_reactor_register 用） */
#define RT_REACTOR_READABLE   0x01u
#define RT_REACTOR_WRITABLE   0x02u
#define RT_REACTOR_ERROR      0x04u
#define RT_REACTOR_HANGUP     0x08u

/* Reactor 创建标志（bitmask，rt_reactor_create_with_flags 用） */
#define RT_REACTOR_FLAG_SQPOLL 0x01u  /* io_uring SQPOLL：内核轮询线程，opt-in（Linux only） */
#define RT_REACTOR_FLAG_LINK   0x02u  /* io_uring IOSQE_IO_LINK：链式操作，下一 SQE 自动链接 */

/* Reactor 完成事件（rt_reactor_poll 输出） */
typedef struct RtIoEvent {
    void*    user_data;   /* 提交时传入的 waker/task 关联 */
    int32_t  result;      /* 字节数 / -errno（<0 表示错误） */
    uint32_t flags;       /* 平台特定标志（如 io_uring cqe flags） */
    int32_t  fd;          /* 关联的 fd（IOCP 需要） */
} RtIoEvent;

/* Reactor 生命周期 */
void*   rt_reactor_create(void);
void*   rt_reactor_create_sqpoll(void); /* Linux io_uring SQPOLL：内核轮询线程，opt-in */
void    rt_reactor_destroy(void* reactor);

/* fd 注册（网络 socket / 文件 fd） */
int32_t rt_reactor_register(void* reactor, int32_t fd, uint32_t events);
int32_t rt_reactor_modify(void* reactor, int32_t fd, uint32_t events);
int32_t rt_reactor_unregister(void* reactor, int32_t fd);

/* 异步 IO 提交（批量 + 零拷贝） */
int32_t rt_reactor_submit_read(void* reactor, int32_t fd, void* buf,
                                uint32_t len, uint64_t offset, void* user_data);
int32_t rt_reactor_submit_write(void* reactor, int32_t fd, const void* buf,
                                 uint32_t len, uint64_t offset, void* user_data);
int32_t rt_reactor_submit_accept(void* reactor, int32_t listen_fd, void* user_data);
int32_t rt_reactor_submit_connect(void* reactor, int32_t fd,
                                   const void* addr, uint32_t addr_len, void* user_data);
int32_t rt_reactor_submit_timeout(void* reactor, uint64_t timeout_ns, void* user_data);

/* 链式操作控制（io_uring IOSQE_IO_LINK，RFC 009 M7） */
void    rt_reactor_set_link_flag(void* reactor, int32_t enable);

/* 批量刷新提交队列（io_uring_enter / IOCP post flush） */
int32_t rt_reactor_flush(void* reactor);

/* 轮询完成事件（timeout_ms=-1 阻塞，0 非阻塞，>0 等待 ms） */
int32_t rt_reactor_poll(void* reactor, RtIoEvent* events, int32_t max_events,
                         int32_t timeout_ms);

/* RFC 009 M6: 跨线程唤醒 —— 注入哨兵事件使阻塞中的 rt_reactor_poll 立即返回。
 * 用于多线程 executor：根任务完成时由 worker 线程唤醒 EventLoop 驱动线程，
 * 使其及时检查退出条件（消除「≤100ms 轮询兜底」延迟）。
 * 后端映射：IOCP=PostQueuedCompletionStatus(NULL)、kqueue=EVFILT_USER、
 *          io_uring/poll=eventfd/pipe（预留，当前 no-op 由超时兜底）。线程安全。 */
void    rt_reactor_wake(void* reactor);

/* 零拷贝缓冲池注册（io_uring_register_buffers / IOCP 模拟） */
int32_t rt_reactor_register_buffers(void* reactor, const void** buffers,
                                     const uint32_t* lengths, int32_t n);

/* 查询当前后端名称（"io_uring" / "iocp" / "kqueue" / "poll"） */
const char* rt_reactor_backend_name(void* reactor);

/* ============================================================
 * 零拷贝缓冲池 ABI (RFC 009 M3)
 *
 * user buffer 预注册到内核（io_uring_register_buffers），
 * 避免 per-IO 的 memcpy。IOCP/kqueue 后端降级为普通池化。
 * ============================================================ */
void*   rt_iobuf_pool_create(uint32_t buf_size, uint32_t buf_count);
void    rt_iobuf_pool_destroy(void* pool);
void*   rt_iobuf_pool_acquire(void* pool, uint32_t* out_len);
void    rt_iobuf_pool_release(void* pool, void* buf);
int32_t rt_iobuf_pool_register(void* pool, void* reactor);

/* 缓冲池内省 API（调试/统计/监控用，未列入稳定 ABI 但长期可用） */
int32_t  rt_iobuf_pool_free_count(void* pool);     /* 空闲 buffer 数（原子读） */
int32_t  rt_iobuf_pool_in_use_count(void* pool);   /* 借出未归还数（原子读） */
uint32_t rt_iobuf_pool_buf_size(void* pool);       /* 单 buffer 字节数 */
uint32_t rt_iobuf_pool_buf_count(void* pool);      /* 总 buffer 数 */

/* Socket ABI（RFC 009 M2）——跨平台 socket 原语（fd-based，Reactor 底层）。
 *
 * 与 RFC 025 M4 的 rt_socket_*（handle-based，facade 层）区分：
 *   - rt_socket_*：opaque handle 指针，面向 Arc 对象模型，codegen 拦截分发
 *   - rt_net_*：raw fd（int32_t），面向 Reactor 异步 IO，C 层直接调用
 *
 * 同步原语作为 Reactor 异步 IO 的基础：rt_net_create/bind/listen 建立
 * 监听端，rt_net_set_nonblocking 切换非阻塞模式后即可提交到 Reactor
 * （submit_accept/submit_connect/submit_read/submit_write）实现异步语义。
 *
 * 返回值约定：fd 类返回 >0 成功 / <0 失败（-errno）；操作类返回 0 成功 / <0 失败。
 *
 * 平台映射：
 *   - Windows：WinSock2（WSAStartup 引用计数 + socket/bind/listen/connect/closesocket）
 *   - POSIX：sys/socket.h + netinet/in.h + arpa/inet.h + netdb.h
 *
 * 参数枚举映射（与 std/Net/Sockets.as 对齐）：
 *   family: 0=InterNetwork(AF_INET), 1=InterNetworkV6(AF_INET6)
 *   type:   0=Stream(SOCK_STREAM), 1=Dgram(SOCK_DGRAM)
 *   proto:  0=Tcp(IPPROTO_TCP), 1=Udp(IPPROTO_UDP)
 */
int32_t rt_net_create(int32_t family, int32_t type, int32_t proto);
int32_t rt_net_bind(int32_t fd, int32_t port, int32_t family);
int32_t rt_net_listen(int32_t fd, int32_t backlog);
int32_t rt_net_connect(int32_t fd, const char* host, int32_t port);
int32_t rt_net_accept(int32_t fd);
int32_t rt_net_set_nonblocking(int32_t fd);
int32_t rt_net_set_reuse_addr(int32_t fd);
int32_t rt_net_set_no_delay(int32_t fd, int32_t enabled);
int32_t rt_net_set_send_buf_size(int32_t fd, int32_t size);
int32_t rt_net_set_recv_buf_size(int32_t fd, int32_t size);
int32_t rt_net_close(int32_t fd);
int32_t rt_net_connected(int32_t fd);
int32_t rt_net_available(int32_t fd);
int32_t rt_net_send(int32_t fd, const void* data, int32_t length);
int32_t rt_net_recv(int32_t fd, void* buf, int32_t bufSize);

#ifdef __cplusplus
}
#endif

#endif /* DLANG_RT_ABI_H */
