// Text-processing ABI (RFC 021 §4.3 M4: std/Arc text facades).
//
// Split out of the runtime so each concern lives in its own translation unit:
// this file implements Base64 / Hex codecs backing the Arc.Text std facade.
// All public entry points return a freshly malloc'd NUL-terminated string
// (caller-managed via ARC). Input NULL is treated as the empty string. All
// functions are thread-safe (no mutable global state). Algorithms are
// self-contained C11 with no external dependency.

#include "rt_abi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- Base64 ------------------------------------------------------------- */

static const char b64_alphabet[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `data` (treated as raw bytes) to a freshly malloc'd base64 string.
/// `len` is the byte length. Returns a NUL-terminated malloc'd buffer.
char* rt_text_base64_encode(const char* data) {
    const uint8_t* in = (const uint8_t*)(data ? data : "");
    int32_t len = (int32_t)strlen((const char*)in);

    /* output size: ceil(len/3)*4 + 1 (NUL) */
    int32_t out_len = ((len + 2) / 3) * 4;
    char* out = (char*)malloc((size_t)out_len + 1);
    if (!out) return NULL;

    int32_t o = 0;
    int32_t i = 0;
    for (; i + 2 < len; i += 3) {
        uint32_t triple = ((uint32_t)in[i] << 16) |
                          ((uint32_t)in[i + 1] << 8) |
                          (uint32_t)in[i + 2];
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 6) & 0x3F];
        out[o++] = b64_alphabet[triple & 0x3F];
    }
    /* remainder */
    int32_t rem = len - i;
    if (rem == 1) {
        uint32_t triple = (uint32_t)in[i] << 16;
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = '=';
        out[o++] = '=';
    } else if (rem == 2) {
        uint32_t triple = ((uint32_t)in[i] << 16) | ((uint32_t)in[i + 1] << 8);
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 6) & 0x3F];
        out[o++] = '=';
    }
    out[o] = '\0';
    return out;
}

/// Decode a single base64 character to its 6-bit value, or -1 if invalid.
static int b64_decode_char(char c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
}

/// Decode a base64 string to freshly malloc'd raw bytes (as a NUL-terminated
/// string). Invalid characters are skipped. Returns malloc'd buffer.
char* rt_text_base64_decode(const char* data) {
    const char* in = data ? data : "";
    int32_t len = (int32_t)strlen(in);

    /* worst-case output size: ceil(len/4)*3 + 1 */
    int32_t cap = ((len + 3) / 4) * 3 + 1;
    char* out = (char*)malloc((size_t)cap);
    if (!out) return NULL;

    int32_t o = 0;
    uint32_t triple = 0;
    int32_t quad = 0; /* number of valid 6-bit values accumulated */
    for (int32_t i = 0; i < len; i++) {
        char c = in[i];
        if (c == '=' || c == '\0') break;
        int v = b64_decode_char(c);
        if (v < 0) continue; /* skip whitespace / invalid */
        triple = (triple << 6) | (uint32_t)v;
        quad++;
        if (quad == 4) {
            out[o++] = (char)((triple >> 16) & 0xFF);
            out[o++] = (char)((triple >> 8) & 0xFF);
            out[o++] = (char)(triple & 0xFF);
            triple = 0;
            quad = 0;
        }
    }
    /* handle trailing partial group */
    if (quad == 2) {
        triple <<= 12;
        out[o++] = (char)((triple >> 16) & 0xFF);
    } else if (quad == 3) {
        triple <<= 6;
        out[o++] = (char)((triple >> 16) & 0xFF);
        out[o++] = (char)((triple >> 8) & 0xFF);
    }
    out[o] = '\0';
    return out;
}

