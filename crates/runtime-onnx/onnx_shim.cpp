// onnx_shim.cpp — Arc.AI.Onnx C ABI shim over ONNX Runtime C++ API.
//
// Implements onnx_shim.h. The ONNX Runtime C++ API (`onnxruntime_cxx_api.h`)
// is the documented C++ facade over the exported C API; the DLL that ships the
// C++ symbols is `onnxruntime.dll`. We compile this TU against the C++ API and
// link onnxruntime.lib so that all inference work happens inside ONNX Runtime.
//
// Every `extern "C"` function catches exceptions at its own boundary and stores
// a message via `set_last_error`; the caller retrieves it with
// `onnx_last_error`. No C++ exception propagates to Arc.
#include "onnx_shim.h"

#include <onnxruntime_cxx_api.h>
#include <dml_provider_factory.h>

#include <algorithm>
#include <cstring>
#include <new>
#include <string>
#include <vector>

#ifdef _WIN32
#include <windows.h>
#endif

namespace {

#ifdef _WIN32
// ONNX Runtime Windows 上 ORTCHAR_T == wchar_t：把 UTF-8 路径转 UTF-16。
std::wstring utf8_to_wide(const char* utf8) {
    if (!utf8 || !*utf8) return std::wstring();
    int len = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, nullptr, 0);
    if (len <= 1) return std::wstring();
    std::wstring w(static_cast<size_t>(len - 1), 0);
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, &w[0], len);
    return w;
}
#endif


// ── last error (thread-local) ───────────────────────────────────────
thread_local std::string g_last_error;

void set_last_error(const char* msg) {
    g_last_error = msg ? msg : "onnx shim error";
}

// Runs `body` in a try/catch that converts any exception to a nonzero status
// and stores the message. Body must return int32_t.
template <typename Fn>
int32_t guard(Fn&& body) {
    try {
        return static_cast<int32_t>(body());
    } catch (const Ort::Exception& e) {
        set_last_error(e.what());
        return -1;
    } catch (const std::exception& e) {
        set_last_error(e.what());
        return -1;
    } catch (...) {
        set_last_error("unknown onnx shim exception");
        return -1;
    }
}

// Maps an ONNX tensor element type to its byte size. 0 if unsupported.
size_t onnx_elem_size(ONNXTensorElementDataType t) {
    switch (t) {
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT32:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT32:   return 4;
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_DOUBLE:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT64:   return 8;
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT16:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_BFLOAT16:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT16:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT16:   return 2;
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_INT8:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT8:
        case ONNX_TENSOR_ELEMENT_DATA_TYPE_BOOL:     return 1;
        default:                                     return 0;
    }
}

// Splits a NUL-separated UTF-8 blob into char* pointers (no copies, no owns).
// `n` bounds how many pointers to produce; stops early on buffer exhaustion.
void split_names(const uint8_t* blob, int32_t len, std::vector<const char*>& out) {
    const uint8_t* p = blob;
    const uint8_t* end = blob + len;
    while (p < end) {
        out.push_back(reinterpret_cast<const char*>(p));
        while (p < end && *p != 0) ++p;
        if (p < end) ++p;  // skip NUL
    }
}

// 惰性默认分配器：避免命名空间级全局 `Ort::AllocatorWithDefaultOptions` 的
// 静态初始化在 DLL 加载期调用 ONNX Runtime GetApi()/CreateAllocator——该初始化
// 失败会触发 ERROR_DLL_INIT_FAILED（错误 1114），使 DLL 加载失败 →
// `Native.IsAvailable("onnx")` 假阴性（真库在位却判不可用）。改用函数局部
// static（C++11 magic static，线程安全），首次使用时才初始化；失败由各
// `guard` 在真实调用点捕获，不阻塞 DLL 加载 / 可用性门闩。
Ort::AllocatorWithDefaultOptions& alloc() {
    static Ort::AllocatorWithDefaultOptions a;
    return a;
}

// Template helpers MUST stay in C++ linkage (not `extern "C"`): templates cannot
// have C language linkage. They are `static`, so no symbol is exported.

