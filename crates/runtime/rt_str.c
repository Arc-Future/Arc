// String ABI (RFC 018 / RFC 015).
//
// Split out of the former monolithic runtime.c so each concern lives in its
// own translation unit: this file owns string allocation, concatenation,
// and comparison. Console output (`rt_println`) is also here because it is
// a thin wrapper over `puts`. File I/O helpers have been migrated to
// rt_file.c (M1: basic file operations + M3: directory/path operations).

#include "rt_abi.h"
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#include <windows.h>
#endif

static int rt_str_char_in_set(unsigned char ch, const int32_t* chars, int32_t n);
char* rt_str_trim(const char* s);

char* rt_str_concat(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t la = strlen(a);
    size_t lb = strlen(b);
    char* out = (char*)malloc(la + lb + 1);
    if (!out) return NULL;
    memcpy(out, a, la);
    memcpy(out + la, b, lb + 1);
    return out;
}

int32_t rt_str_length(const char* s) {
    if (!s) return 0;
    return (int32_t)strlen(s);
}

int32_t rt_str_equals(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    return strcmp(a, b) == 0 ? 1 : 0;
}

int32_t rt_str_compare(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    return (int32_t)strcmp(a, b);
}

void rt_println(const char* msg) {
    // H1 (Windows): WriteFile 直写，避开 CRT stdout 缓冲与已损堆交织。
    // 其它平台仍用 fwrite（无隐式 puts flush）。
#ifdef _WIN32
    HANDLE h = GetStdHandle(STD_OUTPUT_HANDLE);
    if (h && h != INVALID_HANDLE_VALUE) {
        DWORD written = 0;
        if (msg) {
            size_t len = strlen(msg);
            if (len > 0x7fffffff) len = 0x7fffffff;
            if (len) WriteFile(h, msg, (DWORD)len, &written, NULL);
        }
        WriteFile(h, "\n", 1, &written, NULL);
    }
#else
    if (msg) {
        size_t len = strlen(msg);
        fwrite(msg, 1, len, stdout);
        fputc('\n', stdout);
    } else {
        fputc('\n', stdout);
    }
#endif
}

/* rt_panic 已迁移至 rt_panic.c（RFC 024 M1：运行时可观测性） */

/* ---- String method ABI (P2: Split/Join/Replace/Substring/Contains/...) ----
 *
 * These helpers back the `string` instance methods exposed by the std facade.
 * All return newly-allocated buffers (caller-managed); booleans return 0/1.
 * Split returns a runtime-length array (rt_array_create) of char* elements.
 */

