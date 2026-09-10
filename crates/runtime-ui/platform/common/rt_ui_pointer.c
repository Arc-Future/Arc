/*
 * RFC 037 pointer-events: hit-test + arc_ptr binding + click/visual/focus dispatch.
 * OS-agnostic geometry; Win32 WM_* in crates/runtime-ui/platform/windows/pointer_win32.c.
 */
#include "rt_ui_element_internal.h"
#include "rt_ui_platform.h"
#include "rt_ui_props.h"
#include "rt_ui_design_tokens.h"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#if defined(_WIN32)
#include <windows.h>
#endif

/* Arc 委托/lambda 调用 ABI：所有非 env 参数按「指向槽位的指针」传递
 * （codegen emit_closure_indirect_call 对每个实参 alloca+store 后传 ptr；
 * 被调 lambda 侧 `load {ty}, ptr %arg` 取值）。故 native → Arc 回调分派
 * 必须把实参值的地址传给 fn，而非直接按值传——否则 lambda 会把
 * 0/1 之类的标量当槽地址解引用 → 0xC0000005（Button 悬停实测）。 */
typedef void (*RtUiButtonClickFnCap)(void* env, int64_t* platform_handle);
typedef void (*RtUiButtonClickFnBare)(int64_t* platform_handle);
typedef void (*RtUiButtonVisualFnCap)(void* env, int64_t* platform_handle,
                                        int32_t* is_mouse_over, int32_t* is_pressed);
typedef void (*RtUiButtonVisualFnBare)(int64_t* platform_handle,
                                         int32_t* is_mouse_over, int32_t* is_pressed);
typedef void (*RtUiInputFocusFnCap)(void* env, int64_t* platform_handle);
typedef void (*RtUiInputFocusFnBare)(int64_t* platform_handle);
/* M-caret2：点击定位 caret（env 为 null 的静态委托走 bare 路径，全指针参数）。 */
typedef void (*RtUiInputClickFnCap)(void* env, int64_t* platform_handle,
                                    double* local_dip_x);
typedef void (*RtUiInputClickFnBare)(int64_t* platform_handle,
                                     double* local_dip_x);

static RtUiButtonClickFnCap g_rt_ui_button_click_fn = NULL;
static void* g_rt_ui_button_click_env = NULL;
static RtUiButtonVisualFnCap g_rt_ui_button_visual_fn = NULL;
static void* g_rt_ui_button_visual_env = NULL;
static RtUiInputFocusFnCap g_rt_ui_input_focus_fn = NULL;
static void* g_rt_ui_input_focus_env = NULL;

/* ===== RFC 037 D10.6 泛化控件注册表（按 type_name；与 Button 专用通道并行）===== */

typedef void (*RtUiControlClickFnCap)(void* env, int64_t* platform_handle);
typedef void (*RtUiControlClickFnBare)(int64_t* platform_handle);
typedef void (*RtUiControlVisualFnCap)(void* env, int64_t* platform_handle,
                                       int32_t* is_mouse_over, int32_t* is_pressed);
typedef void (*RtUiControlVisualFnBare)(int64_t* platform_handle,
                                        int32_t* is_mouse_over, int32_t* is_pressed);
typedef void (*RtUiControlDragFnCap)(void* env, int64_t* platform_handle,
                                     double* value);
typedef void (*RtUiControlDragFnBare)(int64_t* platform_handle, double* value);

/* Install 登记 Toggle/Check/Radio/Slider/TextBox/PasswordBox/ListView/DataGrid/
 * TabControl/ComboBox/PopupBackdrop 等；8 槽会在 DataGrid 之后丢弃 TabControl+，
 * 导致页签栏点击无回调。预留下一代控件余量。 */
#define RT_UI_CONTROL_HANDLER_MAX 32

typedef struct RtUiControlHandlerEntry {
    char type_name[32];
    void* click_fn;
    void* click_env;
    void* visual_fn;
    void* visual_env;
    void* drag_fn;
    void* drag_env;
    int used;
} RtUiControlHandlerEntry;

static RtUiControlHandlerEntry g_rt_ui_control_handlers[RT_UI_CONTROL_HANDLER_MAX];