// Create an OWNING tensor of element type T with the given shape, then copy
// `data` (data_len elements) into ONNX-allocated storage.
template <typename T>
static int32_t create_typed_tensor(int64_t* shape, int32_t shape_count,
                                   T* data, int32_t data_len, OnnxValue* out) {
    Ort::AllocatorWithDefaultOptions alloc;
    Ort::Value v = Ort::Value::CreateTensor<T>(alloc, shape, static_cast<size_t>(shape_count));
    T* dst = v.GetTensorMutableData<T>();
    // Element count = product of shape; guard against mismatched sizes.
    size_t total = static_cast<size_t>(shape_count) > 0 ? 1 : 0;
    for (int32_t i = 0; i < shape_count; ++i) total *= static_cast<size_t>(shape[i] < 0 ? 0 : shape[i]);
    size_t copy = std::min(total, static_cast<size_t>(data_len));
    if (copy > 0) std::memcpy(dst, data, copy * sizeof(T));
    *out = new Ort::Value(std::move(v));
    return 0;
}

// Copy up to buf_len elements of the typed payload into buf.
template <typename T>
static int32_t read_typed_tensor(OnnxValue v, T* buf, int32_t buf_len, int32_t* out_len) {
    Ort::Value* val = static_cast<Ort::Value*>(v);
    T* src = val->GetTensorMutableData<T>();
    size_t n = val->GetTensorTypeAndShapeInfo().GetElementCount();
    size_t copy = std::min<size_t>(n, static_cast<size_t>(buf_len));
    if (copy > 0) std::memcpy(buf, src, copy * sizeof(T));
    *out_len = static_cast<int32_t>(copy);
    return 0;
}

}  // namespace