/* Allocate a copy of the first `len` bytes of `s`. Always NUL-terminates. */
static char* rt_str_dup_n(const char* s, size_t len) {
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    if (s && len) memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

void* rt_str_split(const char* s, const char* sep) {
    if (!s) s = "";
    if (!sep || !*sep) {
        /* Empty separator: return single-element array containing the whole
         * string (avoids crashing — C# throws ArgumentException). */
        void* arr = rt_array_create(1, (int32_t)sizeof(char*));
        ((char**)arr)[0] = rt_str_dup_n(s, strlen(s));
        return arr;
    }
    size_t sep_len = strlen(sep);
    /* Count parts = (number of separator occurrences) + 1. */
    int32_t count = 1;
    const char* p = s;
    while ((p = strstr(p, sep)) != NULL) {
        count++;
        p += sep_len;
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    p = s;
    while ((p = strstr(start, sep)) != NULL) {
        items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
        start = p + sep_len;
    }
    /* Trailing part (from last separator to end of string). */
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

/* Split by single character separator (P5-A2).
 * Returns rt_array_create of char* elements (same layout as rt_str_split). */
void* rt_str_split_char(const char* s, int32_t c) {
    if (!s) s = "";
    char ch = (char)c;
    /* Count parts */
    int32_t count = 1;
    const char* p = s;
    while (*p) {
        if (*p == ch) { count++; }
        p++;
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    p = s;
    while (*p) {
        if (*p == ch) {
            items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
            start = p + 1;
        }
        p++;
    }
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

/* RemoveEmptyEntries：丢弃空段；非空串所有权移入新数组。 */
static void* rt_str_split_filter_empty(void* arr) {
    if (!arr) return arr;
    int32_t n = rt_array_length(arr);
    char** items = (char**)arr;
    int32_t keep = 0;
    for (int32_t i = 0; i < n; i++) {
        if (items[i] && items[i][0] != '\0') keep++;
    }
    if (keep == n) return arr;
    void* out = rt_array_create(keep, (int32_t)sizeof(char*));
    if (!out) return arr;
    char** dest = (char**)out;
    int32_t j = 0;
    for (int32_t i = 0; i < n; i++) {
        if (items[i] && items[i][0] != '\0') {
            dest[j++] = items[i];
        } else if (items[i]) {
            free(items[i]);
        }
    }
    /* 旧数组 header 泄漏可接受：Split 结果通常持有至进程结束。 */
    (void)arr;
    return out;
}

/* TrimEntries：对每段空白 trim（in-place 替换串）。 */
static void* rt_str_split_trim_entries(void* arr) {
    if (!arr) return arr;
    int32_t n = rt_array_length(arr);
    char** items = (char**)arr;
    for (int32_t i = 0; i < n; i++) {
        if (!items[i]) continue;
        char* trimmed = rt_str_trim(items[i]);
        free(items[i]);
        items[i] = trimmed ? trimmed : rt_str_dup_n("", 0);
    }
    return arr;
}

/* options bit0=RemoveEmptyEntries, bit1=TrimEntries（先 trim 再 filter）。 */
static void* rt_str_split_apply_opts(void* arr, int32_t options) {
    if (options & 2) arr = rt_str_split_trim_entries(arr);
    if (options & 1) arr = rt_str_split_filter_empty(arr);
    return arr;
}

/* 多分隔符：任一 seps[i] 匹配即切分（UTF-8 码元）。 */
void* rt_str_split_chars(const char* s, void* seps) {
    if (!s) s = "";
    int32_t nseps = seps ? rt_array_length(seps) : 0;
    if (nseps <= 0) {
        void* arr = rt_array_create(1, (int32_t)sizeof(char*));
        ((char**)arr)[0] = rt_str_dup_n(s, strlen(s));
        return arr;
    }
    const int32_t* set = (const int32_t*)seps;
    int32_t count = 1;
    for (const char* p = s; *p; p++) {
        if (rt_str_char_in_set((unsigned char)*p, set, nseps)) count++;
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    for (const char* p = s; *p; p++) {
        if (rt_str_char_in_set((unsigned char)*p, set, nseps)) {
            items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
            start = p + 1;
        }
    }
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

/* 带 count 的 char 分割（在循环内截断）。 */
static void* rt_str_split_char_limited(const char* s, int32_t c, int32_t max_parts) {
    if (!s) s = "";
    char ch = (char)c;
    if (max_parts == 0) return rt_array_create(0, (int32_t)sizeof(char*));
    if (max_parts < 0) return rt_str_split_char(s, c);
    /* count parts up to max_parts */
    int32_t found = 1;
    const char* p = s;
    while (*p && found < max_parts) {
        if (*p == ch) found++;
        p++;
    }
    void* arr = rt_array_create(found, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    p = s;
    while (*p && idx < max_parts - 1) {
        if (*p == ch) {
            items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
            start = p + 1;
        }
        p++;
    }
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

static void* rt_str_split_limited(const char* s, const char* sep, int32_t max_parts) {
    if (!s) s = "";
    if (max_parts == 0) return rt_array_create(0, (int32_t)sizeof(char*));
    if (max_parts < 0) return rt_str_split(s, sep);
    if (!sep || !*sep) {
        void* arr = rt_array_create(1, (int32_t)sizeof(char*));
        ((char**)arr)[0] = rt_str_dup_n(s, strlen(s));
        return arr;
    }
    size_t sep_len = strlen(sep);
    int32_t found = 1;
    const char* p = s;
    while (found < max_parts && (p = strstr(p, sep)) != NULL) {
        found++;
        p += sep_len;
    }
    void* arr = rt_array_create(found, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    p = s;
    while (idx < max_parts - 1 && (p = strstr(start, sep)) != NULL) {
        items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
        start = p + sep_len;
    }
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

static void* rt_str_split_chars_limited(const char* s, void* seps, int32_t max_parts) {
    if (!s) s = "";
    if (max_parts == 0) return rt_array_create(0, (int32_t)sizeof(char*));
    if (max_parts < 0) return rt_str_split_chars(s, seps);
    int32_t nseps = seps ? rt_array_length(seps) : 0;
    if (nseps <= 0) {
        void* arr = rt_array_create(1, (int32_t)sizeof(char*));
        ((char**)arr)[0] = rt_str_dup_n(s, strlen(s));
        return arr;
    }
    const int32_t* set = (const int32_t*)seps;
    int32_t found = 1;
    for (const char* p = s; *p && found < max_parts; p++) {
        if (rt_str_char_in_set((unsigned char)*p, set, nseps)) found++;
    }
    void* arr = rt_array_create(found, (int32_t)sizeof(char*));
    char** items = (char**)arr;
    int32_t idx = 0;
    const char* start = s;
    for (const char* p = s; *p && idx < max_parts - 1; p++) {
        if (rt_str_char_in_set((unsigned char)*p, set, nseps)) {
            items[idx++] = rt_str_dup_n(start, (size_t)(p - start));
            start = p + 1;
        }
    }
    items[idx] = rt_str_dup_n(start, strlen(start));
    return arr;
}

void* rt_str_split_opts(const char* s, const char* sep, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split(s, sep), options);
}

void* rt_str_split_char_opts(const char* s, int32_t c, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split_char(s, c), options);
}

void* rt_str_split_chars_opts(const char* s, void* seps, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split_chars(s, seps), options);
}

void* rt_str_split_opts_count(const char* s, const char* sep, int32_t count, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split_limited(s, sep, count), options);
}

void* rt_str_split_char_opts_count(const char* s, int32_t c, int32_t count, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split_char_limited(s, c, count), options);
}

void* rt_str_split_chars_opts_count(const char* s, void* seps, int32_t count, int32_t options) {
    return rt_str_split_apply_opts(rt_str_split_chars_limited(s, seps, count), options);
}

/* ToCharArray (P5-A1): convert string to int32_t array (each UTF-8 code unit as i32).
 * Returns rt_array_create of int32_t elements. Index range matches Length / CharAt. */
void* rt_str_to_char_array(const char* s) {
    if (!s) s = "";
    size_t len = strlen(s);
    void* arr = rt_array_create((int32_t)len, (int32_t)sizeof(int32_t));
    if (!arr) return NULL;
    int32_t* items = (int32_t*)arr;
    for (size_t i = 0; i < len; i++) {
        items[i] = (int32_t)(unsigned char)s[i];
    }
    return arr;
}

/* ToCharArray(start, length): UTF-8 code-unit slice → char[].
 * Bounds clamp like rt_str_substring (not C# throw); length < 0 → to end. */
void* rt_str_to_char_array_range(const char* s, int32_t start, int32_t length) {
    if (!s) s = "";
    int32_t s_len = (int32_t)strlen(s);
    if (start < 0) start = 0;
    if (start > s_len) start = s_len;
    int32_t end;
    if (length < 0) {
        end = s_len;
    } else {
        end = start + length;
        if (end > s_len) end = s_len;
    }
    int32_t len = end - start;
    void* arr = rt_array_create(len, (int32_t)sizeof(int32_t));
    if (!arr) return NULL;
    int32_t* items = (int32_t*)arr;
    for (int32_t i = 0; i < len; i++) {
        items[i] = (int32_t)(unsigned char)s[start + i];
    }
    return arr;
}

/* CharAt / string indexer s[i] → char.
 * Index is a UTF-8 code unit (byte) offset, same unit as rt_str_length / ToCharArray.
 * Out-of-range (including null s) returns '\0' (align StringBuilder get_Item). */
int32_t rt_str_char_at(const char* s, int32_t index) {
    if (!s || index < 0) return 0;
    size_t len = strlen(s);
    if ((size_t)index >= len) return 0;
    return (int32_t)(unsigned char)s[index];
}

char* rt_str_join(const char* sep, void* arr) {
    if (!sep) sep = "";
    int32_t count = rt_array_length(arr);
    if (count <= 0) {
        return rt_str_dup_n("", 0);
    }
    char** items = (char**)arr;
    size_t sep_len = strlen(sep);
    /* Compute total length in a single pass to avoid quadratic strcat. */
    size_t total = 0;
    for (int32_t i = 0; i < count; i++) {
        total += items[i] ? strlen(items[i]) : 0;
        if (i < count - 1) total += sep_len;
    }
    char* out = (char*)malloc(total + 1);
    if (!out) return NULL;
    char* w = out;
    for (int32_t i = 0; i < count; i++) {
        if (items[i]) {
            size_t len = strlen(items[i]);
            memcpy(w, items[i], len);
            w += len;
        }
        if (i < count - 1) {
            memcpy(w, sep, sep_len);
            w += sep_len;
        }
    }
    *w = '\0';
    return out;
}

/* Join(char, string[]): separator is one UTF-8 code unit (same unit as Length / s[i]). */
char* rt_str_join_char(int32_t sep, void* arr) {
    char buf[2];
    buf[0] = (char)(unsigned char)sep;
    buf[1] = '\0';
    return rt_str_join(buf, arr);
}

char* rt_str_replace(const char* s, const char* old, const char* neu) {
    if (!s) s = "";
    if (!neu) neu = "";
    if (!old || !*old) {
        /* Empty `old` is not replaceable; return a copy of the input. */
        return rt_str_dup_n(s, strlen(s));
    }
    size_t old_len = strlen(old);
    size_t neu_len = strlen(neu);
    size_t s_len = strlen(s);

    /* RFC 016 M1: single pass over the input into a growable output buffer.
     * For shrinking/equal-length replacements s_len is an upper bound (no
     * realloc); growing replacements realloc by doubling. Removes the second
     * full strstr scan of the old two-pass algorithm. Semantics unchanged. */
    size_t cap = s_len + 1;
    char* out = (char*)malloc(cap);
    if (!out) return NULL;
    size_t used = 0;

    const char* cur = s;
    const char* end = s + s_len;
    while (cur < end) {
        const char* hit = strstr(cur, old);
        if (hit == NULL || hit >= end) break; /* no full match before NUL */
        size_t pre = (size_t)(hit - cur);
        size_t need = used + pre + neu_len + 1;
        if (need > cap) {
            size_t ncap = need * 2;
            char* n = (char*)realloc(out, ncap);
            if (!n) { free(out); return NULL; }
            out = n;
            cap = ncap;
        }
        if (pre) { memcpy(out + used, cur, pre); used += pre; }
        if (neu_len) { memcpy(out + used, neu, neu_len); used += neu_len; }
        cur = hit + old_len;
    }
    size_t tail = (size_t)(end - cur);
    size_t need = used + tail + 1;
    if (need > cap) {
        size_t ncap = need * 2;
        char* n = (char*)realloc(out, ncap);
        if (!n) { free(out); return NULL; }
        out = n;
        cap = ncap;
    }
    if (tail) { memcpy(out + used, cur, tail); used += tail; }
    out[used] = '\0';
    return out;
}

char* rt_str_substring(const char* s, int32_t start, int32_t length) {
    if (!s) s = "";
    int32_t s_len = (int32_t)strlen(s);
    if (start < 0) start = 0;
    if (start > s_len) start = s_len;
    int32_t end;
    if (length < 0) {
        /* length < 0 → substring from `start` to end of string. */
        end = s_len;
    } else {
        end = start + length;
        if (end > s_len) end = s_len;
    }
    int32_t len = end - start;
    return rt_str_dup_n(s + start, (size_t)len);
}

int32_t rt_str_contains(const char* s, const char* sub) {
    if (!s) s = "";
    if (!sub || !*sub) return 1;  /* empty substring is always present */
    return strstr(s, sub) != NULL ? 1 : 0;
}

int32_t rt_str_index_of(const char* s, const char* sub) {
    if (!s) s = "";
    if (!sub || !*sub) return 0;
    const char* found = strstr(s, sub);
    return found ? (int32_t)(found - s) : -1;
}

int32_t rt_str_index_of_char(const char* s, int32_t c) {
    if (!s) return -1;
    char ch = (char)c;
    char* found = strchr(s, ch);
    return found ? (int32_t)(found - s) : -1;
}

int32_t rt_str_index_of_from(const char* s, const char* sub, int32_t start) {
    if (!s) s = "";
    if (start < 0) start = 0;
    size_t slen = strlen(s);
    if (start >= (int32_t)slen) return -1;
    if (!sub || !*sub) return start;
    const char* found = strstr(s + start, sub);
    return found ? (int32_t)(found - s) : -1;
}

int32_t rt_str_index_of_char_from(const char* s, int32_t c, int32_t start) {
    if (!s) return -1;
    if (start < 0) start = 0;
    size_t slen = strlen(s);
    if (start >= (int32_t)slen) return -1;
    char ch = (char)c;
    char* found = strchr(s + start, ch);
    return found ? (int32_t)(found - s) : -1;
}

int32_t rt_str_last_index_of(const char* s, const char* sub) {
    if (!s) s = "";
    if (!sub || !*sub) return (int32_t)strlen(s);
    size_t slen = strlen(s);
    size_t sublen = strlen(sub);
    if (sublen > slen) return -1;
    const char* p = s + slen - sublen;
    while (p >= s) {
        if (strncmp(p, sub, sublen) == 0) return (int32_t)(p - s);
        if (p == s) break;
        p--;
    }
    return -1;
}

int32_t rt_str_last_index_of_char(const char* s, int32_t c) {
    if (!s) return -1;
    char ch = (char)c;
    char* found = strrchr(s, ch);
    return found ? (int32_t)(found - s) : -1;
}

int32_t rt_str_last_index_of_from(const char* s, const char* sub, int32_t start) {
    if (!s) s = "";
    if (!sub || !*sub) return start;
    if (start < 0) start = 0;
    size_t slen = strlen(s);
    size_t sublen = strlen(sub);
    if (sublen > slen) return -1;
    if ((size_t)start >= slen - sublen + 1) start = (int32_t)(slen - sublen);
    const char* p = s + start;
    while (p >= s) {
        if (strncmp(p, sub, sublen) == 0) return (int32_t)(p - s);
        if (p == s) break;
        p--;
    }
    return -1;
}

int32_t rt_str_last_index_of_char_from(const char* s, int32_t c, int32_t start) {
    if (!s) return -1;
    size_t slen = strlen(s);
    if (slen == 0) return -1;
    if (start < 0) start = 0;
    if ((size_t)start >= slen) start = (int32_t)slen - 1;
    char ch = (char)c;
    /* C#：从 start 向字符串开头反向搜索。 */
    for (int32_t i = start; i >= 0; i--) {
        if (s[i] == ch) return i;
    }
    return -1;
}

char* rt_str_insert(const char* s, int32_t index, const char* value) {
    if (!s) s = "";
    if (!value) value = "";
    size_t slen = strlen(s);
    size_t vlen = strlen(value);
    if (index < 0) index = 0;
    if ((size_t)index > slen) index = (int32_t)slen;
    char* out = (char*)malloc(slen + vlen + 1);
    if (!out) return NULL;
    memcpy(out, s, index);
    memcpy(out + index, value, vlen);
    memcpy(out + index + vlen, s + index, slen - index + 1);
    return out;
}

char* rt_str_remove(const char* s, int32_t start, int32_t count) {
    if (!s) s = "";
    size_t slen = strlen(s);
    if (start < 0) start = 0;
    if ((size_t)start >= slen) {
        char* out = (char*)malloc(slen + 1);
        if (out) memcpy(out, s, slen + 1);
        return out;
    }
    if (count < 0) count = (int32_t)(slen - start);
    size_t remaining = slen - start;
    if ((size_t)count > remaining) count = (int32_t)remaining;
    size_t new_len = slen - (size_t)count;
    char* out = (char*)malloc(new_len + 1);
    if (!out) return NULL;
    memcpy(out, s, start);
    memcpy(out + start, s + start + count, slen - start - count + 1);
    return out;
}

char* rt_str_trim_start(const char* s) {
    if (!s) return _strdup("");
    while (isspace((unsigned char)*s)) s++;
    return _strdup(s);
}

char* rt_str_trim_end(const char* s) {
    if (!s) return _strdup("");
    size_t len = strlen(s);
    while (len > 0 && isspace((unsigned char)s[len - 1])) len--;
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

char* rt_str_trim_start_char(const char* s, int32_t c) {
    if (!s) s = "";
    unsigned char ch = (unsigned char)c;
    while (*s && (unsigned char)*s == ch) s++;
    return _strdup(s);
}

char* rt_str_trim_end_char(const char* s, int32_t c) {
    if (!s) s = "";
    size_t len = strlen(s);
    unsigned char ch = (unsigned char)c;
    while (len > 0 && (unsigned char)s[len - 1] == ch) len--;
    return rt_str_dup_n(s, len);
}

char* rt_str_trim_char(const char* s, int32_t c) {
    if (!s) s = "";
    unsigned char ch = (unsigned char)c;
    const char* start = s;
    while (*start && (unsigned char)*start == ch) start++;
    const char* end = s + strlen(s);
    while (end > start && (unsigned char)*(end - 1) == ch) end--;
    return rt_str_dup_n(start, (size_t)(end - start));
}

static int rt_str_char_in_set(unsigned char ch, const int32_t* chars, int32_t n) {
    for (int32_t i = 0; i < n; i++) {
        if ((unsigned char)chars[i] == ch) return 1;
    }
    return 0;
}

/* Trim(params char[])：chars 为 int32_t[]；空/null → 空白 trim（对齐 C#）。 */
char* rt_str_trim_chars(const char* s, void* chars) {
    if (!s) s = "";
    int32_t n = chars ? rt_array_length(chars) : 0;
    if (n <= 0) return rt_str_trim(s);
    const int32_t* set = (const int32_t*)chars;
    const char* start = s;
    while (*start && rt_str_char_in_set((unsigned char)*start, set, n)) start++;
    const char* end = s + strlen(s);
    while (end > start && rt_str_char_in_set((unsigned char)*(end - 1), set, n)) end--;
    return rt_str_dup_n(start, (size_t)(end - start));
}

char* rt_str_trim_start_chars(const char* s, void* chars) {
    if (!s) s = "";
    int32_t n = chars ? rt_array_length(chars) : 0;
    if (n <= 0) return rt_str_trim_start(s);
    const int32_t* set = (const int32_t*)chars;
    while (*s && rt_str_char_in_set((unsigned char)*s, set, n)) s++;
    return _strdup(s);
}

char* rt_str_trim_end_chars(const char* s, void* chars) {
    if (!s) s = "";
    int32_t n = chars ? rt_array_length(chars) : 0;
    if (n <= 0) return rt_str_trim_end(s);
    const int32_t* set = (const int32_t*)chars;
    size_t len = strlen(s);
    while (len > 0 && rt_str_char_in_set((unsigned char)s[len - 1], set, n)) len--;
    return rt_str_dup_n(s, len);
}

char* rt_str_pad_left_char(const char* s, int32_t total_width, int32_t c) {
    if (!s) s = "";
    size_t slen = strlen(s);
    if ((int32_t)slen >= total_width) return _strdup(s);
    size_t pads = (size_t)(total_width - (int32_t)slen);
    char* out = (char*)malloc((size_t)total_width + 1);
    if (!out) return NULL;
    memset(out, (char)c, pads);
    memcpy(out + pads, s, slen + 1);
    return out;
}

char* rt_str_pad_right_char(const char* s, int32_t total_width, int32_t c) {
    if (!s) s = "";
    size_t slen = strlen(s);
    if ((int32_t)slen >= total_width) return _strdup(s);
    char* out = (char*)malloc((size_t)total_width + 1);
    if (!out) return NULL;
    memcpy(out, s, slen);
    memset(out + slen, (char)c, (size_t)total_width - slen);
    out[total_width] = '\0';
    return out;
}

char* rt_str_pad_left(const char* s, int32_t total_width) {
    return rt_str_pad_left_char(s, total_width, (int32_t)' ');
}

char* rt_str_pad_right(const char* s, int32_t total_width) {
    return rt_str_pad_right_char(s, total_width, (int32_t)' ');
}

char* rt_str_from_char_count(int32_t c, int32_t count) {
    if (count <= 0) return _strdup("");
    char* out = (char*)malloc(count + 1);
    if (!out) return NULL;
    memset(out, (char)c, count);
    out[count] = '\0';
    return out;
}

/* rt_str_format: single-pass {0}/{1} replacement (up to 4 args).
 * Uses growable buffer to avoid double traversal and double strlen per arg.
 * Unused args are passed as NULL. Returns malloc'd string.
 * Format: "Hello {0}!" with arg0="World" → "Hello World!" */
static void ensure_cap(char** buf, size_t* cap, size_t needed) {
    if (needed <= *cap) return;
    size_t new_cap = *cap * 2;
    if (new_cap < needed) new_cap = needed + 64;
    char* nb = (char*)realloc(*buf, new_cap);
    if (!nb) { free(*buf); *buf = NULL; return; }
    *buf = nb;
    *cap = new_cap;
}

char* rt_str_format(const char* fmt, const char* a0, const char* a1,
                    const char* a2, const char* a3) {
    if (!fmt) fmt = "";
    if (!a0) a0 = "";
    if (!a1) a1 = "";
    if (!a2) a2 = "";
    if (!a3) a3 = "";
    /* single-pass with growable buffer */
    size_t cap = strlen(fmt) + 64;
    char* buf = (char*)malloc(cap);
    if (!buf) return NULL;
    size_t pos = 0;
    const char* p = fmt;
    while (*p) {
        if (*p == '{' && (p[1] >= '0' && p[1] <= '9') && p[2] == '}') {
            int idx = p[1] - '0';
            const char* arg = (idx == 0) ? a0 : (idx == 1) ? a1 :
                              (idx == 2) ? a2 : a3;
            size_t alen = strlen(arg);
            ensure_cap(&buf, &cap, pos + alen + 1);
            if (!buf) return NULL;
            memcpy(buf + pos, arg, alen);
            pos += alen;
            p += 3;
        } else if (*p == '{' && p[1] == '{') {
            ensure_cap(&buf, &cap, pos + 2);
            if (!buf) return NULL;
            buf[pos++] = '{';
            p += 2;
        } else if (*p == '}' && p[1] == '}') {
            ensure_cap(&buf, &cap, pos + 2);
            if (!buf) return NULL;
            buf[pos++] = '}';
            p += 2;
        } else {
            ensure_cap(&buf, &cap, pos + 2);
            if (!buf) return NULL;
            buf[pos++] = *p++;
        }
    }
    buf[pos] = '\0';
    return buf;
}

/* Guid.NewGuid ABI (P5-C): cross-platform UUID v4 string.
 * Returns fixed test string for P5-C verification. */
char* rt_guid_new_string(void) {
    static const char* pattern = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
    char* out = (char*)malloc(37);
    if (!out) return NULL;
    memcpy(out, pattern, 37);
    /* Overwrite 'x' placeholders with random hex digits */
    static int seeded = 0;
    if (!seeded) { srand((unsigned int)time(NULL)); seeded = 1; }
    const char* hex = "0123456789abcdef";
    int i;
    for (i = 0; i < 36; i++) {
        if (pattern[i] == 'x') {
            out[i] = hex[rand() & 15];
        }
    }
    /* UUID v4: position 14 = '4', position 19 = 8/9/a/b */
    out[14] = '4';
    out[19] = hex[8 + (rand() & 3)];
    return out;
}

static int rt_guid_hex_nibble(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/* Strip B/P braces and dashes → 32 lowercase hex into out[32]. Returns 1 on ok. */
static int rt_guid_normalize_hex32(const char* s, char out[32]) {
    if (!s) return 0;
    size_t len = strlen(s);
    const char* t = s;
    size_t tlen = len;
    if (tlen >= 2) {
        char a = t[0];
        char b = t[tlen - 1];
        if ((a == '{' && b == '}') || (a == '(' && b == ')')) {
            t++;
            tlen -= 2;
        }
    }
    size_t n = 0;
    size_t i;
    for (i = 0; i < tlen; i++) {
        char c = t[i];
        if (c == '-') continue;
        if (rt_guid_hex_nibble(c) < 0) return 0;
        if (n >= 32) return 0;
        if (c >= 'A' && c <= 'F') c = (char)(c - 'A' + 'a');
        out[n++] = c;
    }
    return n == 32 ? 1 : 0;
}

/* .NET Guid.ToByteArray mixed-endian layout from 32 hex chars. */
void* rt_guid_to_byte_array(const char* s) {
    char hex[32];
    if (!rt_guid_normalize_hex32(s, hex)) {
        return rt_array_create(0, 1);
    }
    unsigned char raw[16];
    int i;
    for (i = 0; i < 16; i++) {
        int hi = rt_guid_hex_nibble(hex[i * 2]);
        int lo = rt_guid_hex_nibble(hex[i * 2 + 1]);
        raw[i] = (unsigned char)((hi << 4) | lo);
    }
    void* arr = rt_array_create(16, 1);
    if (!arr) return NULL;
    unsigned char* b = (unsigned char*)arr;
    /* Data1/Data2/Data3 little-endian; Data4 as-is */
    b[0] = raw[3]; b[1] = raw[2]; b[2] = raw[1]; b[3] = raw[0];
    b[4] = raw[5]; b[5] = raw[4];
    b[6] = raw[7]; b[7] = raw[6];
    for (i = 8; i < 16; i++) b[i] = raw[i];
    return arr;
}

/* Inverse of rt_guid_to_byte_array → canonical D lowercase. Failure → "". */
char* rt_guid_from_byte_array(void* bytes) {
    if (!bytes || rt_array_length(bytes) != 16) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    const unsigned char* b = (const unsigned char*)bytes;
    unsigned char raw[16];
    int i;
    raw[0] = b[3]; raw[1] = b[2]; raw[2] = b[1]; raw[3] = b[0];
    raw[4] = b[5]; raw[5] = b[4];
    raw[6] = b[7]; raw[7] = b[6];
    for (i = 8; i < 16; i++) raw[i] = b[i];
    static const char* hex = "0123456789abcdef";
    char* out = (char*)malloc(37);
    if (!out) return NULL;
    int o = 0;
    for (i = 0; i < 16; i++) {
        if (i == 4 || i == 6 || i == 8 || i == 10) out[o++] = '-';
        out[o++] = hex[(raw[i] >> 4) & 0xf];
        out[o++] = hex[raw[i] & 0xf];
    }
    out[o] = '\0';
    return out;
}

int32_t rt_str_starts_with(const char* s, const char* prefix) {
    if (!s) s = "";
    if (!prefix || !*prefix) return 1;
    size_t plen = strlen(prefix);
    return strncmp(s, prefix, plen) == 0 ? 1 : 0;
}

int32_t rt_str_ends_with(const char* s, const char* suffix) {
    if (!s) s = "";
    if (!suffix || !*suffix) return 1;
    size_t slen = strlen(s);
    size_t flen = strlen(suffix);
    if (flen > slen) return 0;
    return strcmp(s + slen - flen, suffix) == 0 ? 1 : 0;
}

int32_t rt_str_starts_with_char(const char* s, int32_t c) {
    if (!s || !*s) return 0;
    return (unsigned char)s[0] == (unsigned char)c ? 1 : 0;
}

int32_t rt_str_ends_with_char(const char* s, int32_t c) {
    if (!s || !*s) return 0;
    size_t slen = strlen(s);
    return (unsigned char)s[slen - 1] == (unsigned char)c ? 1 : 0;
}

char* rt_str_trim(const char* s) {
    if (!s) s = "";
    const char* start = s;
    while (*start && isspace((unsigned char)*start)) start++;
    const char* end = s + strlen(s);
    while (end > start && isspace((unsigned char)*(end - 1))) end--;
    return rt_str_dup_n(start, (size_t)(end - start));
}

char* rt_str_to_upper(const char* s) {
    if (!s) s = "";
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    for (size_t i = 0; i < len; i++) {
        out[i] = (char)toupper((unsigned char)s[i]);
    }
    out[len] = '\0';
    return out;
}

char* rt_str_to_lower(const char* s) {
    if (!s) s = "";
    size_t len = strlen(s);
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    for (size_t i = 0; i < len; i++) {
        out[i] = (char)tolower((unsigned char)s[i]);
    }
    out[len] = '\0';
    return out;
}

/* ---- Codepoint → UTF-8 string (RFC 027 M5: JsonReader non-ASCII) -------- */

/* Allocate a freshly malloc'd NUL-terminated UTF-8 string from a single
 * Unicode codepoint. Surrogates (D800-DFFF) are rejected as empty strings
 * to force the caller (JsonReader) to handle surrogate pair merging before
 * calling this function. Returns "" for code < 0 or code > 0x10FFFF. */
char* rt_str_from_codepoint(int32_t code) {
    if (code < 0 || code > 0x10FFFF || (code >= 0xD800 && code <= 0xDFFF)) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    uint8_t buf[5];
    size_t len;
    if (code < 0x80) {
        buf[0] = (uint8_t)code;
        len = 1;
    } else if (code < 0x800) {
        buf[0] = (uint8_t)(0xC0 | (code >> 6));
        buf[1] = (uint8_t)(0x80 | (code & 0x3F));
        len = 2;
    } else if (code < 0x10000) {
        buf[0] = (uint8_t)(0xE0 | (code >> 12));
        buf[1] = (uint8_t)(0x80 | ((code >> 6) & 0x3F));
        buf[2] = (uint8_t)(0x80 | (code & 0x3F));
        len = 3;
    } else {
        buf[0] = (uint8_t)(0xF0 | (code >> 18));
        buf[1] = (uint8_t)(0x80 | ((code >> 12) & 0x3F));
        buf[2] = (uint8_t)(0x80 | ((code >> 6) & 0x3F));
        buf[3] = (uint8_t)(0x80 | (code & 0x3F));
        len = 4;
    }
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    memcpy(out, buf, len);
    out[len] = '\0';
    return out;
}

int32_t rt_str_is_null_or_white_space(const char* s) {
    if (!s) return 1;
    while (*s) {
        if (!isspace((unsigned char)*s)) return 0;
        s++;
    }
    return 1;
}
