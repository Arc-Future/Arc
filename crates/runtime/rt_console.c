//! Console runtime ABI（Phase 1+2：核心 I/O + 颜色控制）。
//!
//! 与 C# System.Console 对齐的能力子集：
//!   - Phase 1：rt_print / rt_read_line / rt_read_char
//!   - Phase 2：rt_console_set_fg/bg / rt_console_reset_color / rt_console_get_fg/bg
//!
//! 设计决策：
//!   - 颜色输出基于 ANSI 转义序列，跨平台兼容（Windows 10+ 终端、Unix 终端）。
//!     不引入 SetConsoleMode 依赖以保持纯 C 可移植性。
//!   - Get* 返回运行时默认值（Gray=7/Black=0），不解析终端响应以避免阻塞与
//!     平台依赖（与 std/Arc/Console.as 文档注释声明的偏离一致）。
//!   - rt_read_line 动态扩容，EOF 返回 NULL（Arc 侧表现为空串）。
//!   - rt_read_char 读取单字节，EOF 返回 -1。
//!
//! ConsoleColor 枚举值 0-15 映射到 ANSI：
//!   0-7  → 标准色（前景 30-37，背景 40-47）
//!   8-15 → 亮色（前景 90-97，背景 100-107）

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "rt_abi.h"

#ifdef _WIN32
#include <windows.h>

/* H1: 绕开 CRT stdout FILE*——满套件 WriteResults / 报告期若堆已损，
 * fputs/fwrite/fflush 易放大为末条 0xC0000005；WriteFile 不碰 CRT 缓冲堆块。 */
static void rt_win_stdout_write(const char* data, DWORD len) {
    HANDLE h = GetStdHandle(STD_OUTPUT_HANDLE);
    if (!h || h == INVALID_HANDLE_VALUE) return;
    if (len == 0) return;
    DWORD written = 0;
    WriteFile(h, data, len, &written, NULL);
}
#endif

/* ---- Phase 1：核心 I/O ---- */

/// 无换行输出。NULL 视为空串，与 rt_println 语义一致。
void rt_print(const char* msg) {
#ifdef _WIN32
    if (msg) {
        size_t len = strlen(msg);
        if (len > 0x7fffffff) len = 0x7fffffff;
        rt_win_stdout_write(msg, (DWORD)len);
    }
#else
    if (msg) {
        fputs(msg, stdout);
    }
    fflush(stdout);
#endif
}

/// Phase 3 (2026-07-20): 无换行输出到 stderr。NULL 视为空串。
void rt_print_error(const char* msg) {
    if (msg) {
        fputs(msg, stderr);
    }
    fflush(stderr);
}

/// Phase 3 (2026-07-20): 带换行输出到 stderr。NULL 输出空行。
void rt_println_error(const char* msg) {
    if (msg) {
        fputs(msg, stderr);
    }
    fputc('\n', stderr);
    fflush(stderr);
}

/// 行输入。动态扩容读取一行（不含尾部 '\n'）。
/// EOF 且无输入时返回 NULL（Arc 侧表现为空串）。
char* rt_read_line(void) {
    size_t cap = 128;
    size_t len = 0;
    char* buf = (char*)malloc(cap);
    if (!buf) {
        return NULL;
    }

    int c;
    while ((c = getchar()) != EOF) {
        if (c == '\n') {
            break;
        }
        if (len + 1 >= cap) {
            size_t new_cap = cap * 2;
            char* new_buf = (char*)realloc(buf, new_cap);
            if (!new_buf) {
                free(buf);
                return NULL;
            }
            buf = new_buf;
            cap = new_cap;
        }
        buf[len++] = (char)c;
    }

    /* EOF 且未读到任何字符：返回 NULL 区分"真 EOF"与"空行" */
    if (c == EOF && len == 0) {
        free(buf);
        return NULL;
    }

    buf[len] = '\0';
    return buf;
}

/// 字符输入。读取单字节，EOF 返回 -1。
int32_t rt_read_char(void) {
    int c = getchar();
    if (c == EOF) {
        return -1;
    }
    return (int32_t)c;
}

/* ---- Phase 2：颜色控制（ANSI 转义） ---- */

/// ConsoleColor (0-15) → ANSI 前景码 (30-37 / 90-97)
static int fg_code(int32_t color) {
    if (color < 0 || color > 15) {
        return 37; /* 默认 Gray */
    }
    if (color < 8) {
        return 30 + color;
    }
    return 90 + (color - 8);
}

/// ConsoleColor (0-15) → ANSI 背景码 (40-47 / 100-107)
static int bg_code(int32_t color) {
    if (color < 0 || color > 15) {
        return 40; /* 默认 Black */
    }
    if (color < 8) {
        return 40 + color;
    }
    return 100 + (color - 8);
}

void rt_console_set_fg(int32_t color) {
#ifdef _WIN32
    char buf[16];
    int n = snprintf(buf, sizeof(buf), "\033[%dm", fg_code(color));
    if (n > 0) rt_print(buf);
#else
    printf("\033[%dm", fg_code(color));
    fflush(stdout);
#endif
}

void rt_console_set_bg(int32_t color) {
#ifdef _WIN32
    char buf[16];
    int n = snprintf(buf, sizeof(buf), "\033[%dm", bg_code(color));
    if (n > 0) rt_print(buf);
#else
    printf("\033[%dm", bg_code(color));
    fflush(stdout);
#endif
}

void rt_console_reset_color(void) {
#ifdef _WIN32
    rt_print("\033[0m");
#else
    fputs("\033[0m", stdout);
    fflush(stdout);
#endif
}

/// 返回默认前景色 Gray=7。不解析终端响应以避免阻塞。
int32_t rt_console_get_fg(void) {
    return 7; /* Gray */
}

/// 返回默认背景色 Black=0。不解析终端响应以避免阻塞。
int32_t rt_console_get_bg(void) {
    return 0; /* Black */
}