// ── error ───────────────────────────────────────────────────────────
extern "C" {

int32_t onnx_last_error(uint8_t* buf, int32_t buf_len) {
    if (!buf || buf_len <= 0) return static_cast<int32_t>(g_last_error.size());
    int32_t n = static_cast<int32_t>(std::min<size_t>(g_last_error.size(), static_cast<size_t>(buf_len - 1)));
    std::memcpy(buf, g_last_error.data(), static_cast<size_t>(n));
    buf[n] = 0;
    return n;
}

void onnx_clear_error(void) { g_last_error.clear(); }

// ── env ─────────────────────────────────────────────────────────────
int32_t onnx_create_env(int32_t log_level, const char* name, OnnxEnv* out_env) {
    return guard([&]() -> int32_t {
        Ort::Env* env = new Ort::Env(static_cast<OrtLoggingLevel>(log_level), name ? name : "arc");
        *out_env = env;
        return 0;
    });
}

void onnx_release_env(OnnxEnv env) { delete static_cast<Ort::Env*>(env); }

// ── session options ─────────────────────────────────────────────────
int32_t onnx_create_session_options(OnnxSessionOptions* out) {
    return guard([&]() -> int32_t {
        *out = new Ort::SessionOptions();
        return 0;
    });
}

void onnx_release_session_options(OnnxSessionOptions o) {
    delete static_cast<Ort::SessionOptions*>(o);
}

int32_t onnx_options_set_intra_op_threads(OnnxSessionOptions o, int32_t n) {
    return guard([&]() -> int32_t {
        static_cast<Ort::SessionOptions*>(o)->SetIntraOpNumThreads(n);
        return 0;
    });
}

int32_t onnx_options_set_inter_op_threads(OnnxSessionOptions o, int32_t n) {
    return guard([&]() -> int32_t {
        static_cast<Ort::SessionOptions*>(o)->SetInterOpNumThreads(n);
        return 0;
    });
}

int32_t onnx_options_set_graph_opt_level(OnnxSessionOptions o, int32_t level) {
    return guard([&]() -> int32_t {
        static_cast<Ort::SessionOptions*>(o)->SetGraphOptimizationLevel(
            static_cast<GraphOptimizationLevel>(level));
        return 0;
    });
}

int32_t onnx_options_append_dml(OnnxSessionOptions o, int32_t device_id) {
    return guard([&]() -> int32_t {
        // 1.20.1 的 C++ SessionOptions 已移除 AppendExecutionProvider_DML 成员，
        // 改走 dml_provider_factory.h 的 C API（DirectML 构建导出该符号）。
        OrtSessionOptions* raw = static_cast<Ort::SessionOptions*>(o)->GetUnowned();
        OrtStatusPtr st = OrtSessionOptionsAppendExecutionProvider_DML(raw, device_id);
        if (st) Ort::ThrowOnError(st);
        return 0;
    });
}

// ── session ─────────────────────────────────────────────────────────
int32_t onnx_create_session(OnnxEnv env, const char* model_path,
                            OnnxSessionOptions opts, OnnxSession* out) {
    return guard([&]() -> int32_t {
        Ort::Env* envp = static_cast<Ort::Env*>(env);
        Ort::SessionOptions* op = static_cast<Ort::SessionOptions*>(opts);
        Ort::Session* s;
#ifdef _WIN32
        // Windows 上 ORTCHAR_T == wchar_t：模型路径须转 UTF-16。
        std::wstring wpath = utf8_to_wide(model_path);
        s = new Ort::Session(*envp, wpath.c_str(), *op);
#else
        s = new Ort::Session(*envp, model_path ? model_path : "", *op);
#endif
        *out = s;
        return 0;
    });
}

void onnx_release_session(OnnxSession s) { delete static_cast<Ort::Session*>(s); }

int32_t onnx_session_input_count(OnnxSession s, int32_t* out_count) {
    return guard([&]() -> int32_t {
        *out_count = static_cast<int32_t>(static_cast<Ort::Session*>(s)->GetInputCount());
        return 0;
    });
}

int32_t onnx_session_output_count(OnnxSession s, int32_t* out_count) {
    return guard([&]() -> int32_t {
        *out_count = static_cast<int32_t>(static_cast<Ort::Session*>(s)->GetOutputCount());
        return 0;
    });
}

int32_t onnx_session_get_input_name(OnnxSession s, int32_t index, uint8_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t {
        if (buf_len <= 0) return 0;
        auto n = static_cast<Ort::Session*>(s)->GetInputNameAllocated(static_cast<size_t>(index), alloc());
        const char* name = n.get();
        size_t len = std::strlen(name);
        if (out_len) *out_len = static_cast<int32_t>(len);
        size_t copy = std::min(len, static_cast<size_t>(buf_len - 1));
        std::memcpy(buf, name, copy);
        buf[copy] = 0;
        return 0;
    });
}

int32_t onnx_session_get_output_name(OnnxSession s, int32_t index, uint8_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t {
        if (buf_len <= 0) return 0;
        auto n = static_cast<Ort::Session*>(s)->GetOutputNameAllocated(static_cast<size_t>(index), alloc());
        const char* name = n.get();
        size_t len = std::strlen(name);
        if (out_len) *out_len = static_cast<int32_t>(len);
        size_t copy = std::min(len, static_cast<size_t>(buf_len - 1));
        std::memcpy(buf, name, copy);
        buf[copy] = 0;
        return 0;
    });
}

static int32_t fill_tensor_info(Ort::TypeInfo type_info, int32_t* elem_type,
                                int64_t* dims, int32_t dimCap, int32_t* dim_count) {
    // GetTensorTypeAndShapeInfo() 返回非拥有 ConstTensorTypeAndShapeInfo（Unowned），
    // 不能用拥有式 TensorTypeAndShapeInfo 承接；用 auto 推导即可。
    auto ts = type_info.GetTensorTypeAndShapeInfo();
    *elem_type = static_cast<int32_t>(ts.GetElementType());
    std::vector<int64_t> shape = ts.GetShape();
    *dim_count = static_cast<int32_t>(shape.size());
    int32_t n = std::min<int32_t>(dimCap, static_cast<int32_t>(shape.size()));
    for (int32_t i = 0; i < n; ++i) dims[i] = shape[static_cast<size_t>(i)];
    return 0;
}

