# release/ — 双远端发布工具链

| 远端 | 角色 | 地址 |
|------|------|------|
| GitCode | **内部开发仓**：完整历史 + 内部过程文档（plan / discuss / reviews / proposals） | `https://gitcode.com/rf2026/dlang.git` |
| GitHub | **公开仓**：净导出快照（无内部资产），对外发布与 issue/PR | `https://github.com/Arc-Future/Arc` |

单向流动：`内部仓 → github-export / github-sync → 公开仓`。公开仓的内容永远是内部仓 git 跟踪文件按 [export-exclusions.txt](export-exclusions.txt) 排除后的镜像，反向（公开 → 内部）不存在。

## 日常流（GitCode）

开发与提交都在内部仓进行：

```powershell
git add -A; git commit -m "feat(...)：..."; git push origin main
# 或使用现成的原子提交+推送脚本：
scripts\git-sync.ps1 -Message "feat(...)：..."
```

## 同步公开仓（增量，日常可跑）

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-sync.ps1
# 带自定义提交消息：
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-sync.ps1 -Message "sync: ..."
```

- 仅镜像 git **跟踪**文件（本地产物 / IDE 笔记天然不外泄），按 [export-exclusions.txt](export-exclusions.txt) 排除内部资产；
- 公开仓侧为**普通 fast-forward 提交**（不改写历史），无变更时自动空操作；
- 内部删除的跟踪文件会在公开仓同步移除（孤儿清理）。

## 发版流（GitHub Release）

```powershell
# 1. 构建发布版编译器
cargo build --release -p arc
# 2. 打安装包：容器随宿主——Windows zip（-BundleLlm 捆绑 LLVM + -Manifest 签名清单 + 验收）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\packaging\arc-pack.ps1 -BundleLlm -Manifest
# 3. 多平台收口：把各宿主包（Windows zip / Linux·macOS tar.xz）+ .sha256 汇入同一
#    DistDir，单次发布（github-release.ps1 自动发现全部包、单次重签多 triple manifest、上传全部资产）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-release.ps1 -Version 1.0.0
# 4. 同步脚本/文档变更到公开仓
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-sync.ps1
```

Unix 宿主打 tar.xz 包（Linux/macOS，pwsh core；先 `cargo build --release -p arc`）：

```bash
pwsh -NoProfile -File scripts/packaging/arc-pack.ps1 -BundleLlm
```

`github-release.ps1` 的签名密钥解析顺序：`$env:ARC_RELEASE_SIGNING_KEY` → `~/.arc/keys/release-signing-key-<版本>.txt`（离线文件，**永不提交**）。

## 首次引导 / 重建公开仓

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release\github-export.ps1 -InitGit
```

生成净根提交（作者默认 `LUSIDA (Start) <209404271+lusida2026@users.noreply.github.com>`）并注册 origin；随后的同步走 [github-sync.ps1](github-sync.ps1) 增量模式。重建属**改写公开仓历史**（force push），仅限公开仓刚建立或内容审计后整体重置时使用。

## 签名密钥

- 位置：`~/.arc/keys/release-signing-key-<版本>.txt`（离线托管；**泄露即轮换**）
- 轮换流程：`arc release keygen` → 将新公钥替换 `crates/arc/src/release.rs::RELEASE_PUBLIC_KEY_HEX` → 重建发布 → `arc release verify` 复验；旧版本安装器仍以旧锚验签，属预期（锚随编译器二进制走）