static RtUiControlHandlerEntry* rt_ui_control_handler_lookup(const char* type_name) {
    if (!type_name) return NULL;
    for (size_t i = 0; i < RT_UI_CONTROL_HANDLER_MAX; i++) {
        RtUiControlHandlerEntry* e = &g_rt_ui_control_handlers[i];
        if (e->used && strcmp(e->type_name, type_name) == 0) {
            return e;
        }
    }
    return NULL;
}

static RtUiControlHandlerEntry* rt_ui_control_handler_get_or_create(const char* type_name) {
    if (!type_name) return NULL;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(type_name);
    if (e) return e;
    for (size_t i = 0; i < RT_UI_CONTROL_HANDLER_MAX; i++) {
        RtUiControlHandlerEntry* n = &g_rt_ui_control_handlers[i];
        if (!n->used) {
            memset(n, 0, sizeof(*n));
            strncpy(n->type_name, type_name, sizeof(n->type_name) - 1);
            n->used = 1;
            return n;
        }
    }
    return NULL;
}

void rt_ui_set_control_click_handler(const char* type_name, void* fn, void* env) {
    if (!type_name || !fn) return;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_get_or_create(type_name);
    if (!e) return;
    e->click_fn = fn;
    e->click_env = env;
}

void rt_ui_set_control_visual_state_handler(const char* type_name, void* fn, void* env) {
    if (!type_name || !fn) return;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_get_or_create(type_name);
    if (!e) return;
    e->visual_fn = fn;
    e->visual_env = env;
}

void rt_ui_set_control_drag_handler(const char* type_name, void* fn, void* env) {
    if (!type_name || !fn) return;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_get_or_create(type_name);
    if (!e) return;
    e->drag_fn = fn;
    e->drag_env = env;
}

void rt_ui_clear_control_handlers(void) {
    memset(g_rt_ui_control_handlers, 0, sizeof(g_rt_ui_control_handlers));
}

int rt_ui_has_control_handler(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(elem->type_name);
    if (!e) return 0;
    return (e->click_fn || e->visual_fn || e->drag_fn) ? 1 : 0;
}

/* 像素→Slider 值。与 Arc 层 Slider 布局几何一致（布局 rect 由 Arc 权威同步）：
 * track 从 layout_x + RT_UI_SPACING_SM 起、宽 layout_w - 2*SPACING_SM。
 * frac = (px - track_x)/track_w ∈ [0,1] → 线性 [Minimum, Maximum]，Step 取整、clamp。 */
static double rt_ui_slider_value_from_px(RtUiElement* elem, int32_t px) {
    double min = rt_ui_get_number(elem, "Minimum", 0.0);
    double max = rt_ui_get_number(elem, "Maximum", 100.0);
    double step = rt_ui_get_number(elem, "Step", 1.0);
    double pad_x = (double)RT_UI_SPACING_SM;
    double track_x = elem->layout_x + pad_x;
    double track_w = elem->layout_w - pad_x * 2.0;
    if (track_w <= 0.0) track_w = 1.0;
    double frac = ((double)px - track_x) / track_w;
    if (frac < 0.0) frac = 0.0;
    if (frac > 1.0) frac = 1.0;
    double raw = min + frac * (max - min);
    double value = raw;
    if (step > 0.0) {
        double scaled = raw / step;
        double snapped = scaled < 0.0
            ? (double)(int64_t)(scaled - 0.5)
            : (double)(int64_t)(scaled + 0.5);
        value = snapped * step;
    }
    if (value < min) value = min;
    if (value > max) value = max;
    return value;
}

int rt_ui_dispatch_control_click(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(elem->type_name);
    if (!e || !e->click_fn) return 0;
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (e->click_env) {
        ((RtUiControlClickFnCap)e->click_fn)(e->click_env, &handle);
    } else {
        ((RtUiControlClickFnBare)e->click_fn)(&handle);
    }
    return 1;
}

/* forward decl（rt_ui_element.c 定义）：命中的行 index 经元素 number 属性回传 Arc */
void rt_ui_element_set_number(RtUiElement* elem, const char* name, double value);

/* ListView 行命中：py 落在哪一行的镜像布局 rect 内 → 该行逻辑 index
 * （经行镜像 "ItemIndex" number 属性，PlatformTreeSync 物化时写入）；
 * 未命中任何行返回 -1（越界点击安全）。行几何取镜像布局——layout_* 字段
 * 由 Arc 层权威同步（rt_ui_element_set_number 拦截 Layout* 属性），与渲染一致。 */