/// Encode a `byte[]` payload to a freshly malloc'd base64 string.
/// Uses `rt_array_length` for the byte count (binary-safe — embedded 0x00 kept,
/// unlike the `strlen`-based string facade). NULL/empty array → empty string.
char* rt_text_base64_bytes_encode(void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len < 0) len = 0;
    const uint8_t* in = (const uint8_t*)bytes;
    int32_t out_len = ((len + 2) / 3) * 4;
    char* out = (char*)malloc((size_t)out_len + 1);
    if (!out) return NULL;
    int32_t o = 0;
    int32_t i = 0;
    for (; i + 2 < len; i += 3) {
        uint32_t triple = ((uint32_t)in[i] << 16) |
                          ((uint32_t)in[i + 1] << 8) |
                          (uint32_t)in[i + 2];
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 6) & 0x3F];
        out[o++] = b64_alphabet[triple & 0x3F];
    }
    int32_t rem = len - i;
    if (rem == 1) {
        uint32_t triple = (uint32_t)in[i] << 16;
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = '=';
        out[o++] = '=';
    } else if (rem == 2) {
        uint32_t triple = ((uint32_t)in[i] << 16) | ((uint32_t)in[i + 1] << 8);
        out[o++] = b64_alphabet[(triple >> 18) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 12) & 0x3F];
        out[o++] = b64_alphabet[(triple >> 6) & 0x3F];
        out[o++] = '=';
    }
    out[o] = '\0';
    return out;
}

/// Decode a base64 string into a freshly allocated `byte[]` (elem_size = 1).
/// Invalid characters are skipped; padding or end-of-string stops decoding
/// (RFC 026 M1 §1.2 ⑥ convention — binary-safe output, embedded 0x00 present).
/// NULL/empty → length-0 array.
void* rt_text_base64_bytes_decode(const char* data) {
    const char* in = data ? data : "";
    int32_t len = (int32_t)strlen(in);

    /* first pass: count decoded bytes to size the array */
    int32_t quad = 0;
    int32_t o = 0;
    uint32_t triple = 0;
    for (int32_t i = 0; i < len; i++) {
        char c = in[i];
        if (c == '=' || c == 0) break;
        int v = b64_decode_char(c);
        if (v < 0) continue; /* skip whitespace / invalid */
        triple = (triple << 6) | (uint32_t)v;
        quad++;
        if (quad == 4) { o += 3; triple = 0; quad = 0; }
    }
    if (quad == 2) o += 1;
    else if (quad == 3) o += 2;

    void* arr = rt_array_create(o, 1);
    if (!arr) return NULL;
    uint8_t* out = (uint8_t*)arr;

    /* second pass: fill */
    quad = 0;
    triple = 0;
    o = 0;
    for (int32_t i = 0; i < len; i++) {
        char c = in[i];
        if (c == '=' || c == 0) break;
        int v = b64_decode_char(c);
        if (v < 0) continue;
        triple = (triple << 6) | (uint32_t)v;
        quad++;
        if (quad == 4) {
            out[o++] = (uint8_t)((triple >> 16) & 0xFF);
            out[o++] = (uint8_t)((triple >> 8) & 0xFF);
            out[o++] = (uint8_t)(triple & 0xFF);
            triple = 0; quad = 0;
        }
    }
    if (quad == 2) {
        triple <<= 12;
        out[o++] = (uint8_t)((triple >> 16) & 0xFF);
    } else if (quad == 3) {
        triple <<= 6;
        out[o++] = (uint8_t)((triple >> 16) & 0xFF);
        out[o++] = (uint8_t)((triple >> 8) & 0xFF);
    }
    return arr;
}

/* ---- Hex ---------------------------------------------------------------- */

static const char hex_lower[] = "0123456789abcdef";

/// Encode `data` bytes to a freshly malloc'd lowercase-hex string.
char* rt_text_hex_encode(const char* data) {
    const uint8_t* in = (const uint8_t*)(data ? data : "");
    int32_t len = (int32_t)strlen((const char*)in);

    char* out = (char*)malloc((size_t)len * 2 + 1);
    if (!out) return NULL;

    for (int32_t i = 0; i < len; i++) {
        out[i * 2] = hex_lower[(in[i] >> 4) & 0x0F];
        out[i * 2 + 1] = hex_lower[in[i] & 0x0F];
    }
    out[len * 2] = '\0';
    return out;
}

