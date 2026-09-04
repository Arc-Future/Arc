// BitConverter host-endian encode/decode (Arc.BitConverter Stable 最小面).
// Layout: byte[] via rt_array_create(n, 1). Host endian matches C# BitConverter.

#include "rt_abi.h"
#include <stdint.h>
#include <string.h>

int32_t rt_bitconverter_is_little_endian(void) {
    uint16_t x = 1;
    return (*(uint8_t*)&x == 1) ? 1 : 0;
}

void* rt_bitconverter_get_bytes_i32(int32_t value) {
    void* arr = rt_array_create(4, 1);
    if (!arr) return NULL;
    memcpy(arr, &value, 4);
    return arr;
}

void* rt_bitconverter_get_bytes_i64(int64_t value) {
    void* arr = rt_array_create(8, 1);
    if (!arr) return NULL;
    memcpy(arr, &value, 8);
    return arr;
}

int32_t rt_bitconverter_to_i32(void* bytes, int32_t start_index) {
    if (!bytes) {
        rt_panic("BitConverter.ToInt32: null array");
    }
    int32_t len = rt_array_length(bytes);
    if (start_index < 0 || start_index > len - 4) {
        rt_panic("BitConverter.ToInt32: startIndex out of range");
    }
    int32_t v = 0;
    memcpy(&v, (char*)bytes + start_index, 4);
    return v;
}

int64_t rt_bitconverter_to_i64(void* bytes, int32_t start_index) {
    if (!bytes) {
        rt_panic("BitConverter.ToInt64: null array");
    }
    int32_t len = rt_array_length(bytes);
    if (start_index < 0 || start_index > len - 8) {
        rt_panic("BitConverter.ToInt64: startIndex out of range");
    }
    int64_t v = 0;
    memcpy(&v, (char*)bytes + start_index, 8);
    return v;
}