static int rt_ui_listview_hit_row(RtUiElement* elem, int32_t py) {
    if (!elem) return -1;
    for (size_t i = 0; i < elem->child_count; i++) {
        RtUiElement* panel = elem->children[i];
        if (!panel) continue;
        for (size_t j = 0; j < panel->child_count; j++) {
            RtUiElement* row = panel->children[j];
            if (!row) continue;
            if ((double)py >= row->layout_y && (double)py < row->layout_y + row->layout_h) {
                return (int)rt_ui_get_number(row, "ItemIndex", -1.0);
            }
        }
    }
    return -1;
}

/* DataGrid 行命中：行镜像（DataGridRow）是 grid 的直接子元素（无 ItemsHost
 * 中间层，区别于 ListView 的 panel→row 两层）。py 落在行镜像 layout rect 内
 * → 该行逻辑 index（行镜像 "ItemIndex"；DataGrid.SyncMirrorRows 物化时写入）。
 * 未命中返回 -1（表头区/越界点击安全取消选择）。 */
static int rt_ui_datagrid_hit_row(RtUiElement* elem, int32_t py) {
    if (!elem) return -1;
    for (size_t i = 0; i < elem->child_count; i++) {
        RtUiElement* row = elem->children[i];
        if (!row) continue;
        double item_index = rt_ui_get_number(row, "ItemIndex", -1.0);
        if (item_index < 0.0) continue; /* 超编折叠行 */
        if ((double)py >= row->layout_y && (double)py < row->layout_y + row->layout_h) {
            return (int)item_index;
        }
    }
    return -1;
}

/* TreeView 节点命中：递归 TreeViewItem；仅 Header 条（HeaderHeight）可点；
 * 折叠子节点 layout 在屏外，自然不会命中。返回 FlatIndex；命中展开区写 *out_expand。 */
static int rt_ui_treeview_hit_node(RtUiElement* node, int32_t px, int32_t py, int* out_expand) {
    if (!node || !node->type_name) return -1;
    if (strcmp(node->type_name, "TreeViewItem") == 0) {
        double header_h = rt_ui_get_number(node, "HeaderHeight", 28.0);
        if (header_h <= 0.0) header_h = 28.0;
        if ((double)py >= node->layout_y
            && (double)py < node->layout_y + header_h
            && (double)px >= node->layout_x
            && (double)px < node->layout_x + node->layout_w) {
            int has_items = (int)rt_ui_get_number(node, "HasItems", 0.0);
            /* HasItems 经 ElementSetBool 写入；部分路径可能 number——双读兜底。 */
            if (!has_items) {
                for (size_t bi = 0; bi < node->bool_count; bi++) {
                    if (strcmp(node->bool_names[bi], "HasItems") == 0) {
                        has_items = node->bool_values[bi] ? 1 : 0;
                        break;
                    }
                }
            }
            double expander_w = rt_ui_get_number(node, "ExpanderWidth", 16.0);
            if (expander_w <= 0.0) expander_w = 16.0;
            if (out_expand) {
                *out_expand = 0;
                if (has_items
                    && (double)px >= node->layout_x
                    && (double)px < node->layout_x + expander_w) {
                    *out_expand = 1;
                }
            }
            return (int)rt_ui_get_number(node, "FlatIndex", -1.0);
        }
    }
    for (size_t i = 0; i < node->child_count; i++) {
        int hit = rt_ui_treeview_hit_node(node->children[i], px, py, out_expand);
        if (hit >= 0) return hit;
    }
    return -1;
}

static int rt_ui_treeview_hit_row(RtUiElement* elem, int32_t px, int32_t py, int* out_expand) {
    if (out_expand) *out_expand = 0;
    if (!elem) return -1;
    return rt_ui_treeview_hit_node(elem, px, py, out_expand);
}

