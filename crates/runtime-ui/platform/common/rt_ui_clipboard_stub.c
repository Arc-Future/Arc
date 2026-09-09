/*
 * 非 Win32 剪贴板 stub：Get → ""；Set 空操作。
 * Win32 实现见 platform/windows/clipboard_win32.c。
 */
#include <stdlib.h>

char* rt_ui_clipboard_get_text(void) {
    char* empty = (char*)malloc(1);
    if (empty) {
        empty[0] = '\0';
        return empty;
    }
    return (char*)"";
}

void rt_ui_clipboard_set_text(const char* utf8) {
    (void)utf8;
}