/// Decode a hex string to freshly malloc'd raw bytes (as a NUL-terminated
/// string). Odd-length input drops the trailing nibble. Invalid characters
/// are treated as 0. Returns malloc'd buffer.
char* rt_text_hex_decode(const char* data) {
    const char* in = data ? data : "";
    int32_t len = (int32_t)strlen(in);

    int32_t out_len = len / 2;
    char* out = (char*)malloc((size_t)out_len + 1);
    if (!out) return NULL;

    for (int32_t i = 0; i < out_len; i++) {
        char hi = in[i * 2];
        char lo = in[i * 2 + 1];
        int h = (hi >= '0' && hi <= '9') ? hi - '0'
              : (hi >= 'a' && hi <= 'f') ? hi - 'a' + 10
              : (hi >= 'A' && hi <= 'F') ? hi - 'A' + 10
              : 0;
        int l = (lo >= '0' && lo <= '9') ? lo - '0'
              : (lo >= 'a' && lo <= 'f') ? lo - 'a' + 10
              : (lo >= 'A' && lo <= 'F') ? lo - 'A' + 10
              : 0;
        out[i] = (char)((h << 4) | l);
    }
    out[out_len] = '\0';
    return out;
}

/// Encode a `byte[]` payload to a freshly malloc'd lowercase-hex string.
/// Uses `rt_array_length` (RFC 026 M1 §1.2 ⑥ — `Arc.Text.Hex.ToHexString`).
/// NULL/empty array → empty string.
char* rt_text_hex_bytes_encode(void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len < 0) len = 0;
    char* out = (char*)malloc((size_t)len * 2 + 1);
    if (!out) return NULL;
    const uint8_t* in = (const uint8_t*)bytes;
    for (int32_t i = 0; i < len; i++) {
        out[i * 2] = hex_lower[(in[i] >> 4) & 0x0F];
        out[i * 2 + 1] = hex_lower[in[i] & 0x0F];
    }
    out[len * 2] = '\0';
    return out;
}

/// Decode a hex string into a freshly allocated `byte[]` (elem_size = 1).
/// Odd-length input drops the trailing nibble; invalid chars decode as 0.
/// NULL/empty → length-0 array (RFC 026 M1 §1.2 ⑥ — `Arc.Text.Hex.FromHexString`).
void* rt_text_hex_bytes_decode(const char* data) {
    const char* in = data ? data : "";
    int32_t len = (int32_t)strlen(in);
    int32_t out_len = len / 2;
    void* arr = rt_array_create(out_len, 1);
    if (!arr) return NULL;
    uint8_t* out = (uint8_t*)arr;
    for (int32_t i = 0; i < out_len; i++) {
        char hi = in[i * 2];
        char lo = in[i * 2 + 1];
        int h = (hi >= '0' && hi <= '9') ? hi - '0'
              : (hi >= 'a' && hi <= 'f') ? hi - 'a' + 10
              : (hi >= 'A' && hi <= 'F') ? hi - 'A' + 10
              : 0;
        int l = (lo >= '0' && lo <= '9') ? lo - '0'
              : (lo >= 'a' && lo <= 'f') ? lo - 'a' + 10
              : (lo >= 'A' && lo <= 'F') ? lo - 'A' + 10
              : 0;
        out[i] = (uint8_t)((h << 4) | l);
    }
    return arr;
}

/* ---- UTF-8 Encoding (Encoding.GetBytes / GetString) --------------------- */

/// Copy UTF-8 string bytes into a freshly allocated `byte[]` (`rt_array`,
/// elem_size = 1). NULL/`""` → length-0 array. Returned pointer is the
/// array payload (header lives 8 bytes before).
void* rt_text_utf8_get_bytes(const char* s) {
    if (!s) s = "";
    int32_t len = (int32_t)strlen(s);
    void* arr = rt_array_create(len, 1);
    if (!arr) return NULL;
    if (len > 0) {
        memcpy(arr, s, (size_t)len);
    }
    return arr;
}

/// Decode a `byte[]` payload into a freshly malloc'd NUL-terminated string.
/// Uses `rt_array_length` (not strlen) so trailing content after an interior
/// 0x00 is copied; Arc string ops that use strlen still see the first NUL.
/// NULL array → empty string.
char* rt_text_utf8_get_string(void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len < 0) len = 0;
    char* out = (char*)malloc((size_t)len + 1);
    if (!out) return NULL;
    if (len > 0 && bytes) {
        memcpy(out, bytes, (size_t)len);
    }
    out[len] = '\0';
    return out;
}

