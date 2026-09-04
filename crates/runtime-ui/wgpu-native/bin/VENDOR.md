# RFC 026 §D7.2: wgpu-native 预编译二进制 vendoring 目录

本目录存放 wgpu-native 的平台特定预编译二进制文件，供代码生成阶段链接。

## 来源

wgpu-native v29.0.1.1（https://github.com/gfx-rs/wgpu-native/releases/tag/v29.0.1.1）

## 子目录结构（按平台）

```
bin/windows/                  # Windows x86_64
├── wgpu_native.dll           # 运行时 DLL（链接后自动复制到 exe 同目录）
├── libwgpu_native.dll.a      # MinGW 静态导入库（供 Clang 链接用）
└── wgpu_native.lib           # MSVC 导入库（备用）

bin/linux/                    # Linux x86_64
├── libwgpu_native.so         # 运行时共享库
└── libwgpu_native.so.29      # SONAME 版本化链接

bin/macos/                    # macOS (M3+ 阶段)
├── libwgpu_native.dylib
└── libwgpu_native.a
```

## 手动 vendoring 步骤

1. 从 GitHub Releases 下载对应平台的 tarball
2. 解压到 `bin/<os>/` 目录
3. Windows: 重命名 `wgpu_native.dll.a` → `libwgpu_native.dll.a`（Clang MinGW 惯例）

## 自动 vendoring（CI 脚本，待实现）

```powershell
# Windows
Invoke-WebRequest -Uri "https://github.com/gfx-rs/wgpu-native/releases/download/v29.0.1.1/wgpu-windows-x86_64-release.zip" -OutFile wgpu.zip
Expand-Archive wgpu.zip -DestinationPath bin/windows/
```

## 运行时要求

- Windows: `wgpu_native.dll` 必须在可执行文件同目录（codegen 自动复制）
- Linux: `libwgpu_native.so.29` 需在 `LD_LIBRARY_PATH` 或可执行文件同目录
- macOS: M3+ 阶段实现

## 头文件

跨平台头文件（已 vendoring）：
- `include/webgpu.h` - WebGPU C API
- `include/wgpu.h` - wgpu-native 扩展 API