int rt_ui_dispatch_control_click_at(RtUiElement* elem, int32_t px, int32_t py) {
    if (!elem || !elem->type_name) return 0;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(elem->type_name);
    if (!e || !e->click_fn) return 0;
    /* ListView：点击命中行 index 写入镜像 "HitItemIndex"，Arc 侧
     * RouteListViewClick 读取后 SelectIndex（含 DP + 视觉高亮）。 */
    if (strcmp(elem->type_name, "ListView") == 0) {
        rt_ui_element_set_number(elem, "HitItemIndex",
                                  (double)rt_ui_listview_hit_row(elem, py));
    }
    /* DataGrid：同契约——直接子行命中（DataGridRow 无中间层）；
     * Arc 侧 RouteDataGridClick 读取 HitItemIndex + HitMods → SelectIndexWithMods。
     * HitMods：bit0=Shift bit1=Ctrl（RFC 037 §8；与 KeyboardRouter 对齐）。 */
    if (strcmp(elem->type_name, "DataGrid") == 0) {
        rt_ui_element_set_number(elem, "HitItemIndex",
                                  (double)rt_ui_datagrid_hit_row(elem, py));
        {
            int32_t mods = 0;
#if defined(_WIN32)
            if (GetKeyState(VK_SHIFT) & 0x8000) mods |= 1;
            if (GetKeyState(VK_CONTROL) & 0x8000) mods |= 2;
#endif
            rt_ui_element_set_number(elem, "HitMods", (double)mods);
        }
    }
    /* TreeView：递归命中 Header 条 → HitItemIndex；展开三角区 → HitExpand=1。 */
    if (strcmp(elem->type_name, "TreeView") == 0) {
        int hit_expand = 0;
        int hit_idx = rt_ui_treeview_hit_row(elem, px, py, &hit_expand);
        rt_ui_element_set_number(elem, "HitItemIndex", (double)hit_idx);
        rt_ui_element_set_number(elem, "HitExpand", (double)hit_expand);
    }
    /* TabControl：页签栏内容测宽左对齐——py 落在顶栏。
     * 溢出箭头（HeaderOverflow）：两端 HitTabOverflow=±1，中间 strip 命中页签。
     * content_x = (local_x - arrow_w) + HeaderScrollOffset（挤栏）或 local_x + offset。
     * 与渲染 PushClip + cursorX=stripL-offset 同源。无 HeaderWidth{i} 时回退均分。
     * 栏外点击保持 HitTabIndex=-1、HitTabOverflow=0。 */
    if (strcmp(elem->type_name, "TabControl") == 0) {
        double bar_h = rt_ui_get_number(elem, "HeaderBarHeight", 36.0);
        double tab_count = rt_ui_get_number(elem, "TabCount", 0.0);
        double scroll_off = rt_ui_get_number(elem, "HeaderScrollOffset", 0.0);
        double header_overflow = rt_ui_get_number(elem, "HeaderOverflow", 0.0);
        double arrow_w = rt_ui_get_number(elem, "OverflowArrowWidth", 24.0);
        if (scroll_off < 0.0) {
            scroll_off = 0.0;
        }
        if (arrow_w <= 0.0) {
            arrow_w = 24.0;
        }
        int hit = -1;
        int hit_overflow = 0;
        int n = (int)tab_count;
        if (n > 0 && (double)py >= elem->layout_y
            && (double)py < elem->layout_y + bar_h
            && elem->layout_w > 0.0) {
            double local_x = (double)px - elem->layout_x;
            if (local_x >= 0.0 && local_x < elem->layout_w) {
                int show_arrows = (header_overflow > 0.5)
                    && (elem->layout_w > arrow_w * 2.0);
                if (show_arrows && local_x < arrow_w) {
                    hit_overflow = -1;
                } else if (show_arrows
                    && local_x >= elem->layout_w - arrow_w) {
                    hit_overflow = 1;
                } else {
                    double strip_l = show_arrows ? arrow_w : 0.0;
                    double strip_w = show_arrows
                        ? (elem->layout_w - arrow_w * 2.0)
                        : elem->layout_w;
                    if (strip_w < 0.0) {
                        strip_w = 0.0;
                    }
                    double content_x = (local_x - strip_l) + scroll_off;
                    int have_widths = 0;
                    for (int i = 0; i < n; i++) {
                        char key[32];
                        snprintf(key, sizeof(key), "HeaderWidth%d", i);
                        if (rt_ui_get_number(elem, key, 0.0) > 0.0) {
                            have_widths = 1;
                            break;
                        }
                    }
                    if (have_widths) {
                        double cursor = 0.0;
                        for (int i = 0; i < n; i++) {
                            char key[32];
                            snprintf(key, sizeof(key), "HeaderWidth%d", i);
                            double cell_w = rt_ui_get_number(elem, key, 0.0);
                            if (cell_w <= 0.0) {
                                cell_w = 1.0;
                            }
                            if (content_x >= cursor && content_x < cursor + cell_w) {
                                hit = i;
                                break;
                            }
                            cursor += cell_w;
                        }
                    } else if (strip_w > 0.0) {
                        hit = (int)((local_x - strip_l) / (strip_w / tab_count));
                        if (hit < 0) {
                            hit = 0;
                        }
                        if (hit >= n) {
                            hit = n - 1;
                        }
                    }
                }
            }
        }
        rt_ui_element_set_number(elem, "HitTabIndex", (double)hit);
        rt_ui_element_set_number(elem, "HitTabOverflow", (double)hit_overflow);
    }
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (e->click_env) {
        ((RtUiControlClickFnCap)e->click_fn)(e->click_env, &handle);
    } else {
        ((RtUiControlClickFnBare)e->click_fn)(&handle);
    }
    return 1;
}

