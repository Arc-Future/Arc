#include "rt_abi.h"
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char* skip_ws(const char* s) {
    while (isspace((unsigned char)*s)) s++;
    return s;
}

int32_t rt_parse_int32(const char* s) {
    int32_t result;
    if (rt_parse_int32_try(s, &result)) {
        return result;
    }
    rt_panic("int.Parse failed");
    return 0;
}

int32_t rt_parse_int32_try(const char* s, int32_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;

    char* end;
    errno = 0;
    long val = strtol(s, &end, 10);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    if (val < INT32_MIN || val > INT32_MAX) {
        return 0;
    }
    *out = (int32_t)val;
    return 1;
}

uint32_t rt_parse_uint32(const char* s) {
    uint32_t result;
    if (rt_parse_uint32_try(s, &result)) {
        return result;
    }
    rt_panic("uint.Parse failed");
    return 0;
}

int32_t rt_parse_uint32_try(const char* s, uint32_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;
    if (*s == '-') return 0; /* unsigned: no negative */

    char* end;
    errno = 0;
    unsigned long val = strtoul(s, &end, 10);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    if (val > UINT32_MAX) {
        return 0;
    }
    *out = (uint32_t)val;
    return 1;
}

int64_t rt_parse_int64(const char* s) {
    int64_t result;
    if (rt_parse_int64_try(s, &result)) {
        return result;
    }
    rt_panic("long.Parse failed");
    return 0;
}

int32_t rt_parse_int64_try(const char* s, int64_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;

    char* end;
    errno = 0;
    long long val = strtoll(s, &end, 10);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    *out = (int64_t)val;
    return 1;
}

uint64_t rt_parse_uint64(const char* s) {
    uint64_t result;
    if (rt_parse_uint64_try(s, &result)) {
        return result;
    }
    rt_panic("ulong.Parse failed");
    return 0;
}

int32_t rt_parse_uint64_try(const char* s, uint64_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;
    if (*s == '-') return 0; /* unsigned: no negative */

    char* end;
    errno = 0;
    unsigned long long val = strtoull(s, &end, 10);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    *out = val;
    return 1;
}

double rt_parse_double(const char* s) {
    double result;
    if (rt_parse_double_try(s, &result)) {
        return result;
    }
    rt_panic("double.Parse failed");
    return 0.0;
}

int32_t rt_parse_double_try(const char* s, double* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;

    char* end;
    errno = 0;
    double val = strtod(s, &end);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    *out = val;
    return 1;
}

float rt_parse_float(const char* s) {
    float result;
    if (rt_parse_float_try(s, &result)) {
        return result;
    }
    rt_panic("float.Parse failed");
    return 0.0f;
}

int32_t rt_parse_float_try(const char* s, float* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;

    char* end;
    errno = 0;
    float val = strtof(s, &end);
    if (errno != 0 || end == s || *skip_ws(end) != '\0') {
        return 0;
    }
    *out = val;
    return 1;
}

int32_t rt_parse_bool(const char* s) {
    int32_t result;
    if (rt_parse_bool_try(s, &result)) {
        return result;
    }
    rt_panic("bool.Parse failed");
    return 0;
}

int32_t rt_parse_bool_try(const char* s, int32_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);

    size_t len = strlen(s);
    if (len == 4) {
        if (s[0] == 't' || s[0] == 'T') {
            if ((s[1] == 'r' || s[1] == 'R') &&
                (s[2] == 'u' || s[2] == 'U') &&
                (s[3] == 'e' || s[3] == 'E')) {
                *out = 1;
                return 1;
            }
        }
    } else if (len == 5) {
        if (s[0] == 'f' || s[0] == 'F') {
            if ((s[1] == 'a' || s[1] == 'A') &&
                (s[2] == 'l' || s[2] == 'L') &&
                (s[3] == 's' || s[3] == 'S') &&
                (s[4] == 'e' || s[4] == 'E')) {
                *out = 0;
                return 1;
            }
        }
    }
    return 0;
}

int32_t rt_parse_char(const char* s) {
    int32_t result;
    if (rt_parse_char_try(s, &result)) {
        return result;
    }
    rt_panic("char.Parse failed");
    return 0;
}

int32_t rt_parse_char_try(const char* s, int32_t* out) {
    if (!s || *s == '\0') return 0;
    s = skip_ws(s);
    if (*s == '\0') return 0;
    /* char.Parse 只接受单个字符（C# 行为：string must be exactly one char） */
    int32_t code = (int32_t)(unsigned char)*s;
    s++;
    if (*skip_ws(s) != '\0') return 0; /* trailing content → fail */
    *out = code;
    return 1;
}

/* ---- Char classification/conversion ABI (P3-1b + P3-1c) ---- */

int32_t rt_char_is_digit(int32_t c) {
    return (c >= 0 && c <= 255) ? isdigit((unsigned char)c) : 0;
}

int32_t rt_char_is_letter(int32_t c) {
    return (c >= 0 && c <= 255) ? isalpha((unsigned char)c) : 0;
}

int32_t rt_char_is_white_space(int32_t c) {
    return (c >= 0 && c <= 255) ? isspace((unsigned char)c) : 0;
}

int32_t rt_char_is_upper(int32_t c) {
    return (c >= 0 && c <= 255) ? isupper((unsigned char)c) : 0;
}

int32_t rt_char_is_lower(int32_t c) {
    return (c >= 0 && c <= 255) ? islower((unsigned char)c) : 0;
}

int32_t rt_char_to_upper(int32_t c) {
    return (c >= 0 && c <= 255) ? toupper((unsigned char)c) : c;
}

int32_t rt_char_to_lower(int32_t c) {
    return (c >= 0 && c <= 255) ? tolower((unsigned char)c) : c;
}

/* ---- ToString ABI ---- */

char* rt_int_to_string(int32_t value) {
    char buf[12];
    snprintf(buf, sizeof(buf), "%d", value);
    return _strdup(buf);
}

char* rt_long_to_string(int64_t value) {
    char buf[21];
    snprintf(buf, sizeof(buf), "%lld", (long long)value);
    return _strdup(buf);
}

char* rt_short_to_string(int16_t value) {
    char buf[7];
    snprintf(buf, sizeof(buf), "%d", (int)value);
    return _strdup(buf);
}

char* rt_byte_to_string(int8_t value) {
    char buf[5];
    snprintf(buf, sizeof(buf), "%d", (int)(uint8_t)value);
    return _strdup(buf);
}

char* rt_float_to_string(float value) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%.7g", (double)value);
    return _strdup(buf);
}

char* rt_double_to_string(double value) {
    char buf[32];
    snprintf(buf, sizeof(buf), "%.15g", value);
    return _strdup(buf);
}

char* rt_uint_to_string(uint32_t value) {
    char buf[11];
    snprintf(buf, sizeof(buf), "%u", (unsigned int)value);
    return _strdup(buf);
}

char* rt_ulong_to_string(uint64_t value) {
    char buf[21];
    snprintf(buf, sizeof(buf), "%llu", (unsigned long long)value);
    return _strdup(buf);
}

char* rt_ushort_to_string(uint16_t value) {
    char buf[6];
    snprintf(buf, sizeof(buf), "%u", (unsigned int)value);
    return _strdup(buf);
}

char* rt_sbyte_to_string(int8_t value) {
    char buf[5];
    snprintf(buf, sizeof(buf), "%d", (int)value);
    return _strdup(buf);
}

char* rt_bool_to_string(int32_t value) {
    return _strdup(value ? "True" : "False");
}

char* rt_char_to_string(int32_t value) {
    char buf[2];
    buf[0] = (char)value;
    buf[1] = '\0';
    return _strdup(buf);
}

/* ---- RFC 007 M2a–M2i: format-aware ToString ---- */
/* M2b: N/C/E/P = CultureInfo.InvariantCulture 语义（千分位 `,`、小数 `.`、货币 `¤`、百分号前空格） */
/* M2c–M2e: 自定义 `0`/`#`、分组 `,`、缩放逗号、`;` 节、后缀 `%`（不变文化） */
/* M2f: 引号/'\\' 前缀/后缀字面量 */
/* M2g: 整数段占位间字面量（如 0'x'0） */
/* M2i: 小数段占位间字面量（如 0.0'x'0）；日期走 DateTime.ToString */
/* 立宪硬拒绝：FormattableString、文化感知格式（RFC 006） */

