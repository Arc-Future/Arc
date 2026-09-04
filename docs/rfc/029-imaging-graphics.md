# RFC 029 图像与图形

## 背景

图像编解码、图形绘制、二维码与条形码以 `std/Drawing`（`Arc.Drawing`）独立领域子库交付。设计目标：统一命名空间、C# `System.Drawing` 参照 + 模块对称（Writer↔Reader）、C 生态直连（vendored 单文件底座）、编译期裁剪保证产物干净。与 UI 帧内即时绘制面（`Arc.UI.Rendering` 的 DrawList IR）物理路径/命名空间双区分、不混淆。

## 设计决策

### 类型面（`Arc.Drawing`）

| 类型 | 说明 |
|------|------|
| `RgbColor` | ARGB 四通道 + `FromArgb`/`ToArgb`；不引入 141 色命名色常量表/HSL/HSV 转换 |
| `PixelFormat` | 最小集 `Rgba32`（8-bit 每通道直通 alpha）；Rgb24/Gray8 不在本设计面内 |
| `ImageFormat` | `Png`/`Jpeg`（优先）；`Bmp`/`Tga`（草稿） |
| `Bitmap` | `(width, height)`/`(width, height, format)`；`Width`/`Height`/`PixelFormat`/`GetPixel`/`SetPixel`/`Save(path)`/`Save(stream, format)`/`DrawLine`/`DrawFillRect`/`DrawText`/`GetPixels`（原生 RGBA8 句柄，供 UI 上传 GPU）/`Dispose` |
| `AnimatedGif` | GIF 多帧产物：`Width`/`Height`/`FrameCount`/`DelayMs(i)`/`Frame(i)`（第 i 帧 RGBA8 像素句柄，零拷贝）；`Dispose` |
| `ImageDecoder` | 静态解码门面：`Decode(string path)`/`Decode(byte[] data)`（PNG/JPEG/BMP/GIF 自动探测）；`DecodeGif`→`AnimatedGif`（多帧+每帧延时）；`DecodeSvg(data, scale)`→`Bitmap`（nanosvg 光栅化）；`IsGif`/`IsSvg` 魔数探测（UI Image 组件 Source 路由用） |
| `Font` | `(ttfPath, size)`；`LineHeight`/`MeasureTextWidth`/`Dispose`（stb_truetype） |
| `QrCodeWriter` | `Encode(text)`/`Encode(text, ecc)`/`Encode(text, ecc, mask)` → `Bitmap` |
| `QrCodeReader` | `Read(Bitmap)` → 载荷文本（quirc 静态内置） |
| `QrCodeErrorCorrection` | `L`/`M`/`Q`/`H` |
| `BarcodeWriter` | `EncodeEan13`/`EncodeCode39`/`EncodeCode128` → `Bitmap`（1D 图案表纯 Arc） |
| `BarcodeReader` | `Read(Bitmap)` → 文本（原生 EAN-13/Code39/Code128 主路径 + zxing 可选增强兜底）；`IsZxingAvailable` |

```as
using Arc.Drawing;

var bmp = QrCodeWriter.Encode("https://example.com", QrCodeErrorCorrection.M);
bmp.Save("qr.png");
string text = QrCodeReader.Read(bmp);
```

**设计决策**：

- **模块对称**：QR 与 1D 条码各持 `Writer`（生成）↔ `Reader`（解码）配对，命名同构、返回面一致；每模块解码唯一入口 `Read`，禁双轨（不设 `QrCode.Decode`，各读各自域）。
- **单一惯用法**：`GetPixel`/`SetPixel` 为像素访问唯一入口（`BitmapData`/lock-bits 双轨拒绝）；`Save` 为编码唯一入口。
- **去糟粕**：绘制 = 直线/实心矩形/文本三点（Graphics 精华子集）；不引入椭圆/路径/`Pen`/`Brush` 体系。
- **显式失败**：未检出/无码/解码失败抛显式异常（`BarcodeNotFoundException`），禁静默 0/null。
- **解码机制**：quirc 静态内置（QR）+ 原生 1D 静态内置（EAN-13/Code39/Code128）为**必装路径**；zxing-cpp 为 `.ani` `load="auto"` 可选增强兜底（`Native.IsAvailable` 门闩降级）——未装库时原生路径仍可用、不崩溃。
- **编译期裁剪 · 产物干净**：std 门面经 reachability 触发、runtime C 层 section GC + 能力宏 + TU 拆解、链接期 gc-sections——只用到的部分进产物、未用到的部分不进。
- 相机照片/旋转解码为 zxing 可选增强面；SVG 光栅化（nanosvg）纳入设计面（供 UI Image 多格式展示）；PDF/视频流不在本设计面内。

### 底座与 ABI

- vendored 单文件底座收敛于 `crates/runtime-drawing/`（stb_image/stb_image_write/stb_truetype/stb_rect_pack 公有领域 + qrcodegen MIT + quirc ISC），纯 C 资源目录、非 Rust crate；`VENDOR.md`/`NOTICE` 登记署名。
- 新 ABI 延续 `rt_image_*`/`rt_qrcode_*`/`rt_barcode_*` 前缀；`byte[]` 工作载体经 `RtArrayHeader` 前置头。
- 非 Skip e2e + 测试向量验证（qrcodegen 标准向量、EAN-13 校验位、像素覆盖断言、字体度量 sanity）。

## 边界

- 本文档讲 `Arc.Drawing` 图像处理/条码/绘制面；渲染后端（wgpu、`Arc.UI.Rendering` DrawList IR）见 [037 UI 声明式框架](037-ui.md)。
- **命名冲突**：`Arc.Drawing` 解码门面命名为 `ImageDecoder`（而非 `Image`）——类型注册表以**短名**为键（external 注册「本地优先跳过」），若与 UI 图像控件 `Arc.UI.Components.Image` 同名，消费者侧（UI 库）将无法解析 `Arc.Drawing.Image`。以职责命名消除同名冲突（UI 组件经 `ImageDecoder.IsGif/DecodeGif/DecodeSvg/Decode` 路由解码）。同类：像素色 `Color` 亦与 `Arc.UI.Media.Color` 短键冲突（037 颜色分轨），故命名 `RgbColor`（byte ARGB 像素色，`Arc.Drawing` 图像 ABI 唯一类型）。
- 二维码/条码的编码语义与 protobuf 等二进制编解码无关（见 [030](030-protobuf.md)）。

---

上一节：[028 类型反射面](028-type-reflection.md) · 下一节：[030 Protobuf 二进制序列化](030-protobuf.md)