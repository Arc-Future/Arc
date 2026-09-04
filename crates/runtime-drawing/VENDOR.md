# Vendored stb single-file libraries (RFC 036 · crates/runtime-drawing)

本目录收敛 RFC 036 图像/图形/二维码/条形码系列的 **vendored 单文件 C 底座**。
对齐 `crates/runtime-sqlite/` 先例：纯 C 资源目录，无 Cargo.toml、非 Rust crate、
不进入 Cargo workspace。`rt_image.c` 经 `prepare_runtime_objects`
（`crates/codegen/src/llvm_ir/mod.rs`）直编 + `-I crates/runtime-drawing` 逐源注入。

## M1 已入库

| 文件 | 版本 | 来源 | commit / 版本锁定 | 下载日期 | 用途 |
|------|------|------|-------------------|----------|------|
| `stb_image.h` | v2.30 | https://github.com/nothings/stb | `2c980bb59875b0d32144a71867fbdebb2f77cd20`（repo HEAD；该文件最近改动 `013ac3beddff3dbffafd5177e7972067cd2b5083` 2024-05-31） | 2026-08-04 | 图像解码 PNG/JPEG/BMP/GIF/TGA/HDR（`rt_image.c` 内 `STB_IMAGE_IMPLEMENTATION`） |
| `stb_image_write.h` | v1.16 | https://github.com/nothings/stb | `2c980bb59875b0d32144a71867fbdebb2f77cd20`（repo HEAD） | 2026-08-04 | 图像编码 PNG/JPG/BMP/TGA（`rt_image.c` 内 `STB_IMAGE_WRITE_IMPLEMENTATION`） |

SHA-256（下载时核验）：

| 文件 | SHA-256 |
|------|---------|
| `stb_image.h` | `594C2FE35D49488B4382DBFAEC8F98366DEFCA819D916AC95BECF3E75F4200B3` |
| `stb_image_write.h` | `CBD5F0AD7A9CF4468AFFB36354A1D2338034F2C12473CF1A8E32053CB6914A05` |

## M4 已入库（quirc QR 解码底座 · 2026-08-04）

quirc 上游 `lib/` 为多文件库（`quirc.c` / `decode.c` / `identify.c` /
`version_db.c` + `quirc.h` / `quirc_internal.h`），**全部按上游原样入库**；
`rt_barcode.c` 以 `#include` 方式合并进单一编译单元（对齐 `rt_image.c`
合并 stb 的形态；上游文件不改动，便于版本 diff 与更新）。

| 文件 | 版本 | 来源 | commit / 版本锁定 | 下载日期 | 用途 |
|------|------|------|-------------------|----------|------|
| `quirc.c` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | quirc 主入口（new/destroy/resize/count/strerror） |
| `quirc.h` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | 公共头（quirc_decode API） |
| `quirc_internal.h` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | 内部头（struct quirc / version DB 声明） |
| `decode.c` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | 解码器（quirc_decode/flip） |
| `identify.c` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | 检测器（quirc_begin/end/extract） |
| `version_db.c` | v1.0（master） | https://github.com/dlbeer/quirc | `927d680904dc95fdff4cd9d022eb374b438ff8f2` | 2026-08-04 | QR 版本信息表（quirc_version_db） |

SHA-256（下载时核验）：

| 文件 | SHA-256 |
|------|---------|
| `quirc.c` | `0294B6C56F8C021B256C4C153D70483368164C6CF0CCE643E1B6BE03ED3585C0` |
| `quirc.h` | `49660EA710ADD2D6F304A1323F53190F5A2BF34DB4DD160D633DB0C3F22BFBA5` |
| `quirc_internal.h` | `E383ED1A0CA70C07B0530D76BFEB6BD5525EFDA182589F48C978921A9E54676B` |
| `decode.c` | `D4468C55ECD0D2F905A6813513708005E6D609EF0A3D32A17673313C7552A7C1` |
| `identify.c` | `AE858D86ADCB12DB80AD01F6D941CC2247FB5970ABF0754F24DCA027EDE2BA99` |
| `version_db.c` | `6764AA2F245085080E1E5CEFD9DCD59B9727718A0D4606956E0502A57F5DFF30` |

## 后置（M2+，先不下载，避免 scope creep）

| 文件 | 版本 | 来源 | 里程碑 |
|------|------|------|--------|
| `stb_truetype.h` | — | https://github.com/nothings/stb | M6（字形光栅化） |
| `stb_rect_pack.h` | — | https://github.com/nothings/stb | M6（atlas 打包，可先不启用） |
| `qrcodegen.{c,h}` | — | https://github.com/nayuki/QR-Code-generator（MIT） | M2（QR 生成） |

## M6 已入库（stb_truetype.h · 字体光栅化）

| 文件 | 版本 | 来源 | commit / 版本锁定 | 下载日期 | 用途 |
|------|------|------|-------------------|----------|------|
| `stb_truetype.h` | v1.26 | https://github.com/nothings/stb | `2c980bb59875b0d32144a71867fbdebb2f77cd20`（repo HEAD；与 M1 stb_image*.h 同 pin） | 2026-08-04 | TTF/OTF 字形光栅化 + 度量（`rt_font.c` 内 `STB_TRUETYPE_IMPLEMENTATION`） |

SHA-256（下载时核验）：

| 文件 | SHA-256 |
|------|---------|
| `stb_truetype.h` | `ECD30B05E0DD4FEA3A13C26810DD9E1992DC379049482C393D5A19E6B5090AAB` |