typedef struct {
    char spec;      /* D/X/x/F/G/N/C/E/e/P (x/e keep case) */
    int precision;  /* -1 = default (caller may still see -1 for D/X/G) */
} RtFmtSpec;

#define RT_CUSTOM_PH_MAX 64
#define RT_CUSTOM_LIT_MAX 64

typedef struct {
    char int_ph[RT_CUSTOM_PH_MAX]; /* '0'/'#' only（已去分组/缩放逗号） */
    int int_len;
    int int_zeros;                 /* int 段中 '0' 个数 = 最小整数位 */
    int group;                     /* 模式含分组逗号 → 不变文化千分位 */
    int scale;                     /* 缩放逗号个数 → 值÷1000^scale */
    char frac_ph[RT_CUSTOM_PH_MAX];
    int frac_len;                  /* -1 = 无小数点；否则占位长度 */
    int percent;                   /* 后缀 % → 值×100 后格式化，再追加 '%' */
    char prefix[RT_CUSTOM_LIT_MAX];
    char suffix[RT_CUSTOM_LIT_MAX];
    /* M2g：int_ph[i] 与 int_ph[i+1] 之间的字面量 */
    char int_mid[RT_CUSTOM_PH_MAX][RT_CUSTOM_LIT_MAX];
    int has_int_mid;
    /* M2i：小数点后、首占位前的字面量；frac_ph[i] 与 frac_ph[i+1] 之间 */
    char frac_prefix[RT_CUSTOM_LIT_MAX];
    char frac_mid[RT_CUSTOM_PH_MAX][RT_CUSTOM_LIT_MAX];
    int has_frac_mid;
} RtCustomFmt;

static int rt_custom_lit_push(char* buf, size_t* len, size_t max, char c) {
    if (*len + 1 >= max) return 0;
    buf[(*len)++] = c;
    buf[*len] = '\0';
    return 1;
}

/* M2f/M2g/M2i：`LIT* ([0#]+ LIT*)+(,[0#]+)*(,)*(\. LIT* ([0#]+ LIT*)+)? LIT*%?`
 * LIT = '…'（'' → 单引号）或 \c。
 * M2g：整数段占位之间允许 LIT；M2i：小数段同理（含 `.` 后首占位前 LIT）。
 * `,` 两侧皆占位 → 分组；紧邻 `.`/末尾/后缀 LIT/% → 缩放。 */
static int rt_parse_custom_numeric(const char* format, RtCustomFmt* out) {
    memset(out, 0, sizeof(*out));
    out->frac_len = -1;
    if (!format || !format[0]) return 0;

    size_t n = strlen(format);
    /* 尾部未加引号的 `%` → percent（先剥掉，避免与字面量 `'%'` 混淆） */
    if (n > 0 && format[n - 1] == '%') {
        /* 若 `%` 落在引号内则保留；简单判定：扫描引号配对 */
        int in_q = 0;
        int pct_quoted = 0;
        for (size_t t = 0; t < n; t++) {
            if (in_q) {
                if (format[t] == '\'') {
                    if (t + 1 < n && format[t + 1] == '\'') t++;
                    else in_q = 0;
                }
                if (t == n - 1) pct_quoted = 1;
                continue;
            }
            if (format[t] == '\\') {
                if (t + 1 < n) t++;
                if (t == n - 1) pct_quoted = 1; /* \% 字面量 */
                continue;
            }
            if (format[t] == '\'') in_q = 1;
        }
        if (!pct_quoted && !in_q) {
            out->percent = 1;
            n--;
            if (n == 0) return 0;
        }
    }

    size_t i = 0;
    char pending[RT_CUSTOM_LIT_MAX];
    size_t pend_len = 0;
    pending[0] = '\0';
    int seen_digit = 0;
    int in_frac = 0;
    int need_digit = 0;
    int scale_done = 0;

    while (i < n) {
        char c = format[i];

        if (c == '\'') {
            i++;
            int closed = 0;
            while (i < n) {
                if (format[i] == '\'') {
                    if (i + 1 < n && format[i + 1] == '\'') {
                        if (!rt_custom_lit_push(pending, &pend_len, RT_CUSTOM_LIT_MAX, '\''))
                            return 0;
                        i += 2;
                    } else {
                        i++;
                        closed = 1;
                        break;
                    }
                } else {
                    if (!rt_custom_lit_push(pending, &pend_len, RT_CUSTOM_LIT_MAX, format[i]))
                        return 0;
                    i++;
                }
            }
            if (!closed) return 0;
            continue;
        }
        if (c == '\\') {
            if (i + 1 >= n) return 0;
            if (!rt_custom_lit_push(pending, &pend_len, RT_CUSTOM_LIT_MAX, format[i + 1]))
                return 0;
            i += 2;
            continue;
        }
        if (c == '"') return 0;

        if (c == '0' || c == '#') {
            if (scale_done) return 0;
            if (pend_len > 0) {
                if (!seen_digit) {
                    memcpy(out->prefix, pending, pend_len + 1);
                } else if (in_frac) {
                    if (out->frac_len <= 0) {
                        /* M2i：小数点后、首占位前的字面量 */
                        memcpy(out->frac_prefix, pending, pend_len + 1);
                        out->has_frac_mid = 1;
                    } else {
                        size_t mi = (size_t)(out->frac_len - 1);
                        memcpy(out->frac_mid[mi], pending, pend_len + 1);
                        out->has_frac_mid = 1;
                    }
                } else {
                    /* M2g：挂到前一整数占位之后 */
                    if (out->int_len == 0) return 0;
                    size_t mi = (size_t)(out->int_len - 1);
                    memcpy(out->int_mid[mi], pending, pend_len + 1);
                    out->has_int_mid = 1;
                }
                pend_len = 0;
                pending[0] = '\0';
            }
            if (in_frac) {
                if (out->frac_len < 0) out->frac_len = 0;
                if (out->frac_len >= RT_CUSTOM_PH_MAX) return 0;
                out->frac_ph[out->frac_len++] = c;
            } else {
                if (out->int_len >= RT_CUSTOM_PH_MAX) return 0;
                out->int_ph[out->int_len++] = c;
                if (c == '0') out->int_zeros++;
            }
            seen_digit = 1;
            need_digit = 0;
            i++;
            continue;
        }

        if (c == ',') {
            if (in_frac || !seen_digit || need_digit || pend_len > 0 || scale_done) return 0;
            size_t j = i;
            while (j < n && format[j] == ',') j++;
            char next = (j < n) ? format[j] : '\0';
            if (j == n || next == '.' || next == '\'' || next == '\\') {
                out->scale = (int)(j - i);
                scale_done = 1;
                i = j;
                need_digit = 0;
                continue;
            }
            if (next != '0' && next != '#') return 0;
            out->group = 1;
            need_digit = 1;
            i++;
            continue;
        }

        if (c == '.') {
            if (in_frac || !seen_digit || need_digit || pend_len > 0) return 0;
            in_frac = 1;
            out->frac_len = 0;
            scale_done = 0; /* 缩放后仍可跟小数，如 0,,.00 */
            i++;
            need_digit = 1;
            continue;
        }

        return 0;
    }

    if (!seen_digit || need_digit) return 0;
    if (in_frac && out->frac_len <= 0) return 0;
    if (!in_frac) out->frac_len = -1;

    if (pend_len > 0) {
        memcpy(out->suffix, pending, pend_len + 1);
    }
    return 1;
}