/// UTF-8 码元数（与 `string.Length` / GetBytes.Length 对齐；null → 0）。
int32_t rt_text_utf8_get_byte_count(const char* s) {
    if (!s) return 0;
    return (int32_t)strlen(s);
}

/* ---- URL percent-encoding (Arc.Text.Url / WebUtility 对齐) -------------- */

/// 判断字节是否需要百分号编码（保留字符：A-Z a-z 0-9 - _ . ~）。
static int url_is_unreserved(unsigned char c) {
    if (c >= 'A' && c <= 'Z') return 1;
    if (c >= 'a' && c <= 'z') return 1;
    if (c >= '0' && c <= '9') return 1;
    switch (c) {
        case '-': case '_': case '.': case '~': return 1;
        default: return 0;
    }
}

/// 百分号编码：Unreserved 原样；' ' → '+'; 其余字节 → "%HH"（大写十六进制）。
/// 返回 malloc'd NUL 终止缓冲区；NULL 输入视为空串。
char* rt_text_url_encode(const char* value) {
    static const char url_hex[] = "0123456789ABCDEF";
    const uint8_t* in = (const uint8_t*)(value ? value : "");
    int32_t len = (int32_t)strlen((const char*)in);

    /* 最坏情形：每个字节输出 3 字符（%HH）+ NUL。 */
    char* out = (char*)malloc((size_t)len * 3 + 1);
    if (!out) return NULL;

    int32_t o = 0;
    for (int32_t i = 0; i < len; i++) {
        unsigned char c = in[i];
        if (url_is_unreserved(c)) {
            out[o++] = (char)c;
        } else if (c == ' ') {
            out[o++] = '+';
        } else {
            out[o++] = '%';
            out[o++] = url_hex[(c >> 4) & 0x0F];
            out[o++] = url_hex[c & 0x0F];
        }
    }
    out[o] = '\0';
    return out;
}

/// 还原百分号编码：'+' → ' '; "%HH" → 字节（大小写十六进制均可）；孤立 '%' 原样保留。
/// 返回 malloc'd NUL 终止缓冲区；NULL 输入视为空串。
char* rt_text_url_decode(const char* value) {
    const char* in = value ? value : "";
    int32_t len = (int32_t)strlen(in);

    /* 解码只会缩小（%HH 3→1），allocate len+1 足矣。 */
    char* out = (char*)malloc((size_t)len + 1);
    if (!out) return NULL;

    int32_t o = 0;
    for (int32_t i = 0; i < len; i++) {
        char c = in[i];
        if (c == '+') {
            out[o++] = ' ';
        } else if (c == '%' && i + 2 < len) {
            char hi = in[i + 1];
            char lo = in[i + 2];
            int h = (hi >= '0' && hi <= '9') ? hi - '0'
                  : (hi >= 'a' && hi <= 'f') ? hi - 'a' + 10
                  : (hi >= 'A' && hi <= 'F') ? hi - 'A' + 10
                  : -1;
            int l = (lo >= '0' && lo <= '9') ? lo - '0'
                  : (lo >= 'a' && lo <= 'f') ? lo - 'a' + 10
                  : (lo >= 'A' && lo <= 'F') ? lo - 'A' + 10
                  : -1;
            if (h >= 0 && l >= 0) {
                out[o++] = (char)((h << 4) | l);
                i += 2;
            } else {
                out[o++] = c; /* 格式错误的 %HH 原样保留 */
            }
        } else {
            out[o++] = c;
        }
    }
    out[o] = '\0';
    return out;
}

/* ---- StringBuilder ------------------------------------------------------- */
//
// Mutable growable char buffer backing the Arc.Text.StringBuilder facade.
// Layout: { char* data; size_t len; size_t cap; }. The handle is stored at
// offset 16 of the Arc object (same slot pattern as List/Dictionary `_handle`).
// All append entry points return the handle unchanged so call sites can chain.

typedef struct {
    char*  data;
    size_t len;
    size_t cap;
} rt_sb_t;

/* 热路径：append_char 等单字符追加走内联检查（跨 TU 调用的 ABI 函数本身
 * 无法被 clang 跨编译单元内联，但内部检查内联可省一层调用）。 */
