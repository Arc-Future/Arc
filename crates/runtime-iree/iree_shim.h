// iree_shim.h — Arc.AI.Iree C ABI shim over IREE Runtime.
//
// Wraps IREE's high-level runtime C API (`iree/runtime/api.h` — instance /
// session / call) plus HAL buffer-view I/O behind `extern "C"` functions +
// opaque `void*` handles, so that Arc's verified-FFI machinery (RFC 016 /
// RFC 016 `.ani` contracts) can drive IREE execution with zero `unsafe`, zero
// raw-pointer marshalling, and no C++ exceptions leaking across the ABI.
//
// Ownership model:
//   - Every `void*` handle returned here is an owning heap allocation of the
//     corresponding IREE object (`iree_runtime_instance_t*`,
//     `iree_runtime_session_t*`, `iree_hal_buffer_view_t*`).
//   - Each object has a matching `iree_*_release` entry. Handles must only be
//     released via their own release function.
//   - BufferViews are OWNING: the created IREE buffer holds a private copy of
//     the caller's data, so the caller's buffer may be freed immediately after
//     `iree_create_buffer_*` returns.
//
// Error protocol: every `int`-returning function returns 0 on success and a
// nonzero code on failure. On failure `iree_last_error()` copies the
// NUL-terminated last-error message into a caller buffer; `iree_clear_error()`
// resets it. No C++ exception ever crosses this boundary.
//
// Milestone mapping: M-I0 = this header + degradation chain (module unavailable
// when no real lib); M-I1+ fills real instance/module/invoke bodies.
//
// Compile: link against IREE runtime (see scripts/build-iree-shim.ps1).
#ifndef IREE_SHIM_H
#define IREE_SHIM_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handles (owning heap objects). */
typedef void* IreeInstance;
typedef void* IreeModule;
typedef void* IreeBufferView;

/* ── error ─────────────────────────────────────────────────────────── */
/* Copies the last error message (NUL-terminated, <= buf_len-1 bytes) into
   buf. Returns the message length (not including NUL). 0 if no error. */
int32_t iree_last_error(uint8_t* buf, int32_t buf_len);
void    iree_clear_error(void);

/* ── runtime instance ──────────────────────────────────────────────── */
/* log_level: 0=VERBOSE..4=FATAL, recommended 2=WARNING (mirrors onnx shim). */
int32_t iree_create_runtime(int32_t log_level, const char* name, IreeInstance* out);
void    iree_release_runtime(IreeInstance instance);

/* ── module (load .vmfb) ───────────────────────────────────────────── */
int32_t iree_load_module(IreeInstance instance, const char* module_path, IreeModule* out);
void    iree_release_module(IreeModule module);
int32_t iree_module_function_count(IreeModule module, int32_t* out_count);

/* ── device / driver probe (M-I3) ──────────────────────────────────── */
/* Returns 0 if the named driver is available, nonzero otherwise. */
int32_t iree_device_driver_available(const char* driver);

/* ── buffer view (owning) ──────────────────────────────────────────── */
int32_t iree_create_buffer_float(int64_t* shape, int32_t shape_count,
                                 float* data, int32_t data_len, IreeBufferView* out);
int32_t iree_create_buffer_double(int64_t* shape, int32_t shape_count,
                                  double* data, int32_t data_len, IreeBufferView* out);
int32_t iree_create_buffer_i32(int64_t* shape, int32_t shape_count,
                               int32_t* data, int32_t data_len, IreeBufferView* out);
int32_t iree_create_buffer_i64(int64_t* shape, int32_t shape_count,
                               int64_t* data, int32_t data_len, IreeBufferView* out);
int32_t iree_create_buffer_byte(int64_t* shape, int32_t shape_count,
                                uint8_t* data, int32_t data_len, IreeBufferView* out);
void    iree_release_buffer_view(IreeBufferView v);
int32_t iree_buffer_view_read_float(IreeBufferView v, float* buf, int32_t buf_len, int32_t* out_len);
int32_t iree_buffer_view_read_double(IreeBufferView v, double* buf, int32_t buf_len, int32_t* out_len);
int32_t iree_buffer_view_read_i32(IreeBufferView v, int32_t* buf, int32_t buf_len, int32_t* out_len);
int32_t iree_buffer_view_read_i64(IreeBufferView v, int64_t* buf, int32_t buf_len, int32_t* out_len);
int32_t iree_buffer_view_read_byte(IreeBufferView v, uint8_t* buf, int32_t buf_len, int32_t* out_len);
int32_t iree_buffer_view_get_shape(IreeBufferView v, int64_t* dims, int32_t dimCap, int32_t* dim_count);
int32_t iree_buffer_view_get_elem_type(IreeBufferView v, int32_t* out_type);
int32_t iree_buffer_view_get_total(IreeBufferView v, int64_t* out_total);

/* ── invoke (M-I1) ─────────────────────────────────────────────────── */
/* Executes the named function. inputs: nInputs BufferView handles (as int64);
   its size gives the input count. outputs: caller-owned capacity (size =
   nOutputs); on success receives `out_count` produced BufferView handles
   (owned by caller, release with iree_release_buffer_view). */
int32_t iree_invoke(IreeModule module, const char* function_name,
                    int64_t* inputs, int32_t n_inputs,
                    int64_t* outputs, int32_t n_outputs, int32_t* out_count);

/* ── function metadata (M-I1) ──────────────────────────────────────── */
/* Queries the named function's input/output arg counts (for IAIModel
   InputCount/OutputCount). */
int32_t iree_invoke_arg_count(IreeModule module, const char* function_name,
                              int32_t* out_in_count, int32_t* out_out_count);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* IREE_SHIM_H */