/* 按值符号选取 `;` 节（至多 3 节；`;` 在引号内不分割）。空节 → *out_empty。负节 → *out_abs。 */
static int rt_custom_select_section(
    const char* format,
    int sign,
    char* buf,
    size_t buf_sz,
    int* out_empty,
    int* out_abs
) {
    *out_empty = 0;
    *out_abs = 0;
    if (!format) return 0;

    const char* s0 = format;
    const char* s1 = NULL;
    const char* s2 = NULL;
    int nsemi = 0;
    int in_quote = 0;
    for (const char* p = format; *p; p++) {
        if (in_quote) {
            if (*p == '\'') {
                if (p[1] == '\'') {
                    p++; /* '' */
                } else {
                    in_quote = 0;
                }
            }
            continue;
        }
        if (*p == '\\') {
            if (p[1]) p++;
            continue;
        }
        if (*p == '\'') {
            in_quote = 1;
            continue;
        }
        if (*p == '"') return 0;
        if (*p == ';') {
            nsemi++;
            if (nsemi > 2) return 0;
            if (nsemi == 1) s1 = p + 1;
            else s2 = p + 1;
        }
    }
    if (in_quote) return 0;

    const char* start;
    const char* end;
    size_t flen = strlen(format);

    if (sign < 0 && s1) {
        start = s1;
        end = s2 ? (s2 - 1) : (format + flen);
        *out_abs = 1;
    } else if (sign == 0 && s2) {
        start = s2;
        end = format + flen;
    } else {
        start = s0;
        end = s1 ? (s1 - 1) : (format + flen);
    }

    size_t len = (size_t)(end - start);
    if (len == 0) {
        *out_empty = 1;
        return 1;
    }
    if (len >= buf_sz) return 0;
    memcpy(buf, start, len);
    buf[len] = '\0';
    return 1;
}

/* 仅数字串插入不变文化千分位（无符号、无小数点）。 */
static int rt_fmt_group_digits(const char* digits, char* dst, size_t dst_sz) {
    size_t len = strlen(digits);
    if (len == 0) {
        if (dst_sz == 0) return 0;
        dst[0] = '\0';
        return 1;
    }
    size_t lead = len % 3;
    if (lead == 0) lead = 3;
    size_t di = 0;
    size_t i = 0;
    while (i < len) {
        size_t chunk = (i == 0) ? lead : 3;
        if (i > 0) {
            if (di + 1 >= dst_sz) return 0;
            dst[di++] = ',';
        }
        if (di + chunk >= dst_sz) return 0;
        memcpy(dst + di, digits + i, chunk);
        di += chunk;
        i += chunk;
    }
    dst[di] = '\0';
    return 1;
}

/* 无分组时：按占位从右分配数字，插入 int_mid。 */
static int rt_fmt_emit_int_mids(
    const char* ungrouped,
    const RtCustomFmt* cf,
    char* dst,
    size_t dst_sz
) {
    int n = cf->int_len;
    if (n <= 0) {
        if (dst_sz == 0) return 0;
        dst[0] = '\0';
        return 1;
    }
    size_t dlen = strlen(ungrouped);
    size_t lens[RT_CUSTOM_PH_MAX];
    size_t rem = dlen;
    for (int i = n - 1; i >= 0; i--) {
        if (i == 0) {
            lens[0] = rem;
        } else if (rem > 0) {
            lens[i] = 1;
            rem--;
        } else {
            lens[i] = 0;
        }
    }
    size_t oi = 0;
    size_t di = 0;
    for (int i = 0; i < n; i++) {
        if (oi + lens[i] >= dst_sz) return 0;
        if (lens[i] > 0) {
            memcpy(dst + oi, ungrouped + di, lens[i]);
            oi += lens[i];
            di += lens[i];
        }
        if (i + 1 < n) {
            size_t ml = strlen(cf->int_mid[i]);
            if (oi + ml >= dst_sz) return 0;
            if (ml) {
                memcpy(dst + oi, cf->int_mid[i], ml);
                oi += ml;
            }
        }
    }
    dst[oi] = '\0';
    return 1;
}

/* 有分组 + 占位间字面量：先千分位，再按未分组数字边界插入 mid。 */
static int rt_fmt_emit_int_mids_grouped(
    const char* ungrouped,
    const RtCustomFmt* cf,
    char* dst,
    size_t dst_sz
) {
    char grouped[192];
    if (!ungrouped[0]) {
        return rt_fmt_emit_int_mids(ungrouped, cf, dst, dst_sz);
    }
    if (!rt_fmt_group_digits(ungrouped, grouped, sizeof(grouped))) return 0;

    int n = cf->int_len;
    size_t dlen = strlen(ungrouped);
    size_t lens[RT_CUSTOM_PH_MAX];
    size_t rem = dlen;
    for (int i = n - 1; i >= 0; i--) {
        if (i == 0) {
            lens[0] = rem;
        } else if (rem > 0) {
            lens[i] = 1;
            rem--;
        } else {
            lens[i] = 0;
        }
    }

    /* 边界：ungrouped 中第 bound 个数字之后插入 mid[i] */
    size_t g_len = strlen(grouped);
    char buf[384];
    size_t bi = 0;
    size_t digit_seen = 0;
    size_t next_bound = lens[0];
    int mid_i = 0;
    for (size_t gi = 0; gi <= g_len; gi++) {
        while (mid_i + 1 < n && digit_seen == next_bound) {
            size_t ml = strlen(cf->int_mid[mid_i]);
            if (bi + ml >= sizeof(buf)) return 0;
            if (ml) {
                memcpy(buf + bi, cf->int_mid[mid_i], ml);
                bi += ml;
            }
            mid_i++;
            next_bound += lens[mid_i];
        }
        if (gi == g_len) break;
        if (bi + 1 >= sizeof(buf)) return 0;
        buf[bi++] = grouped[gi];
        if (grouped[gi] != ',') digit_seen++;
    }
    while (mid_i + 1 < n) {
        size_t ml = strlen(cf->int_mid[mid_i]);
        if (bi + ml >= sizeof(buf)) return 0;
        if (ml) {
            memcpy(buf + bi, cf->int_mid[mid_i], ml);
            bi += ml;
        }
        mid_i++;
    }
    buf[bi] = '\0';
    if (bi >= dst_sz) return 0;
    memcpy(dst, buf, bi + 1);
    return 1;
}

/* 整数位：'0' 个数为最小宽度；全 # 且值为 0 → 空串（可出现前导 '.'）。 */
static void rt_fmt_custom_int_digits(
    const char* raw_int,
    int int_zeros,
    char* out,
    size_t out_sz
) {
    int is_zero = (raw_int[0] == '0' && raw_int[1] == '\0');
    if (is_zero && int_zeros == 0) {
        out[0] = '\0';
        return;
    }
    if (is_zero) {
        if ((size_t)int_zeros >= out_sz) {
            rt_panic("custom format int buffer overflow");
            out[0] = '\0';
            return;
        }
        for (int i = 0; i < int_zeros; i++) out[i] = '0';
        out[int_zeros] = '\0';
        return;
    }
    size_t il = strlen(raw_int);
    size_t need = (il < (size_t)int_zeros) ? (size_t)int_zeros - il : 0;
    if (need + il >= out_sz) {
        rt_panic("custom format int buffer overflow");
        out[0] = '\0';
        return;
    }
    for (size_t z = 0; z < need; z++) out[z] = '0';
    memcpy(out + need, raw_int, il + 1);
}

/* 小数位：自右剥离尾随 `#` 对应的 '0'；插入 frac_prefix / frac_mid（对齐 .NET：
 * 占位数字仅保留到 last；字面量一律保留。无剩余数字时省略小数点，字面量仍输出）。 */
static void rt_fmt_custom_frac_out(
    const char* frac_digits,
    const RtCustomFmt* cf,
    char* out,
    size_t out_sz,
    int* out_has_dot
) {
    *out_has_dot = 0;
    out[0] = '\0';
    int frac_len = cf->frac_len;
    if (frac_len <= 0) {
        return;
    }
    int last = frac_len - 1;
    while (last >= 0 && cf->frac_ph[last] == '#' && frac_digits[last] == '0') {
        last--;
    }

    size_t oi = 0;
    size_t plen = strlen(cf->frac_prefix);
    if (plen) {
        if (oi + plen >= out_sz) {
            rt_panic("custom format frac buffer overflow");
            out[0] = '\0';
            return;
        }
        memcpy(out + oi, cf->frac_prefix, plen);
        oi += plen;
    }
    for (int i = 0; i < frac_len; i++) {
        if (i <= last) {
            if (oi + 1 >= out_sz) {
                rt_panic("custom format frac buffer overflow");
                out[0] = '\0';
                return;
            }
            out[oi++] = frac_digits[i];
        }
        if (i + 1 < frac_len) {
            size_t ml = strlen(cf->frac_mid[i]);
            if (oi + ml >= out_sz) {
                rt_panic("custom format frac buffer overflow");
                out[0] = '\0';
                return;
            }
            if (ml) {
                memcpy(out + oi, cf->frac_mid[i], ml);
                oi += ml;
            }
        }
    }
    out[oi] = '\0';
    *out_has_dot = (last >= 0);
}

