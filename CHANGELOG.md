# Changelog

本文件按日期分节记录仓库重要变更（格式参考 Keep a Changelog；括号内为对应提交哈希）。更细粒度登记见 实现规划。

## 2026-09-05

### raw/λ 路径 string→object 实参装箱对齐 + Object 槽 ARC 取舍登记（待设计裁决）
- **修复（一致性/正确性，A/B 两选项下均成立）**：raw/λ（模板克隆体 raw 重降级）调用点缺 typeck 的 `Expr::Box` 插入——`object`/`object?` 形参直收 rodata/堆裸串（无 ArcHeader），按对象消费（unbox vtable 判别）即错、未来参与计数即写爆只读段。新增 `maybe_box_string_to_object`（`crates/mir/src/lower/lower_call.rs`，接线 `method_call_rvalue_with_prep` 实参物化环）：形参为 object/object?、实参静态类型 string、且非既有 Box/Unbox 节点时补 `MirRvalue::Box`（codegen `rt_string_box`，null 保留）——与 typed 路径契约对齐。验证：chord corpus 漂移分布与基线一致（25/25/14 抽样，无回归）；mir/codegen/typeck 单测全绿；clippy 零告警
- **Object 槽 ARC 计数：A/B 取舍登记（未裁决，默认 B）**：选项 A（装箱补齐后 `arc_class_place(Object)=true`）实测 over-dec 归零、corpus 确定性到 idx24；选项 B（object 槽持借用、不计数，性能语义优先）。A 使 object 流量付 inc/dec——是否违背「无装箱」初衷的讨论见仓库往来（值类型零装箱路径未动；string→object 盒为表示层必需 ABI，typed 路径 1.0 即如此）。裁决前保持 B（emit_cfg.rs 注释含两选项依据）；裁决入口：RFC 004 §值类型视图 / 性能语义章节
- **安装态验收闭环复证（Windows 本机、当前树）**：arc-pack（release+BundleLlm，内置 std-consuming 自检）→ install.ps1 就地安装沙箱根 → doctor 10 pass/0 fail（含捆绑 clang 22.1.8、MSVC、native DLLs）→ 隔离环境（PATH=安装根 bin、ARC_HOME 干净、无 ARC_CLANG/ARC_STD_ROOT/ARC_SDK_ROOT）经**指针** bin\arc.exe：StdApp（无依赖声明、`using Arc.Collections` 自动索引）离线构建运行 `stdapp:ok:alpha,beta:3`；MyExt（kind=library + Arc path→安装 SDK lib/std/Arc）`--dynamic` 产出 MyExt.dll；MyApp（path 依赖 + std 自动索引）构建运行 `myapp:ok:2:hello arc!:hello arc?`。质量门：workspace 127 批 + doc-tests exit 0 + clippy 零告警 + fmt clean
- **过程提示**：arc.toml `kind` 合法值为 `binary`/`library`（`app` 解析失败会被 find_arc_manifest 静默吞掉伪装成 "no arc.toml"——建议后续把 load 错误透出为诊断）


### 仓库拓扑核对 + Object 计数实验否决（raw string 异构槽位取证）——权威远程对齐（Arc-Future/Arc）
- **仓库拓扑（本轮核对）**：本仓双远程——`origin` = gitcode.com/rf2026/dlang（内部全量历史），权威公开仓库 = **github.com/Arc-Future/Arc**（`5b5026aa` 1.0 公开首发 + `2c5fe0c2` "sync: internal snapshot 2026-09-04 18:48" 两提交，经 `scripts/release/export-exclusions.txt` 剔除 docs/plan、docs/reviews、docs/rfc/proposals 等内部件做增量镜像）。此前各轮提交只推 gitcode，github 未见进展——已补 `github` remote，发布工具 = `scripts/release/github-sync.ps1`（正常快进提交、无历史改写），本节点起每次提交双推并定期跑 sync
- **Object 槽位 ARC 计数实验（99db6940 提交后取证否决）**：typed-inject over-dec 提示「object? 字段（ServiceEntry._instance）不计 ARC → 注册表持 borrow、typed 路径计数后提前释放」，尝试 `arc_class_place(Object/Nullable{Object}) = true`：over-dec 归零、corpus 推进到 idx0–21 确定性全绿（漂移消失）——**但** Reload/λ 路径新崩 0xC0000005：VEH 转储（`%TEMP%\arc_crash.txt`，内置 arc_dbg_veh）定位 `rt_arc_inc` 内 `lock xadd (%rcx)` 写**代码/只读段**（DATA_ADDR=exe .text+0x4f7 类），即对 **raw string 字面量指针**做 inc——**object 槽位异构**：raw/λ（模板克隆体 raw 重降级）路径缺 typeck 的 string→object Box 插入，`ctx.Provide("svc","v2")` 类调用点把 rodata 裸串直存 object? 槽；无 ArcHeader，计数即写爆字符串内容。结论：**Object 槽位不纳入 ARC 是既有设计约束**，计数前须先根治「raw 路径 string→object 装箱缺失」
- **处置**：回退 Object 计数改动（emit_cfg.rs 恢复原语义并注释原因）；移除临时 per-pointer ring 诊断（rt_arc.c 还原）；corpus 回归 = 上轮已验证态（idx0–21 稳定 + 22+ 漂移）。**根治路径登记**：① 补 raw/λ 调用点 string→object 装箱（对齐 typed 路径 `MirRvalue::Box`/rt_string_box）→ ② object 槽位同质化后重新评估纳入 ARC → ③ chord Reload 断言语义核对（tone 子上下文服务对祖先不可见 vs 测试期望宿主可见，RFC 045 祖先链模型与 D8 服务面切换叙事的缝隙）
- **过程发现**：`cargo test --workspace` 的 L2 运行批会在 std/ 下触发自动拆分改写（约百文件）——运行 L2 前须 `git status` 快照并在跑批后恢复（本轮已恢复）