> **安全注记（对齐 RFC 036 §4 R3）**：stb_truetype.h 头部明确声明「NO SECURITY GUARANTEE — DO NOT USE THIS ON UNTRUSTED FONT FILES」——不做字体文件偏移范围检查。M6 仅消费**可信本地 TTF**（Font 从本地路径加载），不接入网络/不可信输入；fuzz 与解析加固后置。

> **许可注记**：stb_truetype.h 同为 **公有领域 + MIT 双许可**（全文见 `NOTICE` stb 段；stb_truetype.h 尾部许可声明与 stb_image*.h 相同）。

## 后置（M6 之后，先不下载）

| 文件 | 版本 | 来源 | 里程碑 |
|------|------|------|--------|
| `stb_rect_pack.h` | — | https://github.com/nothings/stb | M6 后置优化（字形 atlas 打包，RFC 036 §1.1 已注可先不启用） |

## Image 格式扩展已入库（nanosvg · SVG 光栅化 · 2026-08-19）

| 文件 | 版本 | 来源 | commit / 版本锁定 | 下载日期 | 用途 |
|------|------|------|-------------------|----------|------|
| `nanosvg.h` | master | https://github.com/memononen/nanosvg | `239e102ec2c691f2902e20ace2ed36ee4a35cfe6` | 2026-08-19 | SVG 解析（`rt_image.c` 内 `NANOSVG_IMPLEMENTATION`；文本/外部资源不支持见下方安全注记） |
| `nanosvgrast.h` | master | https://github.com/memononen/nanosvg | `239e102ec2c691f2902e20ace2ed36ee4a35cfe6` | 2026-08-19 | SVG 光栅化（`rt_image.c` 内 `NANOSVGRAST_IMPLEMENTATION`） |

SHA-256（下载时核验）：

| 文件 | SHA-256 |
|------|---------|
| `nanosvg.h` | `E34FD5D084BE106CEA972D19CE5D27FD96D17BA89F8D06BDCEEE058420C8B2B0` |
| `nanosvgrast.h` | `79A9C5F4DB19DEBF9F3A648A1589E96D92854F245A5CB4F3D823F263785234D8` |

> **能力边界（对齐 RFC 029 · Image 格式扩展）**：nanosvg 覆盖 SVG 路径/形状/基础变换/渐变，
> **不支持 `<text>` 文本与外部资源引用**（`<image>` 嵌入位图、`<use>` 外部引用）——UI Image
> 组件 SVG 展示以此为诚实边界；含文本 SVG 由作者自行预光栅化。
>
> **安全注记**：nanosvg 解析不对外部输入做完整 fuzz 加固（与 stb 同族单文件库的既有立场）。
> 首版仅消费**可信本地 SVG**（本地文件/打包资源），不接不可信网络输入；fuzz 后置。

## 更新纪律

上游安全更新 = 独立 PR（单目标）；更新后回填本表版本/commit/SHA-256。
**未跟进不得宣称「已更新」**（RFC 025 §1.1 宣称纪律）。

## M2 已入库（qrcodegen QR 生成底座 · 2026-08-04）

| 文件 | 版本 | 来源 | commit / 版本锁定 | 下载日期 | 用途 |
|------|------|------|-------------------|----------|------|
| `qrcodegen.c` | master | https://github.com/nayuki/QR-Code-generator | `8329a7108fc22be3e1eec0a9f9318978579e3621` | 2026-08-04 | QR 生成（确定性模块矩阵 · ECC L/M/Q/H · mask -1..7；`rt_qrcode.c` 封装，独立 TU 编译） |
| `qrcodegen.h` | master | https://github.com/nayuki/QR-Code-generator | `8329a7108fc22be3e1eec0a9f9318978579e3621` | 2026-08-04 | 公共头（qrcodegen_encodeText / getSize / getModule） |

SHA-256（下载时核验）：

| 文件 | SHA-256 |
|------|---------|
| `qrcodegen.c` | `6A2B9CC65176F2345DDE260C74B6D352627E8A0A6385D086AE0E9C5D0913C70C` |
| `qrcodegen.h` | `E82DF4BFF37D18B5863B9E7486FE6BDA1B6CDA8C3B9ECEBFEC473907265CB589` |

> **许可注记**：qrcodegen 为 **MIT**（Nayuki），全文见 `NOTICE` qrcodegen 段；与 M1 stb（公有领域 + MIT 双许可）、M4 quirc（ISC）同属宽松许可，可直接入库。

> **版本锁定注记**：本目录 qrcodegen.{c,h} 哈希与上游 `8329a7108f` commit 的 `c/qrcodegen.c`/`c/qrcodegen.h` **逐字节一致**（核验于 2026-08-04）。

## M5 外部依赖注记（zxing-cpp · 不 vendored）

| 项 | 值 |
|----|-----|
| 库 | zxing-cpp（Apache-2.0） |
| 形态 | **不 vendored、不进仓库**；`scripts/fetch-zxing-native.ps1` 外部下载/校验（SHA256 版本锁定）/编译 reader-only 共享库 → `target/zxing-native/`（工作区卫生 G″） |
| 桥接 | `shim/zxing_shim.cpp`（本目录桥接源码，`extern "C"` 包 `ZXing::ReadBarcodes`，导出单符号 `zxing_decode_c`；由 `fetch-zxing-native.ps1` 与 zxing-cpp 源码一起编入共享库） |
| 加载 | `.ani` 契约 `load="auto"` + `ARC_ZXING_LIB` 环境变量（RFC 036 §1.6 / RFC 034） |
| 许可 | Apache-2.0（与 zxing-cpp 一致；登记见 `NOTICE`） |