/* 组装自定义格式结果：sign + prefix + int(+mids) + [.frac|frac_lits] + suffix + [%] */
static char* rt_fmt_custom_assemble(
    int neg,
    const char* int_digits,
    const RtCustomFmt* cf,
    const char* frac_out,
    int frac_has_dot
) {
    char int_buf[384];
    const char* id = int_digits;
    if (cf->has_int_mid) {
        int ok = cf->group
            ? rt_fmt_emit_int_mids_grouped(int_digits, cf, int_buf, sizeof(int_buf))
            : rt_fmt_emit_int_mids(int_digits, cf, int_buf, sizeof(int_buf));
        if (!ok) {
            rt_panic("custom format mid-literal buffer overflow");
            return NULL;
        }
        id = int_buf;
    } else if (cf->group && int_digits[0]) {
        if (!rt_fmt_group_digits(int_digits, int_buf, sizeof(int_buf))) {
            rt_panic("custom format group buffer overflow");
            return NULL;
        }
        id = int_buf;
    }
    const char* prefix = cf->prefix;
    const char* suffix = cf->suffix;
    int percent = cf->percent;
    size_t frac_len = frac_out ? strlen(frac_out) : 0;
    size_t plen = prefix ? strlen(prefix) : 0;
    size_t slen = suffix ? strlen(suffix) : 0;
    size_t total = (neg ? 1u : 0u) + plen + strlen(id)
        + (frac_len > 0 ? (frac_has_dot ? 1u : 0u) + frac_len : 0u)
        + slen
        + (percent ? 1u : 0u);
    char* out = (char*)malloc(total + 1);
    if (!out) {
        rt_panic("out of memory");
        return NULL;
    }
    size_t oi = 0;
    if (neg) out[oi++] = '-';
    if (plen) {
        memcpy(out + oi, prefix, plen);
        oi += plen;
    }
    size_t il = strlen(id);
    memcpy(out + oi, id, il);
    oi += il;
    if (frac_len > 0) {
        if (frac_has_dot) out[oi++] = '.';
        memcpy(out + oi, frac_out, frac_len);
        oi += frac_len;
    }
    if (slen) {
        memcpy(out + oi, suffix, slen);
        oi += slen;
    }
    if (percent) out[oi++] = '%';
    out[oi] = '\0';
    return out;
}

static char* rt_fmt_custom_f64(double value, const RtCustomFmt* cf) {
    /* value * (percent ? 100 : 1) / 1000^scale */
    double v = cf->percent ? value * 100.0 : value;
    for (int s = 0; s < cf->scale; s++) v /= 1000.0;
    int neg = (v < 0.0);
    double av = neg ? -v : v;
    int frac_n = (cf->frac_len < 0) ? 0 : cf->frac_len;
    char raw[160];
    snprintf(raw, sizeof(raw), "%.*f", frac_n, av);

    char raw_int[160];
    char raw_frac[160];
    char* dot = strchr(raw, '.');
    if (dot) {
        size_t il = (size_t)(dot - raw);
        memcpy(raw_int, raw, il);
        raw_int[il] = '\0';
        memcpy(raw_frac, dot + 1, strlen(dot + 1) + 1);
    } else {
        memcpy(raw_int, raw, strlen(raw) + 1);
        raw_frac[0] = '\0';
    }

    char int_digits[192];
    char frac_out[384];
    int frac_has_dot = 0;
    rt_fmt_custom_int_digits(raw_int, cf->int_zeros, int_digits, sizeof(int_digits));
    rt_fmt_custom_frac_out(raw_frac, cf, frac_out, sizeof(frac_out), &frac_has_dot);
    return rt_fmt_custom_assemble(
        neg, int_digits, cf, frac_out, frac_has_dot
    );
}

static char* rt_fmt_custom_u64_mag(
    uint64_t mag,
    int neg,
    const RtCustomFmt* cf
) {
    char raw_int[160];
    if (mag == 0) {
        raw_int[0] = '0';
        raw_int[1] = '\0';
    } else {
        snprintf(raw_int, sizeof(raw_int), "%llu", (unsigned long long)mag);
    }
    char int_digits[192];
    rt_fmt_custom_int_digits(raw_int, cf->int_zeros, int_digits, sizeof(int_digits));
    return rt_fmt_custom_assemble(
        neg, int_digits, cf, "", 0
    );
}

static char* rt_fmt_custom_i64(int64_t value, const RtCustomFmt* cf) {
    /* 整数-only 且无 %/缩放：避免大整数经 double 丢精度 */
    if (cf->frac_len < 0 && !cf->percent && cf->scale == 0) {
        int neg = (value < 0);
        uint64_t mag = (value == INT64_MIN)
            ? (uint64_t)INT64_MAX + 1ULL
            : (uint64_t)(neg ? -value : value);
        return rt_fmt_custom_u64_mag(mag, neg, cf);
    }
    return rt_fmt_custom_f64((double)value, cf);
}

static char* rt_fmt_custom_u64(uint64_t value, const RtCustomFmt* cf) {
    if (cf->frac_len < 0 && !cf->percent && cf->scale == 0) {
        return rt_fmt_custom_u64_mag(value, 0, cf);
    }
    return rt_fmt_custom_f64((double)value, cf);
}

/* M2e：选节 → 解析 → 格式化；空节 → ""。 */
static char* rt_fmt_custom_i64_fmt(int64_t value, const char* format) {
    char sec[256];
    int empty = 0, abs_only = 0;
    int sign = (value > 0) ? 1 : ((value < 0) ? -1 : 0);
    if (!rt_custom_select_section(format, sign, sec, sizeof(sec), &empty, &abs_only)) {
        return NULL;
    }
    if (empty) return _strdup("");
    RtCustomFmt cf;
    if (!rt_parse_custom_numeric(sec, &cf)) return NULL;
    if (abs_only) {
        uint64_t mag = (value == INT64_MIN)
            ? (uint64_t)INT64_MAX + 1ULL
            : (uint64_t)(-value);
        if (cf.frac_len < 0 && !cf.percent && cf.scale == 0) {
            return rt_fmt_custom_u64_mag(mag, 0, &cf);
        }
        return rt_fmt_custom_f64((double)mag, &cf);
    }
    return rt_fmt_custom_i64(value, &cf);
}

static char* rt_fmt_custom_u64_fmt(uint64_t value, const char* format) {
    char sec[256];
    int empty = 0, abs_only = 0;
    int sign = (value > 0) ? 1 : 0;
    if (!rt_custom_select_section(format, sign, sec, sizeof(sec), &empty, &abs_only)) {
        return NULL;
    }
    if (empty) return _strdup("");
    RtCustomFmt cf;
    if (!rt_parse_custom_numeric(sec, &cf)) return NULL;
    (void)abs_only;
    return rt_fmt_custom_u64(value, &cf);
}

static char* rt_fmt_custom_f64_fmt(double value, const char* format) {
    char sec[256];
    int empty = 0, abs_only = 0;
    int sign = (value > 0.0) ? 1 : ((value < 0.0) ? -1 : 0);
    if (!rt_custom_select_section(format, sign, sec, sizeof(sec), &empty, &abs_only)) {
        return NULL;
    }
    if (empty) return _strdup("");
    RtCustomFmt cf;
    if (!rt_parse_custom_numeric(sec, &cf)) return NULL;
    double v = abs_only ? (value < 0.0 ? -value : value) : value;
    return rt_fmt_custom_f64(v, &cf);
}

