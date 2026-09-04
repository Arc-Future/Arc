# Arc.UI 拟真引擎

## 概述

`Arc.UI 拟真引擎`是 Arc.UI 体系下的一项能力规划，其定位是 **AI 时代信息化呈现模型（AI-era information-presentation model）**。

> AI 时代的信息化，不再交付静态报告/表格/2D 图表，而是**由 AI 实时生成信息的 3D 表达**（数据 → 场景 → 可交互呈现）。这要求系统架构比今天更高阶——不是 Blender / World Machine / Unity 之间搬数据，而是把「描述→生成→呈现→审视」收敛到**同一高集成运行时**。
>
> 本拟真引擎正是这套呈现模型在「3D 呈现与生成」一侧的落地：程序化产出「有机自然」高保真 3D 模型，既能在 Arc 内以承载型实时渲染，也能导出为标准资产文件。

### 呈现闭环（本引擎的四段）

```
描述（信息/语义模型）──生成（程序化 / AI）──呈现（实时渲染）
          ↑                                        │
          └──────── 审视/反馈（离屏回读校验）──────┘
```

为满足「更高效、集成度更高」，引擎强调**单一运行时的闭环**，而非跨工具数据搬运。

### 在 Arc AI-native 叙事中的角色

Arc 已具备该闭环的多个环节，本引擎补上「生成 + 3D 呈现」两块以贯通整个闭环：

| 闭环段 | Arc 已有 / 本引擎提供 |
|--------|----------------------|
| 描述 | `Arc.UI` 声明式 + MVVM 数据绑定：信息模型→UI 单向正道，无需搬数据 |
| 生成 | 本引擎生成层——程序化，未来可接 AI |
| 呈现 | 本引擎承载渲染层 + wgpu 唯一后端（AOT 机器码，性能贴近原生） |
| 审视/反馈 | 离屏回读 `RenderCapture`（[ai-native-render-capture](../rfc/037-ui/references/ai-native-render-capture.md)）+ [fidelity-loop](../rfc/037-ui/references/ai-native-fidelity-loop.md)：headless 可测，供 LLM 多模态校验，闭环自动化（与 [RFC 034 AI 原生工具链](../rfc/034-ai-toolchain-arcgr.md) 同构） |

### 与技术现状的关系

`Arc.UI` 现阶段是 WPF 对齐的 **2D 声明式框架**，渲染后端唯对接 wgpu（immediate-mode DrawList 合成器）。本拟真引擎**不是在渲染后端动刀**，而是在承载型基础上新增三类能力域：**生成层 · 资产层 · 承载渲染层**。

#### 关键认知

- **后端不是天花板**：wgpu 是完整可编程 GPU 管线（vertex/fragment/compute + 纹理 + 离屏目标 + 后处理），承载型链路复用既有离屏 target 与纹理合成通道，能力足够跑到现代实时引擎同档视觉。
- **瓶颈在生成质量与调色投入**，不在渲染器。——决定"逼真"的是光照模型选得对不对、材质/纹理够不够细、程序化细节够不够密、后处理搭没搭对。
- **有机自然是程序化拟真最有利的地带**：地形、山体、植被、云、海、天空均可数学建模；空气透视本身就是大型自然场景"真实感"的主要来源，纯程序可算得很对，比机械件/人物好做得多。

## 分层架构

```
┌─ 生成层（Procedural）────────────────────────────┐
│  地形：FBM 噪声 → heightfield / marching cubes     │
│  植被：参数化植物 / L-system / 实例化              │
│  有机体：细分曲面(Catmull-Clark) / metaballs       │
│  输出：PolyMesh（顶点-法线-UV-切线）+ 材质         │
└──────────────┬───────────────────────────────────┘
┌─ 资产层（3D 数据模型 + 序列化）────────────────────┐
│  MeshBuilder · PbrMaterial(albedo/normal/roughness/metallic) │
│  导出：ObjWriter · GltfWriter（含 scene graph/buffer） │
└──────────────┬───────────────────────────────────┘
┌─ 承载渲染层（Arc.UI 承载型，复用既有通道）──────────┐
│  SceneView 控件 → 离屏 target 渲染 → 纹理合成进 UI 帧 │
│  光照 + PBR 材质 + 空气透视 + 后处理               │
└─────────────────────────────────────────────────┘
```

