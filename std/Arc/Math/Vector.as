namespace Arc;

// Vector<T, N> —— SIMD vector value type (RFC 021 Phase 2).
//
// Built-in facade: method bodies are never compiled. typeck intercepts
// via `check_builtin_static_method`; codegen intercepts via `try_emit_vector_call`
// and emits LLVM vector instructions directly (`fadd <4 x float>`, etc.).
//
// `T` ∈ {float, double}, `N` ∈ {4, 8, 16}. Monomorphizes to LLVM `<N x T>`.
// Value type (stack-allocated, no ARC, no runtime ABI).
/// <summary>SIMD 向量门面，单指令多数据值类型（RFC 021 Phase 2）。</summary>
public class Vector {
}