int rt_ui_dispatch_control_visual_state(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(elem->type_name);
    if (!e || !e->visual_fn) return 0;
    int64_t handle = (int64_t)(uintptr_t)elem;
    int32_t is_mouse_over = elem->is_mouse_over;
    int32_t is_pressed = elem->is_pressed;
    if (e->visual_env) {
        ((RtUiControlVisualFnCap)e->visual_fn)(e->visual_env, &handle,
                                               &is_mouse_over, &is_pressed);
    } else {
        ((RtUiControlVisualFnBare)e->visual_fn)(&handle,
                                                &is_mouse_over, &is_pressed);
    }
    return 1;
}

int rt_ui_dispatch_control_drag(RtUiElement* elem, int32_t px, int32_t py) {
    if (!elem || !elem->type_name) return 0;
    RtUiControlHandlerEntry* e = rt_ui_control_handler_lookup(elem->type_name);
    if (!e || !e->drag_fn) return 0;
    (void)py;
    /* TextBox：载荷 = 相对元素左缘的局部 DIP X（选区拖拽）。
     * Slider：载荷 = 轨道像素映射后的 Value。 */
    double value;
    if (strcmp(elem->type_name, "TextBox") == 0
        || strcmp(elem->type_name, "PasswordBox") == 0) {
        value = (double)px - elem->layout_x;
    } else {
        value = rt_ui_slider_value_from_px(elem, px);
    }
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (e->drag_env) {
        ((RtUiControlDragFnCap)e->drag_fn)(e->drag_env, &handle, &value);
    } else {
        ((RtUiControlDragFnBare)e->drag_fn)(&handle, &value);
    }
    return 1;
}

void rt_ui_set_button_click_handler(void* fn, void* env) {
    g_rt_ui_button_click_fn = (RtUiButtonClickFnCap)fn;
    g_rt_ui_button_click_env = env;
}

void rt_ui_set_button_visual_state_handler(void* fn, void* env) {
    g_rt_ui_button_visual_fn = (RtUiButtonVisualFnCap)fn;
    g_rt_ui_button_visual_env = env;
}

void rt_ui_set_input_focus_handler(void* fn, void* env) {
    g_rt_ui_input_focus_fn = (RtUiInputFocusFnCap)fn;
    g_rt_ui_input_focus_env = env;
}

static int rt_ui_is_button(RtUiElement* elem) {
    return elem && elem->type_name && strcmp(elem->type_name, "Button") == 0;
}

/* TextBox / PasswordBox：局部 DIP X 拖选 / 免 IsEnabled 门控命中（与 TextBox 同族）。 */
static int rt_ui_is_text_input(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    return strcmp(elem->type_name, "TextBox") == 0
        || strcmp(elem->type_name, "PasswordBox") == 0;
}

/* 指针命中目标：Button / TextBox（既有）或注册了泛化交互回调的非 Button 控件。 */
static int rt_ui_is_pointer_target(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    if (rt_ui_is_button(elem) || rt_ui_is_text_input(elem)) return 1;
    return rt_ui_has_control_handler(elem);
}