### chord corpus 推进（idx1→idx14 稳定）：委托结果临时类型家族 + λ 接口返回 fatptr 契约 + 数组推断（门禁第 6 项运行面）
- **根因一（委托调用结果物化为 i32 本地）**：`EffectEntry.Run: _disposer = _callback()` 的 `Func<IDisposable>` 回调结果经 `ptrtoint→i32→inttoptr` 截断（x64 高位丢失）——MIR 语句级委托调用结果临时按 `TypeId::Void`/推断默认建。修复：`try_lower_delegate_invoke` 返回 `(prep, rvalue, ret_ty)` 三元组（ret_ty = `delegate_return_type`，fallback object），语句级两处（Ident 与非 Ident func）按 ret_ty 建临时；表达式级推断补齐 **Call 臂 func=Ident/Field 委托字段**（裸 `_callback()` 与 `this._f()`：`is_class_field`/receiver 类字段解析 → `delegate_return_type`，不再回落 Int），并新增 `Expr::NewArray` 推断臂（`new string[0]` 数组指针曾经 i32 往返截断——ToneImpl `requires` 崩点根因）
- **根因二（λ 返回裸对象 vs 接口 fatptr）**：`_disposer.Dispose()`（EffectEntry_Revert itable 分派）崩溃——`() => { ...; return new DisposableAction(...); }` 以**函数形参契约** `Func<IDisposable>` 传入但 lifted λ 返回类型按 **body 推断**（DisposableAction 类）提升，调用方按 `{obj,itable}` fatptr 解引用裸对象。修复双管：① `lower_lambda_to_fnptr` 增 `expected_ret`（形参 Func/Action 委托类型解析返回类型；接口返回时按契约提升 fn_ret，`new Func<R>(λ)` 与形参两路径均接线）；② raw 路径 `Stmt::Return` 改走 `lower_return_value`（此前仅 typed 路径做 MakeIface 包裹——raw 方法体/λ 体的接口返回从未物化，covariance_e2e 既有缺口）
- **效果**：chord-only corpus `arc test examples/UnitTest/Chord` 由 idx1 崩（Provide_RevertRestoresPrevious）推进至 **idx0–14 稳定全过**（Provide/Revert/Inject/Config/Effect/Tone 函数形态链全绿）；余崩点漂移于 idx15–25（堆损坏型，见登记）；workspace 全批绿 + clippy 零告警
- **取证工具（全部还原未入提交）**：`ARC_DEBUG_PRUNE` 保留 IR 转储（曾还原，本轮改用 `--obj-dir` 隔离复证）；`ARC_DBG_FREE` DUP 双释打印 + 运行时 **quarantine OVERDEC 仪器化**（free 不归还 + rc≤0 报警）定位过减站点
- **剩余登记（下轮续）**：typed-inject 适配器 λ 链（形状 `__lambda_rt_34__Greeter` = `Inject<T>` 内部 `(ctx) => { GetService; 回调 }`）存在 **over-dec**：某对象首释于该 λ（rc 1→0），随后在**测试帧**再 dec（rc≤0）——StringBuilder-ish vt、两测试（TypedInject_ValueFlows/PendsUntil）确定性复现；关闭 cycle-collector 对照仍现 → 独立于收集器；疑似 class 值经「借→slot 传参」链的 inc/dec 不平衡（GetService 借用 + select inc + 回调形参 dec + 适配器局部 dec），非本轮改动引入（λ 均 void Action、无接口返回），与运行期崩溃漂移（idx14/15/25 随机）同源待清

### UI/Core --dynamic 达链接期：静态同元数重载推断分裂修复（Color 根因）
- **根因（IR 取证→源码定位）**：MIR `infer_type_from_expr` 静态类方法分支仅按「static+元数」取**首个**候选——`Color.Lerp(Color,Color,double)`（公开）与私有 `Lerp(double,double,double)` 同元数时推断命中公开载（返回类型误判 Color），而发射侧 `method_call_rvalue` 按实参类型选中 double 载 → 调用点实参按 struct-Color 物化（`load %struct.Color` 直读 double 结果）→ clang IR 校验失败（`%t21 defined double expected ptr`）
- **修复**：推断先按实参类型 `resolve_method_overload`（与发射同解析），strict 失败（未绑定 λ 等）再回落元数/首候选；`crates/mir/src/lower/lower_type.rs`
- **效果**：`arc build std/UI/Core --dynamic` **IR 编译通过**，达链接期——剩 **vendored wgpu_native.dll 资产债**（`wgpu_font_atlas_create`/`wgpu_create_instance` 蛇形 shim 导出缺失，缓存 DLL 为上游 wgpu-native camel API——Arc UI 期望自带 shim 层，需外部资产重建/换源，登记）；workspace 133 批全绿 + clippy 零告警
- 同族价值：本推断-发射分裂即 corpus Tone→RunApply 边界 AV 的候选根因家族（typeck/MIR 解析不一致），修复后 corpus 复测见下

### UI/Core --dynamic 首达 IR 编译：Color 通道错配取证（新缺陷家族登记）
- 完整性门清零后 `clang IR compile failed`：`Media/Color.as` `Color.Lerp(Color,Color,double)` 体内标量重载 `Color.Lerp(double,double,double)` 调用结果被按 struct-Color 物化（`load %struct.Color, ptr %t21`——double 上直接 load），`FromRgba` 调用点实参按 Color 打包（4×ptr）而 def 为 double×4——调用点目标（double 载）与物化类型（Color）不一致
- 取证：源语义干净（FromRgba(double×4)+私有标量 Lerp）；IR 每通道调用均发射 double 目标、随后按 ptr 载 Color —— 指向调用点实参**推断类型**与重载解析目标间的错配（expr-types/`infer_type_from_spanned` 对嵌套静态调用实参回退 Color 的嫌疑），首次 IR 编译面暴露（此前整个成员在 ResourceDictionary 解析/完整性门失败，Media/Color 从未达 IR 校验）
- 待专项：MIR/typeck 嵌套静态调用实参类型一致性（可能同源 corpus 运行时 Tone 边界 AV 的 typeck-vs-MIR 分裂家族）

### UI/Core --dynamic codegen 完整性门 22→0（机制三修复落地）
- **机制①嵌套泛型类所有者模板未剔**：静态类泛型方法模板（重载后缀名 `ItemSourceView::From_EnumOptions_T`）body 内 `options.Count/Get`（`options: EnumOptions<T>`）产生未单态化目标——`drop_non_emittable_generic_templates` 的接收者判定仅匹配裸形参（`T`），扩展为含泛型形参原子的接收者/类名（`EnumOptions_T`）亦判不可发射
- **机制②模板 lifted λ/占位单态体残留**：模板剔除后其 lowering 期 lifts（`__lambda_rt_N`）与占位体留在 result（`--dynamic` 无入口全量保留）——新增 `drop_placeholder_tainted` 级联剔除：类型名位单大写原子占位规则 + 函数名位仅 `__` 类型实参后缀规则（防误伤单字母属性 `FkCounter_set_X`，L1 field_keyword 回归保护）
- **机制③ compile_to_object stub 补发环缺失**：`TextBuffer_get_LineCount` 等 builtin custom-accessor stub（MIR 无调用边）在 object 角色（--dynamic 库）无补发环 → 补与 exe 路径同契约的 4 轮 stub refill（body 占位取任意既有条目兜底）；`TextBuffer` stub 分支已覆盖 get_LineCount
- **效果**：`arc build std/UI/Core --dynamic` **完整性门 22→0 全过**，首次进入 IR 编译——暴露下一级 `Color.Lerp/FromRgba` 单函数 codegen 错配（clang IR 校验：标量 Lerp 双载调用结果被按 struct-Color 物化/`FromRgba` 实参按 Color 打包——typeck/MIR/expr-types 与 codegen 间目标-类型不一致，首次 IR 编译面暴露，待专项）；workspace 133 批全绿 + clippy 零告警
- 注：exe/corpus 路径不受新剔除影响（L1 field_keyword 等回归批全过）