int32_t onnx_session_get_input_info(OnnxSession s, int32_t index, int32_t* elem_type,
                                    int64_t* dims, int32_t dimCap, int32_t* dim_count) {
    return guard([&]() -> int32_t {
        Ort::TypeInfo info = static_cast<Ort::Session*>(s)->GetInputTypeInfo(static_cast<size_t>(index));
        return fill_tensor_info(std::move(info), elem_type, dims, dimCap, dim_count);
    });
}

int32_t onnx_session_get_output_info(OnnxSession s, int32_t index, int32_t* elem_type,
                                     int64_t* dims, int32_t dimCap, int32_t* dim_count) {
    return guard([&]() -> int32_t {
        Ort::TypeInfo info = static_cast<Ort::Session*>(s)->GetOutputTypeInfo(static_cast<size_t>(index));
        return fill_tensor_info(std::move(info), elem_type, dims, dimCap, dim_count);
    });
}

// ── tensor value (owning) ────────────────────────────────────────────
int32_t onnx_create_tensor_float(int64_t* shape, int32_t shape_count,
                                 float* data, int32_t data_len, OnnxValue* out) {
    return guard([&]() -> int32_t { return create_typed_tensor(shape, shape_count, data, data_len, out); });
}
int32_t onnx_create_tensor_double(int64_t* shape, int32_t shape_count,
                                  double* data, int32_t data_len, OnnxValue* out) {
    return guard([&]() -> int32_t { return create_typed_tensor(shape, shape_count, data, data_len, out); });
}
int32_t onnx_create_tensor_i32(int64_t* shape, int32_t shape_count,
                               int32_t* data, int32_t data_len, OnnxValue* out) {
    return guard([&]() -> int32_t { return create_typed_tensor(shape, shape_count, data, data_len, out); });
}
int32_t onnx_create_tensor_i64(int64_t* shape, int32_t shape_count,
                               int64_t* data, int32_t data_len, OnnxValue* out) {
    return guard([&]() -> int32_t { return create_typed_tensor(shape, shape_count, data, data_len, out); });
}
int32_t onnx_create_tensor_byte(int64_t* shape, int32_t shape_count,
                                uint8_t* data, int32_t data_len, OnnxValue* out) {
    return guard([&]() -> int32_t { return create_typed_tensor(shape, shape_count, data, data_len, out); });
}

void onnx_release_value(OnnxValue v) { delete static_cast<Ort::Value*>(v); }

int32_t onnx_tensor_read_float(OnnxValue v, float* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t { return read_typed_tensor(v, buf, buf_len, out_len); });
}
int32_t onnx_tensor_read_double(OnnxValue v, double* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t { return read_typed_tensor(v, buf, buf_len, out_len); });
}
int32_t onnx_tensor_read_i32(OnnxValue v, int32_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t { return read_typed_tensor(v, buf, buf_len, out_len); });
}
int32_t onnx_tensor_read_i64(OnnxValue v, int64_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t { return read_typed_tensor(v, buf, buf_len, out_len); });
}
int32_t onnx_tensor_read_byte(OnnxValue v, uint8_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t { return read_typed_tensor(v, buf, buf_len, out_len); });
}

int32_t onnx_tensor_get_data(OnnxValue v, uint8_t* buf, int32_t buf_len, int32_t* out_len) {
    return guard([&]() -> int32_t {
        Ort::Value* val = static_cast<Ort::Value*>(v);
        size_t total = val->GetTensorTypeAndShapeInfo().GetElementCount();
        size_t esize = onnx_elem_size(val->GetTensorTypeAndShapeInfo().GetElementType());
        size_t bytes = total * esize;
        const void* p = val->GetTensorData<void>();
        size_t copy = std::min<size_t>(bytes, static_cast<size_t>(buf_len));
        if (copy > 0) std::memcpy(buf, p, copy);
        *out_len = static_cast<int32_t>(copy);
        return 0;
    });
}

