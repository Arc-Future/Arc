/*
 * RFC 037 pointer-events: hit-test + arc_ptr binding + click/visual/focus dispatch.
 * OS-agnostic geometry; Win32 WM_* in crates/runtime-ui/platform/windows/pointer_win32.c.
 */
#include "rt_ui_element_internal.h"
#include "rt_ui_platform.h"
#include "rt_ui_props.h"
#include "rt_ui_design_tokens.h"
#include <stdint.h>
#include <string.h>

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

#define RT_UI_CONTROL_HANDLER_MAX 8

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
     * Arc 侧 RouteDataGridClick 读取后 SelectIndex。 */
    if (strcmp(elem->type_name, "DataGrid") == 0) {
        rt_ui_element_set_number(elem, "HitItemIndex",
                                  (double)rt_ui_datagrid_hit_row(elem, py));
    }
    (void)px;
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
    (void)py; /* 值型拖拽为水平 Slider 语义：只依赖 px */
    double value = rt_ui_slider_value_from_px(elem, px);
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

/* 指针命中目标：Button / TextBox（既有）或注册了泛化交互回调的非 Button 控件。 */
static int rt_ui_is_pointer_target(RtUiElement* elem) {
    if (!elem || !elem->type_name) return 0;
    if (rt_ui_is_button(elem) || strcmp(elem->type_name, "TextBox") == 0) return 1;
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
    /* 非 TextBox 目标（Button 与泛化控件）受 IsEnabled 门控；TextBox 保持既有语义。 */
    if (strcmp(elem->type_name, "TextBox") != 0) {
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