### UI/Core --dynamic codegen 22 符号取证（门禁债第 6 项 UI 面·续）
- 完整性门 IR 转储定位三类机制（chord 同族在 UI 的实例）：
  1. **模板体内占位符类泛型调用未级联替换**：`EnumOptions_T_Get`/`EnumOptions_T_get_Count` 调用位于 **`ItemSourceView_From_EnumOptions_T` 单态体内**（`Enum.GetOptions<T>` 家族经 typeck 单态化链 `From<EnumOptions<T>>`），body 内对 `EnumOptions<T>` 类方法的调用名未随 T→实参替换 → 引用模板名无 define（MIR `substitute_in_rvalue` 已会替换 target_fn——说明是 typeck `instantiate_generic_fn` 侧的调用名固化或 MIR 克隆 generics/concrete 配对错位，下轮沿 define 宿主链定位）
  2. `Task_T`/`int_FromResult` 位于 --dynamic 入口宿主（async/FromResult 模板面）
  3. `TextBuffer_get_LineCount`/`Signal_T_Set`/`BindingRegistry_PutValue__T`/`Element_SetValue__T` 等类方法/泛型方法模板名
- 处置：本轮已完成取证与归类（非 force-keep 缺失），下轮按「先修 typeck 单态体内调用名固化，再补 MIR 克隆配对」推进；所有临时插桩已还原

### UI/Core --dynamic 首过解析+typeck：四处 .as 漂移修复（门禁债第 6 项 UI 面）
- `ResourceDictionary.CollectStyles` 参数名 `into` 为 LINQ 保留字 → 解析级错误（`expected identifier, found Into`）——更名 `target`
- `VirtualizingStackPanel` Update 分支传 `args.NewItem`：RFC 037 M-VZ1 重构后 generator 按索引直读视图（`ItemAt/DisplayAt`），旧直绑 API 残留致重载失配——改 `ApplyUpdate(index, itemDefaults)`
- `MultiSelector`：`SetupMulti` 以 `List<string>` 初始化 `List<object>` 字段（泛型不变性漂移）；`SelectItem` 把 `ItemDataAt`（object）赋给 `string` 局部——类型对齐 object
- `Application`：`SwitchTheme`/`RunCore` 调用 M3 期占位 API（`StyleManager.ApplyImplicitStyles`/`this.ApplyStyleTree`，全库无定义）——统一走现行 `VisualHost.ApplyAllHostStyles`（与 RunCore 启动通道同一引擎）
- **效果**：`arc build std/UI/Core --dynamic` 首度通过 parse+typeck（此前 ResourceDictionary 解析即败）；codegen 完整性门暴露 UI 泛型 mono 命名缺口 22 符号（`EnumOptions_T_get_Count`/`Signal_T_Set`/`BindingRegistry_PutValue__T`/`Element_SetValue__T`/`int_FromResult`/`Task_T` 等——chord 同族问题的 UI 实例，含模板体内占位 T 泛型调用的克隆面，待专项）；workspace 133 批全绿 + clippy 零告警 + arc-ui 测试全绿
- 运行时 0xC0000005 取证进展：崩溃收缩到 chord `Tone→RunApply` 边界（[a1]Tone 入口→[b1]ToneImpl→[b2]child ctor→首个 `RunApply(...)` 调用即崩、RunApply 首语句未达），探针/宿主上下文差异仍待符号化通道修复后定位（详见下）

### corpus 首次越线：完整性门 2→0、测试宿主跑到 507/908（门禁债第 6 项三轮）
- **根因**：剩余 2 符号 `__lambda_rt_38/39__Greeter` = 泛型 mono 体内**嵌套闭包**克隆缺口——闭包克隆（`collect_closure_monos_in_operand`）产物只进 mono_bodies，不会被后续 fixpoint 轮按方法克隆路径再扫（`try_create_mono_body` 只看 Call/MethodCall）；外层 λ 克隆体（`rt_37__Greeter`）内的 `Closure{fn_name: rt_38__Greeter}` 操作数永不触发内层克隆
- **修复**：闭包克隆后立即递归 `collect_closure_mono_targets` 扫描克隆体（与 `try_create_mono_body`/iface 实例化路径同款），带替身名去重；`crates/mir/src/lower.rs`
- **里程碑**：`arc test examples/UnitTest` **完整性门全过、首度进入运行时**（QIF_PROGRESS=1 定位）——508 个测试在 idx 507 `UnitTest.Chord.ChordServiceTests.Provide_VisibleToDescendantsOnly` 处 0xC0000005（508 测试前全过，崩溃在 Tone/ancestor 链，运行时取证待续）；workspace 133 批全绿 + clippy 零告警
- 过程工具（全部还原未入提交）：完整性门 IR 转储（ARC_DEBUG_PRUNE）、MIR fn 名单（ARC_DEBUG_MIRFNS）、烘焙 QIF_PROGRESS 逐测试进度

### corpus 深层推进：MIR/typeck 绑定分叉六符号专治（门禁债第 6 项二轮）
- **方法级定位**：`arc-prune-001` 六符号经完整性门 IR 转储（env 门控临时插桩，已还原）取证为**四类 MIR/typeck 绑定分叉**，非 force-keep 缺失：
  1. MIR 实例重载解析缺 λ 软匹配：`app.InjectReactive([...], ctx => …)` 在 MIR 错绑同名扩展（string 形参收 string[]）→ 参数错位 + λ 形参类型丢失（`int_SetConfig`：ctx 被当 i32 receiver）；`MakeCleanup` 的 `ctx.On("x", _ => { })` 同理错落泛型扩展模板名
  2. 泛型方法 mono 命名分叉：λ 实参致模板唯一匹配失配 → 回退**替换后**签名基底（`Provide_Func_Greeter`）+ `__{type_args}`，与 mono 克隆体占位符基底（`Provide_Func_T__Greeter`）对不上 → 符号缺失
  3. 显式 type_args 调用不可被非泛型实例 λ-soft 抢先（`On<string>` 不得错绑实例 `On(string,…)`）
  4. expr_types 表未命中 λ（span 重写/无捕获路径）→ MIR 推断回退 `Int`（receiver 截断为 i32 的系列根因）
- **修复（全部带单测/回归）**：MIR `method_call_rvalue`×2（simple/with-prep）strict 失败且含 λ 且无显式 type_args → 实例 λ 软匹配（registry `resolve_method_overload_lambda_soft`）；`method_generic_template_link_name` 过滤补 λ 软兼容；新增 `method_generic_template_link_name_by_arity`（泛型数+元数窄匹配，占位符基底回填）；`infer_type_from_expr` 补 `Expr::Lambda` 臂（返回与 typeck 同构 `Func{Infer…}`）；单测 `generic_template_link_picks_func_form_for_unbound_lambda` + 既有嵌套 Func 软匹配回归
- **效果**：`arc test examples/UnitTest` 首次越过 codegen 完整性门从 6 → **2** 符号（剩余 `__lambda_rt_38__Greeter`/`__lambda_rt_39__Greeter`：泛型 mono 体内 lifted λ 的 `__{T}` 克隆缺口——待续）；workspace 133 批全绿 + clippy 零告警
- **过程教训（登记）**：PowerShell `Set-Content` 曾误伤 source 文件编码（全量中文注释 mojibake，`git checkout` 还原后以 edit 工具重放）——源文件一律经工具改写

