#ifndef ARC_CLIPBOARD_WIN32_H
#define ARC_CLIPBOARD_WIN32_H

/*
 * Win32 系统剪贴板 UTF-8 桥（RFC 037 TextBox Ctrl+C/V/X）。
 * 归属：runtime-ui/platform/windows；ABI 见 rt_ui_abi.h。
 */

#ifdef __cplusplus
extern "C" {
#endif

/* 堆分配 UTF-8（空剪贴板 → ""）；调用方按 Arc string 拥有。永不返回 NULL。 */
char* rt_ui_clipboard_get_text(void);

/* 将 UTF-8 写入 CF_UNICODETEXT；utf8 为 NULL 时写空串。 */
void rt_ui_clipboard_set_text(const char* utf8);

#ifdef __cplusplus
}
#endif

#endif /* ARC_CLIPBOARD_WIN32_H */