static int rt_parse_fmt_spec(const char* format, RtFmtSpec* out) {
    if (!format || !format[0]) return 0;
    char c = format[0];
    int prec = -1;
    if (format[1]) {
        char* end = NULL;
        long p = strtol(format + 1, &end, 10);
        if (end == format + 1 || *end != '\0' || p < 0 || p > 99) return 0;
        prec = (int)p;
    }
    switch (c) {
        case 'D': case 'd': out->spec = 'D'; out->precision = prec; return 1;
        case 'X': out->spec = 'X'; out->precision = prec; return 1;
        case 'x': out->spec = 'x'; out->precision = prec; return 1;
        case 'F': case 'f': out->spec = 'F'; out->precision = (prec < 0) ? 2 : prec; return 1;
        case 'G': case 'g': out->spec = 'G'; out->precision = prec; return 1;
        case 'N': case 'n': out->spec = 'N'; out->precision = (prec < 0) ? 2 : prec; return 1;
        case 'C': case 'c': out->spec = 'C'; out->precision = (prec < 0) ? 2 : prec; return 1;
        case 'E': out->spec = 'E'; out->precision = (prec < 0) ? 6 : prec; return 1;
        case 'e': out->spec = 'e'; out->precision = (prec < 0) ? 6 : prec; return 1;
        case 'P': case 'p': out->spec = 'P'; out->precision = (prec < 0) ? 2 : prec; return 1;
        default: return 0;
    }
}

/* Insert invariant group separators into a fixed-point string like "-1234.56". */
static int rt_fmt_insert_groups(const char* src, char* dst, size_t dst_sz) {
    const char* s = src;
    size_t di = 0;
    if (*s == '-' || *s == '+') {
        if (di + 1 >= dst_sz) return 0;
        dst[di++] = *s++;
    }
    const char* dot = strchr(s, '.');
    size_t int_len = dot ? (size_t)(dot - s) : strlen(s);
    if (int_len == 0) return 0;
    size_t lead = int_len % 3;
    if (lead == 0) lead = 3;
    size_t i = 0;
    while (i < int_len) {
        size_t chunk = (i == 0) ? lead : 3;
        if (i > 0) {
            if (di + 1 >= dst_sz) return 0;
            dst[di++] = ',';
        }
        if (di + chunk >= dst_sz) return 0;
        memcpy(dst + di, s + i, chunk);
        di += chunk;
        i += chunk;
    }
    if (dot) {
        size_t frac_len = strlen(dot);
        if (di + frac_len >= dst_sz) return 0;
        memcpy(dst + di, dot, frac_len + 1);
    } else {
        dst[di] = '\0';
    }
    return 1;
}

/* Invariant N: group + decimal; C: same with ¤ after sign; P: *100 then N + " %". */
static char* rt_fmt_ncp(double value, char kind, int precision) {
    char raw[160];
    char grouped[192];
    char out[208];
    double v = value;
    if (kind == 'P') v = value * 100.0;
    snprintf(raw, sizeof(raw), "%.*f", precision, v);
    if (!rt_fmt_insert_groups(raw, grouped, sizeof(grouped))) {
        rt_panic("numeric format buffer overflow");
        return NULL;
    }
    if (kind == 'N') {
        return _strdup(grouped);
    }
    if (kind == 'C') {
        /* InvariantCulture CurrencyPositivePattern 0: ¤n ; negative: -¤n */
        const char* g = grouped;
        size_t oi = 0;
        if (*g == '-') {
            out[oi++] = '-';
            g++;
        }
        out[oi++] = (char)0xC2; /* UTF-8 ¤ = U+00A4 */
        out[oi++] = (char)0xA4;
        size_t glen = strlen(g);
        if (oi + glen >= sizeof(out)) {
            rt_panic("currency format buffer overflow");
            return NULL;
        }
        memcpy(out + oi, g, glen + 1);
        return _strdup(out);
    }
    /* P */
    snprintf(out, sizeof(out), "%s %%", grouped);
    return _strdup(out);
}

/* Invariant E/e: mantissa precision + exponent width ≥ 3 (C#). */
static char* rt_fmt_exp(double value, char spec, int precision) {
    char buf[160];
    char out[160];
    snprintf(buf, sizeof(buf), (spec == 'E') ? "%.*E" : "%.*e", precision, value);
    /* Normalize exponent to at least 3 digits: 1.23E+3 → 1.23E+003 */
    char* ep = strchr(buf, (spec == 'E') ? 'E' : 'e');
    if (!ep) return _strdup(buf);
    char* sign = ep + 1;
    if (*sign != '+' && *sign != '-') return _strdup(buf);
    char* digits = sign + 1;
    size_t dlen = strlen(digits);
    if (dlen >= 3) return _strdup(buf);
    size_t head = (size_t)(digits - buf);
    memcpy(out, buf, head);
    size_t oi = head;
    for (size_t z = 0; z < 3 - dlen; z++) out[oi++] = '0';
    memcpy(out + oi, digits, dlen + 1);
    return _strdup(out);
}

static char* rt_fmt_i64(int64_t value, const char* format) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) {
        char* custom = rt_fmt_custom_i64_fmt(value, format);
        if (custom) return custom;
        rt_panic("unsupported numeric format specifier (RFC 007: D/X/F/G/N/C/E/P or custom 0/#/,/%/; scale/sections)");
        return NULL;
    }
    char buf[160];
    if (fs.spec == 'D') {
        if (fs.precision < 0) {
            snprintf(buf, sizeof(buf), "%lld", (long long)value);
        } else if (value < 0) {
            /* C#: sign then zero-padded digits；避开 INT64_MIN 取负溢出 */
            unsigned long long mag = (value == INT64_MIN)
                ? (unsigned long long)INT64_MAX + 1ULL
                : (unsigned long long)(-(value + 1)) + 1ULL;
            snprintf(buf, sizeof(buf), "-%0*llu", fs.precision, mag);
        } else {
            snprintf(buf, sizeof(buf), "%0*lld", fs.precision, (long long)value);
        }
        return _strdup(buf);
    } else if (fs.spec == 'X' || fs.spec == 'x') {
        /* two's-complement bit pattern as unsigned 64 for long; callers narrow */
        unsigned long long u = (unsigned long long)value;
        const char* fmt = (fs.spec == 'X')
            ? ((fs.precision < 0) ? "%llX" : "%0*llX")
            : ((fs.precision < 0) ? "%llx" : "%0*llx");
        if (fs.precision < 0) snprintf(buf, sizeof(buf), fmt, u);
        else snprintf(buf, sizeof(buf), fmt, fs.precision, u);
        return _strdup(buf);
    } else if (fs.spec == 'F') {
        snprintf(buf, sizeof(buf), "%.*f", fs.precision, (double)value);
        return _strdup(buf);
    } else if (fs.spec == 'G') {
        if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", (double)value);
        else snprintf(buf, sizeof(buf), "%.*g", fs.precision, (double)value);
        return _strdup(buf);
    } else if (fs.spec == 'N' || fs.spec == 'C' || fs.spec == 'P') {
        return rt_fmt_ncp((double)value, fs.spec, fs.precision);
    } else { /* E / e */
        return rt_fmt_exp((double)value, fs.spec, fs.precision);
    }
}

static char* rt_fmt_u64(uint64_t value, const char* format) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) {
        char* custom = rt_fmt_custom_u64_fmt(value, format);
        if (custom) return custom;
        rt_panic("unsupported numeric format specifier (RFC 007: D/X/F/G/N/C/E/P or custom 0/#/,/%/; scale/sections)");
        return NULL;
    }
    char buf[160];
    if (fs.spec == 'D') {
        if (fs.precision < 0) snprintf(buf, sizeof(buf), "%llu", (unsigned long long)value);
        else snprintf(buf, sizeof(buf), "%0*llu", fs.precision, (unsigned long long)value);
        return _strdup(buf);
    } else if (fs.spec == 'X' || fs.spec == 'x') {
        const char* fmt = (fs.spec == 'X')
            ? ((fs.precision < 0) ? "%llX" : "%0*llX")
            : ((fs.precision < 0) ? "%llx" : "%0*llx");
        if (fs.precision < 0) snprintf(buf, sizeof(buf), fmt, (unsigned long long)value);
        else snprintf(buf, sizeof(buf), fmt, fs.precision, (unsigned long long)value);
        return _strdup(buf);
    } else if (fs.spec == 'F') {
        snprintf(buf, sizeof(buf), "%.*f", fs.precision, (double)value);
        return _strdup(buf);
    } else if (fs.spec == 'G') {
        if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", (double)value);
        else snprintf(buf, sizeof(buf), "%.*g", fs.precision, (double)value);
        return _strdup(buf);
    } else if (fs.spec == 'N' || fs.spec == 'C' || fs.spec == 'P') {
        return rt_fmt_ncp((double)value, fs.spec, fs.precision);
    } else {
        return rt_fmt_exp((double)value, fs.spec, fs.precision);
    }
}

