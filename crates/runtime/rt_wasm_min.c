// RFC 037 M-W3 (Draft): minimal wasm32-unknown-unknown runtime subset.
//
// Target-gated replacement for the full desktop runtime chain. No platform.o,
// no rt_ui_*, no wgpu-native, no DOM / browser glue — artifact-only vertical
// slice (`.ll` + `.wasm`); not runnable in a browser without future M-W3 work.

#include <stdint.h>

void rt_env_init(int argc, char **argv) {
    (void)argc;
    (void)argv;
}

__attribute__((noreturn)) void rt_panic(const char *msg) {
    (void)msg;
    __builtin_trap();
}

__attribute__((noreturn)) void rt_panic_at(const char *msg, const char *file, int32_t line,
                                           int32_t col) {
    (void)msg;
    (void)file;
    (void)line;
    (void)col;
    __builtin_trap();
}