### corpus 预存缺陷专项：typeck 阻断全消（门禁债第 6 项首轮落地）
- **根因一（AIWriteAuditEntry / 全链 string↔byte[] 错配）= typeck 陈旧硬编码表**：`check_builtin_static_method` 以旧 string 时代签名（`ComputeHash(string)->string` 等）在 registry 解析**之前**拦截 `SHA256/HMAC/CSPRNG` 公开方法——现代 std（RFC 026 M3）公开面为真实 .as 体（null 判空 + CryptographicException）、仅私有 `_ComputeHash`/`_GetBytes` 为 `[Builtin(ABI)]` stub（codegen 按 `SHA256::_ComputeHash` → `rt_crypto_sha256_arr` 直射）。删除该三组陈旧拦截臂后公开调用回落 registry 真实签名；`crates/typeck/src/checker/check_builtin.rs`
- **根因二（AICheckpointStore 覆写审计 hash 赋值）= .as 缺陷**：`entry.Hash`（string）直收 `SHA256.ComputeHash(content)`（byte[]）漏 `Encoding.GetBytes` + `ToHex` 包装——按 469 行同款正确链修复；`std/AI/Agent.Harness/Checkpoint/AICheckpointStore.as`（诊断经包级归属定位：方法体错误在 check_class 1021 恢复式 push、无 span TypeError 靠 env 门控类/方法级归属插桩定位——插桩已全部还原，未入提交）
- **编译器修复（λ 重载解析三缺口）**：
  1. 嵌套 Func 形参 vs 未绑定 λ：`func_name_infer_compatible` 的 arity=None 回溯按 count 升序取首解，`Func_object_Func_object_object_object` 被低 count 误切（嵌套组作 ret）→ 软匹配零候选 → 回退首签名错绑（expected 2 / found 3）。以实参 λ 元数为目标 arity 显式重解析期望签名（`Some(f_arity)`）；带单测 `soft_match_nested_func_param_against_unbound_lambda`
  2. 扩展方法被同名实例方法屏蔽：实例候选无一适用时应回落扩展（C# 语义），但 λ 链末端 name-only 兜底抢先成功。λ 解析链在 name-only 前插入扩展探测（命中即令外层 Err 臂走既有扩展处理路径）；扩展处理臂补 λ 目标形参定向校验（Func/Action 槽 + λ 实参 → demangle 形参 → `check_func_lambda`，与实例路径同规则）
  3. `bind_args_to_slots` 的 Func/Action 槽 λ 透传（扩展实参统一绑定路径同规则）
- **编译器修复（internal 跨文件可达误报）**：fn 签名首通（forward-reference 注册）未按声明文件切换包上下文（RFC 025 M2），file 级 fn 形参里的同包 internal variant/类被判不可达（`ContentLikeConsume(ContentLike c)`）；首通按 span `enter_package_for_span`，与 item 主循环一致
- **corpus .as 修复**：`ChordLifecycleTests` 两处 `Assert.Equal`（enum / object 实参，Arc Assert 无泛型 Equal）按既有惯例改 `(int)`/`(string)` 显式转换；`ModernTypeTests` 的 `Greeter` 与 UnitTest.Chord 同名跨命名空间 internal 类构成 registry 短名遮蔽（后者 getter 的 `_name` 字段解析落空 → MIR ICE `unresolved ident`），更名 `ExprGreeter` 并注明缘由
- **效果**：`arc test examples/UnitTest`（635 items）typeck **全绿**（原 5 簇：AIWriteAuditEntry/Equal 参数数/ContentLike/RoVec/bytes 错配全消）——首次推进至 MIR/codegen，暴露并定位到 reachability 泛型单态化收集缺口（arc-prune-001：`ChordContext_Provide_Func_*`/`Inject_*`/`ChordContextExtensions_On/Once/OnWaterfall` 等 6 符号，见门禁债第 6 项更新）；workspace 133 批全绿 + clippy 零告警

### SDK 分发：解压即装 + 捆绑 clang 自动接线（1.0 后续 / Linux·macOS 跟进第一梯）
- **分发包内嵌就地安装器**：`install.ps1`（Windows zip）与 `arc-install.sh`（Unix）均支持「解压 SDK 根后原地无参运行即安装」（自动识别 SDK 根）；新增 `-FromDir / --from-dir <dir>` 显式指定已解压目录；包名推导优先 `version.txt`（目录可改名）；下载/SHA256/版本指针布局/PATH 注入/`arc doctor` 收尾契约不变（`scripts/packaging/*`）
- **打包端同步嵌入**：`arc-pack.ps1` 将仓库同源 `install.ps1` 嵌入 Windows zip SDK 根；Unix 就地安装场景由 `verify-arc-install.sh` 新增 T6/T7 用例覆盖（`--from-dir`、改名目录、SDK 根内无参运行）
- **codegen clang 解析序新增 SDK 捆绑位**：`ARC_CLANG` → `arc toolchain` 指针 → SDK 捆绑 `<sdk-root>/lib/llvm/bin/clang[.exe]`（`bundled_llvm_clang_path`，安装包 `-BundleLlm` 落点，解压即得离线构建基线）→ 系统安装位 → PATH；含单测（crates/codegen/src/sdk_layout.rs）
- **文档同步**：docs/user-guide/01-getting-started.md 更新为内嵌就地安装器 + 捆绑 LLVM 自动发现解析序（Windows/Unix 双侧）

### 平台同步 P0：Unix 安装态识别与 zip 执行位（同一迭代批次）
- **安装态 SDK 根标记按平台 exe 名识别**：`sdk_layout::installed_arc_exe_name()`（Windows `arc.exe` / Unix `arc`）成为单一解析来源，安装态根标记、`arc doctor` 结构检查与 `arc self-update` 布局共用；clang 名解析在 sdk_layout 内部收敛为单一函数——Unix 安装态 SDK（`bin/arc`）现可被编译器自定位，`arc doctor` 与安装脚本收尾不再误报 FAIL
- **zip 解压还原 Unix 执行位**：`extract_zip` 应用条目自带 Unix 权限位（zip external attrs）；`arc self-update` staging 按布局契约补回 `bin/arc` 与捆绑 LLVM `lib/llvm/bin/*` 可执行位（覆盖 Windows 产线 zip 无权限位的容器；Unix-only，带门控单测）

### 平台同步 P0：POSIX try/catch 结构化编译门（arc-eh-001）
- 非 Windows 目标上可达函数含 `try/catch` 时，由 ICE（emit_cfg 深处 panic）改为**发射前结构化硬错误** `arc-eh-001`（`CodegenError::UnsupportedTryCatch` + `emit_module` 前置门 `reject_try_catch_outside_windows`，作用域与旧触发面一致，Windows 零路径开销）；`try/finally`/`throw` 的内联 finally 链语义不受影响
- 文档同步：docs/user-guide/11-compilation-model.md「交叉编译」新增 1.0 平台能力边界（try/catch 仅 Windows SEH；POSIX Itanium = 里程碑⑨ / 1.1+）；带递归扫描单测（While→TryFinally→If→TryCatch 嵌套、finally 体内命中、Windows/无 try 放行）

### 文档收敛：平台支持宣称与 1.0 交付事实对齐
- docs/user-guide/01/02/16/18/19 五章收敛：01 Linux/macOS 二进制安装标为**消费端先行**（tar.xz 产线与发布端点未交付，如实标注）+ 构建依赖 clang 行 + 章节重编号；02/16 的 `-r` 示例标注「须为宿主桌面，交叉编译未实现（11 章）」；18 Native 集成与 19 热重载声明**以 Windows 为 1.0 实测面**（三平台句柄后端已接线未验收、POSIX dlopen 语义差异）；与 CHANGELOG 已知限制、11 章平台边界一致