int32_t onnx_tensor_get_shape(OnnxValue v, int64_t* dims, int32_t dimCap, int32_t* dim_count) {
    return guard([&]() -> int32_t {
        std::vector<int64_t> shape = static_cast<Ort::Value*>(v)->GetTensorTypeAndShapeInfo().GetShape();
        *dim_count = static_cast<int32_t>(shape.size());
        int32_t n = std::min<int32_t>(dimCap, static_cast<int32_t>(shape.size()));
        for (int32_t i = 0; i < n; ++i) dims[i] = shape[static_cast<size_t>(i)];
        return 0;
    });
}

int32_t onnx_tensor_get_elem_type(OnnxValue v, int32_t* out_type) {
    return guard([&]() -> int32_t {
        *out_type = static_cast<int32_t>(
            static_cast<Ort::Value*>(v)->GetTensorTypeAndShapeInfo().GetElementType());
        return 0;
    });
}

int32_t onnx_tensor_get_total(OnnxValue v, int64_t* out_total) {
    return guard([&]() -> int32_t {
        *out_total = static_cast<int64_t>(
            static_cast<Ort::Value*>(v)->GetTensorTypeAndShapeInfo().GetElementCount());
        return 0;
    });
}

// ── run ─────────────────────────────────────────────────────────────
int32_t onnx_run(OnnxSession s,
                 uint8_t* input_names, int32_t input_names_len,
                 int64_t* input_values, int32_t n_inputs,
                 uint8_t* output_names, int32_t output_names_len,
                 int64_t* output_values, int32_t n_outputs,
                 int32_t* out_count) {
    return guard([&]() -> int32_t {
        Ort::Session* session = static_cast<Ort::Session*>(s);

        // Input names + borrowed value pointers.
        std::vector<const char*> in_name_ptrs;
        split_names(input_names, input_names_len, in_name_ptrs);
        if (static_cast<int32_t>(in_name_ptrs.size()) < n_inputs) {
            set_last_error("onnx_run: input names blob shorter than n_inputs");
            return -1;
        }
        // Run 的 6 参重载要 `const Value*`（连续 Value 对象数组），而输入经 FFI 以
        // 堆 Value 句柄传入。把各堆 Value 迁入本地连续 vector（Run 内同步消费输入，
        // 返回后 vector 析构释放输入 OrtValue；Arc 侧堆句柄变为 moved-from 空 Value，
        // 其 onnx_release_value 的 delete 安全）。
        std::vector<Ort::Value> in_values;
        in_values.reserve(static_cast<size_t>(n_inputs));
        for (int32_t i = 0; i < n_inputs; ++i) {
            Ort::Value* hp = reinterpret_cast<Ort::Value*>(static_cast<intptr_t>(input_values[i]));
            in_values.emplace_back(std::move(*hp));
        }

        // Output names (explicit subset or all).
        std::vector<const char*> out_name_ptrs;
        std::vector<Ort::AllocatedStringPtr> owned_names;
        size_t want;
        if (output_names_len > 0) {
            split_names(output_names, output_names_len, out_name_ptrs);
            want = out_name_ptrs.size();
        } else {
            want = static_cast<Ort::Session*>(s)->GetOutputCount();
            for (size_t i = 0; i < want; ++i) {
                owned_names.push_back(session->GetOutputNameAllocated(i, alloc()));
                out_name_ptrs.push_back(owned_names.back().get());
            }
        }
        if (want > static_cast<size_t>(n_outputs)) {
            set_last_error("onnx_run: output_values capacity smaller than requested outputs");
            return -1;
        }

        std::vector<Ort::Value> results = session->Run(
            Ort::RunOptions{nullptr},
            in_name_ptrs.data(), in_values.data(), static_cast<size_t>(n_inputs),
            out_name_ptrs.data(), want);

        size_t produced = results.size();
        if (out_count) *out_count = static_cast<int32_t>(produced);
        for (size_t i = 0; i < produced; ++i) {
            output_values[i] = static_cast<int64_t>(reinterpret_cast<intptr_t>(
                new Ort::Value(std::move(results[i]))));
        }
        return 0;
    });
}

}  // extern "C"
