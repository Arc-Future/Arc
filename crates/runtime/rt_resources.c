/* rt_resources.c — RFC 027 M1: localization & resources runtime ABI.
 *
 * Provides:
 *   - rt_os_current_uilocale / rt_os_current_locale: OS locale detection
 *
 * Resource lookup has been eliminated entirely: ResX CodeGen (RFC 027)
 * inlines strongly typed accessors into literals at compile time — zero
 * parsing, zero hashing, zero ABI calls at runtime.
 *
 * All string ABI returns are freshly malloc'd NUL-terminated (caller-owned).
 *
 * Note: Culture data (display names, native names, normalization, parent chain)
 * has been migrated to pure Arc code in std/Arc/Globalization/ (CultureData.as,
 * CultureHelper.as). This C file retains only OS API calls that Arc cannot
 * express natively.
 */

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <stdint.h>

/* ─── OS locale detection (必须保留 C：平台 API 调用) ─── */

#ifdef _WIN32
#include <windows.h>

char* rt_os_current_uilocale(void) {
    LANGID langId = GetUserDefaultUILanguage();
    wchar_t localeName[LOCALE_NAME_MAX_LENGTH];
    int r = LCIDToLocaleName(MAKELCID(langId, SORT_DEFAULT),
                             localeName, LOCALE_NAME_MAX_LENGTH, 0);
    if (r > 0) {
        int len = WideCharToMultiByte(CP_UTF8, 0, localeName, -1,
                                      NULL, 0, NULL, NULL);
        if (len > 0) {
            char* out = (char*)malloc((size_t)len);
            if (out) {
                WideCharToMultiByte(CP_UTF8, 0, localeName, -1,
                                    out, len, NULL, NULL);
                return out;
            }
        }
    }
    return strdup("en-US");
}

char* rt_os_current_locale(void) {
    LANGID langId = GetUserDefaultLCID();
    wchar_t localeName[LOCALE_NAME_MAX_LENGTH];
    int r = LCIDToLocaleName(MAKELCID(langId, SORT_DEFAULT),
                             localeName, LOCALE_NAME_MAX_LENGTH, 0);
    if (r > 0) {
        int len = WideCharToMultiByte(CP_UTF8, 0, localeName, -1,
                                      NULL, 0, NULL, NULL);
        if (len > 0) {
            char* out = (char*)malloc((size_t)len);
            if (out) {
                WideCharToMultiByte(CP_UTF8, 0, localeName, -1,
                                    out, len, NULL, NULL);
                return out;
            }
        }
    }
    return strdup("en-US");
}

#else /* POSIX (Linux, macOS) */

char* rt_os_current_uilocale(void) {
    const char* env = getenv("LANG");
    if (env && env[0]) {
        // Handle "zh_CN.UTF-8" → normalize to "zh-CN"
        char buf[64];
        int32_t i;
        for (i = 0; env[i] && env[i] != '.' && env[i] != '@' && i < 63; i++) {
            buf[i] = (env[i] == '_') ? '-' : env[i];
        }
        buf[i] = '\0';
        return strdup(buf);
    }
    return strdup("en-US");
}

char* rt_os_current_locale(void) {
    const char* env = getenv("LC_ALL");
    if (!env || !env[0]) env = getenv("LC_NUMERIC");
    if (!env || !env[0]) env = getenv("LANG");
    if (env && env[0]) {
        char buf[64];
        int32_t i;
        for (i = 0; env[i] && env[i] != '.' && env[i] != '@' && i < 63; i++) {
            buf[i] = (env[i] == '_') ? '-' : env[i];
        }
        buf[i] = '\0';
        return strdup(buf);
    }
    return strdup("en-US");
}

#endif

/* ─── OS time (DateTime.Now / UtcNow) ───
 *
 * Returns .NET-compatible ticks (100-nanosecond intervals since 0001-01-01).
 * Windows: GetSystemTimeAsFileTime + epoch offset
 * POSIX:   clock_gettime + epoch offset
 *
 * Epoch offsets:
 *   Windows FILETIME epoch = 1601-01-01 → .NET epoch = 584389 days
 *   POSIX time_t epoch     = 1970-01-01 → .NET epoch = 62135596800 seconds
 */

#ifdef _WIN32

int64_t rt_os_now_ticks(void) {
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER ul;
    ul.LowPart = ft.dwLowDateTime;
    ul.HighPart = ft.dwHighDateTime;
    /* FILETIME = 100ns intervals since 1601-01-01.
     * .NET ticks = FILETIME + (584389 days in ticks).
     * 584389 * 864000000000 = 504911232000000000 */
    return (int64_t)ul.QuadPart + 504911232000000000LL;
}

int64_t rt_os_now_utc_ticks(void) {
    /* Same as local time on Windows — GetSystemTimeAsFileTime is already UTC. */
    return rt_os_now_ticks();
}

#else /* POSIX */

#include <time.h>

int64_t rt_os_now_ticks(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    /* time_t seconds since 1970-01-01.
     * .NET ticks = seconds * 10000000 + nanoseconds/100 + 621355968000000000LL */
    return (int64_t)ts.tv_sec * 10000000LL
         + (int64_t)ts.tv_nsec / 100LL
         + 621355968000000000LL;
}

int64_t rt_os_now_utc_ticks(void) {
    /* clock_gettime(CLOCK_REALTIME) already returns UTC on POSIX. */
    return rt_os_now_ticks();
}

#endif

/* ─── Stopwatch（Arc.Diagnostics.Stopwatch）───
 *
 * High-resolution monotonic timestamps for interval measurement.
 * Windows: QueryPerformanceCounter (frequency from QueryPerformanceFrequency)
 * POSIX:   CLOCK_MONOTONIC nanoseconds (frequency = 1e9)
 */

#ifdef _WIN32

int64_t rt_stopwatch_get_timestamp(void) {
    LARGE_INTEGER c;
    QueryPerformanceCounter(&c);
    return (int64_t)c.QuadPart;
}

int64_t rt_stopwatch_frequency(void) {
    static int64_t freq = 0;
    if (freq == 0) {
        LARGE_INTEGER f;
        QueryPerformanceFrequency(&f);
        freq = (int64_t)f.QuadPart;
        if (freq <= 0) {
            freq = 1;
        }
    }
    return freq;
}

int32_t rt_stopwatch_is_high_resolution(void) {
    return 1;
}

#else /* POSIX */

int64_t rt_stopwatch_get_timestamp(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0;
    }
    return (int64_t)ts.tv_sec * 1000000000LL + (int64_t)ts.tv_nsec;
}

int64_t rt_stopwatch_frequency(void) {
    return 1000000000LL;
}

int32_t rt_stopwatch_is_high_resolution(void) {
    return 1;
}

#endif
