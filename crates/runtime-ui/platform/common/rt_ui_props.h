#ifndef ARC_RT_UI_PROPS_H
#define ARC_RT_UI_PROPS_H

#include "rt_ui_element_internal.h"

const char* rt_ui_get_string(RtUiElement* e, const char* name, const char* def);
double rt_ui_get_number(RtUiElement* e, const char* name, double def);
int32_t rt_ui_get_bool(RtUiElement* e, const char* name, int32_t def);

#endif
