// Hierarchical Timing Wheel (RFC 009 M5.3).
//
// 3 级分级时间轮，替代 M3 有序单链表 O(n) 定时器。
// 插入/到期均 O(1)，支撑高并发定时场景（10⁵ 定时器 ≥30× vs M3）。
//
// ## 层级设计
//
//   L1: 256 槽 × 1ms     = 256ms 范围（1ms 精度）
//   L2: 256 槽 × 256ms   = 65.536s 范围
//   L3: 256 槽 × 65.536s = ~4.66h 范围
//
// 超过 L3 范围（>4.66h）的定时器 clamp 到 L3 最后一槽（极端长延迟场景罕见）。
//
// ## 核心操作
//
// - insert: O(1) — 计算 delta → 选层 → 头插
// - tick:   推进 L1 当前槽，触发到期；L1 走完一圈 → cascade L2→L1（均摊 O(1)）
// - cancel: canceled=1，tick 时跳过（惰性删除）
//
// ## 与 EventLoop 集成
//
// M3 的 rt_event_loop_add_timer_internal → rt_timer_wheel_add
// M3 的 rt_event_loop_fire_expired        → rt_timer_wheel_tick
// M3 的 next_timeout 计算                  → rt_timer_wheel_next_timeout

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ---- 常量 ---- */

#define TW_SLOTS 256
#define TW_MASK  0xFF

/* L1: 1ms 精度，范围 [0, 256ms)
 * L2: 256ms 精度，范围 [0, 65.536s)
 * L3: 65.536s 精度，范围 [0, ~4.66h) */
#define TW_L1_GRAN    1ULL          /* 1ms */
#define TW_L2_GRAN    256ULL        /* 256ms */
#define TW_L3_GRAN    65536ULL      /* 65.536s */
#define TW_L1_MAX     (TW_SLOTS * TW_L1_GRAN)   /* 256ms */
#define TW_L2_MAX     (TW_SLOTS * TW_L2_GRAN)   /* 65536ms = 65.536s */
#define TW_L3_MAX     (TW_SLOTS * TW_L3_GRAN)   /* 16777216ms ≈ 4.66h */

/* ---- 时间轮结构 ---- */

typedef struct rt_timer_wheel {
    rt_timer_node*  l1[TW_SLOTS];   /* 1ms 精度 */
    rt_timer_node*  l2[TW_SLOTS];   /* 256ms 精度 */
    rt_timer_node*  l3[TW_SLOTS];   /* 65.536s 精度 */
    uint64_t        current_ms;     /* 当前时间（ms） */
    uint32_t        l1_idx;         /* L1 当前槽 (0-255) */
    uint32_t        l2_idx;
    uint32_t        l3_idx;
    int32_t         count;          /* 当前轮内节点数（canceled 节点清理时递减） */
    int32_t         _pad[11];       /* cache line 对齐 */
} rt_timer_wheel;

/* ---- 内部辅助 ---- */

/* 将 node 头插到 slot 链表 */
static inline void tw_slot_push(rt_timer_node** slot, rt_timer_node* node) {
    node->next = *slot;
    *slot = node;
}

/* 计算定时器应插入的层级与槽位，返回层级 (1/2/3) 并通过 out_slot 返回槽位指针。
 * 使用 ABSOLUTE deadline 选槽（非 delta），保证 l1_idx/l2_idx/l3_idx 与
 * current_ms 同步时槽位精确对应到期时刻。
 * 前置条件：deadline > current_ms（调用方负责已过期检查）。 */
