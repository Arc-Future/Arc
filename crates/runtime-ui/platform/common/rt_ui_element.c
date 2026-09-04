#include "rt_ui_element_internal.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

static char* rt_ui_strdup_(const char* s) {
    if (!s) return NULL;
    size_t n = strlen(s) + 1;
    char* p = (char*)malloc(n);
    if (p) memcpy(p, s, n);
    return p;
}

RtUiElement* rt_ui_element_create(const char* type_name) {
    RtUiElement* e = (RtUiElement*)calloc(1, sizeof(RtUiElement));
    if (!e) return NULL;
    e->type_name = rt_ui_strdup_(type_name ? type_name : "Element");
    return e;
}

void rt_ui_element_set_string(RtUiElement* elem, const char* name, const char* value) {
    if (!elem || !name) return;
    for (size_t i = 0; i < elem->str_count; i++) {
        if (strcmp(elem->str_names[i], name) == 0) {
            free(elem->str_values[i]);
            elem->str_values[i] = rt_ui_strdup_(value);
            return;
        }
    }
    if (elem->str_count == elem->str_cap) {
        size_t new_cap = elem->str_cap ? elem->str_cap * 2 : 4;
        elem->str_names = (char**)realloc(elem->str_names, new_cap * sizeof(char*));
        elem->str_values = (char**)realloc(elem->str_values, new_cap * sizeof(char*));
        elem->str_cap = new_cap;
    }
    elem->str_names[elem->str_count] = rt_ui_strdup_(name);
    elem->str_values[elem->str_count] = rt_ui_strdup_(value);
    elem->str_count++;
}

static void rt_ui_sync_layout_from_props(RtUiElement* e);

void rt_ui_element_set_number(RtUiElement* elem, const char* name, double value) {
    if (!elem || !name) return;
    for (size_t i = 0; i < elem->num_count; i++) {
        if (strcmp(elem->num_names[i], name) == 0) {
            elem->num_values[i] = value;
            rt_ui_sync_layout_from_props(elem);
            return;
        }
    }
    if (elem->num_count == elem->num_cap) {
        size_t new_cap = elem->num_cap ? elem->num_cap * 2 : 4;
        elem->num_names = (char**)realloc(elem->num_names, new_cap * sizeof(char*));
        elem->num_values = (double*)realloc(elem->num_values, new_cap * sizeof(double));
        elem->num_cap = new_cap;
    }
    elem->num_names[elem->num_count] = rt_ui_strdup_(name);
    elem->num_values[elem->num_count] = value;
    elem->num_count++;
    rt_ui_sync_layout_from_props(elem);
}

/* ===== 布局权威同步（unique-wgpu 重构）=====
 * Arc 层（PlatformTreeSync）把 LayoutX/Y/Width/Height 四项 number 属性写入镜像
 * 元素；本函数在四项齐备时把它们直接落到 layout_* 字段并置 layout_valid=1，
 * 使命中测试 / 滚动几何读 Arc 层权威，彻底脱离软件光栅的布局 pass。
 * 任一项缺失（如 PlatformTreeSync 尚未写入完整）则 layout_valid 保持 0。 */
static void rt_ui_sync_layout_from_props(RtUiElement* e) {
    if (!e) return;
    int has_x = 0, has_y = 0, has_w = 0, has_h = 0;
    for (size_t i = 0; i < e->num_count; i++) {
        const char* n = e->num_names[i];
        if (strcmp(n, "LayoutX") == 0) { has_x = 1; e->layout_x = e->num_values[i]; }
        else if (strcmp(n, "LayoutY") == 0) { has_y = 1; e->layout_y = e->num_values[i]; }
        else if (strcmp(n, "LayoutWidth") == 0) { has_w = 1; e->layout_w = e->num_values[i]; }
        else if (strcmp(n, "LayoutHeight") == 0) { has_h = 1; e->layout_h = e->num_values[i]; }
    }
    e->layout_valid = (has_x && has_y && has_w && has_h) ? 1 : e->layout_valid;
}

