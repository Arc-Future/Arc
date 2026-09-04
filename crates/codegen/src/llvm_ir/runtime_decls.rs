//! LLVM IR declare statements for runtime ABI functions (RFC 015 Phase A).
//!
//! All declarations use opaque pointers (LLVM 15+).

/// Emit all runtime function declarations as LLVM IR `declare` statements.
pub fn emit_runtime_decls(is_windows: bool) -> String {
    let mut out = String::new();
    out.push_str("; ---- Runtime ABI declarations ----\n");

    // Environment ABI (Phase 1 + Phase 2)
    // Phase 1: command-line argument access (argc/argv)
    out.push_str("declare void @rt_env_init(i32, ptr)\n");
    out.push_str("declare i32  @rt_env_argc()\n");
    out.push_str("declare ptr  @rt_env_argv(i32)\n");
    // Phase 2: environment variables
    out.push_str("declare ptr  @rt_env_get_var(ptr)\n");
    out.push_str("declare i32  @rt_env_set_var(ptr, ptr)\n");
    // Phase 2: process control
    out.push_str("declare void @rt_env_exit(i32) noreturn\n");
    out.push_str("declare i32  @rt_env_get_exit_code()\n");
    out.push_str("declare void @rt_env_set_exit_code(i32)\n");
    out.push_str("declare void @rt_env_fail_fast(ptr) noreturn\n");
    // Phase 2: system info
    out.push_str("declare ptr  @rt_env_newline()\n");
    out.push_str("declare i32  @rt_env_processor_count()\n");
    out.push_str("declare i32  @rt_env_is_64bit_process()\n");
    // Phase 2: current directory
    out.push_str("declare ptr  @rt_env_get_cwd()\n");
    out.push_str("declare i32  @rt_env_set_cwd(ptr)\n");
    // Phase 2: machine / user name
    out.push_str("declare ptr  @rt_env_machine_name()\n");
    out.push_str("declare ptr  @rt_env_self_exe()\n");
    out.push_str("declare ptr  @rt_env_user_name()\n");
    // Phase 2: platform identifier
    out.push_str("declare ptr  @rt_env_platform()\n");
    out.push_str("declare i32  @rt_env_is_windows()\n");
    out.push_str("declare i32  @rt_env_is_linux()\n");
    out.push_str("declare i32  @rt_env_is_macos()\n");
    out.push_str("declare i32  @rt_env_is_android()\n");
    out.push_str("declare i32  @rt_env_is_ios()\n");
    out.push_str("declare i32  @rt_env_is_ohos()\n");

    // Console
    out.push_str("declare void @rt_println(ptr)\n");
    // Console I/O 扩展（Phase 1+2）：无换行输出、行输入、字符输入、颜色控制
    out.push_str("declare void @rt_print(ptr)\n");
    out.push_str("declare ptr  @rt_read_line()\n");
    out.push_str("declare i32  @rt_read_char()\n");
    out.push_str("declare void @rt_console_set_fg(i32)\n");
    out.push_str("declare void @rt_console_set_bg(i32)\n");
    out.push_str("declare void @rt_console_reset_color()\n");
    out.push_str("declare i32  @rt_console_get_fg()\n");
    out.push_str("declare i32  @rt_console_get_bg()\n");
    // Phase 3 (2026-07-20): stderr output
    out.push_str("declare void @rt_println_error(ptr)\n");
    out.push_str("declare void @rt_print_error(ptr)\n");
    out.push_str("declare void @rt_panic(ptr)\n");
    out.push_str("declare void @rt_panic_at(ptr, ptr, i32, i32)\n"); // RFC 017 M1
    out.push_str("declare i32  @rt_backtrace(ptr, i32)\n"); // RFC 017 M1
    out.push_str("declare void @rt_print_backtrace()\n"); // RFC 017 M1

    // String
    out.push_str("declare ptr  @rt_str_concat(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_length(ptr)\n");
    out.push_str("declare i32  @rt_str_equals(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_compare(ptr, ptr)\n");
    // String instance methods (P2)
    out.push_str("declare ptr  @rt_str_split(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_split_char(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_split_opts(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_split_char_opts(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_split_chars(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_split_chars_opts(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_split_opts_count(ptr, ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_split_char_opts_count(ptr, i32, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_split_chars_opts_count(ptr, ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_to_char_array(ptr)\n");
    out.push_str("declare ptr  @rt_str_to_char_array_range(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_str_char_at(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_join(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_join_char(i32, ptr)\n");
    out.push_str("declare ptr  @rt_str_replace(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_substring(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_str_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_index_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_index_of_char(ptr, i32)\n");
    out.push_str("declare i32  @rt_str_index_of_from(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_str_index_of_char_from(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_str_last_index_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_last_index_of_char(ptr, i32)\n");
    out.push_str("declare i32  @rt_str_last_index_of_from(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_str_last_index_of_char_from(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_insert(ptr, i32, ptr)\n");
    out.push_str("declare ptr  @rt_str_remove(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_trim_start(ptr)\n");
    out.push_str("declare ptr  @rt_str_trim_end(ptr)\n");
    out.push_str("declare ptr  @rt_str_trim_char(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_trim_start_char(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_trim_end_char(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_trim_chars(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_trim_start_chars(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_trim_end_chars(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_str_pad_left(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_pad_right(ptr, i32)\n");
    out.push_str("declare ptr  @rt_str_pad_left_char(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_pad_right_char(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_str_from_char_count(i32, i32)\n");
    out.push_str("declare ptr  @rt_str_format(ptr, ptr, ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_starts_with(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_ends_with(ptr, ptr)\n");
    out.push_str("declare i32  @rt_str_starts_with_char(ptr, i32)\n");
    out.push_str("declare i32  @rt_str_ends_with_char(ptr, i32)\n");
    out.push_str("declare i32  @rt_str_is_null_or_white_space(ptr)\n");
    out.push_str("declare ptr  @rt_str_trim(ptr)\n");
    out.push_str("declare ptr  @rt_str_to_upper(ptr)\n");
    out.push_str("declare ptr  @rt_str_to_lower(ptr)\n");
    out.push_str("declare ptr  @rt_str_from_codepoint(i32)\n");

    // Dictionary<K,V> (generic: void* key/value + hash/eq fn pointers)
    out.push_str("declare ptr  @rt_dict_create(ptr, ptr)\n");
    out.push_str("declare void @rt_dict_ensure_capacity(ptr, i32)\n");
    out.push_str("declare void @rt_dict_set(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_dict_get(ptr, ptr)\n");
    out.push_str("declare i32  @rt_dict_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_dict_contains_value(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_dict_try_add(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_dict_try_get_value(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_dict_count(ptr)\n");
    out.push_str("declare i32  @rt_dict_remove(ptr, ptr)\n");
    out.push_str("declare void @rt_dict_clear(ptr)\n");
    out.push_str("declare ptr  @rt_dict_keys(ptr)\n");
    out.push_str("declare ptr  @rt_dict_values(ptr)\n");
    out.push_str("declare ptr  @rt_dict_get_enumerator(ptr)\n");
    out.push_str("declare i32  @rt_dict_enumerator_move_next(ptr)\n");
    out.push_str("declare ptr  @rt_dict_enumerator_get_key(ptr)\n");
    out.push_str("declare ptr  @rt_dict_enumerator_get_value(ptr)\n");
    out.push_str("declare i32  @rt_hash_str(ptr)\n");
    out.push_str("declare i32  @rt_eq_str(ptr, ptr)\n");
    out.push_str("declare i32  @rt_hash_int(ptr)\n");
    out.push_str("declare i32  @rt_hash_long(ptr)\n");
    out.push_str("declare i32  @rt_eq_int(ptr, ptr)\n");
    out.push_str("declare i32  @rt_cmp_int(ptr, ptr)\n");
    out.push_str("declare i32  @rt_cmp_str(ptr, ptr)\n");

    // HashSet<T> (RFC Phase 5)
    out.push_str("declare ptr  @rt_set_create(ptr, ptr)\n");
    out.push_str("declare void @rt_set_ensure_capacity(ptr, i32)\n");
    out.push_str("declare void @rt_set_destroy(ptr)\n");
    out.push_str("declare i32  @rt_set_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_remove(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_count(ptr)\n");
    out.push_str("declare void @rt_set_clear(ptr)\n");
    out.push_str("declare void @rt_set_union_with(ptr, ptr)\n");
    out.push_str("declare void @rt_set_intersect_with(ptr, ptr)\n");
    out.push_str("declare void @rt_set_except_with(ptr, ptr)\n");
    out.push_str("declare void @rt_set_symmetric_except_with(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_is_subset_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_is_superset_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_is_proper_subset_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_is_proper_superset_of(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_overlaps(ptr, ptr)\n");
    out.push_str("declare i32  @rt_set_set_equals(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_set_to_array(ptr)\n");
    out.push_str("declare i32  @rt_set_get(ptr, i32, ptr)\n");
    out.push_str("declare ptr  @rt_set_get_enumerator(ptr)\n");
    out.push_str("declare i32  @rt_set_enumerator_move_next(ptr)\n");
    out.push_str("declare ptr  @rt_set_enumerator_current(ptr)\n");

    // LinkedList<T> (Phase 3) — 双向链表 + 哨兵节点；节点句柄为 RtLinkedListNode*
    out.push_str("declare ptr  @rt_linked_list_create(i32, ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_linked_list_destroy(ptr)\n");
    out.push_str("declare void @rt_linked_list_clear(ptr)\n");
    out.push_str("declare i32  @rt_linked_list_count(ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_first(ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_last(ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_add_last(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_add_first(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_add_after(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_add_before(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_linked_list_remove_node(ptr, ptr)\n");
    out.push_str("declare i32  @rt_linked_list_remove(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_find(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_find_last(ptr, ptr)\n");
    out.push_str("declare i32  @rt_linked_list_contains(ptr, ptr)\n");
    out.push_str("declare void @rt_linked_list_node_value(ptr, ptr)\n");
    out.push_str("declare void @rt_linked_list_node_set_value(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_node_prev(ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_node_next(ptr)\n");
    out.push_str("declare ptr  @rt_linked_list_node_list(ptr)\n");

    // SortedDictionary<K, V> (Phase 3) — 红黑树实现的有序映射
    out.push_str("declare ptr  @rt_sorted_dict_create(ptr)\n");
    out.push_str("declare void @rt_sorted_dict_destroy(ptr)\n");
    out.push_str("declare void @rt_sorted_dict_clear(ptr)\n");
    out.push_str("declare i32  @rt_sorted_dict_count(ptr)\n");
    out.push_str("declare i32  @rt_sorted_dict_contains(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_sorted_dict_get(ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_dict_try_get(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_sorted_dict_set(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_dict_add(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_dict_remove(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_sorted_dict_keys(ptr)\n");
    out.push_str("declare ptr  @rt_sorted_dict_values(ptr)\n");

    // SortedSet<T> (Phase 3) — 红黑树实现的有序集合
    out.push_str("declare ptr  @rt_sorted_set_create(ptr)\n");
    out.push_str("declare void @rt_sorted_set_destroy(ptr)\n");
    out.push_str("declare void @rt_sorted_set_clear(ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_count(ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_remove(ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_min(ptr, ptr)\n");
    out.push_str("declare i32  @rt_sorted_set_max(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_sorted_set_to_array(ptr)\n");
    out.push_str("declare ptr  @rt_sorted_set_reverse(ptr)\n");
    out.push_str("declare ptr  @rt_sorted_set_view_between(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_sorted_set_union(ptr, ptr)\n");
    out.push_str("declare void @rt_sorted_set_intersect(ptr, ptr)\n");
    out.push_str("declare void @rt_sorted_set_except(ptr, ptr)\n");

    // Queue<T> (RFC Phase 5)
    out.push_str("declare ptr  @rt_queue_create(i32)\n");
    out.push_str("declare void @rt_queue_enqueue(ptr, ptr)\n");
    out.push_str("declare i32  @rt_queue_dequeue(ptr, ptr)\n");
    out.push_str("declare i32  @rt_queue_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_queue_count(ptr)\n");
    out.push_str("declare void @rt_queue_clear(ptr)\n");
    out.push_str("declare i32  @rt_queue_contains(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_queue_to_array(ptr)\n");

    // Stack<T>
    out.push_str("declare ptr  @rt_stack_create(i32, ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_stack_push(ptr, ptr)\n");
    out.push_str("declare i32  @rt_stack_pop(ptr, ptr)\n");
    out.push_str("declare i32  @rt_stack_try_pop(ptr, ptr)\n");
    out.push_str("declare i32  @rt_stack_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_stack_try_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_stack_count(ptr)\n");
    out.push_str("declare void @rt_stack_clear(ptr)\n");
    out.push_str("declare i32  @rt_stack_contains(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_stack_to_array(ptr)\n");

    // ConcurrentDictionary<K,V> (RFC 024 M1: per-bucket lock + lock-free read)
    out.push_str("declare ptr  @rt_concurrent_dict_create(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_create_level(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_create_level_cap(ptr, ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_concurrent_dict_try_add(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_dict_try_get(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_dict_try_update(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_concurrent_dict_set(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_get_or_default(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_dict_try_remove(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_get_or_add(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_get_or_add_val(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_add_or_update(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_add_or_update_pf(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_dict_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_dict_count(ptr)\n");
    out.push_str("declare void @rt_concurrent_dict_clear(ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_keys(ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_values(ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_dict_to_array(ptr)\n");

    // ConcurrentQueue<T> (RFC 024 M2)
    out.push_str("declare ptr  @rt_concurrent_queue_create()\n");
    out.push_str("declare void @rt_concurrent_queue_enqueue(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_queue_try_dequeue(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_queue_try_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_queue_count(ptr)\n");
    out.push_str("declare i32  @rt_concurrent_queue_is_empty(ptr)\n");
    out.push_str("declare void @rt_concurrent_queue_clear(ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_queue_to_array(ptr)\n");

    // ConcurrentBag<T> (RFC 024 M3)
    out.push_str("declare ptr  @rt_concurrent_bag_create()\n");
    out.push_str("declare void @rt_concurrent_bag_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_bag_try_take(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_bag_try_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_bag_count(ptr)\n");
    out.push_str("declare i32  @rt_concurrent_bag_is_empty(ptr)\n");
    out.push_str("declare void @rt_concurrent_bag_clear(ptr)\n");
    out.push_str("declare ptr  @rt_concurrent_bag_to_array(ptr)\n");

    // ConcurrentStack<T> (RFC 024 M4)
    out.push_str("declare ptr  @rt_concurrent_stack_create()\n");
    out.push_str("declare void @rt_concurrent_stack_push(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_stack_try_pop(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_stack_try_peek(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_stack_count(ptr)\n");
    out.push_str("declare i32  @rt_concurrent_stack_is_empty(ptr)\n");
    out.push_str("declare void @rt_concurrent_stack_clear(ptr)\n");
    out.push_str("declare void @rt_concurrent_stack_push_range(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_concurrent_stack_try_pop_range(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_concurrent_stack_to_array(ptr)\n");

    // ConcurrentQueue/Bag/Stack PCC surface (RFC 024 M7)
    out.push_str("declare i32  @rt_concurrent_queue_try_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_queue_try_take(ptr, ptr)\n");
    out.push_str("declare void @rt_concurrent_queue_copy_to(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_concurrent_bag_try_add(ptr, ptr)\n");
    out.push_str("declare void @rt_concurrent_bag_copy_to(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_concurrent_stack_try_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_concurrent_stack_try_take(ptr, ptr)\n");
    out.push_str("declare void @rt_concurrent_stack_copy_to(ptr, ptr, i32)\n");
    // BlockingCollection<T> (RFC 024 M5/M7)
    out.push_str("declare ptr  @rt_blocking_collection_create(i32, i32)\n");
    out.push_str("declare ptr  @rt_blocking_collection_create_with(ptr, i32, i32, i32)\n");
    out.push_str("declare ptr  @rt_blocking_collection_create_with_queue(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_blocking_collection_create_with_bag(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_blocking_collection_create_with_stack(ptr, i32, i32)\n");
    out.push_str("declare void @rt_blocking_collection_add(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_blocking_collection_take(ptr)\n");
    out.push_str("declare void @rt_blocking_collection_complete(ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_is_completed(ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_count(ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_bounded_capacity(ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_try_add(ptr, ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_try_take(ptr, ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_is_adding_completed(ptr)\n");
    out.push_str("declare ptr  @rt_blocking_collection_to_array(ptr)\n");
    out.push_str("declare i32  @rt_blocking_collection_try_add_to(ptr, ptr, i64)\n");
    out.push_str("declare i32  @rt_blocking_collection_try_take_to(ptr, ptr, i64)\n");
    out.push_str("declare void @rt_blocking_collection_copy_to(ptr, ptr, i32)\n");

    // List<T> (RFC 007 Phase 1 + Phase 2 + Phase 4 ARC)
    out.push_str("declare ptr  @rt_list_create(i32, ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_list_create_with_capacity(i32, i32, ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_list_destroy(ptr)\n");
    out.push_str("declare void @rt_list_push(ptr, ptr)\n");
    out.push_str("declare void @rt_list_get(ptr, i32, ptr)\n");
    out.push_str("declare void @rt_list_set(ptr, i32, ptr)\n");
    out.push_str("declare ptr  @rt_list_at(ptr, i32)\n");
    out.push_str("declare void @rt_list_ensure_capacity(ptr, i32)\n");
    out.push_str("declare i32  @rt_list_size(ptr)\n");
    // List 索引器越界消息（与 rt_list_at / 直访路径共用）
    out.push_str(
        "@__arc_list_oob = private unnamed_addr constant [25 x i8] c\"list index out of bounds\\00\"\n",
    );
    // Collection stub fallthrough: missing emit_builtin/emit_stubs case must not
    // `ret void` under a non-void call site (FindIndex→garbage i32; ContainsValue→false).
    out.push_str(
        "@__arc_stub_unimplemented = private unnamed_addr constant [37 x i8] c\"unimplemented collection method stub\\00\"\n",
    );
    // List comparer 重载（BinarySearch(T, IComparer<T>)）：rt_list_cmp_fn 是 C 函数
    // 指针，Arc 对象无法直传——调用点确定性 panic，禁止把对象指针当函数指针调用。
    out.push_str(
        "@__arc_list_cmp_unsupported = private unnamed_addr constant [39 x i8] c\"List.BinarySearch comparer unsupported\\00\"\n",
    );
    out.push_str(
        "@__arc_span_oob = private unnamed_addr constant [25 x i8] c\"span index out of bounds\\00\"\n",
    );
    // RFC 004 P0 后续 Sprint：object → 接口动态 downcast 失败（rt_obj_to_iface
    // 返回 null）的清晰异常，对齐 rt_box.c unbox 口径（InvalidCastException）。
    out.push_str(
        "@__arc_invalid_cast = private unnamed_addr constant [58 x i8] c\"InvalidCastException: object does not implement interface\\00\"\n",
    );
    // RFC 016 M3 §3.3: 零拷贝 List<T> marshal ABI
    out.push_str("declare void @rt_list_buffer_and_size(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_contains(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_index_of(ptr, ptr)\n");
    out.push_str("declare void @rt_list_insert(ptr, i32, ptr)\n");
    out.push_str("declare void @rt_list_remove_at(ptr, i32)\n");
    out.push_str("declare void @rt_list_clear(ptr)\n");
    out.push_str("declare i32  @rt_list_remove(ptr, ptr)\n");
    out.push_str("declare void @rt_list_reverse(ptr)\n");
    out.push_str("declare i32  @rt_list_eq_str(ptr, ptr)\n");
    out.push_str("declare void @rt_list_arc_inc_ref(ptr)\n");
    out.push_str("declare void @rt_list_arc_dec_ref(ptr)\n");

    // List<T> (RFC 007 Phase 3: predicate/comparison/array callbacks)
    out.push_str("declare i32  @rt_list_find_get(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_list_find_all(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_exists(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_find_index(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_find_last_index(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_true_for_all(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_last_index_of(ptr, ptr)\n");
    out.push_str("declare void @rt_list_for_each(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_remove_all(ptr, ptr)\n");
    out.push_str("declare void @rt_list_sort(ptr, ptr)\n");
    out.push_str("declare void @rt_list_sort_default(ptr)\n");
    out.push_str("declare i32  @rt_list_cmp_str(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_list_to_array(ptr)\n");
    out.push_str("declare void @rt_list_copy_to(ptr, ptr, i32)\n");
    out.push_str("declare void @rt_list_add_range_list(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_capacity(ptr)\n");
    out.push_str("declare void @rt_list_set_capacity(ptr, i32)\n");
    out.push_str("declare i32  @rt_list_is_read_only(ptr)\n");
    out.push_str("declare void @rt_list_remove_range(ptr, i32, i32)\n");
    out.push_str("declare void @rt_list_trim_excess(ptr)\n");
    out.push_str("declare void @rt_list_insert_range(ptr, i32, ptr, i32)\n");
    out.push_str("declare ptr  @rt_list_get_range(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_list_binary_search(ptr, ptr)\n");
    out.push_str("declare i32  @rt_list_binary_search_cmp(ptr, ptr, ptr)\n");

    // File & Directory I/O (M1 + M3: 基础文件操作 + 目录与路径)
    // 与 C# System.IO.File / Directory / Path 对齐。
    out.push_str("declare ptr  @rt_read_file(ptr)\n");
    out.push_str("declare i32  @rt_write_file(ptr, ptr)\n");
    out.push_str("declare i32  @rt_file_exists(ptr)\n");
    out.push_str("declare i32  @rt_file_delete(ptr)\n");
    out.push_str("declare i32  @rt_file_append(ptr, ptr)\n");
    out.push_str("declare i32  @rt_file_copy(ptr, ptr)\n");
    out.push_str("declare i32  @rt_file_move(ptr, ptr)\n");
    out.push_str("declare i32  @rt_dir_create(ptr)\n");
    out.push_str("declare i32  @rt_dir_exists(ptr)\n");
    out.push_str("declare i32  @rt_dir_delete(ptr)\n");
    // Directory.GetFiles → string[]（完整路径；失败/空 Length 0）
    out.push_str("declare ptr  @rt_dir_list_files(ptr)\n");
    out.push_str("declare ptr  @rt_dir_list_files_pattern(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_dir_list_dirs(ptr)\n");
    out.push_str("declare ptr  @rt_path_combine(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_path_get_dir_name(ptr)\n");
    out.push_str("declare ptr  @rt_path_get_file_name(ptr)\n");
    out.push_str("declare ptr  @rt_path_get_file_name_without_ext(ptr)\n");
    out.push_str("declare ptr  @rt_path_get_extension(ptr)\n");
    out.push_str("declare ptr  @rt_path_change_extension(ptr, ptr)\n");
    out.push_str("declare i32  @rt_path_has_extension(ptr)\n");
    out.push_str("declare ptr  @rt_path_get_temp_path()\n");
    out.push_str("declare ptr  @rt_file_read_all_bytes(ptr)\n");
    out.push_str("declare i32  @rt_file_write_all_bytes(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_file_read_all_lines(ptr)\n");

    // FileStream (标准库就绪 P0)
    out.push_str("declare ptr  @rt_file_stream_open(ptr, i32)\n");
    out.push_str("declare void @rt_file_stream_close(ptr)\n");
    out.push_str("declare i32  @rt_file_stream_read(ptr, ptr, i32, i32)\n");
    out.push_str("declare void @rt_file_stream_write(ptr, ptr, i32, i32)\n");
    out.push_str("declare i64  @rt_file_stream_seek(ptr, i64, i32)\n");
    out.push_str("declare i64  @rt_file_stream_get_length(ptr)\n");
    out.push_str("declare i64  @rt_file_stream_get_position(ptr)\n");
    out.push_str("declare void @rt_file_stream_set_position(ptr, i64)\n");
    out.push_str("declare void @rt_file_stream_set_length(ptr, i64)\n");
    out.push_str("declare void @rt_file_stream_flush(ptr)\n");
    out.push_str("declare i32  @rt_file_stream_can_read(ptr)\n");
    out.push_str("declare i32  @rt_file_stream_can_write(ptr)\n");
    out.push_str("declare i32  @rt_file_stream_can_seek(ptr)\n");

    // FileStream 真异步（文件 I/O 专用池卸载 + 完成投递；rt_file_stream_async.c）
    out.push_str("declare ptr  @rt_file_stream_read_async(ptr, ptr, i32, i32, ptr)\n");
    out.push_str("declare ptr  @rt_file_stream_write_async(ptr, ptr, i32, i32, ptr)\n");
    out.push_str("declare ptr  @rt_file_stream_flush_async(ptr, ptr)\n");
    out.push_str("declare void @rt_file_stream_async_shutdown()\n");

    // L3 Orm SQLite execute MVP
    out.push_str("declare i32  @rt_sqlite_open(ptr)\n");
    out.push_str("declare void @rt_sqlite_close(i32)\n");
    out.push_str("declare i32  @rt_sqlite_exec(i32, ptr)\n");
    out.push_str("declare i32  @rt_sqlite_prepare(i32, ptr)\n");
    out.push_str("declare i32  @rt_sqlite_step(i32)\n");
    out.push_str("declare i32  @rt_sqlite_column_count(i32)\n");
    out.push_str("declare i32  @rt_sqlite_column_type(i32, i32)\n");
    out.push_str("declare i32  @rt_sqlite_column_int(i32, i32)\n");
    out.push_str("declare double @rt_sqlite_column_double(i32, i32)\n");
    out.push_str("declare ptr  @rt_sqlite_column_text(i32, i32)\n");
    out.push_str("declare ptr  @rt_sqlite_column_name(i32, i32)\n");
    out.push_str("declare void @rt_sqlite_finalize(i32)\n");
    out.push_str("declare ptr  @rt_sqlite_errmsg(i32)\n");
    out.push_str("declare i32  @rt_sqlite_bind_text(i32, i32, ptr)\n");
    out.push_str("declare i32  @rt_sqlite_bind_int(i32, i32, i32)\n");
    out.push_str("declare i32  @rt_sqlite_changes(i32)\n");

    // RFC 029 M1 图像编解码（std/Drawing · Arc.Drawing）
    // 主 ABI（§1.5）：decode 输出 stbi 缓冲 / encode 输出 malloc 缓冲，rt_image_free 释放。
    out.push_str("declare i32  @rt_image_decode(ptr, i64, ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_image_decode_file(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_image_encode_png(ptr, i32, i32, ptr, ptr)\n");
    out.push_str("declare i32  @rt_image_encode_jpg(ptr, i32, i32, i32, ptr, ptr)\n");
    out.push_str("declare void @rt_image_free(ptr)\n");
    // Bitmap 像素面补充 ABI（M1 内部）
    out.push_str("declare ptr  @rt_image_alloc(i32, i32)\n");
    out.push_str("declare i64  @rt_image_get_pixel(ptr, i32, i32, i32, i32)\n");
    out.push_str("declare i32  @rt_image_set_pixel(ptr, i32, i32, i32, i32, i64)\n");
    out.push_str("declare i32  @rt_image_fill_rect(ptr, i32, i32, i32, i32, i32, i32, i64)\n");
    out.push_str("declare i32  @rt_image_write_buffer(ptr, ptr, i64)\n");
    // RFC 029 M2 GIF 多帧解码 + SVG 光栅化（rt_image.c · stb_gif + vendored nanosvg）
    // decode_gif: out_delays 为 int32* 指针槽（ptr）；decode_svg: scale 为 float。
    out.push_str("declare i32  @rt_image_decode_gif(ptr, i64, ptr, ptr, ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_image_gif_frame(ptr, i32, i32, i32)\n");
    out.push_str("declare i32  @rt_image_gif_delay(ptr, i32)\n");
    out.push_str("declare i32  @rt_image_decode_svg(ptr, i64, ptr, ptr, ptr, float)\n");
    // RFC 029 M2 二维码生成（rt_qrcode.c + qrcodegen.c 独立 TU）
    out.push_str("declare i32  @rt_qrcode_encode(ptr, i32, i32, ptr, ptr)\n");
    // RFC 029 M4 条形码解码（rt_barcode.c · quirc 单 TU）；text_cap 为 size_t → i64
    out.push_str("declare i32  @rt_barcode_quirc_decode(ptr, i32, i32, ptr, i64)\n");
    // RFC 029 M4 原生 1D 解码（rt_barcode.c · EAN-13/Code39/Code128）
    out.push_str("declare i32  @rt_barcode_1d_decode(ptr, i32, i32, ptr, i64)\n");
    // RFC 029 M6 字体（rt_font.c · stb_truetype 单 TU）
    out.push_str("declare ptr  @rt_image_font_load(ptr, i64, float)\n");
    out.push_str("declare i32  @rt_image_font_metrics(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare float @rt_image_font_measure(ptr, ptr)\n");
    out.push_str("declare i32  @rt_image_font_glyph(ptr, i32, ptr, ptr, ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_image_font_free(ptr)\n");

    // Memory-mapped file (RFC 037 M-CE1)
    out.push_str("declare ptr  @rt_file_mmap_open(ptr)\n");
    out.push_str("declare void @rt_file_mmap_close(ptr)\n");
    out.push_str("declare i64  @rt_file_mmap_length(ptr)\n");
    out.push_str("declare ptr  @rt_file_mmap_data(ptr)\n");

    // CodeEditor piece-table buffer (RFC 037 M-CE1)
    out.push_str("declare ptr  @rt_editor_create_empty()\n");
    out.push_str("declare ptr  @rt_editor_open_path(ptr)\n");
    out.push_str("declare void @rt_editor_destroy(ptr)\n");
    out.push_str("declare i64  @rt_editor_length(ptr)\n");
    out.push_str("declare i32  @rt_editor_line_count(ptr)\n");
    out.push_str("declare i32  @rt_editor_ensure_lines(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_editor_line_text(ptr, i32)\n");
    out.push_str("declare i32  @rt_editor_set_text(ptr, ptr)\n");
    out.push_str("declare i32  @rt_editor_insert(ptr, i64, ptr)\n");
    out.push_str("declare i32  @rt_editor_delete(ptr, i64, i64)\n");
    out.push_str("declare i32  @rt_editor_is_mmap_backed(ptr)\n");

    // Exception (zero-cost EH: invoke/landingpad — legacy registry removed in milestone ⑥)
    out.push_str("declare void @rt_throw(ptr)\n");
    out.push_str("declare ptr  @rt_get_exception()\n");
    out.push_str("declare ptr  @rt_format_stacktrace()\n");
    // Zero-cost EH milestone ② (Windows SEH): native raise + C++ personality.
    // `catch ptr null` (catch-all) needs no typeinfo; the minimal ThrowInfo
    // (typeDescriptor RVA = 0) lives in rt_exc.c. `__CxxFrameHandler3` is the
    // MSVC x64 personality function referenced by may-throw user functions.
    if is_windows {
        out.push_str("declare void @_CxxThrowException(ptr, ptr)\n");
        out.push_str("declare i32  @__CxxFrameHandler3(...)\n");
    }

    // RFC 006 A3 S2/S3: `static readonly` 惰性初始化 guard（类级状态机）。
    // state 为类级全局 i32：0=未初始化, 1=初始化中, 2=已初始化。
    out.push_str("declare i32  @rt_lazy_is_initialized(ptr)\n");
    out.push_str("declare i32  @rt_lazy_init_begin(ptr)\n");
    out.push_str("declare void @rt_lazy_init_commit(ptr)\n");

    // ARC
    out.push_str("declare void @rt_arc_inc(ptr) nounwind\n");
    out.push_str("declare void @rt_arc_dec(ptr) nounwind\n");
    out.push_str("declare i32  @rt_arc_count(ptr) nounwind readonly\n");
    // RFC 005 M2: 循环收集器字段遍历（vtable slot 2 walk）
    out.push_str("declare void @rt_arc_walk_fields(ptr, ptr, ptr) nounwind\n");
    // RFC 005 §2.2: Weak<T> runtime ABI. rt_arc_weak_create returns an opaque
    // RtWeak* slot; try_get returns the strong-retained target or NULL;
    // destroy decs weakcount and frees the slot. All nounwind readonly=false
    //（destroy 写入 weakcount + free；create 写入 weakcount + malloc）。
    out.push_str("declare ptr  @rt_arc_weak_create(ptr) nounwind\n");
    out.push_str("declare ptr  @rt_arc_weak_try_get(ptr) nounwind\n");
    out.push_str("declare void @rt_arc_weak_destroy(ptr) nounwind\n");

    // RFC 018 M1: 运行时类型判断 ABI
    // rt_obj_isa(obj, target_typeinfo) → i32 (0/1)
    out.push_str("declare i32  @rt_obj_isa(ptr, ptr) nounwind readonly\n");
    // RFC 004 P0 后续 Sprint：动态 downcast（object → 接口）itable 查找。
    // rt_obj_to_iface(obj, target_iface_typeinfo) → itable ptr 或 null。
    out.push_str("declare ptr  @rt_obj_to_iface(ptr, ptr) nounwind readonly\n");

    // RFC 017 阶段一（ALC 共享 dll）：基元 typeinfo 经**函数符号**查询。
    // 共享库边界上数据符号的导入 thunk 是「指向数据的指针」而非数据本身，
    // 直接引用数据符号会别名 thunk 导致语义错误；函数符号经 thunk 解析
    // 天然正确。基元 typeinfo 全局已在 rt_type.c 中 static 化并从导出面移除，
    // codegen 侧按 id 序查询（对齐 rt_primitive_table）：
    // int=0 long=1 short=2 byte=3 char=4 float=5 double=6 bool=7 string=8 void=9 object=10。
    out.push_str("declare ptr @rt_typeinfo_prim(i32) nounwind\n");
    // 基元装箱 vtable 查询（emit_box 装箱点 slot0）：返回 runtime 内静态
    // [3 x ptr] 表 { &rt_typeinfo_<prim>, null, null }，仅接受可装箱基元 id。
    out.push_str("declare ptr @rt_box_vtable(i32) nounwind\n");

    // RFC 017 阶段一：dbg 表加载期登记（宿主 exe `__arc_module_init` 注入调用，
    // 插件由 `rt_library_load` 持 OS 句柄登记）。仅 MainObject 非 wasm 角色发射
    // 调用点——未引用的 declare 不产生链接依赖（wasm 路径亦无害）。
    out.push_str("declare i32 @rt_debug_module_register(ptr, ptr, i32)\n");

    // FFI Marshal 装箱 ABI（RFC 016 v2 M2 / RFC 016 M3）
    out.push_str("declare ptr  @rt_box_create(i32, i32) nounwind\n");
    out.push_str("declare void @rt_box_destroy(ptr) nounwind\n");
    out.push_str("declare i32  @rt_box_unbox(ptr, i32, ptr, i32) nounwind\n");

    // RFC 006 M3: string→object 装箱（rt_type.c）——object 槽持有 string 的
    // 包装/提取，使 `o is string` 可识别且其它类型判别安全。
    out.push_str("declare ptr @rt_string_box(ptr) nounwind\n");
    out.push_str("declare ptr @rt_string_unbox(ptr) nounwind\n");

    // Runtime-length array ABI (RFC 015 Phase B)
    out.push_str("declare ptr  @rt_array_create(i32, i32) nounwind\n");
    out.push_str("declare i32  @rt_array_length(ptr) nounwind readonly\n");
    out.push_str("declare void @rt_array_destroy(ptr) nounwind\n");
    // P5-F: Array utility methods
    out.push_str("declare void @rt_array_copy(ptr, i32, ptr, i32, i32) nounwind\n");
    out.push_str("declare void @rt_array_clear(ptr, i32, i32) nounwind\n");
    out.push_str("declare void @rt_array_reverse(ptr) nounwind\n");
    // BitConverter host-endian + Buffer.BlockCopy → rt_array_copy
    out.push_str("declare i32  @rt_bitconverter_is_little_endian() nounwind readonly\n");
    out.push_str("declare ptr  @rt_bitconverter_get_bytes_i32(i32) nounwind\n");
    out.push_str("declare ptr  @rt_bitconverter_get_bytes_i64(i64) nounwind\n");
    out.push_str("declare i32  @rt_bitconverter_to_i32(ptr, i32) nounwind readonly\n");
    out.push_str("declare i64  @rt_bitconverter_to_i64(ptr, i32) nounwind readonly\n");
    out.push_str("declare i32  @rt_array_index_of_int(ptr, i32) nounwind readonly\n");
    out.push_str("declare i32  @rt_array_last_index_of_int(ptr, i32) nounwind readonly\n");
    out.push_str("declare void @rt_array_resize(ptr, i32) nounwind\n");
    out.push_str("declare i32  @rt_array_exists(ptr, ptr)\n");
    out.push_str("declare i32  @rt_array_find_int(ptr, ptr)\n");
    out.push_str("declare i32  @rt_array_find_last_int(ptr, ptr)\n");
    out.push_str("declare i32  @rt_array_find_index(ptr, ptr)\n");
    out.push_str("declare i32  @rt_array_find_last_index(ptr, ptr)\n");
    out.push_str("declare i32  @rt_array_true_for_all(ptr, ptr)\n");
    out.push_str("declare void @rt_array_for_each(ptr, ptr)\n");
    out.push_str("declare void @rt_array_sort_int(ptr) nounwind\n");
    out.push_str("declare i32  @rt_array_binary_search_int(ptr, i32) nounwind readonly\n");
    out.push_str("declare ptr  @rt_array_find_all_int(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_array_convert_all_int(ptr, ptr)\n");
    // RFC 017 #8：spread 拼接
    out.push_str("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1) nounwind\n");
    // RFC 039 M2：栈 alloca 生命周期区间标记（lifetime markers）。
    // `nounwind` 标注使 optimizer 视为不抛异常 intrinsic，可被 StackColoring /
    // 栈收缩 / 槽复用自由调度；`p0` 表示 addrspace(0) 指针。
    out.push_str("declare void @llvm.lifetime.start.p0(i64 immarg, ptr nocapture) nounwind\n");
    out.push_str("declare void @llvm.lifetime.end.p0(i64 immarg, ptr nocapture) nounwind\n");

    // Task / async (RFC 015 Phase A + RFC 009 M1 扩展)
    out.push_str("declare ptr  @rt_task_alloc()\n");
    out.push_str("declare ptr  @rt_task_from_int(i32)\n");
    out.push_str("declare ptr  @rt_task_void()\n");
    out.push_str("declare i32  @rt_task_poll(ptr)\n");
    out.push_str("declare i32  @rt_task_result_int(ptr)\n");
    // RFC 009 M1: 泛型结果提取、取消、状态查询、状态机句柄、waker
    out.push_str("declare ptr  @rt_task_from_ptr(ptr)\n");
    out.push_str("declare ptr  @rt_task_from_class(ptr)\n");
    out.push_str("declare ptr  @rt_task_from_value(ptr, i32)\n");
    out.push_str("declare i32  @rt_task_status(ptr)\n");
    out.push_str("declare ptr  @rt_task_result_ptr(ptr)\n");
    out.push_str("declare void @rt_task_result_value(ptr, ptr, i32)\n");
    out.push_str("declare void @rt_task_cancel(ptr)\n");
    out.push_str("declare i32  @rt_task_is_canceled(ptr)\n");
    out.push_str("declare ptr  @rt_task_from_state_machine(ptr, ptr)\n");
    // plan.md 阶段 3 I2：协程 Task ABI（CoroSplit 单帧所有权）——
    // 单次调用创建协程 Task（resume=thunk、dtor=destroy）。
    out.push_str("declare ptr  @rt_task_from_coroutine(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_task_create_pending()\n");
    out.push_str("declare void @rt_task_adopt(ptr, ptr)\n");
    // RFC 008 AsyncStream：TCS get_Task 扇出注册（leader 完成级联传播）
    out.push_str("declare void @rt_task_add_follower(ptr, ptr)\n");
    out.push_str("declare void @rt_task_set_result_int(ptr, i32)\n");
    out.push_str("declare void @rt_task_set_result_ptr(ptr, ptr)\n");
    out.push_str("declare void @rt_task_set_result_class(ptr, ptr)\n");
    out.push_str("declare void @rt_task_set_result_value(ptr, ptr, i32)\n");
    out.push_str("declare void @rt_task_set_waker(ptr, ptr)\n");
    out.push_str("declare void @rt_waker_wake(ptr)\n");
    // RFC 009 M3: EventLoop + Task.Delay + waker 真实唤醒
    out.push_str("declare void @rt_task_complete(ptr)\n");
    // RFC 008 AsyncStream：TCS SetException / await FAULTED 通道
    // （rt_task.c 已有定义，此处补 declare——否则 IR 引用报 undefined value）
    out.push_str("declare void @rt_task_fault(ptr, ptr)\n");
    out.push_str("declare void @rt_task_register_waker(ptr, ptr)\n");
    // M6.2 协程暖启动：async 调用点首 poll 驱动 body 同步前缀（打破 create→await 串行化）
    out.push_str("declare void @rt_task_autostart(ptr)\n");
    out.push_str("declare ptr  @rt_task_delay(i32)\n");
    out.push_str("declare ptr  @rt_task_delay_ct(i32, ptr)\n"); /* RFC 009 M4: Task.Delay(ms, ct) */
    out.push_str("declare ptr  @rt_task_when_all(ptr, i32)\n"); /* RFC 009 M4: WhenAll */
    out.push_str("declare ptr  @rt_task_when_any(ptr, i32)\n"); /* RFC 009 M4: WhenAny */
    out.push_str("declare ptr  @rt_event_loop_create()\n");
    out.push_str("declare void @rt_event_loop_destroy(ptr)\n");
    out.push_str("declare void @rt_event_loop_run(ptr)\n");
    out.push_str("declare void @rt_event_loop_stop(ptr)\n");
    out.push_str("declare i32  @rt_event_loop_tick(ptr)\n");
    out.push_str("declare void @rt_event_loop_pump(ptr)\n");
    out.push_str("declare void @rt_event_loop_spawn(ptr, ptr)\n");
    out.push_str("declare void @rt_event_loop_set_current(ptr)\n");
    out.push_str("declare ptr  @rt_event_loop_current()\n");
    out.push_str("declare void @rt_event_loop_inc_pending(ptr)\n");
    out.push_str("declare void @rt_event_loop_dec_pending(ptr)\n");
    out.push_str("declare void @rt_event_loop_set_root(ptr, ptr)\n");
    // RFC 009 M2: Reactor 集成 —— 绑定/查询 IO 后端
    out.push_str("declare void @rt_event_loop_set_reactor(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_event_loop_get_reactor(ptr)\n");
    // RFC 009 M6: 多线程 Executor —— 绑定/解绑续体执行器（线程池）
    out.push_str("declare void @rt_event_loop_set_threadpool(ptr, ptr)\n");
    // RFC 009 M4: 通用定时器回调注册 + CancellationTokenSource ABI
    out.push_str("declare void @rt_event_loop_schedule(ptr, ptr, ptr, i64)\n");
    out.push_str("declare ptr  @rt_cts_create()\n");
    out.push_str("declare i32  @rt_cts_is_canceled(ptr)\n");
    out.push_str("declare i32  @rt_cts_can_be_canceled(ptr)\n");
    out.push_str("declare void @rt_cts_cancel(ptr)\n");
    out.push_str("declare void @rt_cts_register(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_cts_cancel_after(ptr, i32)\n");
    out.push_str("declare void @rt_cts_destroy(ptr)\n");
    out.push_str("declare void @rt_cts_throw_if_canceled(ptr)\n");
    out.push_str("declare void @rt_cts_callback_trampoline(ptr)\n");

    // RFC 009 M5.4: CTS 无锁化（Treiber stack + atomic flag）
    // rt_cts_node 布局：{ ptr cb, ptr data, ptr next, i32 registered, [13 x i32] pad }
    out.push_str("declare void @rt_cts_register_lf(ptr, ptr)\n");
    out.push_str("declare void @rt_cts_node_try_fire(ptr)\n");
    out.push_str("declare ptr  @rt_cts_node_alloc()\n");
    out.push_str("declare void @rt_cts_node_free(ptr)\n");
    // RFC 016 M2: native callback TLS 回调表（有捕获 lambda → C 回调）
    out.push_str("declare ptr  @rt_ffi_get_callback(i32) nounwind readonly\n");
    out.push_str("declare void @rt_ffi_set_callback(i32, ptr) nounwind\n");
    out.push_str("declare void @rt_ffi_clear_callback(i32) nounwind\n");

    // RFC 009 M5.5: Thread + 同步原语 ABI
    out.push_str("declare ptr  @rt_thread_create(ptr, ptr)\n");
    out.push_str("declare void @rt_thread_join(ptr)\n");
    out.push_str("declare void @rt_thread_detach(ptr)\n");
    out.push_str("declare void @rt_thread_sleep(i64)\n");
    out.push_str("declare ptr  @rt_thread_current()\n");
    out.push_str("declare i64  @rt_thread_current_id()\n");
    out.push_str("declare ptr  @rt_mutex_create()\n");
    out.push_str("declare void @rt_mutex_lock(ptr)\n");
    out.push_str("declare i32  @rt_mutex_try_lock(ptr)\n");
    out.push_str("declare void @rt_mutex_unlock(ptr)\n");
    out.push_str("declare void @rt_mutex_destroy(ptr)\n");
    out.push_str("declare ptr  @rt_semaphore_create(i32, i32)\n");
    out.push_str("declare void @rt_semaphore_wait(ptr)\n");
    out.push_str("declare i32  @rt_semaphore_wait_timeout(ptr, i64)\n");
    out.push_str("declare void @rt_semaphore_release(ptr)\n");
    out.push_str("declare void @rt_semaphore_release_n(ptr, i32)\n");
    out.push_str("declare void @rt_semaphore_destroy(ptr)\n");
    out.push_str("declare void @rt_monitor_enter(ptr)\n");
    out.push_str("declare void @rt_monitor_exit(ptr)\n");
    out.push_str("declare i32  @rt_monitor_try_enter(ptr)\n");
    out.push_str("declare void @rt_monitor_wait(ptr)\n");
    out.push_str("declare void @rt_monitor_pulse(ptr)\n");
    out.push_str("declare void @rt_monitor_pulse_all(ptr)\n");
    out.push_str("declare ptr  @rt_lock_create()\n");
    out.push_str("declare void @rt_lock_destroy(ptr)\n");

    out.push_str("declare ptr  @rt_thread_handle_create(ptr, ptr)\n");
    out.push_str("declare void @rt_thread_handle_start(ptr)\n");
    out.push_str("declare void @rt_thread_handle_join(ptr)\n");
    out.push_str("declare i32  @rt_thread_handle_is_alive(ptr)\n");
    out.push_str("declare void @rt_thread_handle_destroy(ptr)\n");

    // RFC 009 M5.1: Work-Stealing Deque + ThreadPool ABI
    // rt_work_t = { ptr fn, ptr data }，按值传递；LLVM IR 用 inline struct 表示。
    out.push_str("declare ptr  @rt_ws_deque_create(i32, i32)\n");
    out.push_str("declare void @rt_ws_deque_destroy(ptr)\n");
    out.push_str("declare void @rt_ws_push(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_ws_pop(ptr)\n");
    out.push_str("declare ptr  @rt_ws_steal(ptr)\n");
    out.push_str("declare i32  @rt_ws_deque_size(ptr)\n");
    out.push_str("declare i32  @rt_ws_deque_worker_id(ptr)\n");
    out.push_str("declare void @rt_ws_deque_set_overflow_handler(ptr)\n");
    out.push_str("declare ptr  @rt_threadpool_create(i32, i32)\n");
    out.push_str("declare void @rt_threadpool_destroy(ptr)\n");
    out.push_str("declare void @rt_threadpool_spawn(ptr, { ptr, ptr })\n");
    out.push_str("declare void @rt_threadpool_spawn_local(ptr, { ptr, ptr })\n");
    out.push_str("declare i32  @rt_threadpool_worker_id()\n");
    out.push_str("declare i32  @rt_threadpool_pending_count(ptr)\n");
    out.push_str("declare i32  @rt_threadpool_worker_count(ptr)\n");
    out.push_str("declare void @rt_threadpool_wait_idle(ptr)\n");
    out.push_str("declare void @rt_threadpool_shutdown(ptr)\n");
    out.push_str("declare ptr  @rt_threadpool_current_worker_ctx()\n");

    // RFC 009 M2: async preemption ABI (codegen emit_async_sm 调用)
    out.push_str("declare i32   @rt_worker_preempt_check(ptr)\n");
    out.push_str("declare void  @rt_worker_preempt_clear(ptr)\n");

    // RFC 009 M3: env destructor callback
    out.push_str("declare void  @rt_task_set_dtor_fn(ptr, ptr)\n");

    // RFC 009 I1（plan.md 阶段 3）：LLVM 协程 intrinsic 声明——签名以本机
    // clang 22 前端产出（C++ 协程 .ll 探针 ref_raw.ll）为准，非 LLVM 经典
    // 文档：coro.end 三参数 void、coro.save 收 ptr、coro.suspend switch 语义
    // 0=resume / 1=destroy / default=suspend。CoroSplit 由函数属性
    // `presplitcoroutine` 驱动（默认管线含 -O0 均跑）。
    out.push_str("declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n");
    out.push_str("declare i1 @llvm.coro.alloc(token)\n");
    out.push_str("declare i64 @llvm.coro.size.i64()\n");
    out.push_str("declare ptr @llvm.coro.begin(token, ptr)\n");
    out.push_str("declare token @llvm.coro.save(ptr)\n");
    out.push_str("declare i8 @llvm.coro.suspend(token, i1)\n");
    out.push_str("declare void @llvm.coro.end(ptr, i1, token)\n");
    out.push_str("declare ptr @llvm.coro.free(token, ptr)\n");
    out.push_str("declare void @llvm.coro.resume(ptr)\n");
    out.push_str("declare void @llvm.coro.destroy(ptr)\n");
    out.push_str("declare i1 @llvm.coro.done(ptr)\n");

    out.push_str("declare ptr  @rt_task_run(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_task_run_on_pool(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_task_run_func(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_task_run_func_ct(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_default_pool_shutdown()\n");

    // RFC 009 M5.7: Wait / WaitAll / WaitAny / FromCanceled 新增
    out.push_str("declare i32  @rt_task_wait_timeout(ptr, i32)\n");
    out.push_str("declare i32  @rt_task_wait_ct(ptr, ptr)\n");
    out.push_str("declare void @rt_task_wait_all(ptr, i32)\n");
    out.push_str("declare i32  @rt_task_wait_any(ptr, i32)\n");
    out.push_str("declare ptr  @rt_task_from_canceled()\n");
    out.push_str("declare ptr  @rt_task_from_exception(ptr)\n");
    out.push_str("declare i32  @rt_task_is_faulted(ptr)\n");
    out.push_str("declare ptr  @rt_task_get_exception(ptr)\n");

    // M5.7 Async: File.*Async
    out.push_str("declare ptr  @rt_file_read_all_text_async(ptr)\n");
    out.push_str("declare ptr  @rt_file_write_all_text_async(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_file_append_all_text_async(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_file_copy_async(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_file_move_async(ptr, ptr)\n");

    // IO Async 补全（RFC 009 异步优先）：File 其余 + Directory（均返回 Task*）
    out.push_str("declare ptr  @rt_file_read_all_lines_async(ptr)\n");
    out.push_str("declare ptr  @rt_file_read_all_bytes_async(ptr)\n");
    out.push_str("declare ptr  @rt_file_write_all_bytes_async(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_file_delete_async(ptr)\n");
    out.push_str("declare ptr  @rt_file_exists_async(ptr)\n");
    out.push_str("declare ptr  @rt_dir_create_async(ptr)\n");
    out.push_str("declare ptr  @rt_dir_exists_async(ptr)\n");
    out.push_str("declare ptr  @rt_dir_delete_async(ptr)\n");
    out.push_str("declare ptr  @rt_dir_list_files_async(ptr)\n");
    out.push_str("declare ptr  @rt_dir_list_files_pattern_async(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_dir_list_dirs_async(ptr)\n");

    out.push_str("declare i32  @rt_parallel_for(i32, i32, ptr, ptr, ptr, ptr, i32)\n");

    // RFC 009 M6: Parallel.ForEach ABI —— 数组源并行遍历
    // (array_ptr, array_len, body_fn, env, pool, cts, max_degree) -> partitions
    // elem_size 从数组 header 自动读取，codegen 无需传递
    out.push_str("declare i32  @rt_parallel_foreach(ptr, i32, ptr, ptr, ptr, ptr, i32)\n");

    // RFC 009 M4: SoA 数据布局 ABI —— [SoA] struct 数组的 Structure-of-Arrays 布局
    // rt_soa_array_create(length, num_fields, field_sizes*) -> rt_soa_array*
    // rt_soa_field_ptr(arr, field_idx) -> void*（第 field_idx 个字段数组首指针）
    // rt_soa_length(arr) -> i32（元素数，兼容 arr.Length 语义）
    // rt_soa_free(arr) —— 释放 SoA 数组（含所有字段数组）
    out.push_str("declare ptr  @rt_soa_array_create(i32, i32, ptr)\n");
    out.push_str("declare ptr  @rt_soa_field_ptr(ptr, i32)\n");
    out.push_str("declare i32  @rt_soa_length(ptr)\n");
    out.push_str("declare void @rt_soa_free(ptr)\n");

    // RFC 009 M5.2: Task slab allocator ABI
    out.push_str("declare void @rt_task_slab_thread_init()\n");
    out.push_str("declare void @rt_task_slab_thread_destroy()\n");
    out.push_str("declare ptr  @rt_task_slab_alloc()\n");
    out.push_str("declare void @rt_task_slab_free(ptr)\n");
    out.push_str("declare i32  @rt_task_slab_free_count()\n");
    out.push_str("declare i32  @rt_task_slab_in_use()\n");
    out.push_str("declare i32  @rt_task_slab_total_alloc()\n");
    out.push_str("declare void @rt_task_release(ptr)\n");

    // RFC 009 M5.3: Hierarchical timing wheel ABI.
    // EventLoop 委托 add/tick/next_timeout/count；node 字段由调用方填充。
    // rt_timer_node 布局：{ i64 deadline_ms, ptr fn, ptr data, i32 canceled, ptr next }
    out.push_str("declare ptr  @rt_timer_wheel_create()\n");
    out.push_str("declare void @rt_timer_wheel_destroy(ptr)\n");
    out.push_str("declare void @rt_timer_wheel_add(ptr, ptr)\n");
    out.push_str("declare void @rt_timer_wheel_tick(ptr, i64)\n");
    out.push_str("declare i64  @rt_timer_wheel_next_timeout(ptr)\n");
    out.push_str("declare i32  @rt_timer_wheel_count(ptr)\n");

    // RFC 009 M1: Reactor ABI —— 跨平台 IO 多路复用（io_uring/IOCP/kqueue/poll）
    // 生命周期 + fd 注册 + 异步 IO 提交（批量 + 零拷贝）+ flush/poll + 缓冲池注册 + 后端名查询
    out.push_str("declare ptr  @rt_reactor_create()\n");
    out.push_str("declare void @rt_reactor_destroy(ptr)\n");
    out.push_str("declare i32  @rt_reactor_register(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_reactor_modify(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_reactor_unregister(ptr, i32)\n");
    out.push_str("declare i32  @rt_reactor_submit_read(ptr, i32, ptr, i32, i64, ptr)\n");
    out.push_str("declare i32  @rt_reactor_submit_write(ptr, i32, ptr, i32, i64, ptr)\n");
    out.push_str("declare i32  @rt_reactor_submit_accept(ptr, i32, ptr)\n");
    out.push_str("declare i32  @rt_reactor_submit_connect(ptr, i32, ptr, i32, ptr)\n");
    out.push_str("declare i32  @rt_reactor_flush(ptr)\n");
    out.push_str("declare i32  @rt_reactor_poll(ptr, ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_reactor_register_buffers(ptr, ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_reactor_backend_name(ptr)\n");

    // RFC 009 M3: 零拷贝缓冲池 ABI —— user buffer 预注册到内核（io_uring_register_buffers）
    // IOCP/kqueue/poll 后端降级为普通池化（无内核注册，但仍提供 acquire/release 借用语义）
    out.push_str("declare ptr  @rt_iobuf_pool_create(i32, i32)\n");
    out.push_str("declare void @rt_iobuf_pool_destroy(ptr)\n");
    out.push_str("declare ptr  @rt_iobuf_pool_acquire(ptr, ptr)\n");
    out.push_str("declare void @rt_iobuf_pool_release(ptr, ptr)\n");
    out.push_str("declare i32  @rt_iobuf_pool_register(ptr, ptr)\n");
    out.push_str("declare i32  @rt_iobuf_pool_free_count(ptr)\n");
    out.push_str("declare i32  @rt_iobuf_pool_in_use_count(ptr)\n");
    out.push_str("declare i32  @rt_iobuf_pool_buf_size(ptr)\n");
    out.push_str("declare i32  @rt_iobuf_pool_buf_count(ptr)\n");

    // RFC 009 M2: Socket ABI —— 跨平台 socket 原语（fd-based，Reactor 底层）
    // 与 RFC 025 M4 的 rt_socket_*（handle-based，facade 层）区分：
    //   rt_net_* 返回 raw fd，供 Reactor submit_* 直接消费
    out.push_str("declare i32  @rt_net_create(i32, i32, i32)\n");
    out.push_str("declare i32  @rt_net_bind(i32, i32, i32)\n");
    out.push_str("declare i32  @rt_net_listen(i32, i32)\n");
    out.push_str("declare i32  @rt_net_connect(i32, ptr, i32)\n");
    out.push_str("declare i32  @rt_net_accept(i32)\n");
    out.push_str("declare i32  @rt_net_set_nonblocking(i32)\n");
    out.push_str("declare i32  @rt_net_set_reuse_addr(i32)\n");
    out.push_str("declare i32  @rt_net_set_no_delay(i32, i32)\n");
    out.push_str("declare i32  @rt_net_set_send_buf_size(i32, i32)\n");
    out.push_str("declare i32  @rt_net_set_recv_buf_size(i32, i32)\n");
    out.push_str("declare i32  @rt_net_close(i32)\n");
    out.push_str("declare i32  @rt_net_connected(i32)\n");
    out.push_str("declare i32  @rt_net_available(i32)\n");
    out.push_str("declare i32  @rt_net_send(i32, ptr, i32)\n");
    out.push_str("declare i32  @rt_net_recv(i32, ptr, i32)\n");

    // RFC 023 M1: DI 依赖解析桥接 —— 工厂函数通过 itable 调用 IServiceProvider.GetService。
    // 仅此一个 ABI（v0.7 精简，原 12 个 rt_di_* 已随 DI 运行时迁移至 Arc 移除）。
    out.push_str("declare ptr  @rt_di_resolve(ptr, i32)\n");

    // RFC 032 Phase 2c: QIF 纯 Arc 化，不再需要 C ABI 声明

    // Resource/culture ABI (RFC 027 M1: localization & resources)
    // 在 crates/arc/native/rt_resources.ani 中声明，由 emit_native_decls 发射 declare。
    // rt_os_current_uilocale/locale 也已在 .ani 声明，此处为后备（未来移除）。

    // Type info field query ABI (RFC 018 M2: RtTypeInfo field accessors)
    out.push_str("declare ptr  @rt_type_get_name(ptr)\n");
    out.push_str("declare ptr  @rt_type_get_full_name(ptr)\n");
    out.push_str("declare i32  @rt_type_get_kind(ptr)\n");
    out.push_str("declare ptr  @rt_type_get_base(ptr)\n");

    // Assembly execution context ABI (RFC 017 M1)
    // 在 crates/arc/native/rt_library.ani 中声明，由 emit_native_decls 发射 declare，
    // 此处不重复声明。

    // Window + events (platform library)
    out.push_str("declare ptr  @rt_window_create(ptr, i32, i32)\n");
    out.push_str("declare void @rt_window_destroy(ptr)\n");
    out.push_str("declare i32  @rt_window_should_close(ptr)\n");
    out.push_str("declare i32  @rt_event_poll(ptr)\n");
    out.push_str("declare i32  @rt_event_wait(ptr, i32)\n");
    out.push_str("declare void @rt_ui_wake_ui_thread()\n");
    out.push_str("declare void @rt_window_close(ptr)\n");
    // Arc `Window.Run(title, width, height)` builtin bridge (crates/runtime-ui/platform/<os>/window.*)
    out.push_str("declare void @__arc_window_run(ptr, i32, i32)\n");
    // Arc `Window.RunWithText(title, width, height, text)` builtin bridge (RFC 037 ARML demo)
    out.push_str("declare void @__arc_window_run_with_text(ptr, i32, i32, ptr)\n");
    // Arc `rt_window_set_text` ABI (platform-specific, stub on non-Win32)
    out.push_str("declare void @rt_window_set_text(ptr, ptr)\n");
    // RFC 037 M3 UI Element Tree ABI —— 跨平台元素树镜像（wgpu 唯一后端）
    // 声明与 crates/runtime-ui/platform/<os>/window.* 公共 C ABI 一一对应。handle 在 Arc 侧为 i64
    //（long），codegen 在调用前 emit `inttoptr`，返回后 emit `ptrtoint`。
    out.push_str("declare ptr  @rt_ui_element_create(ptr)\n");
    out.push_str("declare void @rt_ui_element_set_string(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_ui_element_set_number(ptr, ptr, double)\n");
    out.push_str("declare void @rt_ui_element_set_bool(ptr, ptr, i32)\n");
    out.push_str("declare void @rt_ui_element_add_child(ptr, ptr)\n");
    out.push_str("declare void @rt_ui_element_destroy(ptr)\n");
    // RFC 037 M3.5 元素树只读访问器——供 WgpuRender.RenderElementTree 遍历
    out.push_str("declare ptr   @rt_ui_element_get_type_name(ptr)\n");
    out.push_str("declare ptr   @rt_ui_element_get_string(ptr, ptr, ptr)\n");
    out.push_str("declare double @rt_ui_element_get_number(ptr, ptr, double)\n");
    out.push_str("declare i32   @rt_ui_element_get_bool(ptr, ptr, i32)\n");
    out.push_str("declare i32   @rt_ui_element_get_child_count(ptr)\n");
    out.push_str("declare ptr   @rt_ui_element_get_child(ptr, i32)\n");
    out.push_str("declare void @rt_ui_set_button_click_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_window_set_root_element(ptr, ptr)\n");
    out.push_str("declare void @rt_window_set_wgpu_active(ptr, i32)\n");
    out.push_str("declare void @rt_ui_element_set_arc_ptr(ptr, i64)\n");
    out.push_str(
        "declare ptr  @rt_ui_hit_test(ptr, i32, i32, i32, i32)
",
    );
    out.push_str(
        "declare void @rt_ui_set_button_visual_state_handler(ptr, ptr)
",
    );
    out.push_str("declare void @rt_ui_set_control_click_handler(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_ui_set_control_visual_state_handler(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_ui_set_control_drag_handler(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_ui_clear_control_handlers()\n");
    out.push_str("declare void @rt_ui_set_input_focus_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_ui_set_input_click_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_ui_set_keyboard_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_window_invalidate(ptr)\n");
    out.push_str("declare void @rt_ui_ime_install_arc_handler()\n");
    out.push_str("declare void @rt_ui_ime_set_focus(ptr)\n");
    out.push_str("declare void @rt_ui_ime_set_candidate_rect(ptr, i32, i32, i32, i32)\n");
    // Arc `WindowHost.RunWithRoot(title, w, h, root_handle)` builtin bridge
    // (RFC 037 M3) —— root_handle 为 i64 句柄，C 侧 cast 为 RtUiElement*。
    out.push_str("declare void @__arc_window_run_with_root(ptr, i32, i32, i64)\n");
    // RFC 037 §D7.2: 提取平台原生窗口 handle（HWND/Window/NSView）供
    // WgpuRender.Initialize → wgpu_create_surface_from_handle 使用。
    // Arc 侧 WindowHost.NativeHandle(window) → codegen emit 此 ABI。
    out.push_str("declare i64 @rt_window_native_handle(ptr)\n");
    // 获取窗口客户区实际尺寸（out i32* w, out i32* h）
    out.push_str("declare void @rt_window_get_client_size(ptr, ptr, ptr)\n");
    // 系统 DPI 缩放系数（DPI / 96.0）
    out.push_str("declare double @rt_window_dpi_scale()\n");

    // LLVM math intrinsics + libm (RFC 021 Phase 0) — no rt_math_* ABI
    out.push_str("; ---- LLVM math intrinsics (RFC 021 Phase 0) ----\n");
    out.push_str("declare double @llvm.sqrt.f64(double)\n");
    out.push_str("declare double @llvm.sin.f64(double)\n");
    out.push_str("declare double @llvm.cos.f64(double)\n");
    out.push_str("declare double @llvm.exp.f64(double)\n");
    out.push_str("declare double @llvm.log.f64(double)\n");
    out.push_str("declare double @llvm.log10.f64(double)\n");
    out.push_str("declare double @llvm.log2.f64(double)\n");
    out.push_str("declare double @llvm.pow.f64(double, double)\n");
    out.push_str("declare double @llvm.fabs.f64(double)\n");
    out.push_str("declare double @llvm.floor.f64(double)\n");
    out.push_str("declare double @llvm.ceil.f64(double)\n");
    out.push_str("declare double @llvm.round.f64(double)\n");
    out.push_str("declare double @llvm.trunc.f64(double)\n");
    out.push_str("declare i32 @llvm.abs.i32(i32, i1)\n");
    out.push_str("declare i64 @llvm.abs.i64(i64, i1)\n");
    out.push_str("declare double @llvm.minnum.f64(double, double)\n");
    out.push_str("declare double @llvm.maxnum.f64(double, double)\n");
    out.push_str("declare double @llvm.fmuladd.f64(double, double, double)\n");
    out.push_str("declare double @llvm.copysign.f64(double, double)\n");
    // libm (no LLVM intrinsic): Tan/Asin/Acos/Atan/Atan2/Sinh/Cosh/Tanh/Cbrt/Hypot/remainder
    out.push_str("; ---- libm math (RFC 021 honesty deepen) ----\n");
    out.push_str("declare double @tan(double)\n");
    out.push_str("declare double @asin(double)\n");
    out.push_str("declare double @acos(double)\n");
    out.push_str("declare double @atan(double)\n");
    out.push_str("declare double @atan2(double, double)\n");
    out.push_str("declare double @sinh(double)\n");
    out.push_str("declare double @cosh(double)\n");
    out.push_str("declare double @tanh(double)\n");
    out.push_str("declare double @cbrt(double)\n");
    out.push_str("declare double @hypot(double, double)\n");
    out.push_str("declare double @remainder(double, double)\n");

    // LLVM vector FMA intrinsics (RFC 021 Phase 2) — for Vector<float, N> / Vector<double, N>
    out.push_str("; ---- LLVM vector FMA intrinsics (RFC 021 Phase 2) ----\n");
    out.push_str(
        "declare <4 x float>  @llvm.fmuladd.v4f32(<4 x float>, <4 x float>, <4 x float>)\n",
    );
    out.push_str(
        "declare <8 x float>  @llvm.fmuladd.v8f32(<8 x float>, <8 x float>, <8 x float>)\n",
    );
    out.push_str(
        "declare <16 x float> @llvm.fmuladd.v16f32(<16 x float>, <16 x float>, <16 x float>)\n",
    );
    out.push_str(
        "declare <4 x double>  @llvm.fmuladd.v4f64(<4 x double>, <4 x double>, <4 x double>)\n",
    );
    out.push_str(
        "declare <8 x double>  @llvm.fmuladd.v8f64(<8 x double>, <8 x double>, <8 x double>)\n",
    );
    out.push_str(
        "declare <16 x double> @llvm.fmuladd.v16f64(<16 x double>, <16 x double>, <16 x double>)\n",
    );

    // Tensor<T> runtime ABI (RFC 021 Phase 1)
    out.push_str("; ---- Tensor runtime ABI (RFC 021 Phase 1) ----\n");
    out.push_str("declare ptr  @rt_tensor_create(i32, i32, i32)\n");
    out.push_str("declare void @rt_tensor_destroy(ptr)\n");
    out.push_str("declare i32  @rt_tensor_rank(ptr)\n");
    out.push_str("declare i32  @rt_tensor_rows(ptr)\n");
    out.push_str("declare i32  @rt_tensor_cols(ptr)\n");
    out.push_str("declare i32  @rt_tensor_total(ptr)\n");
    out.push_str("declare void @rt_tensor_get(ptr, i32, i32, ptr)\n");
    out.push_str("declare void @rt_tensor_set(ptr, i32, i32, ptr)\n");
    out.push_str("declare ptr  @rt_tensor_add(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_tensor_sub(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_tensor_mul(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_tensor_matmul(ptr, ptr)\n");

    // Arc.Security crypto ABI (RFC 026 M3: arc-security).
    // All entry points return a freshly malloc'd NUL-terminated lowercase-hex
    // string; caller owns it (ARC manages the returned ptr).
    out.push_str("declare ptr @rt_crypto_md5(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha1(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha256(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha512(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha384(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha3_256(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha3_512(ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha256(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha384(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha512(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_random_bytes(i32)\n");
    /* byte[] 变体（RFC 026 M3 修订）：字节进字节出，绕过 hex-string 中转。
     * 入参/出参均为 RtArray byte[] payload（见 rt_abi.h 对应段）；失败返回
     * NULL，Arc 门面负责转译为 CryptographicException。 */
    out.push_str("declare ptr @rt_crypto_md5_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha1_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha256_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha384_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha512_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha3_256_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_sha3_512_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha256_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha384_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_hmac_sha512_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_crypto_random_bytes_arr(i32)\n");
    /* RFC 026 M1: S0 TLS 1.3 原语 ABI（vendored crypto_native.dll · mbedTLS）。
     * 命名与 RFC 042 P2P 在途 ABI（rt_crypto_aead_* / rt_crypto_x25519_dh）区分，
     * 避免符号冲突；byte[] 载体为 RtArrayHeader+payload（免 hex 往返）。 */
    out.push_str("declare ptr  @rt_crypto_aesgcm_new_key()\n");
    out.push_str("declare ptr  @rt_crypto_aesgcm_encrypt(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_aesgcm_decrypt(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_rsa_keygen(i32)\n");
    out.push_str("declare ptr  @rt_crypto_rsa_spki_export(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_rsa_spki_import(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_rsa_pkcs8_export(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_rsa_sign_pss(ptr, ptr)\n");
    out.push_str("declare i32  @rt_crypto_rsa_verify_pss(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_crypto_x25519_keygen()\n");
    out.push_str("declare ptr  @rt_crypto_x25519_pubkey(i32)\n");
    out.push_str("declare i32  @rt_crypto_x25519_import_private(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_x25519_derive(i32, ptr)\n");
    /* RFC 026 M3: X.509 证书解析（vendored crypto_native.dll · mbedTLS）。
     * opaque 句柄 = mbedtls_x509_crt*（64 位指针直传）；subject 返回 C 字符串；pubkey
     * 返回 mbedtls_pk_context*（RSA 公钥，可直接进 rt_crypto_rsa_verify_pss）。 */
    out.push_str("declare ptr  @rt_crypto_x509_parse_der(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_x509_parse_pem(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_x509_subject(ptr)\n");
    out.push_str("declare ptr  @rt_crypto_x509_pubkey(ptr)\n");
    out.push_str("declare i32  @rt_crypto_x509_verify(ptr, ptr)\n");
    out.push_str("declare void @rt_crypto_x509_free(ptr)\n");
    /* RFC 026 M3: TLS 1.3 会话（TlsClientSession）。handle = opaque tls_session*（64 位
     * 指针直传）；handshake 返回 send_out byte[] + state 经 out 参数；alpn 返回 C 字符串。 */
    out.push_str("declare ptr  @rt_crypto_tls_client_new(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_server_new(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_handshake(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_write(ptr, ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_read(ptr, ptr, ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_crypto_tls_alpn(ptr)\n");
    out.push_str("declare void @rt_crypto_tls_free(ptr)\n");
    /* RFC 026 S5: TLS 1.3 完整面（逐项独立立宪 · 不改既有签名/语义）。
     * set_verify(handle, mode, blob)：mode 0=None/1=Anchor DER/2=FullChain PEM；
     * set_crl(handle, crl_der)；verify_result(handle) → i32 位标志（0=通过）；
     * set_client_cert(handle, cert_der, key_der)；session_save(handle) → byte[]；
     * session_load(handle, bytes)；server_new_ex(cert, key, alpn, flags, ca_blob)；
     * drain(handle) → byte[]；enable_early_data(handle, enabled)；
     * write_early_data(handle, recv, plain, int32_t* state) → byte[]；
     * early_data_status(handle) → i32（0=未指示/1=ACCEPTED/2=REJECTED）；
     * read_early_data(handle, enc, buffer, offset, count) → i32。 */
    out.push_str("declare i32  @rt_crypto_tls_set_verify(ptr, i32, ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_load_system_roots(ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_set_crl(ptr, ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_verify_result(ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_set_client_cert(ptr, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_session_save(ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_session_load(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_server_new_ex(ptr, ptr, ptr, i32, ptr)\n");
    out.push_str("declare ptr  @rt_crypto_tls_drain(ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_enable_early_data(ptr, i32)\n");
    out.push_str("declare ptr  @rt_crypto_tls_write_early_data(ptr, ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_early_data_status(ptr)\n");
    out.push_str("declare i32  @rt_crypto_tls_read_early_data(ptr, ptr, ptr, i32, i32)\n");
    /* RFC 042 M1: P2P crypto primitives */
    out.push_str("declare void @rt_crypto_ed25519_keygen(ptr, ptr)\n");
    out.push_str("declare void @rt_crypto_ed25519_seed_keygen(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_crypto_ed25519_sign(ptr, i32, ptr, ptr)\n");
    out.push_str("declare i32 @rt_crypto_ed25519_verify(ptr, i32, ptr, ptr)\n");
    /* Ed25519 RtArray byte[] 包装（PeerKey 门面专用；语义见 rt_abi.h）。 */
    out.push_str("declare ptr @rt_crypto_ed25519_keygen_arr()\n");
    out.push_str("declare ptr @rt_crypto_ed25519_seed_keygen_arr(ptr)\n");
    out.push_str("declare ptr @rt_crypto_ed25519_sign_arr(ptr, ptr)\n");
    out.push_str("declare i32 @rt_crypto_ed25519_verify_arr(ptr, ptr, ptr)\n");
    out.push_str("declare void @rt_crypto_x25519_dh(ptr, ptr, ptr)\n");
    out.push_str("declare i32 @rt_crypto_aead_encrypt(ptr, i32, ptr, ptr, ptr, i32, ptr, ptr)\n");
    out.push_str("declare i32 @rt_crypto_aead_decrypt(ptr, i32, ptr, ptr, ptr, i32, ptr, ptr)\n");
    /* RFC 042 M5: Noise Protocol */
    out.push_str("declare ptr @rt_noise_session_create(ptr, ptr, i32)\n");
    out.push_str("declare void @rt_noise_session_destroy(ptr)\n");
    out.push_str("declare i32 @rt_noise_initiate_handshake(ptr, ptr, i32)\n");
    out.push_str("declare i32 @rt_noise_respond_handshake(ptr, ptr, i32, ptr, i32)\n");
    out.push_str("declare i32 @rt_noise_initiate_finalize(ptr, ptr, i32)\n");
    out.push_str("declare i32 @rt_noise_session_encrypt(ptr, ptr, i32, ptr, ptr)\n");
    out.push_str("declare i32 @rt_noise_session_decrypt(ptr, ptr, i32, ptr, ptr)\n");
    /* RFC 042 M5 P0-2: Noise byte[] 门面（arr wrapper；数组出参为新建 RtArray） */
    out.push_str("declare ptr @rt_noise_session_create_arr(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_noise_initiate_handshake_arr(ptr)\n");
    out.push_str("declare ptr @rt_noise_respond_handshake_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_noise_initiate_finalize_arr(ptr, ptr)\n");
    out.push_str("declare i32 @rt_noise_respond_finalize_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_noise_session_encrypt_arr(ptr, ptr)\n");
    out.push_str("declare ptr @rt_noise_session_decrypt_arr(ptr, ptr, ptr)\n");
    out.push_str("declare ptr @rt_noise_session_handshake_hash_arr(ptr)\n");
    /* RFC 042 M8: Kademlia DHT */
    out.push_str("declare ptr @rt_kad_table_create()\n");
    out.push_str("declare void @rt_kad_table_destroy(ptr)\n");
    out.push_str("declare void @rt_kad_table_set_local(ptr, ptr)\n");
    out.push_str("declare i32 @rt_kad_table_add(ptr, ptr, ptr)\n");
    out.push_str("declare i32 @rt_kad_table_remove(ptr, ptr)\n");
    out.push_str("declare i32 @rt_kad_table_find_nearest(ptr, ptr, i32)\n");

    // Arc.Net network ABI (RFC 025 M4: arc-net).
    // Socket facade: handle is an opaque RtSocket* ptr stored as object payload.
    // Methods take handle as first arg via codegen receiver dispatch.
    out.push_str("declare ptr  @rt_socket_create(i32, i32, i32)\n");
    out.push_str("declare void @rt_socket_close(ptr)\n");
    out.push_str("declare i32  @rt_socket_connect(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_bind(ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_listen(ptr, i32)\n");
    out.push_str("declare ptr  @rt_socket_accept(ptr)\n");
    out.push_str("declare i32  @rt_socket_send(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_socket_receive(ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_sendto_bytes(ptr, ptr, i32, ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_recvfrom_bytes(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_available(ptr)\n");
    out.push_str("declare i32  @rt_socket_connected(ptr)\n");
    out.push_str("declare void @rt_socket_shutdown(ptr, i32)\n");
    out.push_str("declare i32  @rt_socket_poll(ptr, i32, i32)\n");
    out.push_str("declare void @rt_socket_set_recv_timeout(ptr, i32)\n");
    out.push_str("declare void @rt_socket_set_send_timeout(ptr, i32)\n");
    out.push_str("declare void @rt_socket_set_no_delay(ptr, i32)\n");
    out.push_str("declare void @rt_socket_set_send_buf_size(ptr, i32)\n");
    out.push_str("declare void @rt_socket_set_recv_buf_size(ptr, i32)\n");
    // RFC 048: Named pipe facade (本机 IPC · rt_pipe_* 同步面)
    out.push_str("declare ptr  @rt_pipe_server_create(ptr, i32)\n");
    out.push_str("declare i32  @rt_pipe_server_wait_connect(ptr)\n");
    out.push_str("declare ptr  @rt_pipe_client_create(ptr)\n");
    out.push_str("declare i32  @rt_pipe_client_connect(ptr, i32)\n");
    out.push_str("declare i32  @rt_pipe_read(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_pipe_write(ptr, ptr, i32)\n");
    out.push_str("declare i32  @rt_pipe_server_disconnect(ptr)\n");
    out.push_str("declare i32  @rt_pipe_is_connected(ptr)\n");
    out.push_str("declare void @rt_pipe_close(ptr)\n");
    // RFC 009 M2: Async network IO facade (returns RtTask* as completion token)
    out.push_str("declare ptr  @rt_socket_connect_async(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_socket_accept_async(ptr)\n");
    out.push_str("declare ptr  @rt_socket_send_async(ptr, ptr, i32)\n");
    out.push_str("declare ptr  @rt_socket_receive_async(ptr, i32)\n");
    // RFC 009 异步为主：字节面异步接收（写入调用方 buffer，含 0x00 不 NUL 截断；
    // 用于 TLS 密文等二进制面真异步读；TcpClient.ReceiveBytesAsync）。
    out.push_str("declare ptr  @rt_socket_receive_bytes_async(ptr, ptr, i32)\n");
    // RFC 009 M2: IO completion handler (called by EventLoop tick on reactor_poll events)
    out.push_str("declare void @rt_io_completion_complete(ptr, i32)\n");
    // DNS facade
    out.push_str("declare ptr  @rt_dns_resolve(ptr)\n");
    out.push_str("declare ptr  @rt_dns_get_host_name()\n");
    out.push_str("declare ptr  @rt_dns_resolve_all(ptr)\n");

    // Arc.Text text-processing ABI (RFC 021 §4.3 M4): Base64/Hex codecs.
    // All return a freshly malloc'd NUL-terminated string (ARC-managed).
    out.push_str("declare ptr @rt_text_base64_encode(ptr)\n");
    out.push_str("declare ptr @rt_text_base64_decode(ptr)\n");
    out.push_str("declare ptr @rt_text_hex_encode(ptr)\n");
    out.push_str("declare ptr @rt_text_hex_decode(ptr)\n");
    /* RFC 026 M1 §1.2 ⑥: Arc.Text.Hex.ToHexString(byte[]) / FromHexString(string) */
    out.push_str("declare ptr @rt_text_hex_bytes_encode(ptr)\n");
    out.push_str("declare ptr @rt_text_hex_bytes_decode(ptr)\n");
    /* RFC 037 M1 §1.2 ⑥: Arc.Text.Base64.ToBase64String(byte[]) / FromBase64String(string) */
    out.push_str("declare ptr @rt_text_base64_bytes_encode(ptr)\n");
    out.push_str("declare ptr @rt_text_base64_bytes_decode(ptr)\n");
    // Arc.Text.Url percent-encoding (Encode/Decode).
    out.push_str("declare ptr @rt_text_url_encode(ptr)\n");
    out.push_str("declare ptr @rt_text_url_decode(ptr)\n");
    // UTF-8 Encoding.GetBytes / GetString (std readiness P0).
    out.push_str("declare ptr @rt_text_utf8_get_bytes(ptr)\n");
    out.push_str("declare ptr @rt_text_utf8_get_string(ptr)\n");
    out.push_str("declare i32 @rt_text_utf8_get_byte_count(ptr)\n");
    // Encoding variants: UTF-16LE / Latin-1 (byte[] interop).
    out.push_str("declare ptr @rt_text_utf16_get_bytes(ptr)\n");
    out.push_str("declare ptr @rt_text_utf16_get_string(ptr)\n");
    out.push_str("declare ptr @rt_text_latin1_get_bytes(ptr)\n");
    out.push_str("declare ptr @rt_text_latin1_get_string(ptr)\n");
    // Regex facade (rt_regex.c): pattern/input/replacement are ptr strings.
    out.push_str("declare i32 @rt_regex_is_match(ptr, ptr)\n");
    out.push_str("declare ptr @rt_regex_match(ptr, ptr)\n");
    out.push_str("declare ptr @rt_regex_match_group(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_regex_matches(ptr, ptr)\n");
    out.push_str("declare ptr @rt_regex_replace(ptr, ptr, ptr)\n");
    out.push_str("declare ptr @rt_regex_split(ptr, ptr)\n");
    // Regex with RegexOptions (int32 flags): IgnoreCase=1 Multiline=2 Singleline=4 ExplicitCapture=8.
    out.push_str("declare i32 @rt_regex_is_match_opt(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_regex_match_opt(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_regex_match_group_opt(ptr, ptr, i32, i32)\n");
    out.push_str("declare ptr @rt_regex_matches_opt(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_regex_replace_opt(ptr, ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_regex_split_opt(ptr, ptr, i32)\n");
    out.push_str("declare ptr @rt_guid_to_byte_array(ptr)\n");
    out.push_str("declare ptr @rt_guid_from_byte_array(ptr)\n");
    // StringBuilder facade: handle is opaque ptr; append* return the handle ptr.
    out.push_str("declare ptr  @rt_text_sb_new()\n");
    out.push_str("declare ptr  @rt_text_sb_new_with_str(ptr)\n");
    out.push_str("declare ptr  @rt_text_sb_new_with_capacity(i32)\n");
    out.push_str("declare ptr  @rt_text_sb_append(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_text_sb_append_int(ptr, i32)\n");
    out.push_str("declare ptr  @rt_text_sb_append_long(ptr, i64)\n");
    out.push_str("declare ptr  @rt_text_sb_append_bool(ptr, i8)\n");
    out.push_str("declare ptr  @rt_text_sb_append_char(ptr, i32)\n");
    out.push_str("declare ptr  @rt_text_sb_append_float(ptr, float)\n");
    out.push_str("declare ptr  @rt_text_sb_append_double(ptr, double)\n");
    out.push_str("declare ptr  @rt_text_sb_append_line(ptr, ptr)\n");
    out.push_str("declare ptr  @rt_text_sb_to_string(ptr)\n");
    out.push_str("declare ptr  @rt_text_sb_to_string_range(ptr, i32, i32)\n");
    out.push_str("declare i32  @rt_text_sb_length(ptr)\n");
    out.push_str("declare i32  @rt_text_sb_get_capacity(ptr)\n");
    out.push_str("declare void @rt_text_sb_clear(ptr)\n");
    out.push_str("declare void @rt_text_sb_ensure_capacity(ptr, i32)\n");
    out.push_str("declare ptr  @rt_text_sb_insert(ptr, i32, ptr)\n");
    out.push_str("declare ptr  @rt_text_sb_remove(ptr, i32, i32)\n");
    out.push_str("declare ptr  @rt_text_sb_replace(ptr, ptr, ptr)\n");
    out.push_str("declare i32  @rt_text_sb_get_char(ptr, i32)\n");
    out.push_str("declare void @rt_text_sb_set_char(ptr, i32, i32)\n");

    // Parse ABI (RFC 007 M1): 数值类型字符串解析
    out.push_str("declare i32  @rt_parse_int32(ptr)\n");
    out.push_str("declare i32  @rt_parse_int32_try(ptr, ptr)\n");
    out.push_str("declare i64  @rt_parse_int64(ptr)\n");
    out.push_str("declare i32  @rt_parse_int64_try(ptr, ptr)\n");
    out.push_str("declare double @rt_parse_double(ptr)\n");
    out.push_str("declare i32  @rt_parse_double_try(ptr, ptr)\n");
    out.push_str("declare float @rt_parse_float(ptr)\n");
    out.push_str("declare i32  @rt_parse_float_try(ptr, ptr)\n");
    out.push_str("declare i32  @rt_parse_bool(ptr)\n");
    out.push_str("declare i32  @rt_parse_bool_try(ptr, ptr)\n");
    out.push_str("declare i32  @rt_parse_char(ptr)\n");
    out.push_str("declare i32  @rt_parse_char_try(ptr, ptr)\n");
    out.push_str("declare i32  @rt_char_is_digit(i32)\n");
    out.push_str("declare i32  @rt_char_is_letter(i32)\n");
    out.push_str("declare i32  @rt_char_is_white_space(i32)\n");
    out.push_str("declare i32  @rt_char_is_upper(i32)\n");
    out.push_str("declare i32  @rt_char_is_lower(i32)\n");
    out.push_str("declare i32  @rt_char_to_upper(i32)\n");
    out.push_str("declare i32  @rt_char_to_lower(i32)\n");
    out.push_str("declare i32  @rt_parse_uint32(ptr)\n");
    out.push_str("declare i32  @rt_parse_uint32_try(ptr, ptr)\n");
    out.push_str("declare i64  @rt_parse_uint64(ptr)\n");
    out.push_str("declare i32  @rt_parse_uint64_try(ptr, ptr)\n");

    // ToString ABI: 数值类型 → 字符串
    out.push_str("declare ptr  @rt_int_to_string(i32)\n");
    out.push_str("declare ptr  @rt_long_to_string(i64)\n");
    out.push_str("declare ptr  @rt_short_to_string(i16)\n");
    out.push_str("declare ptr  @rt_byte_to_string(i8)\n");
    out.push_str("declare ptr  @rt_float_to_string(float)\n");
    out.push_str("declare ptr  @rt_double_to_string(double)\n");
    out.push_str("declare ptr  @rt_bool_to_string(i32)\n");
    out.push_str("declare ptr  @rt_char_to_string(i32)\n");
    out.push_str("declare ptr  @rt_uint_to_string(i32)\n");
    out.push_str("declare ptr  @rt_ulong_to_string(i64)\n");
    out.push_str("declare ptr  @rt_ushort_to_string(i16)\n");
    out.push_str("declare ptr  @rt_sbyte_to_string(i8)\n");
    // RFC 007 M2a/M2b/M2c: format-aware ToString (D/X/F/G/N/C/E/P + custom 0/0.00)
    out.push_str("declare ptr  @rt_int_to_string_fmt(i32, ptr)\n");
    out.push_str("declare ptr  @rt_long_to_string_fmt(i64, ptr)\n");
    out.push_str("declare ptr  @rt_short_to_string_fmt(i16, ptr)\n");
    out.push_str("declare ptr  @rt_byte_to_string_fmt(i8, ptr)\n");
    out.push_str("declare ptr  @rt_sbyte_to_string_fmt(i8, ptr)\n");
    out.push_str("declare ptr  @rt_uint_to_string_fmt(i32, ptr)\n");
    out.push_str("declare ptr  @rt_ulong_to_string_fmt(i64, ptr)\n");
    out.push_str("declare ptr  @rt_ushort_to_string_fmt(i16, ptr)\n");
    out.push_str("declare ptr  @rt_float_to_string_fmt(float, ptr)\n");
    out.push_str("declare ptr  @rt_double_to_string_fmt(double, ptr)\n");
    // RFC 027 M5: culture-aware ToString(format, provider) → rt_*_to_string_fmt_p(value, format, provider)
    out.push_str("declare ptr  @rt_int_to_string_fmt_p(i32, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_long_to_string_fmt_p(i64, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_short_to_string_fmt_p(i16, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_byte_to_string_fmt_p(i8, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_sbyte_to_string_fmt_p(i8, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_uint_to_string_fmt_p(i32, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_ulong_to_string_fmt_p(i64, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_ushort_to_string_fmt_p(i16, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_float_to_string_fmt_p(float, ptr, ptr)\n");
    out.push_str("declare ptr  @rt_double_to_string_fmt_p(double, ptr, ptr)\n");
    out.push('\n'); // libc
    out.push_str("declare ptr @malloc(i64)\n");
    out.push_str("declare ptr @calloc(i64, i64)\n");
    out.push_str("declare void @free(ptr)\n");

    out.push('\n');
    out.push_str("declare void @rt_ui_set_scroll_wheel_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_ui_set_scroll_bar_handler(ptr, ptr)\n");
    out.push_str("declare void @rt_ui_invalidate_active_window()\n");
    out
}