static int tw_select_slot(rt_timer_wheel* tw, uint64_t deadline,
                          rt_timer_node*** out_slot) {
    uint64_t delta = deadline - tw->current_ms;
    if (delta >= TW_L3_MAX) {
        /* 超过 L3 范围 → clamp 到 L3 最后一槽（相对当前 l3_idx 的 255 偏移） */
        *out_slot = &tw->l3[(tw->l3_idx + TW_MASK) & TW_MASK];
        return 3;
    }
    if (delta >= TW_L2_MAX) {
        /* L3：absolute deadline / 65536 */
        *out_slot = &tw->l3[(uint32_t)(deadline / TW_L3_GRAN) & TW_MASK];
        return 3;
    }
    if (delta >= TW_L1_MAX) {
        /* L2：absolute deadline / 256 */
        *out_slot = &tw->l2[(uint32_t)(deadline / TW_L2_GRAN) & TW_MASK];
        return 2;
    }
    /* L1：absolute (deadline - 1) & 0xFF。
     * 减 1 是因为 tick 循环先递增 current_ms 再 fire l1[l1_idx]，故 l1[I]
     * 实际在 current_ms = I+1 (mod 256) 时被触发。使用 deadline-1 对齐槽位
     * 与到期时刻。前置条件 deadline > current_ms 保证不下溢。 */
    *out_slot = &tw->l1[(uint32_t)((deadline - 1) / TW_L1_GRAN) & TW_MASK];
    return 1;
}

/* 将一条链表上的所有未取消定时器重新插入时间轮（cascade）。
 * 取消的定时器直接 free 并递减 count。
 * 已过期的定时器（deadline <= current_ms）立即触发。 */
static void tw_cascade_chain(rt_timer_wheel* tw, rt_timer_node* chain) {
    while (chain) {
        rt_timer_node* next = chain->next;
        chain->next = NULL;
        if (chain->canceled) {
            free(chain);
            tw->count--;
        } else if (chain->deadline_ms <= tw->current_ms) {
            /* cascade 时 deadline 已过 → 立即触发（避免 tw_select_slot 下溢） */
            if (chain->fn) {
                chain->fn(chain->data);
            }
            free(chain);
            tw->count--;
        } else {
            rt_timer_node** slot;
            tw_select_slot(tw, chain->deadline_ms, &slot);
            tw_slot_push(slot, chain);
        }
        chain = next;
    }
}

/* 触发一条链表上的所有到期定时器（deadline <= current_ms）。
 * 未到期的重新插入（cascade 到更精确的层级）。
 * 取消的定时器 free 并递减 count。 */
static void tw_fire_slot(rt_timer_wheel* tw, rt_timer_node** slot) {
    rt_timer_node* node = *slot;
    *slot = NULL;
    while (node) {
        rt_timer_node* next = node->next;
        node->next = NULL;
        if (node->canceled) {
            free(node);
            tw->count--;
        } else if (node->deadline_ms <= tw->current_ms) {
            /* 到期：触发回调 */
            if (node->fn) {
                node->fn(node->data);
            }
            free(node);
            tw->count--;
        } else {
            /* 未到期（精度误差）：重新插入 */
            rt_timer_node** new_slot;
            tw_select_slot(tw, node->deadline_ms, &new_slot);
            tw_slot_push(new_slot, node);
        }
        node = next;
    }
}

/* ---- 公开 ABI ---- */

rt_timer_wheel* rt_timer_wheel_create(void) {
    rt_timer_wheel* tw = (rt_timer_wheel*)calloc(1, sizeof(rt_timer_wheel));
    return tw;
}

void rt_timer_wheel_destroy(rt_timer_wheel* tw) {
    if (!tw) return;
    /* 释放所有槽位中的定时器 */
    for (int i = 0; i < TW_SLOTS; i++) {
        rt_timer_node* n = tw->l1[i];
        while (n) { rt_timer_node* next = n->next; free(n); n = next; }
        n = tw->l2[i];
        while (n) { rt_timer_node* next = n->next; free(n); n = next; }
        n = tw->l3[i];
        while (n) { rt_timer_node* next = n->next; free(n); n = next; }
    }
    free(tw);
}

void rt_timer_wheel_add(rt_timer_wheel* tw, rt_timer_node* node) {
    if (!tw || !node) return;
    node->next = NULL;
    if (node->deadline_ms <= tw->current_ms) {
        /* 已过期 → 放入当前 L1 槽，下一 tick 立即触发（避免 tw_select_slot 下溢） */
        tw_slot_push(&tw->l1[tw->l1_idx], node);
    } else {
        rt_timer_node** slot;
        tw_select_slot(tw, node->deadline_ms, &slot);
        tw_slot_push(slot, node);
    }
    tw->count++;
}