### 与现有代码的复用与缺口

**已存在、直接复用：**
- 离屏 target + 纹理合成通道（`IRender.CreateOffscreenTarget/RenderToOffscreen/ReadbackPixels`，见 [ai-native-render-capture](../rfc/037-ui/references/ai-native-render-capture.md)）。
- 唯一 wgpu 后端 + `arc-ui 规则`「禁降级、唯一对接 wgpu」。

**仓库暂缺、属于新地基（须在规范/RFC 层级立项）：**
- 3D 数学：`Vec3/Mat4/Quat` 矩阵栈（`std/` 现仅 2D `Color`/`Thickness`）。
- 噪声与过程生成：fractal noise、simplex、L-system。
- 3D 资产模型与序列化：PolyMesh、PbrMaterial、OBJ/GLTF writer。
- 3D 内容渲染器：camera、mesh 入 wgpu、PBR/阴影/后处理 shader。

> 约束：以上均为**新领域能力**，按 RFC 036「基础面冻结、新能力须立项」流程办理，不得静默改动 `std/Arc` Stable 稳定面。

## 程序化生成算法选型（有机自然）

| 对象 | 算法 | 保真要点 |
|------|------|----------|
| 地形 | FBM 噪声（Simplex 多倍频）→ heightfield；洞穴用 marching cubes | 分形细节 + 法线贴图 + 空气透视 |
| 植被 | 参数化植物（主茎/分支/叶）+ 实例化 | 冠层自遮挡、叶片 alpha-cut、风场摆动 |
| 有机体/菌类/水体 | metaballs / 细分曲面 + 次表面散射 | 生物柔软感靠 SSS 材质，非仅几何 |
| 草/灌木 | 大规模实例化（GPU instancing） | 高密度而不爆顶点 |

## 高保真视觉路径（收容在承载渲染层 shader）

- PBR 材质（albedo/normal/roughness/metallic）+ 方向光/环境光。
- **空气透视雾效**（大型自然场景真实感主要来源）。
- 阴影 / 环境光遮蔽（AO）。
- 后处理（tone mapping）。

### 逼真档位轴（诚实边界，不打包票）

| 档 | 视觉 | 靠什么达到 | 难度 |
|----|------|-----------|------|
| 1 | 卡通/示意 | 平光 + 基础材质 | 低 |
| 2 | 氛围真实 | PBR + 单方向光 + 雾 | 中 |
| 3 | 明显真实 | 进阶光照 + 高细噪声贴图 + 后处理 | 较高 |
| 4 | 照片级（近看难辨） | 置换 + SSS + 大气散射 + 烘焙/GI，常需 AI 补细节 | 高 |

- 程序化可**稳定达到第 3 档**；第 4 档取决于投入，且纯程序化噪声在近景微细节上会有"合成感"，要稳定到照片级通常需**程序化 + AI 双轨**（AI 补材质/细节残差）。
- 实时光栅 GI 有限，照片级 GI 需烘焙或未来上光追（wgpu 支持光追扩展，属后话）。
- 现实定位：**程序化先打到第 3 档「高保真自然可信」**，第 4 档把 AI 补材质/细节作为可选增强接入——与仓库 `ai-native-*`（RenderCapture 为"眼睛"）方向咬合。

## P0 通道探针 · 实测结论（2026-08-23）

> 立项决策的实证来源：`crates/arc-integration/tests/wgpu_3d_offscreen_probe_e2e.rs`
> （该探针宿主已随 arc-integration 退场，a2627a0f；**探针实测数据保留于本节作为历史记录**，能力结论径由本节留存）。