static inline void rt_sb_ensure(rt_sb_t* sb, size_t additional) {
    size_t needed = sb->len + additional + 1;
    if (needed <= sb->cap) return;
    while (sb->cap < needed) sb->cap *= 2;
    char* new_data = (char*)realloc(sb->data, sb->cap);
    if (new_data) sb->data = new_data;
}

/* 懒 NUL 终止：append_char 不在每次追加后写 NUL（省一次 store），
 * 需要 C 串语义的入口（to_string / replace / insert 等）自行落 NUL。 */
static inline void rt_sb_nt(rt_sb_t* sb) {
    sb->data[sb->len] = '\0';
}

/// Create an empty StringBuilder. Returns a malloc'd handle, or NULL on OOM.
void* rt_text_sb_new(void) {
    rt_sb_t* sb = (rt_sb_t*)malloc(sizeof(rt_sb_t));
    if (!sb) return NULL;
    sb->cap = 16;
    sb->len = 0;
    sb->data = (char*)malloc(sb->cap);
    if (!sb->data) { free(sb); return NULL; }
    sb->data[0] = '\0';
    return sb;
}

/// Create a StringBuilder initialized with `initial`. NULL is treated as empty.
/// Returns NULL on OOM.
void* rt_text_sb_new_with_str(const char* initial) {
    rt_sb_t* sb = (rt_sb_t*)malloc(sizeof(rt_sb_t));
    if (!sb) return NULL;
    size_t slen = initial ? strlen(initial) : 0;
    sb->cap = slen + 1;
    if (sb->cap < 16) sb->cap = 16;
    sb->len = slen;
    sb->data = (char*)malloc(sb->cap);
    if (!sb->data) { free(sb); return NULL; }
    if (slen > 0) memcpy(sb->data, initial, slen);
    sb->data[slen] = '\0';
    return sb;
}

/// Create a StringBuilder with at least `capacity` bytes pre-allocated.
/// Returns NULL on OOM.
void* rt_text_sb_new_with_capacity(int32_t capacity) {
    rt_sb_t* sb = (rt_sb_t*)malloc(sizeof(rt_sb_t));
    if (!sb) return NULL;
    size_t cap = (size_t)(capacity > 0 ? capacity : 0) + 1;
    if (cap < 16) cap = 16;
    sb->cap = cap;
    sb->len = 0;
    sb->data = (char*)malloc(sb->cap);
    if (!sb->data) { free(sb); return NULL; }
    sb->data[0] = '\0';
    return sb;
}

/// Append `s` to the buffer. NULL is treated as the empty string. Returns
/// `handle` unchanged to support fluent chaining (`sb.Append(a).Append(b)`).
void* rt_text_sb_append(void* handle, const char* s) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (!s) return sb;
    size_t slen = strlen(s);
    rt_sb_ensure(sb, slen);
    memcpy(sb->data + sb->len, s, slen);
    sb->len += slen;
    sb->data[sb->len] = '\0';
    return sb;
}

/// Append `s` followed by a newline. NULL `s` appends only the newline.
void* rt_text_sb_append_line(void* handle, const char* s) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (s) rt_text_sb_append(sb, s);
    rt_text_sb_append(sb, "\n");
    return sb;
}

/// Copy the accumulated content to a fresh NUL-terminated malloc'd string.
char* rt_text_sb_to_string(void* handle) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    size_t len = sb->len;
    char* result = (char*)malloc(len + 1);
    if (!result) return NULL;
    if (len > 0) memcpy(result, sb->data, len);
    result[len] = '\0';
    return result;
}

/// Current length (excluding NUL terminator).
int32_t rt_text_sb_length(void* handle) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    return (int32_t)sb->len;
}

/// Clear the buffer (reset to empty without freeing capacity).
void rt_text_sb_clear(void* handle) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    sb->len = 0;
    sb->data[0] = '\0';
}

/// Append an int32_t value (decimal representation).
void* rt_text_sb_append_int(void* handle, int32_t value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    char buf[32];
    int n = snprintf(buf, sizeof(buf), "%d", value);
    rt_sb_ensure(sb, (size_t)n);
    memcpy(sb->data + sb->len, buf, (size_t)n);
    sb->len += (size_t)n;
    sb->data[sb->len] = '\0';
    return sb;
}

