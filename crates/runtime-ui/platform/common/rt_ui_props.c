#include "rt_ui_props.h"
#include <string.h>

const char* rt_ui_get_string(RtUiElement* e, const char* name, const char* def) {
    if (!e || !name) return def;
    for (size_t i = 0; i < e->str_count; i++) {
        if (strcmp(e->str_names[i], name) == 0) {
            return e->str_values[i] ? e->str_values[i] : def;
        }
    }
    return def;
}

double rt_ui_get_number(RtUiElement* e, const char* name, double def) {
    if (!e || !name) return def;
    for (size_t i = 0; i < e->num_count; i++) {
        if (strcmp(e->num_names[i], name) == 0) {
            return e->num_values[i];
        }
    }
    return def;
}

int32_t rt_ui_get_bool(RtUiElement* e, const char* name, int32_t def) {
    if (!e || !name) return def;
    for (size_t i = 0; i < e->bool_count; i++) {
        if (strcmp(e->bool_names[i], name) == 0) {
            return e->bool_values[i];
        }
    }
    return def;
}