### CI 门禁补挂：Unix 安装协议验收（unix-install-protocol job）
- `.github/workflows/ci.yml` 新增 ubuntu job：`sh scripts/packaging/verify-arc-install.sh`（T1–T7 全用例）——把「verify-arc-install.sh CI 可复跑」从声明落实为门禁

### 打包产线宿主感知：arc-pack.ps1 Unix tar.xz 分支（P1 第一落）
- `scripts/packaging/arc-pack.ps1` 随宿主产出容器：Windows zip（原逻辑不变）；Unix（Linux/macOS，pwsh core）产出 `arc-<ver>-<triple>.tar.xz`——归档前恢复 `bin/arc`/`install.sh`/捆绑 LLVM 工具可执行位，嵌入 `arc-install.sh`（更名 install.sh），`-BundleLlm` 工具名单与 clang 资源布局按平台（clang/lld/ld.lld vs clang.exe/lld-link/llvm-rc），Find-ClangBinary 增 Unix 标准安装位，判别验收走 tar 解包 + 平台 exe 名/输出名（macOS lld 失败回落系统链接器）
- 文档同步：docs/rfc/017 sdk-layout.md 打包行（容器随宿主）
- 验证限制：本机为 Windows 宿主——Unix 分支经 PowerShell 5.1/pwsh7 双解析校验 + 逻辑评审，实机执行须 Linux/macOS（或 CI 对应 OS job）；Windows zip 路径保持原样

### 发布收口多平台：github-release.ps1 单次发布全部宿主资产
- 资产发现改为按版本 glob `arc-<ver>-<triple>.(zip|tar.xz)`（每包校验 `.sha256` sidecar）；manifest **单次重签**携带全部 package/triple（`arc release manifest` 多 `--archive`/`--triple`，URL 按文件名派生）；notes 下载表按包生成（平台用途/triple 列）；上传与最终断言改用动态资产数；DryRun 完整列出全部资产
- 本地实测：win zip + linux tar.xz 双包 dry-run → 生成双 triple 签名 manifest（URL/大小/哈希正确）+ `arc release verify` 验签通过；scripts/release/README.md 发版流补多平台收口步骤

### 平台化收尾（工具脚本与布局契约文档）
- `scripts/sdk-stage.ps1` 宿主感知：`arc[.exe]` 命名随宿主 + Unix chmod +x（与 arc-pack 同模式）
- docs/rfc/017 sdk-layout.md 布局契约文普适化：安装态根标记/自更新指针图为 `bin/arc(.exe)`（`installed_arc_exe_name` 单一来源），判别段容器覆盖 Windows zip / Unix tar.xz

### 链接失败归因：vendored 底座缺口指引（arc-vendor-001）
- 非 Windows 目标链接失败且错误含 wgpu/crypto 底座命名空间（undefined/unresolved）时，由裸「clang link failed」升级为结构化归因（`diagnose_vendored_link_gap` + `enriched_link_failure`：失败路径重跑同参数 clang 捕获 stderr，保留链接器原始输出并叠加底座供应现状指引——wgpu Linux `bin/linux` M3+ 未供应/macOS 未接线、crypto Linux/macOS M1+ 未供应）；Windows 目标与无关符号零误报，成功路径零开销
- 单测 5 例（linux wgpu / macOS crypto 命中、Windows/无关符号/无 undefined 关键字不命中）

### 质量门修复：spec-guard.cjs 去除 BOM
- `scripts/spec-guard/spec-guard.cjs` 文件头残留 UTF-8 BOM + shebang——现代 Node（≥v22）解析即 SyntaxError，导致 `scripts/check-spec.ps1 -All`（CI 门禁）在本机/新 Node 上不可运行；去 BOM 后门禁可执行（存量 38 errors/7911 warnings 为 HEAD 既有结构债，与本变更无关，单独立项）

### L2 watchdog 参数化（full-rt 门禁前置）
- 批运行 watchdog 默认无进展超时 120s → **180s**（全量批跑联载下慢宿主实测 125s+ 触发误杀；正常批 <60s，180s 仍 ~2~3 倍余量），并新增 `ARC_BATCH_TIMEOUT_SECS` 环境变量显式覆盖（缓慢 CI 宿主调大 / 本地取证调小）——债务 4（§7.3/§7.4）参数面收口，为 CI full-rt job 铺路

### L1 管线级回归：POSIX try/catch 编译门（arc-eh-001）
- 新增 `crates/arc-tests/tests/l1_eh_gate_target.rs`（无 feature 门控）：`arc::compile_file` 指定 Linux 目标 + try/catch 断言返回 `arc-eh-001` 结构化错误（含构造名与函数名）；无 try/catch 对照不得误伤——固化 ③ 的管线级行为，防回退成 ICE（2 case，1.7s）

### Release LTO 链接器按目标选择（平台审计 Top-10 #4/#6 修复）
- `clang_link` Release 的 `-fuse-ld` 由**宿主 cfg 一刀切**改为**按目标三元组**：Windows MSVC → `lld-link`、Windows GNU/MinGW → `lld`、ELF 系（Linux/OHOS）→ `lld`、**macOS → 不注入**（Apple clang 无 lld，注入即 Release 必败；系统 ld64 原生支持 thin LTO）——修复 macOS 默认工具链 Release 必败与交叉方向错配；新增 `release_linker_follows_target_not_host` 单测（4 目标断言）

### Linux 编译器 CLI 去 X11 链入（平台审计 Top-10 #9 修复）
- `crates/codegen/build.rs` 移除自首提交遗留的 `cargo:rustc-link-lib=X11`——编译器自身不调用 X11，该链入使 headless Linux 上连 `arc --version` 都因缺 libX11.so.6 无法启动（CI 此前以 libx11-dev 掩盖）；X11 仍在**目标程序**链接期按需注入（`platform_link_flags` Linux 分支），行为语义不变

### .as 头注释平台归属修正（审计 ③ 次要点）
- `std/Net/P2P/NoiseTransport.as` / `PeerKey.as` 头注释原称 `rt_noise_*`/`rt_crypto_ed25519_*` 依赖 vendored crypto_native.dll——实测符号定义于**可移植原生 runtime**（crates/runtime/rt_noise.c / rt_ed25519.c，随程序编译），已修正归属（Security AesGcm/ECDH 确属 vendor，注释保持）

### C runtime POSIX 守卫化（审计 S2 #4/#5）
- `rt_thread.c` Monitor 取证侧表（等待者/持锁者登记、转储）整块加 `#if defined(_WIN32)` 守卫，POSIX 提供跨文件符号空实现（`rt_mon_diag_current_owner_obj_of` 恒 NULL、`rt_mon_diag_dump` 空）——此前 GetCurrentThreadId/_Interlocked*/GetTickCount64 无守卫编译进 POSIX 对象，靠死代码 + `--gc-sections` 侥幸通过
- `rt_threadpool.c` 跨线程取栈（SuspendThread/StackWalk64/dbghelp）同样 Win32 守卫 + POSIX 空实现
- `rt_preempt.c` SIGURG 抢占分支收窄为 `SIGURG && __linux__`——macOS/BSD 定义 SIGURG 但无 sigqueue(2)，误入分支潜伏链接错误；此类平台走协作式降级（`rt_preempt_is_supported()=0`，与文档「自动降级」一致）
- `rt_abi.h` 抢占 ABI 注释补**现状注记**（审计 S2 #6）：注入侧（signal_impl/init）零调用方、await 点检测面已发射——语义为协作式钩子预留，「1ms 定时抢占」须调度器接线后生效

