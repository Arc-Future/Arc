// RFC 016 M3 §3.3 List<T> marshal e2e 测试辅助 C 实现。
//
// 本文件为 `crates/arc/native/arc_test.ani` 契约的 C 实现，随 runtime 一起编译链接。
// 函数签名对应 `.ani` 声明中 `List<T>` 展开后的 `ptr buffer, i32 size` 两个参数。
//
// 设计意图：
// - 验证 codegen `emit_call` native dispatch 对 `ParamMarshal::List` 参数正确生成
//   `rt_list_buffer_and_size` 调用并传递 buffer+size 两个 LLVM 参数
// - 验证零拷贝传递：C 函数直接访问 List<T> 内部 buffer，无拷贝
// - 验证 buffer 内容正确：求和结果与 Arc 侧预期一致
//
// 性能考量（RFC 009 M5 零分配热路径 + RFC 009 IO 吞吐 ≥20×）：
// - C 函数直接遍历 buffer，O(n) 复杂度，无额外分配
// - 与未来 ORM 查询结果 List<T> 直接传递给 FFI 的场景协同

#include "rt_abi.h"

// 求和 List<int> 所有元素。
// C 签名：int rt_native_test_sum_list(const int* buf, int size)
int rt_native_test_sum_list(const int* buf, int size) {
    int sum = 0;
    for (int i = 0; i < size; i++) {
        sum += buf[i];
    }
    return sum;
}

// 返回 List<int> 的元素数量（直接回传 size 参数）。
// C 签名：int rt_native_test_list_size(const int* buf, int size)
int rt_native_test_list_size(const int* buf, int size) {
    // 静默未使用参数警告：buf 不被读取，仅验证 size 参数正确传递
    (void)buf;
    return size;
}

// RFC 016 M1：调用 callback 验证 trampoline 端到端通路。
// C 签名：int rt_native_test_call_cb(int (*cb)(int, int), int a, int b)
//
// 实现：直接调用 cb(a, b) 返回结果。
// Arc 侧传入无捕获 lambda → codegen 生成 trampoline（剥离 env 参数）
// → 传 trampoline 函数指针给 C → C 端通过函数指针调用 trampoline
// → trampoline 调用 Arc lambda 函数 → 返回结果。
//
// 本函数用于验证整条 trampoline 调用链路：
// 1. codegen 正确识别 callback 形参类型
// 2. trampoline 函数 IR 正确生成（参数顺序、返回类型匹配）
// 3. trampoline 在模块级发射且符号可见
// 4. C 端通过函数指针调用 trampoline 时 ABI 兼容
int rt_native_test_call_cb(int (*cb)(int, int), int a, int b) {
    return cb(a, b);
}

// RFC 005 M3/M4：循环收集器测试钩子（arc_test 契约）。
// 转发到 crates/runtime/rt_arc.c 的试删收集入口；返回 0 表示调用成功。
// 开启开关见 rt_arc_set_cycle_collection（由 rt_arc.c 直接提供）。
int rt_arc_collect(void) {
    rt_arc_collect_cycles();
    return 0;
}

// ---- RFC 017 §2.3：根扫描遍历测试钩子（rt_arc_walk_fields） ----

// 与 `rt_lib_scan_visit`（rt_library.c）同构的 visited 集环防护 DFS——
// 测试直接复现运行时根扫描的遍历语义：沿 strong class 字段（vtable slot 2
// walk 函数）递归，visited 集按指针去重，环/共享节点不重复访问、必然终止。

#define RT_NATIVE_WALK_CAP 64

typedef struct {
    void* visited[RT_NATIVE_WALK_CAP];
    int32_t count;
} RtNativeWalkCtx;

static void rt_native_walk_visit(void* ctx, void* field) {
    RtNativeWalkCtx* c = (RtNativeWalkCtx*)ctx;
    if (!field) return;
    if (c->count >= RT_NATIVE_WALK_CAP) return;
    for (int32_t i = 0; i < c->count; i++) {
        if (c->visited[i] == field) return;
    }
    c->visited[c->count++] = field;
}

// 从 root 出发经 rt_arc_walk_fields BFS 统计可达 distinct 对象数（含 root 自身）。
// 环防护生效 → 遍历必然终止；超 RT_NATIVE_WALK_CAP 返回 -1（遍历失控信号）。
// C 签名：int rt_native_test_walk_count(void* root)（arc_test.ani：NativePtr → ptr）
int rt_native_test_walk_count(void* root) {
    if (!root) return 0;
    RtNativeWalkCtx c;
    c.count = 0;
    c.visited[c.count++] = root;
    int32_t head = 0;
    while (head < c.count) {
        void* obj = c.visited[head++];
        rt_arc_walk_fields(obj, rt_native_walk_visit, &c);
    }
    if (c.count >= RT_NATIVE_WALK_CAP) return -1;
    return (int)c.count;
}

// RFC 043 P1：故意崩溃测试钩子（arc_test.ani 契约）。
// volatile 空指针写 → 访问冲突（Windows 0xC0000005 / POSIX SIGSEGV），
// 供 AIPerfAnomaly.Crash 分类 e2e 使用（不返回）。
int rt_native_test_crash(void) {
    volatile int* p = (volatile int*)0;
    *p = 42;
    return 0;
}