static RtUiElement* rt_ui_hit_test_elem(RtUiElement* elem, int32_t px, int32_t py) {
    if (!elem || !elem->layout_valid) return NULL;

    for (size_t i = elem->child_count; i > 0; i--) {
        RtUiElement* hit = rt_ui_hit_test_elem(elem->children[i - 1], px, py);
        if (hit) return hit;
    }

    if (!rt_ui_is_pointer_target(elem)) {
        return NULL;
    }
    /* 非文本输入目标（Button 与泛化控件）受 IsEnabled 门控；TextBox/PasswordBox 保持既有语义。 */
    if (strcmp(elem->type_name, "TextBox") != 0
        && strcmp(elem->type_name, "PasswordBox") != 0) {
        for (size_t i = 0; i < elem->bool_count; i++) {
            if (strcmp(elem->bool_names[i], "IsEnabled") == 0 && !elem->bool_values[i]) {
                return NULL;
            }
        }
    }
    if ((double)px >= elem->layout_x && (double)px < elem->layout_x + elem->layout_w &&
        (double)py >= elem->layout_y && (double)py < elem->layout_y + elem->layout_h) {
        return elem;
    }
    return NULL;
}

RtUiElement* rt_ui_hit_test(RtUiElement* root, int32_t width, int32_t height,
                            int32_t x, int32_t y) {
    /* unique-wgpu 重构：不再运行软件光栅布局 pass。layout_* 字段由 Arc 层
     * （PlatformTreeSync 写 LayoutX/Y/Width/Height）通过 rt_ui_element_set_number
     * 同步而来，为唯一权威。width/height 仅用于空窗守卫。 */
    if (!root || width <= 0 || height <= 0) return NULL;
    return rt_ui_hit_test_elem(root, x, y);
}

void rt_ui_element_set_arc_ptr(RtUiElement* elem, int64_t arc_ptr) {
    if (!elem) return;
    elem->arc_ptr = (void*)(uintptr_t)arc_ptr;
}

void rt_ui_dispatch_button_visual_state(RtUiElement* elem) {
    if (!elem || !g_rt_ui_button_visual_fn || !rt_ui_is_button(elem)) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    int32_t is_mouse_over = elem->is_mouse_over;
    int32_t is_pressed = elem->is_pressed;
    if (g_rt_ui_button_visual_env) {
        g_rt_ui_button_visual_fn(g_rt_ui_button_visual_env, &handle,
                                 &is_mouse_over, &is_pressed);
    } else {
        ((RtUiButtonVisualFnBare)g_rt_ui_button_visual_fn)(&handle,
                                                           &is_mouse_over,
                                                           &is_pressed);
    }
}

void rt_ui_dispatch_button_click(RtUiElement* elem) {
    if (!elem || !g_rt_ui_button_click_fn) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (g_rt_ui_button_click_env) {
        g_rt_ui_button_click_fn(g_rt_ui_button_click_env, &handle);
    } else {
        ((RtUiButtonClickFnBare)g_rt_ui_button_click_fn)(&handle);
    }
}

void rt_ui_dispatch_input_focus(RtUiElement* elem) {
    if (!elem || !g_rt_ui_input_focus_fn) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (g_rt_ui_input_focus_env) {
        g_rt_ui_input_focus_fn(g_rt_ui_input_focus_env, &handle);
    } else {
        ((RtUiInputFocusFnBare)g_rt_ui_input_focus_fn)(&handle);
    }
}

/* M-caret2：点击定位 caret——local_dip_x 为相对元素左缘的 DIP 偏移。 */
static RtUiInputClickFnCap g_rt_ui_input_click_fn = NULL;
static void* g_rt_ui_input_click_env = NULL;

void rt_ui_set_input_click_handler(void* fn, void* env) {
    g_rt_ui_input_click_fn = (RtUiInputClickFnCap)fn;
    g_rt_ui_input_click_env = env;
}

void rt_ui_dispatch_input_click_at(RtUiElement* elem, int32_t local_dip_x) {
    if (!elem || !g_rt_ui_input_click_fn) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    double local_x = (double)local_dip_x;
    if (g_rt_ui_input_click_env) {
        g_rt_ui_input_click_fn(g_rt_ui_input_click_env, &handle, &local_x);
    } else {
        ((RtUiInputClickFnBare)g_rt_ui_input_click_fn)(&handle, &local_x);
    }
}