/// Append a bool value ("true" / "false").
void* rt_text_sb_append_bool(void* handle, int8_t value) {
    return rt_text_sb_append(handle, value ? "true" : "false");
}

/// Append a single char.
void* rt_text_sb_append_char(void* handle, int32_t value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    rt_sb_ensure(sb, 1);
    sb->data[sb->len++] = (char)(value & 0xFF);
    return sb;
}

/// Append an int64_t value (decimal representation).
void* rt_text_sb_append_long(void* handle, int64_t value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    char buf[32];
    int n = snprintf(buf, sizeof(buf), "%lld", (long long)value);
    rt_sb_ensure(sb, (size_t)n);
    memcpy(sb->data + sb->len, buf, (size_t)n);
    sb->len += (size_t)n;
    sb->data[sb->len] = '\0';
    return sb;
}

/// Append a float value (shortest decimal representation).
void* rt_text_sb_append_float(void* handle, float value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    char buf[64];
    int n = snprintf(buf, sizeof(buf), "%g", (double)value);
    rt_sb_ensure(sb, (size_t)n);
    memcpy(sb->data + sb->len, buf, (size_t)n);
    sb->len += (size_t)n;
    sb->data[sb->len] = '\0';
    return sb;
}

/// Append a double value (shortest decimal representation).
void* rt_text_sb_append_double(void* handle, double value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    char buf[64];
    int n = snprintf(buf, sizeof(buf), "%g", value);
    rt_sb_ensure(sb, (size_t)n);
    memcpy(sb->data + sb->len, buf, (size_t)n);
    sb->len += (size_t)n;
    sb->data[sb->len] = '\0';
    return sb;
}

/// Insert `s` at `index`. Clamps invalid indices to no-op.
void* rt_text_sb_insert(void* handle, int32_t index, const char* s) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (!s || index < 0 || index > (int32_t)sb->len) return sb;
    size_t slen = strlen(s);
    rt_sb_ensure(sb, slen);
    memmove(sb->data + index + slen, sb->data + index, sb->len - (size_t)index + 1);
    memcpy(sb->data + index, s, slen);
    sb->len += slen;
    rt_sb_nt(sb);
    return sb;
}

/// Remove `length` characters starting at `startIndex`. Clamps invalid range to no-op.
void* rt_text_sb_remove(void* handle, int32_t start_index, int32_t length) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (start_index < 0 || length < 0 || (size_t)(start_index + length) > sb->len) return sb;
    memmove(sb->data + start_index,
            sb->data + start_index + length,
            sb->len - (size_t)(start_index + length) + 1);
    sb->len -= (size_t)length;
    rt_sb_nt(sb);
    return sb;
}

/// Replace all occurrences of `old_val` with `new_val`.
void* rt_text_sb_replace(void* handle, const char* old_val, const char* new_val) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (!old_val || !new_val || !*old_val) return sb;
    rt_sb_nt(sb);  /* strstr 依赖 C 串终止（append_char 为懒终止） */

    size_t old_len = strlen(old_val);
    size_t new_len = strlen(new_val);

    // Count occurrences to compute final size
    size_t count = 0;
    const char* scan = sb->data;
    while ((scan = strstr(scan, old_val)) != NULL) {
        count++;
        scan += old_len;
    }
    if (count == 0) return sb;

    size_t new_total = sb->len + count * (new_len - old_len);
    char* temp = (char*)malloc(new_total + 1);
    if (!temp) return sb;

    const char* src = sb->data;
    char* dst = temp;
    while (*src) {
        const char* found = strstr(src, old_val);
        if (found == src) {
            memcpy(dst, new_val, new_len);
            dst += new_len;
            src += old_len;
        } else {
            *dst++ = *src++;
        }
    }
    *dst = '\0';

    free(sb->data);
    sb->data = temp;
    sb->len = new_total;
    sb->cap = new_total + 1;
    return sb;
}

/// Ensure at least `capacity` bytes of buffer space (excluding NUL).
void rt_text_sb_ensure_capacity(void* handle, int32_t capacity) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (capacity <= 0) return;
    size_t needed = (size_t)capacity + 1;
    if (needed <= sb->cap) return;
    while (sb->cap < needed) sb->cap *= 2;
    char* new_data = (char*)realloc(sb->data, sb->cap);
    if (new_data) sb->data = new_data;
}