### Windows 契约测试修复：.gitattributes 路径笔误
- `std/UI/Styling/BuiltInTheme.Colors.g.as` eol=lf 规则路径**漏写 `Core/`**（实际 `std/UI/Core/Styling/...`）——Windows（core.autocrlf=true）检出 CRLF，`design_tokens_contract::builtin_theme_colors_g_as_in_sync` 恒红；修正路径并将生成文件重规范化为 LF（生成器恒发 LF）
- 验证：`cargo test --workspace` **133 测试块全绿（exit 0，0 fail）**

### CI 诚实化：wasm 草稿 job 更名标注
- `.github/workflows/ci.yml` 原 `wasm32-hello-draft` job 名与步骤名暗示已跑 wasm 编译——实际仅 ubuntu 冗余构建（continue-on-error 非门禁，wasm 编译 e2e 未接线）；更名 `ubuntu-extra-draft` 并在注释/步骤名如实标注 RFC 031 M-W3 Draft 现状

### 安装指针自定位修复 + 安装态标准库扩展开发验收
- **发布级缺口实证与修复**：安装根 `bin/arc(.exe)` 指针副本在普通调用路径无 re-exec 也无自定位——实证 `SDK_LAYOUT=none`（从 PATH 启动即断）；`sdk_layout` 新增 `resolve_pointer_sdk_root`：读 `versions/current` 标记 → 前缀 `arc-<ver>-` 匹配含完整 SDK 的版本目录返回（与 install.ps1/arc-install.sh/self-update 指针布局契约同源；直接解析而非 spawn，保留指针设计对更新/回滚的收益）；单测 2 例 + 回归
- **验收闭环（Windows 本机实测）**：arc-pack → install.ps1 沙箱安装（doctor 11/11 含捆绑 clang）→ **指针** `bin\arc.exe` 离线：`SDK_LAYOUT=installed`、std 消费应用（`using Arc.Collections` 无依赖声明）构建运行、**扩展子库**（`MyExt`：arc.toml kind=library + `Arc` path 依赖指向 `lib/std/Arc`）`--dynamic` 构建 + 应用 path 依赖消费全通
- docs/user-guide/13-standard-library.md 新增「标准库扩展开发（安装态）」小节：推荐独立子库形态与样例；如实注明「向 lib/std 添加新命名空间目录不会被索引自动解析」（1.0 实测）

### install.ps1 -FromDir 缺陷修复（就地安装路径）
- **实证缺陷**：`[string]` 类型约束参数 `$FromDir` 直接接收 `Resolve-Path` PathInfo 被强转 string，随后 `.Path` 取属性得 $null → `Join-Path` 空参——`-FromDir`/内嵌就地安装恒失败；改为先解入未约束局部再解引用
- **ASCII 纪律回归**：该文件头自述「keep ASCII-only（PS5.1 以 ANSI 读无 BOM 文件）」，修复注释曾引入中文导致 PS5.1 解析错位（错误行号与文件不符为证）——注释全部 ASCII 化；PS 5.1/pwsh7 双解析 0 错，-FromDir 与内嵌无参就地安装（含 doctor 11/11）实测通过

### 1.0 发布前门禁存量债登记（非本批引入，处置建议随附）
供发布负责人在 CI/发布窗口前逐项决策（本会话已完成各自可验面并留痕）：
1. ~~rustfmt 漂移~~ **已归一化（本会话）**：rustfmt 1.97.1 全库归一化（32 文件 +240/−187，`65004862`，已登记 .git-blame-ignore-revs）——`cargo fmt --all -- --check` 现收敛（门禁可复跑）
2. **spec-guard 38 errors** → **已降 26**（本会话）：.as 风格类全消（switch/missing-braces×5、TODO 注释×2、文件名与主类型一致×1），余项为结构债（arc-tests/typeck lib.rs 门面、巨型文件、超长测试文件）与本地噪音（.mbedtls-src 缓存×2，CI 无）；LinqTests `let d` 两处经核实为 **查询 let**（非局部声明）——规则误报，建议 spec-guard 增加查询上下文豁免（低优先）
6. **发布级预存缺陷（本会话实证推进，已越线至运行时）**：typeck 五簇已修复全绿；codegen 完整性门六+二符号全消——`arc test examples/UnitTest` 现可完整编译并**运行**（QIF_PROGRESS 实测 508 测试全过后在 idx 507 `ChordServiceTests.Provide_VisibleToDescendantsOnly`（Tone/祖先链）0xC0000005——运行时取证待续，可能是 chord 首次进入运行时的潜在缺陷或本会话 MIR 改动引入，须栈映射定位）；`std/UI/Core --dynamic` ResourceDictionary 解析失败（`found Into`，未触及）——继续专项
3. ~~clippy 存量告警 3 处~~ **已清零（本会话）**：codegen mod.rs doc 空行/未用变量、typeck needless 引用、parse doc 列表缩进——`cargo clippy --workspace --all-targets` exit 0 零告警，`-D warnings` 可作门禁复跑
4. **CI 观察窗**：三平台矩阵（lint/build-test）在 HEAD 的门禁真实状态、`unix-install-protocol` job 首跑、ubuntu-extra-draft（wasm 未接线，continue-on-error）——需 CI 首跑信号后按结果处置（本机无 runner 观察面）
5. **外部交付线**：Linux/macOS tar.xz 产线执行端（arc-pack Unix 分支已就绪，需 Unix 宿主/CI job 实跑）、发布端点定版（RFC 031 §12）、macOS 安装协议实机（LibreSSL/xz 兼容）——均需外部执行面

## 1.0.0（2026-09-04）

**Arc 1.0 —— 首个稳定版**。语言、编译器、标准库与运行时的首个正式发布：单一 `arc` 可执行文件 + 源码分发的标准库 + 随包 runtime C 源码（首次构建经内容寻址缓存按需编译），AOT 编译至原生机器码，无 JIT 运行时。

### 支持面

| 项 | 状态 |
|----|------|
| 平台 | Windows x86_64（安装包交付）；Linux/macOS 构建门禁 CI 绿，安装脚本实机验收通过（harness），安装包产线后续交付 |
| 工具链 | 捆绑瘦身版 LLVM（clang + lld 子集，完全离线构建）或外部 clang ≥ 22 |
| 标准库 | 37 个子库源码分发（Arc / Collections / Threading / DI / Illusory / UI / Net / Orm / Web / AI / Chord 等） |
| 安装态自检 | `arc doctor` 九项检测（SDK 结构 / clang 基线 / MSVC 探测 / rt_cache 完整性 / native DLL） |
| 发布分发 | `arc release`（签名发布清单）/ `arc self-update` / `arc publish`（`.aopkg` 源码分发包）随 1.0 交付 |
| 验收 | workspace 133 测试批全绿；运行时判据批 bisect 200 + channels 200 + u5 20 轮零失败；Illusory M1 门禁 5 case 全绿 |