static char* rt_fmt_f64(double value, const char* format) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) {
        char* custom = rt_fmt_custom_f64_fmt(value, format);
        if (custom) return custom;
        rt_panic("unsupported numeric format specifier (RFC 007: D/X/F/G/N/C/E/P or custom 0/#/,/%/; scale/sections)");
        return NULL;
    }
    char buf[160];
    if (fs.spec == 'D' || fs.spec == 'X' || fs.spec == 'x') {
        rt_panic("D/X format requires integer type");
        return NULL;
    }
    if (fs.spec == 'F') {
        snprintf(buf, sizeof(buf), "%.*f", fs.precision, value);
        return _strdup(buf);
    }
    if (fs.spec == 'G') {
        if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", value);
        else snprintf(buf, sizeof(buf), "%.*g", fs.precision, value);
        return _strdup(buf);
    }
    if (fs.spec == 'N' || fs.spec == 'C' || fs.spec == 'P') {
        return rt_fmt_ncp(value, fs.spec, fs.precision);
    }
    return rt_fmt_exp(value, fs.spec, fs.precision);
}

char* rt_int_to_string_fmt(int32_t value, const char* format) {
    /* X on int32: 8-digit hex of unsigned bit pattern when no precision — C# default */
    RtFmtSpec fs;
    if (rt_parse_fmt_spec(format, &fs) && (fs.spec == 'X' || fs.spec == 'x')) {
        uint32_t u = (uint32_t)value;
        char buf[32];
        if (fs.precision < 0) {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%X" : "%x", u);
        } else {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%0*X" : "%0*x", fs.precision, u);
        }
        return _strdup(buf);
    }
    return rt_fmt_i64((int64_t)value, format);
}

char* rt_long_to_string_fmt(int64_t value, const char* format) {
    return rt_fmt_i64(value, format);
}

char* rt_short_to_string_fmt(int16_t value, const char* format) {
    RtFmtSpec fs;
    if (rt_parse_fmt_spec(format, &fs) && (fs.spec == 'X' || fs.spec == 'x')) {
        uint16_t u = (uint16_t)value;
        char buf[16];
        if (fs.precision < 0) {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%X" : "%x", (unsigned)u);
        } else {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%0*X" : "%0*x", fs.precision, (unsigned)u);
        }
        return _strdup(buf);
    }
    return rt_fmt_i64((int64_t)value, format);
}

char* rt_byte_to_string_fmt(uint8_t value, const char* format) {
    return rt_fmt_u64((uint64_t)value, format);
}

char* rt_sbyte_to_string_fmt(int8_t value, const char* format) {
    RtFmtSpec fs;
    if (rt_parse_fmt_spec(format, &fs) && (fs.spec == 'X' || fs.spec == 'x')) {
        uint8_t u = (uint8_t)value;
        char buf[8];
        if (fs.precision < 0) {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%X" : "%x", (unsigned)u);
        } else {
            snprintf(buf, sizeof(buf), fs.spec == 'X' ? "%0*X" : "%0*x", fs.precision, (unsigned)u);
        }
        return _strdup(buf);
    }
    return rt_fmt_i64((int64_t)value, format);
}

char* rt_uint_to_string_fmt(uint32_t value, const char* format) {
    return rt_fmt_u64((uint64_t)value, format);
}

char* rt_ulong_to_string_fmt(uint64_t value, const char* format) {
    return rt_fmt_u64(value, format);
}

char* rt_ushort_to_string_fmt(uint16_t value, const char* format) {
    return rt_fmt_u64((uint64_t)value, format);
}

char* rt_float_to_string_fmt(float value, const char* format) {
    return rt_fmt_f64((double)value, format);
}

char* rt_double_to_string_fmt(double value, const char* format) {
    return rt_fmt_f64(value, format);
}

/* =====================================================================
 * RFC 027 M5: 文化感知 ToString(format, provider)
 * ---------------------------------------------------------------------
 * 基元数值的 `ToString(string format, IFormatProvider provider)` 有参重载。
 * codegen 将 3 参调用路由到 `rt_*_to_string_fmt_p(value, format, provider)`，
 * 本段在 C 侧解析 provider → NumberFormatInfo 的模板字段（按 typeck layout.rs
 * 的确定性类布局偏移读取），并对 F/N/C/P 标准格式做文化感知渲染。
 *
 * 布局依据（typeck/src/layout.rs `collect_fields` + `abi_size_align`，
 * HEADER_SIZE = 16，string/int 分别 8/4 字节对齐）：
 *   NumberFormatInfo 实例字段偏移（对应 NumberFormatInfo.as 声明序）：
 *     +16  NumberDecimalSeparator    +24 NumberGroupSeparator
 *     +40  CurrencySymbol            +48 CurrencyDecimalSeparator
 *     +56  CurrencyGroupSeparator    +64 PercentSymbol
 *     +80  NegativeSign
 *     +104 PercentDecimalSeparator   +112 PercentGroupSeparator
 *     +120 CurrencyPositivePattern   +124 CurrencyNegativePattern
 *     +128 NumberNegativePattern     +132 PercentPositivePattern
 *     +136 PercentNegativePattern
 *   CultureInfo 实例字段偏移：+16 _numberFormat（首个实例字段）
 * 偏移随 std 源字段顺序变更时必须同步更新（见 NumberFormatInfo.as /
 * CultureInfo.as 字段声明顺序）。
 * ===================================================================== */

typedef struct {
    const char* num_decimal_sep;
    const char* num_group_sep;
    const char* currency_symbol;
    const char* currency_decimal_sep;
    const char* currency_group_sep;
    const char* percent_symbol;
    const char* neg_sign;
    const char* percent_decimal_sep;
    const char* percent_group_sep;
    int currency_pos_pattern;
    int currency_neg_pattern;
    int number_neg_pattern;
    int percent_pos_pattern;
    int percent_neg_pattern;
} RtNfi;

/* C# 负数模式：n=绝对值，-=负号，¤=货币/百分号符号，空格=字面空格 */
static const char* NUM_NEG_PATTERNS[5] = { "(n)", "-n", "- n", "n-", "n -" };
static const char* CUR_POS_PATTERNS[4] = { "\xC2\xA4n", "n\xC2\xA4", "\xC2\xA4 n", "n \xC2\xA4" };
static const char* CUR_NEG_PATTERNS[16] = {
    "(\xC2\xA4n)", "-\xC2\xA4n", "\xC2\xA4-n", "\xC2\xA4n-", "(n\xC2\xA4)", "-n\xC2\xA4",
    "n-\xC2\xA4", "n\xC2\xA4-", "-n \xC2\xA4", "-\xC2\xA4 n", "n \xC2\xA4-", "\xC2\xA4 n-",
    "\xC2\xA4 -n", "n- \xC2\xA4", "(\xC2\xA4 n)", "(n \xC2\xA4)"
};
static const char* PER_POS_PATTERNS[4] = { "n %", "n%", "%n", "% n" };
static const char* PER_NEG_PATTERNS[16] = {
    "-n %", "-n%", "-%n", "%-n", "% -n", "n %-", "n%-", "-% n",
    "n- %", "n-%", "% n-", "%n-", "n %-", "n%-", "%-n", "%n-"
};

#define NFI_NUM_DEC_SEP  16
#define NFI_NUM_GRP_SEP  24
#define NFI_CUR_SYMBOL   40
#define NFI_CUR_DEC_SEP  48
#define NFI_CUR_GRP_SEP  56
#define NFI_PERCENT_SYM  64
#define NFI_NEG_SIGN     80
#define NFI_PERCENT_DEC  104
#define NFI_PERCENT_GRP  112
#define NFI_CUR_POS_PAT  120
#define NFI_CUR_NEG_PAT  124
#define NFI_NUM_NEG_PAT  128
#define NFI_PER_POS_PAT  132
#define NFI_PER_NEG_PAT  136
#define CI_NUM_FORMAT    16