void rt_timer_wheel_tick(rt_timer_wheel* tw, uint64_t now_ms) {
    if (!tw) return;
    /* now_ms 不能倒退（单调时钟保证） */
    if (now_ms < tw->current_ms) return;

    /* 快速 jump：若 wheel 为空（无定时器可丢失），直接同步 current_ms 与 idx 到 now_ms。
     * 这是 EventLoop 创建后第一次 tick 的关键路径——rt_now_ms() 返回系统时间（很大），
     * 而 wheel 的 current_ms 初始为 0，朴素 while 循环会卡死（O(now_ms) 次）。
     * wheel 为空时 jump 无副作用（无 timer 需 fire/cascade）。 */
    if (tw->count == 0 && now_ms > tw->current_ms) {
        tw->current_ms = now_ms;
        tw->l1_idx = (uint32_t)(now_ms / TW_L1_GRAN) & TW_MASK;
        tw->l2_idx = (uint32_t)(now_ms / TW_L2_GRAN) & TW_MASK;
        tw->l3_idx = (uint32_t)(now_ms / TW_L3_GRAN) & TW_MASK;
        return;
    }

    while (tw->current_ms < now_ms) {
        tw->current_ms++;
        /* 推进 L1 当前槽 */
        tw_fire_slot(tw, &tw->l1[tw->l1_idx]);
        tw->l1_idx = (tw->l1_idx + 1) & TW_MASK;

        /* L1 走完一圈 → advance L2 idx，cascade 新 L2 槽 → L1。
         * 注意：必须先 advance 再 cascade，因为新 L2 槽存放的是
         * 下一圈 L1 范围内的定时器；旧 L2 槽的定时器应已在进入该圈时 cascade 完毕。 */
        if (tw->l1_idx == 0) {
            tw->l2_idx = (tw->l2_idx + 1) & TW_MASK;
            rt_timer_node* chain = tw->l2[tw->l2_idx];
            tw->l2[tw->l2_idx] = NULL;
            tw_cascade_chain(tw, chain);

            /* L2 走完一圈 → advance L3 idx，cascade 新 L3 槽 → L2/L1 */
            if (tw->l2_idx == 0) {
                tw->l3_idx = (tw->l3_idx + 1) & TW_MASK;
                rt_timer_node* chain3 = tw->l3[tw->l3_idx];
                tw->l3[tw->l3_idx] = NULL;
                tw_cascade_chain(tw, chain3);
            }
        }
    }
}

uint64_t rt_timer_wheel_next_timeout(rt_timer_wheel* tw) {
    if (!tw) return UINT64_MAX;
    /* 从 L1 当前槽开始扫描，找到第一个非空且未全部取消的槽 */
    for (uint32_t i = 0; i < TW_SLOTS; i++) {
        uint32_t idx = (tw->l1_idx + i) & TW_MASK;
        rt_timer_node* n = tw->l1[idx];
        /* 跳过全取消的链表 */
        while (n) {
            if (!n->canceled) {
                /* 找到有效定时器，计算距 current_ms 的剩余时间 */
                uint64_t deadline = n->deadline_ms;
                if (deadline > tw->current_ms) {
                    return deadline - tw->current_ms;
                }
                return 0;  /* 已到期 */
            }
            n = n->next;
        }
    }
    /* L1 全空或全取消 → 检查 L2/L3（返回粗略上界） */
    for (uint32_t i = 0; i < TW_SLOTS; i++) {
        uint32_t idx = (tw->l2_idx + i) & TW_MASK;
        rt_timer_node* n = tw->l2[idx];
        while (n) {
            if (!n->canceled) {
                uint64_t deadline = n->deadline_ms;
                if (deadline > tw->current_ms) {
                    return deadline - tw->current_ms;
                }
                return 0;
            }
            n = n->next;
        }
    }
    for (uint32_t i = 0; i < TW_SLOTS; i++) {
        uint32_t idx = (tw->l3_idx + i) & TW_MASK;
        rt_timer_node* n = tw->l3[idx];
        while (n) {
            if (!n->canceled) {
                uint64_t deadline = n->deadline_ms;
                if (deadline > tw->current_ms) {
                    return deadline - tw->current_ms;
                }
                return 0;
            }
            n = n->next;
        }
    }
    return UINT64_MAX;  /* 无定时器 */
}

int32_t rt_timer_wheel_count(rt_timer_wheel* tw) {
    return tw ? tw->count : 0;
}