### 本周期要点

- **开源准备（GitHub 公开发布）**：版权与署名统一落位 LUSIDA（Start）（LICENSE / Cargo authors / PE 版本资源 / 安装包 version.txt / README 中英双版）；文档清洁——修复 130+ 处相对链接层级错位与陈旧命名（RFC 045 更名残留等），公开文档与内部过程文档（plan / discuss / reviews / proposals）解链；新增 `scripts/release/github-export.ps1` 开源导出（跟踪文件 + 内部资产排除清单 + 发布前安全扫描：绝对路径/邮箱/密钥材料/大文件/占位签名密钥轮换门禁）；**发布签名密钥完成正式轮换**（开发占位密钥退役，新 seed 离线托管于发布者、不入库，编译期内嵌信任锚同步替换）
- **发布分发链补齐（1.0 门槛收口）**：`arc release`（keygen / manifest / verify——Ed25519 签名发布清单，信任锚内置 + `$ARC_RELEASE_PUBKEY` 覆盖）、`arc self-update`（验签 → staging → `--version` 自检 → 原子提交 → `--rollback`，指针 re-exec 与 AV 瞬时锁容忍）、`arc publish`（`.aopkg` 源码分发包：FILES 完整性清单 + 可选分离签名 + `--verify` 消费端校验）、`arc-install.sh` 补 `--ca` 与解压布局加固并以 harness 实机验收（WSL2 Ubuntu 端到端 10/10；`scripts/packaging/verify-arc-install.sh` CI 可复跑）——分发以**源码打包形态**回归，依赖求解体系维持裁撤（RFC 031 §13 / RFC 017 禁止项修订）
- **任务图竞态收敛**：八处协议级修复（follower 链全局锁、WhenAll 聚合器双竞态、poll_inner 纳锁、NOTIFIED 双向验证、el 等待链心跳预算、AB-BA 破环、注册表幂等保护、Delay/WhenAll 任务 slab 注册）——收敛判据全量重验零失败
- **Illusory M1 门禁点亮**：首批系统性踩中标记接口与 struct 值语义盲区，八处编译器/运行时协议缺陷修复（标记接口 itable、Copy struct 字段悬垂、重载接口方法槽序、实参接口包裹、foreach 元素类型、struct 静态字段默认值、泛型型参 is 折叠、泛型转调单态化传播）
- **接口相等语义**：接口元素 List 的 Remove/Contains/IndexOf 与 `==` 按底层对象身份比较（内联 + stub 双路径）

### 已知限制

- 官方发布端点（`manifest.json` 托管，现占位 `static.arc.dev`）定版与 Linux/macOS 安装包（tar.xz 产线）为外部依赖待交付；`arc self-update` 分发容器统一 zip
- 标准库以源码分发：项目首次构建按需编译 runtime C（内容寻址缓存后增量）

## 2026-09-02

### RFC 045 D14：Chord 类型化服务与 DI 融合（以终为始·API 面以 C# 惯性收敛）
- **类型即契约**：`Provide<T>(T)` / `Provide<T>(Func<T>)`（工厂按需构造，首次解析构造并缓存，MEDI 工厂语义同构）/ `GetService<T>()` / `HasService<T>()` / `Inject<T>((ctx, value) => …)` / `InjectReactive<T>` 以 `typeof(T).FullName` 派生键——零魔法字符串、零强转、值直入回调；字符串名形态全保留（运行期动态名场景），单一分工律：契约按类型、通道按语义名
- **DI 融合**：`ChordContext` 持有 `IServiceProvider`（`IChordContext` 契约注册进 `ServiceCollection`）——类型化解析动态阴影链优先（可逆层）、DI 容器兜底（静态层）；DI 可解析注入依赖恒就绪（无挂起语义）
- **词系**：`Context` → `ChordContext`（实现）+ `IChordContext`（DI 契约），`ContextExtensions` → `ChordContextExtensions`；std/Chord 全程零 Plugin 字眼
- **贡献机制四件套（D11 修订，插件容器热插拔）**：`IContribute`（贡献项，Id 标识）/ `ContributeOptions`（**结构体值类型**：GroupId/Order/ParentId，getter-only + 完整构造，`Group` 命中保留字更名 `GroupId`）/ `IContributeHost`（容器 = 扩展点宿主，Register/Unregister 严格对称）/ `IContributeRegistry`（统一注册表，`Add`/`Remove` 容器热插拔 + `Register`/`Unregister` 按 hostId 定向；实现 `ContributeRegistry`）——纯库契约剥离语言核心，可逆性经 Effect 账本在扩展层组合；`IContributionPoint<TEntry>` 退场。**融合分层**：`IContributeRegistry` 直注 `ServiceCollection` 即组装基座（无需上下文直接拆装 = 热插拔），`IChordContext` 体系为编排增强层（账本可逆/事务/回滚/准入/热替换）；命名家族化：Contribute / Chord / Tone 三族前缀自洽
- **codegen：`Type.FullName` 唯一限定名落地（RFC 018 M2）**：layout 层新增 `type_full_names`（HIR namespace 经 `type_fqn` 拼接，键与各布局表同源），`emit_typeinfos` 四循环（interface/class/struct/enum）发射 `name`/`full_name`/`ns` 三常量——full_name/ns 为真实点分限定名；`name` 与 `type_id` 哈希输入不变（RFC 026 `type_name_to_id` 勿动共识）；RuntimeType 注释对齐
- **验证**：`arc build std/Chord --dynamic` 全绿（Arc/Arc.DI/Arc.Chord 三库构建）；typeck/mir/codegen 测试全绿（含新增 `ProgramLayouts.type_full_names` 面）；QIF 语料扩至类型键/工厂/DI 兜底/贡献四件套用例（语料 typeck 仍受既有 lambda unify 缺口阻断，见 plan.md 登记）

## 2026-09-01

### 语言核心洁净度：贡献机制残留清收（核心裁决收尾）
- codegen 载体发射全删：`ContributionsMeta`、`@__arc_contributions`(+count) IR 嵌入与导出面注入、单测 `contribution_carrier_emit.rs`——收集器 `contributions.rs` 已删后的发射端孤儿清零，编译器核心不再残留任何贡献机制面
- std 记账面退场：`std/Arc` 四件套（`ContributionAttribute`/`ContributionDescriptor`/`ContributionArg`/`IContributionRegistry`）与 `std/DI/InjectRegistry`、`std/AI/Agent/Tools/AIToolRegistry` 删除；`InjectAttribute`/`AIToolAttribute` 改直接派生 `Attribute`（`[Inject]`/`[AITool]` 静态绑定合成不受影响）
- 过期注释与文档同步：合成宿主唯一为 `__AIToolHost`（`generate_ai_tool_host`/`maybe_inject_ai_tool_host`/`maybe_inject_di_bindings` 更名对齐）；RFC 012 历史注记、RFC 045 索引边界、领域文档与 ArcAgent 示例改指「显式静态注册」

### 同批在途工作落地（工作树既有 WIP，组合全量验收）
- `CLAUDE.md` → `AGENTS.md` 权威迁移：CI spec-guard 与 `arc-language` 规则引用同步
- `--emit-llvm`（keep_ir）产物域贯通：CLI/pipeline/equipment/codegen/arc-tests 全链 + runtime debug/ABI 配套、mir/parse 局部修复
- arc-ui/ARML 样式体系增强：`arc-ui` codegen/verify/ast 扩面、`arml_style` 测试扩量、`std/UI/Core/Styling` 与 Markup/Rendering 配套重构