static const RtNfi RT_NFI_INVARIANT = {
    ".", ",", "\xC2\xA4", ".", ",", "%", "-", ".", ",",
    0, 0, 1, 0, 0
};

/* 对象 vtable slot 0 = RtTypeInfo*（ArcHeader offset 8 = vtable）。 */
static const RtTypeInfo* rt_obj_typeinfo(void* obj) {
    if (!obj) return NULL;
    void* vtbl = *(void**)((char*)obj + 8);
    return vtbl ? *(const RtTypeInfo**)vtbl : NULL;
}

static void rt_nfi_fill(void* nfi_obj, RtNfi* out) {
    const char* b = (const char*)nfi_obj;
    out->num_decimal_sep = *(const char**)(b + NFI_NUM_DEC_SEP);
    out->num_group_sep = *(const char**)(b + NFI_NUM_GRP_SEP);
    out->currency_symbol = *(const char**)(b + NFI_CUR_SYMBOL);
    out->currency_decimal_sep = *(const char**)(b + NFI_CUR_DEC_SEP);
    out->currency_group_sep = *(const char**)(b + NFI_CUR_GRP_SEP);
    out->percent_symbol = *(const char**)(b + NFI_PERCENT_SYM);
    out->neg_sign = *(const char**)(b + NFI_NEG_SIGN);
    out->percent_decimal_sep = *(const char**)(b + NFI_PERCENT_DEC);
    out->percent_group_sep = *(const char**)(b + NFI_PERCENT_GRP);
    out->currency_pos_pattern = *(int32_t*)(b + NFI_CUR_POS_PAT);
    out->currency_neg_pattern = *(int32_t*)(b + NFI_CUR_NEG_PAT);
    out->number_neg_pattern = *(int32_t*)(b + NFI_NUM_NEG_PAT);
    out->percent_pos_pattern = *(int32_t*)(b + NFI_PER_POS_PAT);
    out->percent_neg_pattern = *(int32_t*)(b + NFI_PER_NEG_PAT);
    if (!out->num_decimal_sep) out->num_decimal_sep = ".";
    if (!out->num_group_sep) out->num_group_sep = ",";
    if (!out->currency_symbol) out->currency_symbol = "\xC2\xA4";
    if (!out->percent_symbol) out->percent_symbol = "%";
    if (!out->neg_sign) out->neg_sign = "-";
}

/* 解析 provider → RtNfi。仅识别 NumberFormatInfo / CultureInfo；null 或其它回退 Invariant。
 * 注意：codegen 不调用 rt_type_register（用户类型表为空），故不能经 rt_type_by_name
 * 解析用户类型；改为直接读取对象 typeinfo 的 full_name（RtTypeInfo 偏移 24，含命名空间
 * 之前的简单类名常量）做精确匹配。 */
static void rt_nfi_resolve(void* provider, RtNfi* out) {
    *out = RT_NFI_INVARIANT;
    if (!provider) return;
    const RtTypeInfo* ti = rt_obj_typeinfo(provider);
    if (!ti) return;
    const char* full = ti->full_name;
    if (full && strcmp(full, "NumberFormatInfo") == 0) {
        rt_nfi_fill(provider, out);
        return;
    }
    if (full && strcmp(full, "CultureInfo") == 0) {
        void* nfi = *(void**)((char*)provider + CI_NUM_FORMAT);
        if (nfi) rt_nfi_fill(nfi, out);
        return;
    }
    /* 其它 IFormatProvider：不解析（诚实回退 Invariant） */
}

/* 将定点字符串（'.' 小数、无分组）按文化 sep 分组并替换小数点。 */
static int rt_fmt_numeric_group(const char* raw, const char* grp_sep, const char* dec_sep,
                                char* dst, size_t dst_sz) {
    const char* s = raw;
    size_t di = 0;
    if (*s == '-' || *s == '+') {
        if (di + 1 >= dst_sz) return 0;
        dst[di++] = *s++;
    }
    const char* dot = strchr(s, '.');
    size_t int_len = dot ? (size_t)(dot - s) : strlen(s);
    size_t gs = strlen(grp_sep);
    if (int_len == 0) return 0;
    if (gs == 0) {
        if (di + int_len + 1 >= dst_sz) return 0;
        memcpy(dst + di, s, int_len); di += int_len;
    } else {
        size_t lead = int_len % 3;
        if (lead == 0) lead = 3;
        size_t i = 0;
        while (i < int_len) {
            size_t chunk = (i == 0) ? lead : 3;
            if (i > 0) {
                if (di + gs + 1 >= dst_sz) return 0;
                memcpy(dst + di, grp_sep, gs); di += gs;
            }
            if (di + chunk + 1 >= dst_sz) return 0;
            memcpy(dst + di, s + i, chunk); di += chunk;
            i += chunk;
        }
    }
    if (dot) {
        size_t ds = strlen(dec_sep);
        size_t flen = strlen(dot + 1);
        if (di + ds + flen + 1 >= dst_sz) return 0;
        memcpy(dst + di, dec_sep, ds); di += ds;
        memcpy(dst + di, dot + 1, flen); di += flen;
    }
    dst[di] = '\0';
    return 1;
}

/* 模式展开：n→值，-→负号，¤→货币符号，%→百分号符号，其余字面量。 */
static char* rt_fmt_pattern(const char* tpl, const char* n, const char* sign,
                            const char* cur_sym, const char* pct_sym) {
    char buf[320];
    size_t oi = 0;
    for (const char* p = tpl; *p && oi + 64 < sizeof(buf); p++) {
        if (*p == 'n') {
            size_t l = strlen(n);
            if (oi + l + 1 >= sizeof(buf)) break;
            memcpy(buf + oi, n, l); oi += l;
        } else if (*p == '-') {
            size_t l = strlen(sign);
            if (oi + l + 1 >= sizeof(buf)) break;
            memcpy(buf + oi, sign, l); oi += l;
        } else if (*p == '\xC2' && p[1] == '\xA4') { /* ¤ */
            size_t l = strlen(cur_sym);
            if (oi + l + 1 >= sizeof(buf)) break;
            memcpy(buf + oi, cur_sym, l); oi += l;
            p++;
        } else if (*p == '%') {
            size_t l = strlen(pct_sym);
            if (oi + l + 1 >= sizeof(buf)) break;
            memcpy(buf + oi, pct_sym, l); oi += l;
        } else {
            buf[oi++] = *p;
        }
    }
    buf[oi] = '\0';
    return _strdup(buf);
}

static char* rt_fmt_apply_num(const char* grouped, int pattern, const char* neg_sign) {
    int neg = grouped[0] == '-';
    const char* n = neg ? grouped + 1 : grouped;
    /* 正数无需套用负数模式（N 无独立正数模式；C/P 才用 POS_PATTERNS）。 */
    if (!neg) return _strdup(n);
    if (pattern < 0 || pattern > 4) pattern = 1;
    return rt_fmt_pattern(NUM_NEG_PATTERNS[pattern], n, neg_sign, "", "");
}

static char* rt_fmt_apply_cur(const char* grouped, int pos_p, int neg_p,
                              const char* sym, const char* neg_sign) {
    int neg = grouped[0] == '-';
    const char* n = neg ? grouped + 1 : grouped;
    if (neg) {
        if (neg_p < 0 || neg_p > 15) neg_p = 0;
        return rt_fmt_pattern(CUR_NEG_PATTERNS[neg_p], n, neg_sign, sym, "");
    }
    if (pos_p < 0 || pos_p > 3) pos_p = 0;
    return rt_fmt_pattern(CUR_POS_PATTERNS[pos_p], n, "", sym, "");
}

static char* rt_fmt_apply_per(const char* grouped, int pos_p, int neg_p,
                              const char* sym, const char* neg_sign) {
    int neg = grouped[0] == '-';
    const char* n = neg ? grouped + 1 : grouped;
    if (neg) {
        if (neg_p < 0 || neg_p > 15) neg_p = 0;
        return rt_fmt_pattern(PER_NEG_PATTERNS[neg_p], n, neg_sign, "", sym);
    }
    if (pos_p < 0 || pos_p > 3) pos_p = 0;
    return rt_fmt_pattern(PER_POS_PATTERNS[pos_p], n, "", "", sym);
}

