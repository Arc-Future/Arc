# packaging/ — SDK 安装包与安装器

与 [../release/README.md](../release/README.md) 配套：本目录负责**打包装包 / 本机安装**；发 GitHub Release 走 `scripts/release/github-release.ps1`。

版本号取自 `arc --version`（与 Cargo `workspace.package.version` 一致；现行 **0.1.0**）。

## Windows（zip）

```powershell
# 仓库根；先编发布编译器
cargo build --release -p arc

# 打安装包：捆绑 LLVM + 签名清单 + 冒烟验收
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -BundleLlm -Manifest

# 产物默认落 target\dist\：
#   arc-0.1.0-x86_64-pc-windows-msvc.zip (+ .sha256)
#   manifest.json / manifest.json.sig
```

就地安装（解压后在 SDK 根）：

```powershell
.\install.ps1
# 或从仓库脚本：
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\install.ps1 -Archive target\dist\arc-0.1.0-x86_64-pc-windows-msvc.zip
```

## Unix（tar.xz · Linux/macOS）

须在 **Linux/macOS 宿主**实跑（Windows 上可编交叉逻辑，但产线验收以 Unix 宿主为准）：

```bash
cargo build --release -p arc
pwsh -NoProfile -File scripts/packaging/arc-pack.ps1 -BundleLlm -Manifest
```

安装：

```bash
sh scripts/packaging/arc-install.sh --from-dir <解压出的 sdk 根>
# 或 --url <tar.xz 的 HTTPS URL>（附 .sha256）
```

安装协议 harness：`scripts/packaging/verify-arc-install.sh`（WSL 可复跑）。

## 发版衔接

1. 各宿主 `arc-pack.ps1` 产物 + `.sha256` 汇入同一 `target/dist`
2. `scripts/release/github-release.ps1 -Version 0.1.0`（重签 manifest 为 GitHub download URL 并上传）
3. 消费端：`ARC_RELEASE_BASE=https://github.com/Arc-Future/Arc/releases/download/v0.1.0`  
   （`https://static.arc.dev/dist` 仍为占位，未通前勿写进安装文档当官方端点）

签名密钥：`$env:ARC_RELEASE_SIGNING_KEY` 或 `~/.arc/keys/release-signing-key-<版本>.txt`（离线，永不提交）。
