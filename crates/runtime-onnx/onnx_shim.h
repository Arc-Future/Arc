// onnx_shim.h — Arc.AI.Onnx C ABI shim over ONNX Runtime C++ API.
//
// Wraps `onnxruntime_cxx_api.h` (the C++ header-only facade over the C API)
// behind `extern "C"` functions + opaque `void*` handles, so that Arc's
// verified-FFI machinery (RFC 016 / RFC 016 `.ani` contracts) can drive
// inference with zero `unsafe`, zero raw-pointer marshalling, and no C++
// exceptions leaking across the ABI boundary.
//
// Ownership model:
//   - Every `void*` handle returned here is an owning heap allocation of the
//     corresponding `Ort::` object (`Ort::Env`, `Ort::Session`,
//     `Ort::SessionOptions`, `Ort::Value`).
//   - Each object has a matching `onnx_*_release` entry. Handles must only be
//     released via their own release function.
//   - Tensors are OWNING: the created Ort::Value holds a private copy of the
//     caller's data (allocated by ONNX), so the caller's buffer may be freed
//     immediately after `onnx_create_tensor_*` returns.
//
// Error protocol: every `int`-returning function returns 0 on success and a
// nonzero code on failure. On failure `onnx_last_error()` copies the
// NUL-terminated last-error message into a caller buffer; `onnx_clear_error()`
// resets it. No C++ exception ever crosses this boundary.
//
// Compile: link against onnxruntime (see scripts/build-onnx-shim.ps1).
#ifndef ONNX_SHIM_H
#define ONNX_SHIM_H

#include <stdint.h>
#include <stddef.h>

/* DLL export/import annotation. This header is the shim's ABI surface: the
   `extern "C"` functions below are what Arc binds via onnx.ani. On Windows PE,
   symbols are NOT exported by default (clang++/lld-link) — without explicit
   `__declspec(dllexport)` the export table stays empty and symbol binding
   (`Native.IsAvailable("onnx")`) fails. The shim is always built as a DLL
   (scripts/build-onnx-shim.ps1), so we export unconditionally. */
#if defined(_WIN32)
#  define ONNX_SHIM_API __declspec(dllexport)
#else
#  define ONNX_SHIM_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handles (owning heap objects). */
typedef void* OnnxEnv;
typedef void* OnnxSession;
typedef void* OnnxSessionOptions;
typedef void* OnnxValue;

/* ── error ─────────────────────────────────────────────────────────── */
/* Copies the last error message (NUL-terminated, <= buf_len-1 bytes) into
   buf. Returns the message length (not including NUL). 0 if no error.
   buf_len is the caller-provided buffer capacity (Arc List<byte> size). */
int32_t ONNX_SHIM_API onnx_last_error(uint8_t* buf, int32_t buf_len);
void    ONNX_SHIM_API onnx_clear_error(void);

/* ── env ───────────────────────────────────────────────────────────── */
/* log_level: ORT_LOGGING_LEVEL (0=VERBOSE..4=FATAL), recommended 2=WARNING. */
int32_t ONNX_SHIM_API onnx_create_env(int32_t log_level, const char* name, OnnxEnv* out_env);
void    ONNX_SHIM_API onnx_release_env(OnnxEnv env);

/* ── session options ───────────────────────────────────────────────── */
int32_t ONNX_SHIM_API onnx_create_session_options(OnnxSessionOptions* out);
void    ONNX_SHIM_API onnx_release_session_options(OnnxSessionOptions o);
int32_t ONNX_SHIM_API onnx_options_set_intra_op_threads(OnnxSessionOptions o, int32_t n);
int32_t ONNX_SHIM_API onnx_options_set_inter_op_threads(OnnxSessionOptions o, int32_t n);
/* level: 0=DISABLE 1=ENABLE_BASIC 2=ENABLE_EXTENDED 3=ENABLE_ALL */
int32_t ONNX_SHIM_API onnx_options_set_graph_opt_level(OnnxSessionOptions o, int32_t level);
/* Append DirectML execution provider (Windows). device_id: DML device. */
int32_t ONNX_SHIM_API onnx_options_append_dml(OnnxSessionOptions o, int32_t device_id);

/* ── session (model load / metadata) ───────────────────────────────── */
int32_t ONNX_SHIM_API onnx_create_session(OnnxEnv env, const char* model_path,
                            OnnxSessionOptions opts, OnnxSession* out);