**结论：真实 3D 通道成立 —— 零后端改动可跑通 headless 3D 渲染 + 像素回读 + 立体性量化判据。**

探针以**全程序化 WGSL**（`@builtin(vertex_index)` 生成两个不同视深 near/far 透视矩形，顶点着色器内完成透视除法）走既有 wgpu ABI 全链路，未触碰 `rt_wgpu_native.c` 与 2D 合成器。实测：

```
wgpu3d_metrics blue=930 bw=30 amber=176 aw=16
wgpu3d_pass
```

| 判据 | 含义 | 实测 |
|------|------|------|
| C1 非空 | 回读存在彩色像素 | 近(蓝)+远(琥珀)=1106 px ✓ |
| C2 非平 | 两个不同深度面各 >20 px | 930 / 176 ✓ |
| C3 透视 | 近面跨距 > 远面跨距（近大远小） | 30 > 16 ✓ |
| C4 闭环 | create→pass→draw→submit→readback 全成功 | ✓ |

C3 的 30:16≈1.9 与两矩形视深比 `3.5/1.8≈1.94` 高度吻合——证明**透视投影数值正确**，并非凑巧输出。

**通道已验证的能力面**（零后端改动）：WGSL 3D 顶点/片元着色器编译 ✓ · 离屏（headless 无窗）渲染 ✓ · 透视投影近大远小 ✓ · GPU→CPU 像素回读 ✓ · Arc 侧 (`public Bitmap`) 逐像素量化判定 ✓。

**顺带修复的既有缺陷**：`wgpu_offscreen_readback` 原硬编码 `bytesPerRow = width*4`，
未按 `COPY_BYTES_PER_ROW_ALIGNMENT`(256) 对齐，非 64 倍宽的离屏目标回读会触发 wgpu
校验错误。已修复于 `rt_wgpu_native.c`：行宽补齐到 256、回读缓冲按对齐行分配、逐行剔除
padding——探针在非对齐宽度（96）下实测通过，任意宽度均可回读。修复属 RFC 036 流程项，
随本里程碑一并合入。

**以此通过立项门槛**：渲染器非天花板，承载型 3D 路径可行；后续按里程碑 P0–P4 推进，保真度由生成/调色投入决定（见「逼真档位轴」，程序化先打到第 3 档）。

## 里程碑与验收

| 步 | 内容 | 验收 |
|----|------|------|
| P0 | 3D 数学基础（Vec/Mat/Quat）+ `SceneView` 把离屏 3D target 合进 UI 树 | 立方体+相机可见、帧率达标 |
| P1 | 生成层地形：FBM heightfield → PolyMesh → 导出 OBJ + 运行时渲染 | 同一座山在 Blender 打开与 Arc 内一致 |
| P2 | 植被实例化 + PBR 材质 + 光照 | 参数化森林，导出 GLTF 含材质 |
| P3 | 空气透视 + 阴影/AO + 后处理，有机自然大场景 | 相对 P1 视觉显著提升、显存受控 |
| P4 | 有机体（细分曲面/metaballs）+ SSS | 柔软生物体可导出可用 |

## 边界

- **渲染后端**：本分册不扩展 2D 合成器；3D 内容以承载型（离屏 target + 纹理合成）落入 Arc.UI，与 `VideoSurface`/`texture-surface` 同一惯用法。
- **命名空间归属**：拟真引擎为独立内容/能力域，其命名空间层级遵循「基类在根、派生在子」原则，后续按 RFC 立项时确定（如 `Arc.UI.Rendering` 下挂抽象，`Arc.Procedural` 内容生成）。
- **不承诺**：纯程序化零外部资产达到照片级不可辨；第 4 档需 AI 双轨增强。
- **文档分工**：本分册讲技术规划与能力面；各 API 的精确契约在对应 RFC/规范层定义。