void rt_ui_element_set_bool(RtUiElement* elem, const char* name, int32_t value) {
    if (!elem || !name) return;
    if (strcmp(name, "IsFocused") == 0) {
        fprintf(stderr, "[DBG] set_bool IsFocused elem=%p val=%d\n", (void*)elem, (int)value);
    }
    for (size_t i = 0; i < elem->bool_count; i++) {
        if (strcmp(elem->bool_names[i], name) == 0) {
            elem->bool_values[i] = value ? 1 : 0;
            return;
        }
    }
    if (elem->bool_count == elem->bool_cap) {
        size_t new_cap = elem->bool_cap ? elem->bool_cap * 2 : 4;
        elem->bool_names = (char**)realloc(elem->bool_names, new_cap * sizeof(char*));
        elem->bool_values = (int32_t*)realloc(elem->bool_values, new_cap * sizeof(int32_t));
        elem->bool_cap = new_cap;
    }
    elem->bool_names[elem->bool_count] = rt_ui_strdup_(name);
    elem->bool_values[elem->bool_count] = value ? 1 : 0;
    elem->bool_count++;
}

void rt_ui_element_add_child(RtUiElement* parent, RtUiElement* child) {
    if (!parent || !child || child->parent) return;
    if (parent->child_count == parent->child_cap) {
        size_t new_cap = parent->child_cap ? parent->child_cap * 2 : 4;
        parent->children = (RtUiElement**)realloc(parent->children, new_cap * sizeof(RtUiElement*));
        parent->child_cap = new_cap;
    }
    parent->children[parent->child_count++] = child;
    child->parent = parent;
}

void rt_ui_element_destroy(RtUiElement* elem) {
    if (!elem) return;
    for (size_t i = 0; i < elem->child_count; i++) {
        rt_ui_element_destroy(elem->children[i]);
    }
    free(elem->children);
    for (size_t i = 0; i < elem->str_count; i++) {
        free(elem->str_names[i]);
        free(elem->str_values[i]);
    }
    free(elem->str_names);
    free(elem->str_values);
    for (size_t i = 0; i < elem->num_count; i++) {
        free(elem->num_names[i]);
    }
    free(elem->num_names);
    free(elem->num_values);
    for (size_t i = 0; i < elem->bool_count; i++) {
        free(elem->bool_names[i]);
    }
    free(elem->bool_names);
    free(elem->bool_values);
    free(elem->type_name);
    free(elem);
}

/* ===== 公共只读访问 ABI（RFC 037 M3.5）——供 WgpuRender 等渲染后端遍历元素树 ===== */

const char* rt_ui_element_get_type_name(RtUiElement* elem) {
    if (!elem || !elem->type_name) return "Element";
    return elem->type_name;
}

const char* rt_ui_element_get_string(RtUiElement* elem,
                                      const char* name,
                                      const char* def) {
    if (!elem || !name) return def;
    for (size_t i = 0; i < elem->str_count; i++) {
        if (strcmp(elem->str_names[i], name) == 0) {
            return elem->str_values[i] ? elem->str_values[i] : def;
        }
    }
    return def;
}

double rt_ui_element_get_number(RtUiElement* elem,
                                 const char* name,
                                 double def) {
    if (!elem || !name) return def;
    for (size_t i = 0; i < elem->num_count; i++) {
        if (strcmp(elem->num_names[i], name) == 0) {
            return elem->num_values[i];
        }
    }
    return def;
}

int32_t rt_ui_element_get_bool(RtUiElement* elem,
                                const char* name,
                                int32_t def) {
    if (!elem || !name) return def;
    for (size_t i = 0; i < elem->bool_count; i++) {
        if (strcmp(elem->bool_names[i], name) == 0) {
            return elem->bool_values[i] ? 1 : 0;
        }
    }
    return def;
}

int32_t rt_ui_element_get_child_count(RtUiElement* elem) {
    if (!elem) return 0;
    return (int32_t)elem->child_count;
}

RtUiElement* rt_ui_element_get_child(RtUiElement* elem, int32_t index) {
    if (!elem || index < 0 || (size_t)index >= elem->child_count) return NULL;
    return elem->children[index];
}
