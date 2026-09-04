# 015 覆盖率 · 引用子项：LLVM source-based coverage

> 本文件承载 [015 LLVM 原生后端(../../015-llvm-backend.md) 的**覆盖率能力子项**。015 主文档保留架构级表述；覆盖率插桩的机制、CLI、报告格式与 QIF 衔接细节下沉至此。**一子项一文档**；本文只补细节，不与 015 / 031 / 032 主文档重复既有决策。

## 背景

Arc 测试（QIF）无任何覆盖率设施——代码树 `coverage / covprof / gcov / lcov / cobertura` 零命中。std 量产需要覆盖门禁：断言「被测代码被真实执行」，而非仅「测试通过」。本子项定义覆盖率能力，补全 QIF 质量闭环缺失的一环。

覆盖率是**编译/运行层能力**（codegen 插桩 + 运行时计数回写），与 QIF（测试发现/执行）正交：QIF 负责「跑哪些测试」，覆盖率负责「跑完之后哪些行被触及」。`arc test --coverage` 是两者的串联点。

## 设计决策

### 1. 机制选择：LLVM source-based coverage（零自研插桩）

| 候选 | 裁决 | 理由 |
|------|------|------|
| LLVM source-based coverage | ✅ 采用 | AOT 语言天然选择：复用 015 的 clang 管线，`-fprofile-instr-generate -fcoverage-mapping` 编译插桩 + `-fprofile-instr-generate` 链接 profile runtime，零自研插桩 |
| gcov（`--coverage`） | ❌ 拒绝 | 依赖 gcov 运行时与 `.gcno`/`.gcda`，非 LLVM 系；优化下精度弱于 source-based |
| 自研插桩 | ❌ 拒绝 | 违背「无调用方的抽象 / 零自研」；LLVM 已提供完整机制 |

管线（对标 LLVM 官方 source-based coverage）：

```text
编译：  clang -fprofile-instr-generate -fcoverage-mapping   （仅 Arc 生成的 .ll → .o）
链接：  clang -fprofile-instr-generate                      （拉入 libclang_rt.profile）
运行：  测试二进制退出时写 .profraw（LLVM_PROFILE_FILE 控制落点，%p/%m 模式分流）
合并：  llvm-profdata merge -sparse *.profraw -o coverage.profdata
导出：  llvm-cov export -format=lcov -instr-profile=coverage.profdata <bin> > coverage.lcov
        llvm-cov show <bin> -instr-profile=coverage.profdata -format=html -output-dir=html/  （可选 HTML）
```

**插桩范围：仅 Arc 源码（生成的 `.ll`）**。runtime C（`rt_*.c` / `sqlite3.c` 等）不插桩——覆盖率门禁面向 Arc std/应用代码，不面向运行时 C。实现上 coverage 标志沿 `compile_module* → clang_compile` 注入 `.ll` 编译、沿 `clang_link` 注入链接；不触碰 runtime object 缓存。与 `sanitize_flag()` 的**全局**环境变量注入不同，coverage 是 **per-二进制、per-命令的显式能力**，非诊断旁路。

**实现锚点（现状先例）**：`optimize::clang_compile` / `clang_link` 已按 `debug_info: bool` 注入 `-g -gdwarf-5`，`sanitize_flag()` 注入 `-fsanitize=`；coverage 复用同一下沉模式，新增 `coverage: bool`（或 `CoverageMode`）贯穿 `arc test → compile_test_dispatch → codegen::compile_module* → clang_compile/clang_link`。增量指纹与 runtime object 缓存键须纳入 coverage 标志（插桩产物与非插桩产物不得互串，对齐 `sanitize_suffix` 纳入 runtime 缓存键的先例）。

### 2. CLI：`arc test --coverage`

| 选项 | 含义 | 默认 |
|------|------|------|
| `--coverage` | 启用覆盖率：编译插桩 → 运行产 `.profraw` → 汇总产 lcov 报告 | 关闭 |

流程（`arc test examples/UnitTest --coverage`）：

1. **编译**：向 `.ll` 编译与链接注入 coverage 标志（§1）；
2. **运行**：`LLVM_PROFILE_FILE=<coverage_dir>/<stem>-%p.profraw` 指向覆盖率产物目录（`%p`=PID，多进程各自分流；`%m`=二进制签名，跨二进制分流）；
3. **汇总**：`llvm-profdata merge -sparse` 聚合 → `llvm-cov export -format=lcov` 产 lcov 主报告；可选 `llvm-cov show -format=html` 产 HTML；
4. **报告落点**：`<qif.output>/coverage/`（缺省 `obj/qif/coverage/`），文件 `coverage.lcov` + `coverage.profdata`（+ 可选 `html/`）。

失败语义：`--coverage` 下插桩/汇总失败照常非零退出（与测试失败同等待遇），不静默降级为「无覆盖率报告」。

### 3. 报告格式

| 格式 | 地位 | 说明 |
|------|------|------|
| lcov（`coverage.lcov`） | **主格式**（CI 可消费、通用） | `llvm-cov export -format=lcov` 原生输出；供 Codecov / coveralls / GitLab 等直接消费 |
| HTML | 可选（开发者浏览） | `llvm-cov show -format=html` |
| cobertura | **可扩展**（非原生） | `llvm-cov` 不原生产 cobertura；经 lcov → cobertura 转换（如 `lcov-cobertura`）派生；列为后续扩展而非 v1 主路径 |

llvm-cov 非 gcov 感知：不认 gcov 的 `LCOV_EXCL` 语义，排除标记以 llvm-cov 自身的注释标记与 `-ignore-filename-regex` 为准（实现期以 LLVM 官方 SourceBasedCodeCoverage 文档为权威）。

### 4. 与 QIF 的关系（正交）

| 层 | 归属 | 职责 |
|----|------|------|
| 覆盖率（插桩/汇总/报告） | 015（codegen）+ 031（LLVM 工具链） | 「哪些行被执行」 |
| 测试（发现/执行/断言/跳过） | 032 QIF | 「跑哪些测试、是否通过」 |

两者正交：覆盖率不改变 QIF 的发现/执行语义；QIF 不感知插桩。`arc test --coverage` 是编译/运行层的串联——`arc test` 既触发 QIF 执行，又（可选）触发 coverage 插桩与汇总。覆盖门禁消费方（std 量产门禁 / DoD 线覆盖阈值）不在本文定义阈值，本文仅定义「能力与格式」。

### 5. 验收标准（行为/探针清单）

| # | 验收探针 | 判据 |
|---|---------|------|
| A1 | `arc test examples/UnitTest --coverage` | 编译注入 `-fprofile-instr-generate -fcoverage-mapping`（compile）与 `-fprofile-instr-generate`（link），测试正常运行，退出码与不带 `--coverage` 一致 |
| A2 | 运行后产物 | `<qif.output>/coverage/coverage.profdata` 与 `coverage.lcov` 生成；无插桩失败静默 |
| A3 | lcov 内容 | `coverage.lcov` 含 `SF:`（源文件）/`DA:`（行命中）/`LF:`/`LH:` 记录；命中行与 `[Fact]`/`[Theory]` 实际覆盖一致 |
| A4 | 未覆盖行 | 存在未执行分支时对应行 `DA:<line>,0`（零命中），可被 CI 门禁消费 |
| A5 | 非目标隔离 | runtime C（`rt_*.c` / `sqlite3.c`）不出现在 `SF:` 列表；仅 Arc 源码被插桩 |
| A6 | 增量隔离 | 带/不带 `--coverage` 产物不互串（指纹与缓存键含 coverage 标志）；`--no-build` 复用插桩二进制语义正确 |
| A7 | 工具链 | `llvm-profdata`/`llvm-cov` 从与 clang 同一 LLVM 安装解析；`arc doctor` 可检测缺失并给出修复提示 |
| A8 | 无标志回归 | 不带 `--coverage` 的 `arc test`/`arc build` 零新增 flag、零 `.profraw` 产物（门禁无感） |

### 6. 非目标

- **不承诺 branch coverage 精确度**：v1 以行覆盖（line coverage）为准；分支/区域（region）覆盖率不承诺数值精度，仅由 llvm-cov 原生能力透出。
- **不承诺 MC/DC**：修改条件/判定覆盖（`-fcoverage-mcdc`）不在本文面内。
- **不定义覆盖阈值门禁**：std 量产「X% 线覆盖」阈值与 DoD 接入由消费方（成熟度/DoD）另立，本文只交付能力与格式。
- **非测试二进制覆盖**：`arc run` / `arc build` 任意二进制手动运行产 profraw 不在 v1（覆盖门禁面向 `arc test`）。
- **cobertura 原生输出**：v1 不产 cobertura，仅留 lcov → cobertura 扩展位。

## 待拍板决策点

| # | 决策点 | 影响 | 建议 |
|---|--------|------|------|
| D1 | 落点归属：本文归 015 references（机制）是否成立 | 若认为覆盖率应归 032 QIF 而非 015，需迁移 | 按「机制归 015、用户面/关系归 032、工具链归 031」三角分布 |
| D2 | `llvm-profdata`/`llvm-cov` 进入工具链解析与瘦身捆绑（031 §13.2 现仅 clang 族 + lld + llvm-rc/ar/ranlib） | `--coverage` 在无 llvm-profdata/llvm-cov 的 SDK 上不可用 | 纳入 `arc toolchain`/`arc doctor` 解析序与 `arc-pack.ps1 -BundleLlm` 清单 |
| D3 | 报告默认格式 v1 仅 lcov（cobertura 后置） | 若 CI 强依赖 cobertura，需前置 lcov→cobertura 转换 | 维持 lcov 主、cobertura 扩展（本文立场） |
| D4 | coverage 与 `-c Release`（`-O3` + ThinLTO）的兼容 | 优化下 source-based coverage 行归属可能偏移 | v1 建议 coverage 仅 `-c Debug`（默认），Release 覆盖率后置 |

## 边界

- 本文只定义**覆盖率机制、CLI、报告格式、QIF 衔接**；codegen 管线编排见 [013(../../013-compiler-pipeline.md)，LLVM 后端正文见 [015(../../015-llvm-backend.md)，`arc test` 用户面见 [032(../../032-qif.md)，LLVM 工具链解析/捆绑见 [031(../../031-compiler-cli.md) §12/§13。
- 覆盖阈值门禁、DoD 接入、std 量产阈值由消费方另立，不在本文面内。
- gcov / 自研插桩 / MC/DC 为拒绝项，见 §1 / §6。

---

[返回 015 主题入口(../../015-llvm-backend.md) · [返回 015 references 索引](index.md) · [返回 RFC 索引](../../index.md)