### 插件内核更名与 RFC 045 修订（Arc.Chord）
- `Arc.Plugins` → `Arc.Chord`：chord 是 arc 上两点的连线（内核即连接物）+ 心弦词根与 Cordis 同族；`std/Plugins/` → `std/Chord/`（git mv 保历史）、15 文件 namespace 与 arc.toml 同步、入列 std workspace members（30 子库）；docs 全链更名（RFC 045/017、SUMMARY、domain 索引、plugins.md → chord.md）
- RFC 045 修订：新增架构分层（显式静态注册 / 内核 / 贡献点四层单轨）、D11 贡献点（`IContributionPoint<TEntry>` + `Contribute` 副作用语义，VSCode contributes 运行期化）、D12 依赖声明（`IPluginDependencies.Requires` 挂起准入/启动序推导）、D5.1 瀑布事件（`OnWaterfall`/`Waterfall` 同步 next 委托）、D13 组合即数据（另立 RFC）；验收要点扩至 11 项

### Chord 内核实现与编译器缺口登记
- **去 plugin 化**：`IPlugin`→`ITone`（音）、`Plugin(...)`→`Context.Tone(...)`、`IPluginDependencies`→`IToneRequirements`、作用域名 plugin→tone、RFC 文件 045-plugin-kernel.md→045-chord.md——文件名/类名/方法零 plugin 字眼
- **Context 门面落码（D1–D12 全语义）**：副作用账本、动态服务阴影注册、注入就绪/挂起/丢弃/反应式回滚重跑、事件三级广播、瀑布管道、副作用事务、失败回滚、热替换（原位保序）、依赖准入挂起唤醒、贡献点扩展；QIF 语料 `examples/UnitTest/Chord/` 三用例类 33 Fact
- **编译器修复**：`parse_program_in_file` 剥离 UTF-8 BOM（外部编辑器产物容忍）
- **编译器缺口登记**（阻断 Chord QIF 语料运行，详见 plan.md）：`string[]?` parse 静默错位 / 赋值表达式 lambda 体 / bare `throw;` / lambda→`Action<object?>` unify / 库模式 `arc build std/Arc` MIR panic / `AIWriteAuditEntry`（并发 WIP）

### 编译器缺口架构级修正（第一梯次）
- **统一类型后缀文法**（`ty.rs`）：`?` 升级为每层复合类型的后缀运算符——`string[]?`/`string?[]?` 均合法（此前 `?` 仅基类型级消费，遗留 `?` 被语句层误吞为三元，产生静默解析错位）；**库模式 MIR `unresolved ident 'int'` panic 随之根除**（确证为 parse 错位下游），`arc build std/Chord --dynamic` 327 文件 parse 零错误实证
- **bare `throw;` 语言级支持**：Parser 引入 catch 绑定栈，裸重抛脱糖为 `throw <绑定名>`（合成名/实名一视同仁），rt_* 零改动；非 catch 上下文显式报错（对齐 C# CS0156 家族）
- **UTF-8 BOM 容忍**：`parse_program_in_file` 入口剥离 `\u{FEFF}`
- **待专项登记**（plan.md ④⑤⑥）：库模式 typeck 依赖解析缺陷（`--dynamic` 泛型实参丢失/using 解析失败，exe 模式同代码全绿）、lambda→`Action<T>` unify、赋值表达式四层落码

## 2026-08-29

### 语言核心与诊断
- P1/P12：`BlockingCollection` 构造第一实参约束前移 typeck 诊断，`emit_call` 用户可达 panic 清零（c2f5895e）
- 泛型体系加固：field_check 验证器、诊断去重管道、UI Markup 框架（3908f3c7）
- LSP 兜底：arc-server dispatch 级 panic 兜底 + 全部锁中毒恢复（1ae17573）

### 工具链收敛
- 锁定工具链 + rustfmt 全库收敛 + correctness 级 lint 真修复（231c6e20）
- workspace.lints 统一基线 + 全库 clippy 真修复（da692318）
- P0-3b/c clippy 长尾清零 + rustfmt 全库收敛（8283c90c），并登记 `.git-blame-ignore-revs`（162be055）

## 2026-08-27
- 修复泛型接口转换与运行时服务注入崩溃（409c4d80）

## 2026-08-26
- QIF 框架量产收口合并里程碑 + RFC 009 M6 多线程 Executor（ae314b95）
- 实现 GAP #5 delegate 委托类型支持（be286d4a）——plan.md 基础面缺漏 #5 关闭
- RFC 引用编号统一至 037 标准（f08530c7、514c6796）
- codegen/eh 平台属性辅助函数提取（85a8b32b）

## 2026-08-25

### 测试重构：进程内快测 + 批量门控分层
- 新增 `arc-tests` 进程内快测框架与首批批量测试用例（e695fc58）：L1 `arc::compile_file`（256MB 大栈线程 + 全局锁串行，`assert_compiles` / `assert_rejected`）+ L2 `build_and_run_batch`（`full-rt` feature 门控）
- `arc-integration` 集成测试包退场，workspace 移除成员（a2627a0f）；验证矩阵切换为 `cargo test --workspace` / `cargo test -p arc-tests`（运行时面 `--features full-rt`）

### 语言面
- 浮点字面量双精度/单精度后缀 + 多字段声明支持（d864d366）——基础面缺漏 #3/#4 关闭；新增 L1 工件布局批量回归 `l1_artifact_layout_batch.rs`

## 2026-08-24
- 修复批量同名类型匹配与接口返回值悬垂指针（ebe41de2）
- 整理修复多类编译与运行时问题（96ab8b55）

## 2026-08-23

### 拟真引擎与文档
- 拟真引擎 P0 离屏 3D 渲染探针 + wgpu 回读对齐缺陷修复（1fde9d2c）
- Arc.UI 拟真引擎文档与领域导航（365eecc9）

### 类型系统与代码生成
- comdat 前置：跨命名空间同名类在 layout/typeck 层 FQN 物化与路由（704ed6e1）；推进现状与验收教训登记（98561c0c）
- 闭包重构：单块捕获簇 + 定形 capture struct，2×malloc→1×malloc（af872ddd）
- await-in-loop→coro 推广：While/LinqForeach/CFG backedge 分支放开，循环内 await 走 pre-split 协程（890b6665）
- 阶段 3/4 缺陷校准：null_flow 跨函数泄漏修复 + reachability 契约对齐（1f198a7f）

### 测试批迁移（arc-integration 内部，一次编译一次运行）
- text 域 6 case（c81bc40a）、ternary 域 10 case（87500a90）、nullable_boxing 6 case（82d4b863）入批，删除对应旧 e2e 文件

### 底座收敛
- 落实双层架构裁决：`Task.ContinueWith` 残面全面删除（std stub + typeck 分支 + codegen 发射 + runtime 实现），合法表面 WhenAll/WhenAny/Run/Delay 不动（c72d9fde）；ContinueWith 消除与 TCS 保留机制登记（bc7a1c69）
