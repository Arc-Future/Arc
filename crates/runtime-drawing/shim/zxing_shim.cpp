// zxing_shim.cpp — zxing-cpp 通用条码解码 `extern "C"` 桥接（RFC 036 M5 真实语义）
//
// 对齐 RFC 036 §1.5 `rt_barcode_zxing_decode` ABI 与 §1.6 zxing 接入形态：
//   - zxing-cpp 为 C++ 库，本 shim 以 C++ 编译并调 `zxing::ReadBarcodes`；
//     导出符号用 `extern "C"`（`zxing_decode_c`），与
//     `crates/arc/native/zxing.ani` 契约签名逐符号对应。
//   - 输入 RGBA8 像素缓冲（`rgba` + `rgba_len` = `w*h*4`）+ 宽高；
//     输出 NUL 终止文本缓冲（`text_out` + `text_cap`）；返回 0 成功 / 非零失败。
//   - 像素/文本缓冲 `List<byte>` 经编译器零拷贝展开为 `ptr,i32`（RFC 027 M3 §3.3），
//     故签名含 `rgba_len`/`text_len` 两个 i32 长度参数。
//
// 构建：不 vendored，由 `scripts/fetch-zxing-native.ps1` 将本文件与 zxing-cpp
// 源码（core/src 全量 + reader 路径）一起编译进共享库 `zxing.dll` / `libzxing.*`，
// 产物落 `target/`（不进仓库；工作区卫生 G″）。
//
// 许可证：Apache-2.0（Arc 兼容；与 zxing-cpp 一致）。

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

#include "ReadBarcode.h"
#include "ImageView.h"
#include "ReaderOptions.h"

#if defined(_WIN32)
#  define ZXING_SHIM_EXPORT extern "C" __declspec(dllexport)
#else
#  define ZXING_SHIM_EXPORT extern "C" __attribute__((visibility("default")))
#endif

/// 解码 RGBA8 图像中的第一个条码（QR / 1D / DataMatrix / PDF417 等全格式），
/// 把文本写入 text_out（NUL 终止）。返回 0 成功；-1 无码/失败；-2 参数非法。
ZXING_SHIM_EXPORT int zxing_decode_c(const uint8_t* rgba, int rgba_len, int w, int h,
                                     char* text_out, int text_len, int text_cap) {
    if (!rgba || !text_out || w <= 0 || h <= 0 || text_cap <= 0) {
        return -2;
    }
    if (rgba_len < w * h * 4) {
        return -2;
    }

    // RGBA8（R,G,B,A 布局，与 Bitmap Rgba32 像素面一致；Arc 侧 BarcodeReader
    // 按 R,G,B,A 顺序打包）。zxing 的 ImageFormat::RGBA 即按 R,G,B,A 解析。
    ZXing::ImageView image(rgba, w, h, ZXing::ImageFormat::RGBA, w * 4, 4);

    ZXing::ReaderOptions opts;
    opts.setTryHarder(true);
    opts.setTryRotate(true);

    ZXing::Barcodes results = ZXing::ReadBarcodes(image, opts);
    for (const ZXing::Barcode& b : results) {
        if (!b.isValid()) {
            continue;
        }
        std::string text = b.text();
        if (text.empty()) {
            continue;
        }
        size_t cap = static_cast<size_t>(text_cap) - 1;
        size_t len = text.size();
        if (len > cap) {
            len = cap;
        }
        std::memcpy(text_out, text.data(), len);
        text_out[len] = '\0';
        return 0;
    }

    return -1;
}
