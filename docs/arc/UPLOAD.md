# 上传与发布

本目录 `docs/arc/` 按 **rust-webx / Docbit** 规范维护，是官方文档站的**可直接拷贝上传单元**。

## 上传哪一层？

将仓库中的 **`docs/arc/`** 整目录拷到服务端：

```
<app_base>/docs/arc/
```

拷贝后应出现：

```
docs/arc/FOREWORD.md
docs/arc/INDEX.md
docs/arc/INDEX.json
docs/arc/logo.png
docs/arc/01-introduction/...
docs/arc/02-quickstart/...
…
```

**不要**上传整个 `docs/`（其中含 `rfc/`、`plan.md`、reviews 等非发布材料）。

Docbit API 约定（对标 rust-webx）：

- `GET /api/docs/arc/index`
- `GET /api/docs/arc/content/{path}`

## 规范要点

| 项 | 要求 |
|----|------|
| 产品目录名 = slug | `arc` |
| 根文件 | `FOREWORD.md`、`INDEX.md`、`INDEX.json`（必需） |
| 章节 | `NN-slug/INDEX.md` + `NN-slug/{sectionId}.md` |
| 受众 | Arc **语言使用者**（写应用），不是 RFC 读者 / 编译器实现者 |
| logo | `logo.png`（可换品牌图） |

## 与仓库其它文档的关系

| 路径 | 角色 | 是否上传 |
|------|------|----------|
| `docs/arc/` | 官方开发者手册（本包） | ✅ |
| `docs/rfc/` | 设计决策权威 | ❌（进阶章可链到仓库） |
| `docs/preface.md`、`docs/manifesto.md` | 旧书签单页跳转 | ❌ |
| `docs/plan.md`、`docs/reviews/` | 内部过程资产 | ❌ |

上传后以本目录 `INDEX.json` / `INDEX.md` 为阅读入口；语义冲突以仓库已接受 RFC 裁决。
