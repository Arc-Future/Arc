/* rt_qrcode.c — RFC 029 M2 二维码生成 ABI（vendored qrcodegen 封装）
 *
 * 本 TU 封装 Nayuki qrcodegen（crates/runtime-drawing/qrcodegen.{c,h}，独立 TU
 * 编译、上游原样、MIT），向 std/Drawing/QrCodeNative 暴露 `rt_qrcode_*` ABI。
 *
 * ABI 清单（对齐 RFC 029 §1.5；返回值约定：成功 0 / 失败非零）：
 *   rt_qrcode_encode 文本 → qrcodegen 模块矩阵（bit-packed）
 *
 * modules 形态：调用方预分配 ≥ qrcodegen_BUFFER_LEN_MAX（3918）字节；成功时
 * modules[0] = 模块边长（21..177），模块 (x,y) 存于 modules[1 + (y*size+x)/8]
 * 的第 ((y*size+x) % 8) 位（LSB-first，qrcodegen_getModule 语义，见
 * qrcodegen.c getModuleBounded）。
 *
 * 确定性：同一 (text, ecc, mask) → 同一模块矩阵（qrcodegen 纯函数）；ECC 经
 * boostEcl=false 严格保持请求等级（ecc 0-3 直射 qrcodegen_Ecc LOW..HIGH）。
 *
 * 防御：非法输入（NULL / ecc 越界 / mask 越界 / 编码失败）一律返回失败码，
 * 不崩溃。
 *
 * 编译裁剪（RFC 029 §1.2）：本 TU 仅引用 qrcodegen 编码链；qrcodegen.c 独立
 * TU，未引用函数经链接期 section GC 裁掉；裁剪断言见 qrcode_prune_e2e。
 *
 * 注册（rfc036-int 收口完成）：本 TU + qrcodegen.c 已在 prepare_runtime_objects
 * （crates/codegen/src/llvm_ir/mod.rs）注册为独立源并注入 `-I crates/runtime-drawing`；
 * 分派经 runtime_decls.rs declare + builtin_dispatch.rs/emit_call.rs
 * `QrCodeNative.*` → `rt_qrcode_*`（对齐 M1 rt_image.c 先例）。
 */

#include <stdint.h>
#include <stddef.h>

#include "qrcodegen.h"

int32_t rt_qrcode_encode(const char* text, int32_t ecc, int32_t mask,
                         uint8_t* modules, int32_t* size) {
    if (!text || !modules || !size) return 1;
    if (ecc < 0 || ecc > 3) return 1;    /* qrcodegen_Ecc LOW(0)..HIGH(3) */
    if (mask < -1 || mask > 7) return 1; /* qrcodegen_Mask AUTO(-1)..7 */
    *size = 0;
    uint8_t temp[qrcodegen_BUFFER_LEN_MAX];
    if (!qrcodegen_encodeText(text, temp, modules, (enum qrcodegen_Ecc)ecc,
                              qrcodegen_VERSION_MIN, qrcodegen_VERSION_MAX,
                              (enum qrcodegen_Mask)mask, false)) {
        return 1; /* 文本过长等编码失败 */
    }
    *size = qrcodegen_getSize(modules);
    return 0;
}