/* N/C/P 文化感知核心（value 已按 C# 规则：P 内部 *100）。 */
static char* rt_fmt_ncp_culture(double value, char kind, int precision, const RtNfi* nf) {
    char raw[160];
    char grouped[224];
    double v = value;
    const char* dec_sep;
    const char* grp_sep;
    if (kind == 'C') {
        dec_sep = nf->currency_decimal_sep;
        grp_sep = nf->currency_group_sep;
    } else if (kind == 'P') {
        v = value * 100.0;
        dec_sep = nf->percent_decimal_sep;
        grp_sep = nf->percent_group_sep;
    } else {
        dec_sep = nf->num_decimal_sep;
        grp_sep = nf->num_group_sep;
    }
    snprintf(raw, sizeof(raw), "%.*f", precision, v);
    if (!rt_fmt_numeric_group(raw, grp_sep, dec_sep, grouped, sizeof(grouped))) {
        rt_panic("numeric format buffer overflow");
        return NULL;
    }
    if (kind == 'N') return rt_fmt_apply_num(grouped, nf->number_neg_pattern, nf->neg_sign);
    if (kind == 'C') return rt_fmt_apply_cur(grouped, nf->currency_pos_pattern, nf->currency_neg_pattern,
                                             nf->currency_symbol, nf->neg_sign);
    return rt_fmt_apply_per(grouped, nf->percent_pos_pattern, nf->percent_neg_pattern,
                            nf->percent_symbol, nf->neg_sign);
}

/* F 格式：用文化小数分隔符替换 '.'。 */
static char* rt_fmt_f_culture(const char* s, const char* dec_sep) {
    const char* dot = strchr(s, '.');
    if (!dot || strcmp(dec_sep, ".") == 0) return _strdup(s);
    size_t head = (size_t)(dot - s);
    size_t ds = strlen(dec_sep);
    size_t tail = strlen(dot + 1);
    char* out = malloc(head + ds + tail + 1);
    if (!out) { rt_panic("out of memory"); return NULL; }
    memcpy(out, s, head);
    memcpy(out + head, dec_sep, ds);
    memcpy(out + head + ds, dot + 1, tail + 1);
    return out;
}

static char* rt_fmt_i64_culture(int64_t value, const char* format, const RtNfi* nf) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) return rt_fmt_custom_i64_fmt(value, format);
    char buf[160];
    switch (fs.spec) {
        case 'D':
            if (fs.precision < 0) snprintf(buf, sizeof(buf), "%lld", (long long)value);
            else if (value < 0) {
                unsigned long long mag = (value == INT64_MIN)
                    ? (unsigned long long)INT64_MAX + 1ULL
                    : (unsigned long long)(-(value + 1)) + 1ULL;
                snprintf(buf, sizeof(buf), "-%0*llu", fs.precision, mag);
            } else snprintf(buf, sizeof(buf), "%0*lld", fs.precision, (long long)value);
            return _strdup(buf);
        case 'X': case 'x': {
            unsigned long long u = (unsigned long long)value;
            const char* f = (fs.spec == 'X')
                ? ((fs.precision < 0) ? "%llX" : "%0*llX")
                : ((fs.precision < 0) ? "%llx" : "%0*llx");
            if (fs.precision < 0) snprintf(buf, sizeof(buf), f, u);
            else snprintf(buf, sizeof(buf), f, fs.precision, u);
            return _strdup(buf);
        }
        case 'F':
            snprintf(buf, sizeof(buf), "%.*f", fs.precision, (double)value);
            return rt_fmt_f_culture(buf, nf->num_decimal_sep);
        case 'G':
            if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", (double)value);
            else snprintf(buf, sizeof(buf), "%.*g", fs.precision, (double)value);
            return _strdup(buf);
        case 'N': case 'C': case 'P':
            return rt_fmt_ncp_culture((double)value, fs.spec, fs.precision, nf);
        default:
            return rt_fmt_exp((double)value, fs.spec, fs.precision);
    }
}

static char* rt_fmt_u64_culture(uint64_t value, const char* format, const RtNfi* nf) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) return rt_fmt_custom_u64_fmt(value, format);
    char buf[160];
    switch (fs.spec) {
        case 'D':
            if (fs.precision < 0) snprintf(buf, sizeof(buf), "%llu", (unsigned long long)value);
            else snprintf(buf, sizeof(buf), "%0*llu", fs.precision, (unsigned long long)value);
            return _strdup(buf);
        case 'X': case 'x': {
            const char* f = (fs.spec == 'X')
                ? ((fs.precision < 0) ? "%llX" : "%0*llX")
                : ((fs.precision < 0) ? "%llx" : "%0*llx");
            if (fs.precision < 0) snprintf(buf, sizeof(buf), f, (unsigned long long)value);
            else snprintf(buf, sizeof(buf), f, fs.precision, (unsigned long long)value);
            return _strdup(buf);
        }
        case 'F':
            snprintf(buf, sizeof(buf), "%.*f", fs.precision, (double)value);
            return rt_fmt_f_culture(buf, nf->num_decimal_sep);
        case 'G':
            if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", (double)value);
            else snprintf(buf, sizeof(buf), "%.*g", fs.precision, (double)value);
            return _strdup(buf);
        case 'N': case 'C': case 'P':
            return rt_fmt_ncp_culture((double)value, fs.spec, fs.precision, nf);
        default:
            return rt_fmt_exp((double)value, fs.spec, fs.precision);
    }
}

static char* rt_fmt_f64_culture(double value, const char* format, const RtNfi* nf) {
    RtFmtSpec fs;
    if (!rt_parse_fmt_spec(format, &fs)) return rt_fmt_custom_f64_fmt(value, format);
    char buf[160];
    switch (fs.spec) {
        case 'F':
            snprintf(buf, sizeof(buf), "%.*f", fs.precision, value);
            return rt_fmt_f_culture(buf, nf->num_decimal_sep);
        case 'G':
            if (fs.precision < 0) snprintf(buf, sizeof(buf), "%.15g", value);
            else snprintf(buf, sizeof(buf), "%.*g", fs.precision, value);
            return _strdup(buf);
        case 'N': case 'C': case 'P':
            return rt_fmt_ncp_culture(value, fs.spec, fs.precision, nf);
        default:
            return rt_fmt_exp(value, fs.spec, fs.precision);
    }
}

#define RT_FMT_P_INT32_X(value, format, out) { \
    RtFmtSpec fs; \
    if (rt_parse_fmt_spec(format, &fs) && (fs.spec == 'X' || fs.spec == 'x')) { \
        uint32_t u = (uint32_t)(value); \
        char b[32]; \
        if (fs.precision < 0) snprintf(b, sizeof(b), fs.spec == 'X' ? "%X" : "%x", u); \
        else snprintf(b, sizeof(b), fs.spec == 'X' ? "%0*X" : "%0*x", fs.precision, u); \
        return _strdup(b); \
    } \
}

char* rt_int_to_string_fmt_p(int32_t value, const char* format, void* provider) {
    RT_FMT_P_INT32_X(value, format, NULL);
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_i64_culture((int64_t)value, format, &nf);
}
char* rt_long_to_string_fmt_p(int64_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_i64_culture(value, format, &nf);
}
char* rt_short_to_string_fmt_p(int16_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_i64_culture((int64_t)value, format, &nf);
}
char* rt_sbyte_to_string_fmt_p(int8_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_i64_culture((int64_t)value, format, &nf);
}
char* rt_uint_to_string_fmt_p(uint32_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_u64_culture((uint64_t)value, format, &nf);
}
char* rt_ulong_to_string_fmt_p(uint64_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_u64_culture(value, format, &nf);
}
char* rt_ushort_to_string_fmt_p(uint16_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_u64_culture((uint64_t)value, format, &nf);
}
char* rt_byte_to_string_fmt_p(uint8_t value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_u64_culture((uint64_t)value, format, &nf);
}
char* rt_float_to_string_fmt_p(float value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_f64_culture((double)value, format, &nf);
}
char* rt_double_to_string_fmt_p(double value, const char* format, void* provider) {
    RtNfi nf; rt_nfi_resolve(provider, &nf);
    return rt_fmt_f64_culture(value, format, &nf);
}