/// Current capacity (excluding NUL terminator).
int32_t rt_text_sb_get_capacity(void* handle) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    return (int32_t)(sb->cap - 1);
}

/// Copy a substring range to a fresh NUL-terminated string.
char* rt_text_sb_to_string_range(void* handle, int32_t start_index, int32_t length) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (start_index < 0 || length < 0 || (size_t)(start_index + length) > sb->len) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* result = (char*)malloc((size_t)length + 1);
    if (!result) return NULL;
    memcpy(result, sb->data + start_index, (size_t)length);
    result[length] = '\0';
    return result;
}

/// Get the character at `index`. Out-of-range returns `'\0'`.
int32_t rt_text_sb_get_char(void* handle, int32_t index) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (index < 0 || (size_t)index >= sb->len) return 0;
    return (int32_t)(unsigned char)sb->data[index];
}

/// Set the character at `index`. Out-of-range is a no-op.
void rt_text_sb_set_char(void* handle, int32_t index, int32_t value) {
    rt_sb_t* sb = (rt_sb_t*)handle;
    if (index < 0 || (size_t)index >= sb->len) return;
    sb->data[index] = (char)(value & 0xFF);
}

/* ---- Encoding 变体：UTF-16LE / Latin-1 ──────────────────────────────── */
// Arc `string` 为 UTF-8 NUL 终止字节序列。UTF-16LE / Latin-1 均以
// `byte[]`（rt_array, elem_size=1）往返，内部嵌入 0x00 由 rt_array_length
// 计数而非 strlen 截断，与既有 rt_text_utf8_get_string 同一模型。
// 对齐 C# System.Text.Encoding：UTF-16 GetBytes 不写 BOM；Latin-1 将码点
// >0xFF 映射为 '?'（与 .NET Latin1 一致）。

/// UTF-8 流中取一个码点，推进 `*pos`。返回 0xFFFFFFFF 表示串尾。
static uint32_t rt_u8_next(const char* s, int32_t len, int32_t* pos) {
    if (*pos >= len) return 0xFFFFFFFFu;
    uint8_t c0 = (uint8_t)s[*pos];
    if (c0 < 0x80) { (*pos) += 1; return (uint32_t)c0; }
    int32_t need;
    uint32_t cp;
    if ((c0 & 0xE0) == 0xC0)       { need = 1; cp = c0 & 0x1Fu; }
    else if ((c0 & 0xF0) == 0xE0)  { need = 2; cp = c0 & 0x0Fu; }
    else if ((c0 & 0xF8) == 0xF0)  { need = 3; cp = c0 & 0x07u; }
    else { (*pos) += 1; return (uint32_t)c0; } /* 非法首字节：按原字节透传 */
    if (*pos + need > len) { (*pos) += 1; return (uint32_t)c0; }
    for (int32_t k = 1; k <= need; k++) {
        uint8_t ck = (uint8_t)s[*pos + k];
        if ((ck & 0xC0) != 0x80) { (*pos) += 1; return (uint32_t)c0; }
        cp = (cp << 6) | (uint32_t)(ck & 0x3Fu);
    }
    *pos += need + 1;
    return cp;
}