void    ONNX_SHIM_API onnx_release_session(OnnxSession s);
int32_t ONNX_SHIM_API onnx_session_input_count(OnnxSession s, int32_t* out_count);
int32_t ONNX_SHIM_API onnx_session_output_count(OnnxSession s, int32_t* out_count);
/* Writes the index-th input/output name, NUL-terminated, into buf (buf_len
   capacity). Truncated to buf_len-1 if the name is longer; out_len = length. */
int32_t ONNX_SHIM_API onnx_session_get_input_name(OnnxSession s, int32_t index,
                                    uint8_t* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_session_get_output_name(OnnxSession s, int32_t index,
                                     uint8_t* buf, int32_t buf_len, int32_t* out_len);
/* Fills elem_type (ONNXTensorElementDataType) and up to dimCap dims.
   dim_count receives the true dimension count (may exceed dimCap). */
int32_t ONNX_SHIM_API onnx_session_get_input_info(OnnxSession s, int32_t index,
                                    int32_t* elem_type, int64_t* dims,
                                    int32_t dimCap, int32_t* dim_count);
int32_t ONNX_SHIM_API onnx_session_get_output_info(OnnxSession s, int32_t index,
                                     int32_t* elem_type, int64_t* dims,
                                     int32_t dimCap, int32_t* dim_count);

/* ── tensor value (owning) ────────────────────────────────────────── */
/* Create an OWNING tensor of the given shape, copying `data` (data_len
   elements) into ONNX-allocated storage. The returned Value owns its payload,
   so the caller's data buffer may be freed immediately after creation. */
int32_t ONNX_SHIM_API onnx_create_tensor_float(int64_t* shape, int32_t shape_count,
                                 float* data, int32_t data_len, OnnxValue* out);
int32_t ONNX_SHIM_API onnx_create_tensor_double(int64_t* shape, int32_t shape_count,
                                  double* data, int32_t data_len, OnnxValue* out);
int32_t ONNX_SHIM_API onnx_create_tensor_i32(int64_t* shape, int32_t shape_count,
                               int32_t* data, int32_t data_len, OnnxValue* out);
int32_t ONNX_SHIM_API onnx_create_tensor_i64(int64_t* shape, int32_t shape_count,
                               int64_t* data, int32_t data_len, OnnxValue* out);
int32_t ONNX_SHIM_API onnx_create_tensor_byte(int64_t* shape, int32_t shape_count,
                                uint8_t* data, int32_t data_len, OnnxValue* out);
void    ONNX_SHIM_API onnx_release_value(OnnxValue v);
/* Copies up to buf_len elements of the typed payload into buf; out_len = elements
   actually copied. buf_len is the caller List<T> capacity. */
int32_t ONNX_SHIM_API onnx_tensor_read_float(OnnxValue v, float* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_tensor_read_double(OnnxValue v, double* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_tensor_read_i32(OnnxValue v, int32_t* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_tensor_read_i64(OnnxValue v, int64_t* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_tensor_read_byte(OnnxValue v, uint8_t* buf, int32_t buf_len, int32_t* out_len);
/* Raw byte read for exotic element types; copies up to buf_len bytes. */
int32_t ONNX_SHIM_API onnx_tensor_get_data(OnnxValue v, uint8_t* buf, int32_t buf_len, int32_t* out_len);
int32_t ONNX_SHIM_API onnx_tensor_get_shape(OnnxValue v, int64_t* dims, int32_t dimCap,
                              int32_t* dim_count);
int32_t ONNX_SHIM_API onnx_tensor_get_elem_type(OnnxValue v, int32_t* out_type);
int32_t ONNX_SHIM_API onnx_tensor_get_total(OnnxValue v, int64_t* out_total);

/* ── run ───────────────────────────────────────────────────────────── */
/* Executes the session.
   input_names : NUL-separated UTF-8 names blob (length = input_names_len).
   input_values: nInputs opaque OnnxValue handles (as int64); its size gives
                 the input count.
   output_names: if non-empty, a NUL-separated blob naming the outputs to
                 produce (the subset); if empty, run ALL outputs.
   output_values: caller-owned capacity (size = nOutputs). On success receives
                 `out_count` produced OnnxValue handles (owned by caller,
                 release with onnx_release_value). Input value handles are
                 borrowed and remain owned by the caller. */
int32_t ONNX_SHIM_API onnx_run(OnnxSession s,
                 uint8_t* input_names, int32_t input_names_len,
                 int64_t* input_values, int32_t n_inputs,
                 uint8_t* output_names, int32_t output_names_len,
                 int64_t* output_values, int32_t n_outputs,
                 int32_t* out_count);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* ONNX_SHIM_H */
