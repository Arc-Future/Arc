#ifndef ARC_RT_UI_ELEMENT_INTERNAL_H
#define ARC_RT_UI_ELEMENT_INTERNAL_H

#include <stddef.h>
#include <stdint.h>

typedef struct RtUiElement {
    char* type_name;
    char** str_names;
    char** str_values;
    size_t str_count;
    size_t str_cap;
    char** num_names;
    double* num_values;
    size_t num_count;
    size_t num_cap;
    char** bool_names;
    int32_t* bool_values;
    size_t bool_count;
    size_t bool_cap;
    struct RtUiElement** children;
    size_t child_count;
    size_t child_cap;
    struct RtUiElement* parent;

    /* unique-wgpu 重构：layout rects 由 Arc 层权威同步（rt_ui_element_set_number
     * 拦截 LayoutX/Y/Width/Height），供命中测试 + 滚动几何消费。 */
    double layout_x;
    double layout_y;
    double layout_w;
    double layout_h;
    int32_t layout_valid;

    void* arc_ptr;
    int32_t is_mouse_over;
    int32_t is_pressed;
} RtUiElement;

#endif /* ARC_RT_UI_ELEMENT_INTERNAL_H */