/// 将码点编码为 UTF-8 追加到 `out`，推进 `*o`。
static void rt_u8_append(char* out, int32_t* o, uint32_t cp) {
    if (cp < 0x80) {
        out[(*o)++] = (char)cp;
    } else if (cp < 0x800) {
        out[(*o)++] = (char)(0xC0 | (cp >> 6));
        out[(*o)++] = (char)(0x80 | (cp & 0x3F));
    } else if (cp < 0x10000) {
        out[(*o)++] = (char)(0xE0 | (cp >> 12));
        out[(*o)++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[(*o)++] = (char)(0x80 | (cp & 0x3F));
    } else {
        out[(*o)++] = (char)(0xF0 | (cp >> 18));
        out[(*o)++] = (char)(0x80 | ((cp >> 12) & 0x3F));
        out[(*o)++] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[(*o)++] = (char)(0x80 | (cp & 0x3F));
    }
}

/// string（UTF-8）→ UTF-16LE 字节数组（无 BOM）。
void* rt_text_utf16_get_bytes(const char* s) {
    if (!s) s = "";
    int32_t len = (int32_t)strlen(s);
    int32_t units = 0, i = 0;
    while (i < len) {
        uint32_t cp = rt_u8_next(s, len, &i);
        if (cp == 0xFFFFFFFFu) break;
        units += (cp > 0xFFFF && cp <= 0x10FFFF) ? 2 : 1;
    }
    void* arr = rt_array_create(units * 2, 1);
    if (!arr) return NULL;
    uint8_t* out = (uint8_t*)arr;
    int32_t o = 0;
    i = 0;
    while (i < len) {
        uint32_t cp = rt_u8_next(s, len, &i);
        if (cp == 0xFFFFFFFFu) break;
        if (cp <= 0xFFFF || cp > 0x10FFFF) {
            uint16_t u = (uint16_t)cp;
            out[o++] = (uint8_t)(u & 0xFF);
            out[o++] = (uint8_t)((u >> 8) & 0xFF);
        } else {
            uint32_t v = cp - 0x10000u;
            uint16_t hi = (uint16_t)(0xD800u + ((v >> 10) & 0x3FFu));
            uint16_t lo = (uint16_t)(0xDC00u + (v & 0x3FFu));
            out[o++] = (uint8_t)(hi & 0xFF);
            out[o++] = (uint8_t)((hi >> 8) & 0xFF);
            out[o++] = (uint8_t)(lo & 0xFF);
            out[o++] = (uint8_t)((lo >> 8) & 0xFF);
        }
    }
    return arr;
}

/// UTF-16LE 字节数组 → string（UTF-8）。字节数须为偶数，越界尾字节丢弃。
char* rt_text_utf16_get_string(void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len < 0) len = 0;
    uint8_t* in = (uint8_t*)bytes;
    char* out = (char*)malloc((size_t)len * 2 + 5);
    if (!out) return NULL;
    int32_t o = 0, i = 0;
    while (i + 1 < len) {
        uint16_t u = (uint16_t)(in[i] | ((uint16_t)in[i + 1] << 8));
        i += 2;
        uint32_t cp;
        if (u >= 0xD800 && u <= 0xDBFF && i + 1 < len) {
            uint16_t lo = (uint16_t)(in[i] | ((uint16_t)in[i + 1] << 8));
            if (lo >= 0xDC00 && lo <= 0xDFFF) {
                i += 2;
                cp = 0x10000u + (((uint32_t)(u - 0xD800)) << 10) + (lo - 0xDC00);
            } else {
                cp = 0xFFFDu;
            }
        } else {
            cp = u;
        }
        rt_u8_append(out, &o, cp);
    }
    out[o] = '\0';
    return out;
}

/// string（UTF-8）→ Latin-1 字节数组（码点 >0xFF → '?'）。
void* rt_text_latin1_get_bytes(const char* s) {
    if (!s) s = "";
    int32_t len = (int32_t)strlen(s);
    int32_t n = 0, i = 0;
    while (i < len) {
        uint32_t cp = rt_u8_next(s, len, &i);
        if (cp == 0xFFFFFFFFu) break;
        n++;
    }
    void* arr = rt_array_create(n, 1);
    if (!arr) return NULL;
    uint8_t* out = (uint8_t*)arr;
    int32_t o = 0;
    i = 0;
    while (i < len) {
        uint32_t cp = rt_u8_next(s, len, &i);
        if (cp == 0xFFFFFFFFu) break;
        out[o++] = cp > 0xFF ? (uint8_t)'?' : (uint8_t)cp;
    }
    return arr;
}

/// Latin-1 字节数组 → string（UTF-8）。
char* rt_text_latin1_get_string(void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len < 0) len = 0;
    uint8_t* in = (uint8_t*)bytes;
    char* out = (char*)malloc((size_t)len * 2 + 1);
    if (!out) return NULL;
    int32_t o = 0;
    for (int32_t i = 0; i < len; i++) rt_u8_append(out, &o, in[i]);
    out[o] = '\0';
    return out;
}
