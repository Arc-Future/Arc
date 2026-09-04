// iree_shim.cpp — Arc.AI.Iree C ABI shim over IREE Runtime.
//
// Wraps IREE's high-level runtime C API (`iree/runtime/api.h`) behind the
// `extern "C"` surface declared in iree_shim.h. Mirrors onnx_shim.cpp's
// conventions: opaque owning handles, 0=success/nonzero=failure, last-error
// string protocol, and no C++ exceptions crossing the ABI boundary.
//
// Milestone mapping (RFC 053 S3):
//   - M-I0: this file is the skeleton. The degradation chain does NOT load a
//     real lib — it exercises the `.ani` `load="auto"` latch (module
//     unavailable -> IreeNative.IsAvailable == false). Real bodies land in
//     M-I1 (instance/module/invoke) and M-I2 (buffer_view typed I/O).
//
// Requires: IREE runtime headers (target/iree-native/include) + import lib,
// vendored by scripts/fetch-iree-native.ps1. Built by
// scripts/build-iree-shim.ps1.
#include "iree_shim.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#define IREE_SHIM_EXPORT __declspec(dllexport)
#else
#define IREE_SHIM_EXPORT __attribute__((visibility("default")))
#endif

namespace {

// Last-error storage (single-threaded per shim; onnx_shim.cpp same model).
char g_last_error[1024] = {0};

void set_error(const char* msg) {
    snprintf(g_last_error, sizeof(g_last_error), "%s", msg ? msg : "unknown error");
}

const char* status_to_string(const char* fallback) {
    // M-I0: bodies not yet wired to real IREE; M-I1 replaces with
    // iree_status_to_string / iree_api_version fields.
    return fallback;
}

}  // namespace

extern "C" {

int32_t iree_last_error(uint8_t* buf, int32_t buf_len) {
    if (!buf || buf_len <= 0) return 0;
    size_t n = strlen(g_last_error);
    if (n >= (size_t)buf_len) n = (size_t)buf_len - 1;
    memcpy(buf, g_last_error, n);
    buf[n] = 0;
    return (int32_t)n;
}

void iree_clear_error(void) { g_last_error[0] = 0; }

// ── runtime instance ─────────────────────────────────────────────────
// M-I1: iree_runtime_instance_create(...) wiring lands here.
int32_t iree_create_runtime(int32_t log_level, const char* name, IreeInstance* out) {
    (void)log_level; (void)name;
    if (!out) return 1;
    set_error("IREE runtime instance not yet implemented (M-I1)");
    *out = nullptr;
    return 1;
}

void iree_release_runtime(IreeInstance instance) {
    // M-I1: iree_runtime_instance_release(...).
    (void)instance;
}

// ── module ───────────────────────────────────────────────────────────
int32_t iree_load_module(IreeInstance instance, const char* module_path, IreeModule* out) {
    (void)instance; (void)module_path;
    if (!out) return 1;
    set_error("IREE module load not yet implemented (M-I1)");
    *out = nullptr;
    return 1;
}

void iree_release_module(IreeModule module) { (void)module; }

int32_t iree_module_function_count(IreeModule module, int32_t* out_count) {
    (void)module;
    if (!out_count) return 1;
    set_error("IREE module metadata not yet implemented (M-I1)");
    return 1;
}

// ── device / driver probe (M-I3) ─────────────────────────────────────
int32_t iree_device_driver_available(const char* driver) {
    (void)driver;
    set_error("IREE driver probe not yet implemented (M-I3)");
    return 1;
}

// ── buffer view (M-I2) ───────────────────────────────────────────────
#define IREE_UNIMPLEMENTED_BUFFER_VIEW(name, T)                                \
    int32_t name(int64_t* shape, int32_t shape_count, T* data, int32_t data_len, \
                 IreeBufferView* out) {                                         \
        (void)shape; (void)shape_count; (void)data; (void)data_len;              \
        if (!out) return 1;                                                     \
        set_error("IREE buffer view creation not yet implemented (M-I2)");      \
        *out = nullptr;                                                         \
        return 1;                                                               \
    }

IREE_UNIMPLEMENTED_BUFFER_VIEW(iree_create_buffer_float, float)
IREE_UNIMPLEMENTED_BUFFER_VIEW(iree_create_buffer_double, double)
IREE_UNIMPLEMENTED_BUFFER_VIEW(iree_create_buffer_i32, int32_t)
IREE_UNIMPLEMENTED_BUFFER_VIEW(iree_create_buffer_i64, int64_t)
IREE_UNIMPLEMENTED_BUFFER_VIEW(iree_create_buffer_byte, uint8_t)

void iree_release_buffer_view(IreeBufferView v) { (void)v; }

#define IREE_UNIMPLEMENTED_READ(name, T)                                        \
    int32_t name(IreeBufferView v, T* buf, int32_t buf_len, int32_t* out_len) { \
        (void)v; (void)buf; (void)buf_len; (void)out_len;                        \
        set_error("IREE buffer view read not yet implemented (M-I2)");          \
        return 1;                                                               \
    }

IREE_UNIMPLEMENTED_READ(iree_buffer_view_read_float, float)
IREE_UNIMPLEMENTED_READ(iree_buffer_view_read_double, double)
IREE_UNIMPLEMENTED_READ(iree_buffer_view_read_i32, int32_t)
IREE_UNIMPLEMENTED_READ(iree_buffer_view_read_i64, int64_t)
IREE_UNIMPLEMENTED_READ(iree_buffer_view_read_byte, uint8_t)

int32_t iree_buffer_view_get_shape(IreeBufferView v, int64_t* dims, int32_t dimCap, int32_t* dim_count) {
    (void)v; (void)dims; (void)dimCap; (void)dim_count;
    set_error("IREE buffer view shape not yet implemented (M-I2)");
    return 1;
}

int32_t iree_buffer_view_get_elem_type(IreeBufferView v, int32_t* out_type) {
    (void)v; (void)out_type;
    set_error("IREE buffer view elem type not yet implemented (M-I2)");
    return 1;
}

int32_t iree_buffer_view_get_total(IreeBufferView v, int64_t* out_total) {
    (void)v; (void)out_total;
    set_error("IREE buffer view total not yet implemented (M-I2)");
    return 1;
}

// ── invoke (M-I1) ────────────────────────────────────────────────────
int32_t iree_invoke(IreeModule module, const char* function_name,
                    int64_t* inputs, int32_t n_inputs,
                    int64_t* outputs, int32_t n_outputs, int32_t* out_count) {
    (void)module; (void)function_name; (void)inputs; (void)n_inputs;
    (void)outputs; (void)n_outputs; (void)out_count;
    set_error("IREE invoke not yet implemented (M-I1)");
    return 1;
}

int32_t iree_invoke_arg_count(IreeModule module, const char* function_name,
                              int32_t* out_in_count, int32_t* out_out_count) {
    (void)module; (void)function_name; (void)out_in_count; (void)out_out_count;
    set_error("IREE function arg count not yet implemented (M-I1)");
    return 1;
}

}  // extern "C"
