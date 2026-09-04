//! Async state machine lowering (RFC 009 M2).
//!
//! 将 async 函数编译为状态机：env struct + resume 函数 + 构造函数。
//!
//! ## 状态机布局
//!
//! 每个 async 函数 `F` 编译为：
//! - **env struct**：`%struct.__async_env_{F} = type { i32 state, ptr awaiter, ptr task_ptr, <params>, <locals> }`
//! - **resume 函数**：`define i32 @__async_resume_{F}(ptr %env_ptr, ptr %waker)`，switch state 驱动状态机
//! - **构造函数**：`define ptr @{F}(<params>)`，malloc env + 初始化 + rt_task_from_state_machine
//!
//! ## 状态分割（整图 CFG）
//!
//! resume 的 state 0 发射**完整 MIR CFG**（与同步函数同构的 `bbN` 标签与 terminator）。
//! 每个 `Await` 就地降为 suspend/resume：
//! - Pending → 保存 locals + 登记 waker + `ret PENDING`
//! - Ready / 唤醒后 → 提取 result → 落入 `sm_after_await_i` 继续同块后续语句
//!
//! 因此 **多块 await 链**（`if/else` 两侧皆 await、QIF 多 `RunBatch` 臂）与 **循环内 await**
//!（CFG 回边保留）均走 M2 真实 suspend，不再依赖「唯一宿主块」切分。
//!
//! ## 可 lowering 约束（诚实）
//!
//! - `is_async` 且至少一个活块含 await
//! - **M1+pump 残余**：无 await 的 async（纯同步完成）仍走 M1 构造路径；本文件不覆盖
//! - 跨 await 存活的 locals 全部提升为 env 字段（不做 liveness 分析）
//! - resume 入口为每个 local 创建 alloca 并从 env load；suspend 前 store 回 env

use super::*;
use ast::TypeId;
use mir::{BlockId, LocalId, MirBlock, MirOperand, MirRvalue, MirStatement, MirTerminator};

/// 遍历语句收集被闭包捕获的局部（`MirOperand::Closure` 的 env 中的 `Local`）。
fn collect_captured_locals_stmt(stmt: &MirStatement, out: &mut HashSet<LocalId>) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => collect_captured_locals_rvalue(rvalue, out),
        MirStatement::FieldSet { value, .. } => collect_captured_locals_rvalue(value, out),
        MirStatement::StaticFieldSet { value, .. } => collect_captured_locals_rvalue(value, out),
        MirStatement::IndexSet { value, .. } => collect_captured_locals_rvalue(value, out),
        MirStatement::Return(Some(rv)) => collect_captured_locals_rvalue(rv, out),
        MirStatement::Throw { value } => collect_captured_locals_rvalue(value, out),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                collect_captured_locals_stmt(s, out);
            }
            for s in catch_body {
                collect_captured_locals_stmt(s, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                collect_captured_locals_stmt(s, out);
            }
            for s in finally {
                collect_captured_locals_stmt(s, out);
            }
        }
        MirStatement::LinqForeach { body, .. } | MirStatement::While { body, .. } => {
            for s in body {
                collect_captured_locals_stmt(s, out);
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for s in then_body {
                collect_captured_locals_stmt(s, out);
            }
            for s in else_body {
                collect_captured_locals_stmt(s, out);
            }
        }
        MirStatement::Await { task, .. } => collect_captured_locals_rvalue(task, out),
        _ => {}
    }
}

fn collect_captured_locals_rvalue(rv: &MirRvalue, out: &mut HashSet<LocalId>) {
    match rv {
        MirRvalue::Use(op) | MirRvalue::Coalesce { left: op, .. } => {
            collect_captured_locals_operand(op, out)
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            collect_captured_locals_operand(cond, out);
            collect_captured_locals_operand(then_val, out);
            collect_captured_locals_operand(else_val, out);
        }
        MirRvalue::Binary { left, right, .. } => {
            collect_captured_locals_operand(left, out);
            collect_captured_locals_operand(right, out);
        }
        MirRvalue::Call { args, .. } | MirRvalue::New { args, .. } => {
            for a in args {
                collect_captured_locals_operand(a, out);
            }
        }
        MirRvalue::MethodCall { receiver, args, .. } => {
            collect_captured_locals_operand(receiver, out);
            for a in args {
                collect_captured_locals_operand(a, out);
            }
        }
        MirRvalue::StructLit { fields, .. } => {
            for (_, op) in fields {
                collect_captured_locals_operand(op, out);
            }
        }
        MirRvalue::IndexGet { array, index, .. } => {
            collect_captured_locals_operand(array, out);
            collect_captured_locals_operand(index, out);
        }
        MirRvalue::FieldGet { object, .. } => collect_captured_locals_operand(object, out),
        _ => {}
    }
}

fn collect_captured_locals_operand(op: &MirOperand, out: &mut HashSet<LocalId>) {
    if let MirOperand::Closure { env, .. } = op {
        for (_, src) in env {
            if let MirOperand::Local(id) = src {
                out.insert(*id);
            }
        }
    }
}

pub(super) fn is_dead_block(block: &MirBlock) -> bool {
    matches!(block.terminator, MirTerminator::Unreachable) && block.statements.is_empty()
}

/// 语句（含嵌套 region body）是否含 await。try-finally 的 `finally` body 不
/// 递归：funclet 双发射（正常路径 inline + cleanup funclet）下 await 无法 suspend
/// （RFC 010 里程碑⑦ 已知限制——异常不跨 Pending 边界；await-in-finally 不支持）。
fn stmt_has_await(stmt: &MirStatement) -> bool {
    match stmt {
        MirStatement::Await { .. } => true,
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => try_body.iter().any(stmt_has_await) || catch_body.iter().any(stmt_has_await),
        MirStatement::TryFinally { body, .. } => body.iter().any(stmt_has_await),
        MirStatement::LinqForeach { body, .. } => body.iter().any(stmt_has_await),
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => then_body.iter().any(stmt_has_await) || else_body.iter().any(stmt_has_await),
        MirStatement::While { body, .. } => body.iter().any(stmt_has_await),
        _ => false,
    }
}

fn block_has_await(block: &MirBlock) -> bool {
    block.statements.iter().any(stmt_has_await)
}

/// 递归收集语句内的 await 位点。`path` 为块顶层语句下标 + 各嵌套 region body
/// 下标的完整路径（与发射端 `FnEmitter::stmt_path` 同构）。
fn collect_await_sites_in_stmt(
    stmt: &MirStatement,
    path: &mut Vec<usize>,
    sites: &mut Vec<(BlockId, Vec<usize>)>,
    block_id: BlockId,
) {
    match stmt {
        MirStatement::Await { .. } => sites.push((block_id, path.clone())),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for (i, s) in try_body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
            for (i, s) in catch_body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
        }
        MirStatement::TryFinally { body, .. } => {
            for (i, s) in body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
        }
        MirStatement::LinqForeach { body, .. } => {
            for (i, s) in body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
        }
        MirStatement::If {
            then_body,
            else_body,
            ..
        } => {
            for (i, s) in then_body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
            for (i, s) in else_body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
        }
        MirStatement::While { body, .. } => {
            for (i, s) in body.iter().enumerate() {
                path.push(i);
                collect_await_sites_in_stmt(s, path, sites, block_id);
                path.pop();
            }
        }
        _ => {}
    }
}

/// 按 (BlockId, stmt_path) 稳定顺序收集 await 位点（try-finally 的 finally body
/// 除外——funclet 双发射下 await 不支持，见 [`stmt_has_await`]）。
fn collect_await_sites(cfg: &mir::MirCfgBody) -> Vec<(BlockId, Vec<usize>)> {
    let mut blocks: Vec<&MirBlock> = cfg.blocks.values().filter(|b| !is_dead_block(b)).collect();
    blocks.sort_by_key(|b| b.id.0);
    let mut sites = Vec::new();
    for block in blocks {
        let mut path = Vec::new();
        for (stmt_idx, stmt) in block.statements.iter().enumerate() {
            path.push(stmt_idx);
            collect_await_sites_in_stmt(stmt, &mut path, &mut sites, block.id);
            path.pop();
        }
    }
    sites
}

/// 检测 async 函数是否可 lowering 为 M2 状态机。
///
/// 条件：`is_async` 且至少一个活块含 await（含多块链与循环内）。
pub(crate) fn can_lower_as_state_machine(cfg: &mir::MirCfgBody) -> bool {
    if !cfg.is_async {
        return false;
    }
    cfg.blocks
        .values()
        .filter(|b| !is_dead_block(b))
        .any(block_has_await)
}

impl<'a> FnEmitter<'a> {
    /// 发射 async 状态机（M2）：env struct type + resume 函数 + 构造函数。
    pub(crate) fn emit_async_state_machine(&mut self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let sanitized = mangled.replace("::", "_");
        let internal = if is_entry_fn(name) {
            "__async_main".to_string()
        } else {
            mangled.clone()
        };
        let resume_name = format!("__async_resume_{sanitized}");
        let env_type = format!("%struct.__async_env_{sanitized}");

        // 保存上下文
        let prev_in_sm = std::mem::replace(&mut self.in_state_machine, false);
        let prev_env_type = std::mem::replace(&mut self.sm_env_type, env_type.clone());
        let prev_await_index = std::mem::take(&mut self.sm_await_index);
        let prev_await_count = self.sm_await_count;
        let prev_await_live = std::mem::take(&mut self.await_live_locals);
        let prev_env_local_index = std::mem::take(&mut self.sm_env_local_index);
        let prev_cleanup_label = self.sm_cleanup_label.take();

        // RFC 016：MIR 侧跨 await liveness pass 输出 —— 只提升真正跨 await
        // 存活的局部为 env 字段并参与 ARC 配对面（单一事实来源）。
        let await_live = mir::cross_await_live_locals(&self.cfg);
        self.await_live_locals = await_live.clone();

        // env 局部集合 = 参数 ∪ 捕获 ∪ 跨 await 存活 ∪ spill。
        // 参数/捕获恒入 env（ctor 存储 / 借引用槽地址）；未存活普通局部不入 env。
        self.build_sm_env_local_index();

        // 收集 env 字段
        let env_fields = self.collect_env_fields();

        // env struct type 定义
        let env_type_def = self.emit_env_struct_type(&env_type, &env_fields);

        // 索引全部 await 位点（整图 CFG 发射；try/catch/if/while/linq 嵌套 body 内
        // 的 await 经 stmt_path 唯一键索引——RFC 004 里程碑⑦）
        let sites = collect_await_sites(&self.cfg);
        self.sm_await_count = sites.len();
        self.sm_await_index = sites
            .iter()
            .enumerate()
            .map(|(i, (bid, path))| ((bid.0, path.clone()), i))
            .collect();

        // resume 函数（in_state_machine = true，使 Return terminator 走状态机路径）
        // RFC 016：dtor_name 同时传给 resume 函数——resume 级 EH cleanup pad 在
        // 任何 unwind（faulted throw / 异常传播）时调用 dtor 释放 env。
        let dtor_name = format!("__async_dtor_{sanitized}");
        self.in_state_machine = true;
        self.sm_env_type = env_type.clone();
        let resume_fn = self.emit_resume_function(name, &resume_name, &env_type, &dtor_name);
        self.in_state_machine = false;

        // C12: env 持独立所有权后，task 释放时须 dec 所有 class env 字段 +
        // free(env)。发射 dtor 函数，由 ctor 经 rt_task_set_dtor_fn 登记，
        // rt_task_release 在释放 env 前调用。
        let dtor_fn = self.emit_sm_dtor(&dtor_name, &env_type);

        // 构造函数
        let ctor_fn = self.emit_sm_ctor(&internal, &resume_name, &env_type, &dtor_name);

        // 恢复上下文
        self.in_state_machine = prev_in_sm;
        self.sm_env_type = prev_env_type;
        self.sm_await_index = prev_await_index;
        self.sm_await_count = prev_await_count;
        self.await_live_locals = prev_await_live;
        self.sm_env_local_index = prev_env_local_index;
        self.sm_cleanup_label = prev_cleanup_label;

        // 注意：main entry wrapper 由 emit_function 统一生成（M1/M2 共用），
        // 此处不重复生成，避免 main 重复定义。
        format!("{env_type_def}\n{resume_fn}\n{dtor_fn}\n{ctor_fn}\n")
    }

    /// RFC 016：构建 env 局部 → env 字段索引映射。
    ///
    /// env 局部 = 参数（id < param_count）∪ 捕获 ∪ 跨 await 存活 ∪ spill。
    /// 字段索引 = 3 + 在 env 局部（按 id 升序）中的序号。未存活普通局部不在
    /// env 中（零 env 字段、零 save/load、零 dtor 配对）。
    fn build_sm_env_local_index(&mut self) {
        let max_local = self.cfg.locals.keys().map(|id| id.0).max().unwrap_or(0);
        let mut env_ids: Vec<LocalId> = Vec::new();
        for i in 0..=max_local {
            let id = LocalId(i);
            if !self.cfg.locals.contains_key(&id) {
                continue;
            }
            let is_param = (i as usize) < self.cfg.param_count;
            let is_capture = self.cfg.captures.iter().any(|(cid, _, _)| *cid == id);
            let is_live = self.await_live_locals.contains(&id);
            let is_spill = self.spill_set.contains(&(i as usize));
            if is_param || is_capture || is_live || is_spill {
                env_ids.push(id);
            }
        }
        env_ids.sort_by_key(|id| id.0);
        let mut map = HashMap::new();
        for (idx, id) in env_ids.iter().enumerate() {
            map.insert(*id, 3 + idx);
        }
        self.sm_env_local_index = map;
    }

    /// C12: 发射 async env 的析构函数 `__async_dtor_{name}`。
    ///
    /// 由 `rt_task_release` 在释放 env 前调用（经 `rt_task_set_dtor_fn` 登记）。
    /// 遍历所有 class env 字段（index 3+，arc_class_place）执行 `rt_arc_dec`
    /// 释放 env 持有的独立 +1 引用，最后 `free(env)`。
    ///
    /// 跳过 capture 局部：async lambda 的 capture 由外层 env 持有所有权，
    /// 本 dtor 不 dec（避免双重释放）。capture 在 ctor 也裸 store（不 inc），
    /// 保持「借引用」语义。非 class 字段（state/awaiter/task_ptr/基元）跳过。
    fn emit_sm_dtor(&mut self, dtor_name: &str, env_type: &str) -> String {
        self.output
            .push_str(&format!("define void @{dtor_name}(ptr %env_ptr) {{\n"));
        self.output.push_str("entry:\n");

        // RFC 016：仅 dec **env 唯一 owner** 的 class 局部（跨 await 存活、在 env
        // 中、非 capture、arc_class_place）。未存活局部零 env 字段、零配对——
        // 其所有权由 body 内局部 Drop / 段内 epilogue 释放，不在此处。捕获局部
        // 为 ByRef 借引用（外层 env 持所有权），不 dec（避免双重释放）。
        let capture_local_ids: HashSet<LocalId> =
            self.cfg.captures.iter().map(|(id, _, _)| *id).collect();
        // RFC 016：class_locals 必须与 `is_env_owned_class_local` 权威谓词**逐字一致**
        // （在 env ∪ 非 capture ∪ **跨 await 存活** ∪ class place）——dtor 仅 dec
        // env 唯一 owner 的 +1 引用。非存活 class 局部（如未跨 await 存活的 class
        // 参数）已在 body 内 `emit_drop` 释放，此处不得二次 dec，否则双重释放
        // （0xC0000374，`ARC_DBG_FREE` 报 `rt_arc_dec:free DUP`）。
        let class_locals: Vec<LocalId> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, (_, ty))| {
                self.sm_env_local_index.contains_key(id)
                    && !capture_local_ids.contains(id)
                    && self.await_live_locals.contains(id)
                    && !matches!(ty, TypeId::Void)
                    && Self::arc_class_place(ty, self.layouts)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in &class_locals {
            let field_idx = self.local_env_field_index(*id);
            let field_ptr = self.fresh_temp();
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
            ));
            let val = self.fresh_temp();
            self.emit(&format!("{val} = load ptr, ptr {field_ptr}"));
            self.emit(&format!("call void @rt_arc_dec(ptr {val})"));
        }
        // RFC 009 M3：释放每个 spilled local 的堆槽（free 对 null 安全；
        // save_locals 不覆写 env 字段，槽指针始终有效）。
        let spill_locals: Vec<LocalId> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, _)| self.spill_set.contains(&(id.0 as usize)))
            .map(|(id, _)| *id)
            .collect();
        for id in &spill_locals {
            let field_idx = self.local_env_field_index(*id);
            let field_ptr = self.fresh_temp();
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
            ));
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = load ptr, ptr {field_ptr}"));
            self.emit(&format!("call void @free(ptr {slot})"));
        }
        self.emit("call void @free(ptr %env_ptr)");
        self.emit("ret void");
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// 收集 env struct 字段类型。
    /// 布局: [state(i32), awaiter(ptr), task_ptr(ptr), <env 局部 0>, <env 局部 1>, ...]
    /// RFC 016：仅提升 env 局部（参数 ∪ 捕获 ∪ 跨 await 存活 ∪ spill），
    /// 字段索引 = 3 + env 局部序号（见 `sm_env_local_index`）。Void 类型局部用
    /// i32 占位。RFC 009 M3：spilled local 的字段由值类型替换为 ptr（堆槽指针）。
    fn collect_env_fields(&self) -> Vec<TypeId> {
        let mut fields: Vec<TypeId> = vec![
            TypeId::Int,                   // 0: state
            TypeId::Generic("ptr".into()), // 1: awaiter
            TypeId::Generic("ptr".into()), // 2: task_ptr (反向指向 RtTask)
        ];
        let mut env_ids: Vec<(u32, LocalId)> = self
            .sm_env_local_index
            .iter()
            .map(|(id, idx)| (*idx as u32, *id))
            .collect();
        env_ids.sort_by_key(|(idx, _)| *idx);
        for (_, id) in env_ids {
            let ty = self
                .cfg
                .locals
                .get(&id)
                .map(|(_, t)| t.clone())
                .unwrap_or(TypeId::Int);
            let field_ty = if matches!(ty, TypeId::Void) {
                TypeId::Int
            } else if self.spill_set.contains(&(id.0 as usize)) {
                // RFC 009 M3：大值类型 local spill 为堆槽指针（8B 字段宽）。
                TypeId::Generic("ptr".into())
            } else if self.is_byref_captured_local(id) {
                // ByRef 捕获局部（标量变量捕获）：字段存「外层权威槽地址」，
                // 恒 ptr 宽——按值类型生成 i32 字段会被捕获恢复的 8 字节
                // store 写穿相邻字段。
                TypeId::Generic("ptr".into())
            } else {
                ty
            };
            fields.push(field_ty);
        }
        fields
    }

    /// local_id → env 字段索引（3 + env 局部序号；RFC 016 布局）。
    fn local_env_field_index(&self, id: LocalId) -> usize {
        self.sm_env_local_index[&id]
    }

    /// RFC 016：local 是否被提升为 env 字段（参数/捕获/跨 await 存活/spill）。
    fn is_env_local(&self, id: LocalId) -> bool {
        self.sm_env_local_index.contains_key(&id)
    }

    /// ByRef 捕获局部（变量捕获）：SM 链路中槽/字段存「外层权威槽地址」，
    /// 恒 ptr 宽——与值类型 llvm_type_of 无关（标量 ByRef 捕获修复：
    /// i32 字段/alloca 会被 8 字节 ptr store 写穿）。
    fn is_byref_captured_local(&self, id: LocalId) -> bool {
        self.cfg
            .captures
            .iter()
            .any(|(cid, _, c)| *cid == id && matches!(c.mode, ast::CaptureMode::ByRef))
    }

    /// RFC 016：local 是否为「env 唯一 owner」的 class 局部。
    ///
    /// 为 true 时，该局部所有权只由 env 字段持有；body 内 Assign / await 提取
    /// 的所有 ARC 覆写都写穿到 env 字段（alloca 仅作镜像），由 dtor + EH
    /// cleanup pad 释放恰一次。捕获局部（ByRef 借引用）与未存活局部不在此列。
    pub(crate) fn is_env_owned_class_local(&self, id: LocalId) -> bool {
        if !self.in_state_machine {
            return false;
        }
        if !self.sm_env_local_index.contains_key(&id) {
            return false;
        }
        if self.cfg.captures.iter().any(|(cid, _, _)| *cid == id) {
            return false;
        }
        if !self.await_live_locals.contains(&id) {
            return false;
        }
        let ty = self.local_type(id);
        !matches!(ty, TypeId::Void) && Self::arc_class_place(&ty, self.layouts)
    }

    /// RFC 016：resume 函数是否存在 env 唯一 owner 的 class 局部（需 EH cleanup
    /// pad 释放）。用于决定 resume 函数是否强制附加 uwtable + personality 并
    /// 发射 cleanup pad。捕获局部（借引用）不计入。
    fn has_env_owned_class_local(&self) -> bool {
        let capture_local_ids: HashSet<LocalId> =
            self.cfg.captures.iter().map(|(id, _, _)| *id).collect();
        self.await_live_locals.iter().any(|id| {
            self.sm_env_local_index.contains_key(id) && !capture_local_ids.contains(id) && {
                let ty = self.local_type(*id);
                !matches!(ty, TypeId::Void) && Self::arc_class_place(&ty, self.layouts)
            }
        })
    }

    /// RFC 016：body 内对 env 唯一 owner 的 class 局部赋值——ARC 覆写**写穿到
    /// env 字段**（唯一 owner），alloca 仅作镜像（纯 store，不持所有权）。
    /// dtor + EH cleanup pad 成为释放唯一通道。inc-before-dec 防自赋值（`x = x`）。
    pub(crate) fn emit_env_owned_class_assign(
        &mut self,
        place: LocalId,
        store_ty: &str,
        store_val: &str,
        rvalue: &MirRvalue,
        effective_ty: &TypeId,
    ) {
        // 仅拷贝语义 rvalue inc 新值（`new`/Call 移交所有权不 inc）。
        let retain = Self::assign_needs_arc_retain(rvalue, effective_ty, self.layouts);
        self.emit_env_owned_class_store(place, store_ty, store_val, retain);
    }

    /// RFC 016：env 唯一 owner class 局部写穿的统一实现。
    ///
    /// `retain=true` 时先 inc 新值（拷贝语义 / await 借引用结果）；然后写穿到
    /// env 字段（load 旧 → store 新 → dec 旧，inc-before-dec 防自赋值），最后
    /// 同步 alloca 镜像供 body 读取。env 字段为唯一 owner，由 dtor + EH cleanup
    /// pad 释放恰一次；alloca 仅为当前值镜像，不改变所有权。
    fn emit_env_owned_class_store(
        &mut self,
        place: LocalId,
        store_ty: &str,
        store_val: &str,
        retain: bool,
    ) {
        let env_type = self.sm_env_type.clone();
        let field_idx = self.local_env_field_index(place);
        if retain {
            self.emit(&format!("call void @rt_arc_inc(ptr {store_val})"));
        }
        let field_ptr = self.fresh_temp();
        self.emit(&format!(
            "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
        ));
        let old = self.fresh_temp();
        self.emit(&format!("{old} = load {store_ty}, ptr {field_ptr}"));
        self.emit(&format!("store {store_ty} {store_val}, ptr {field_ptr}"));
        self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
        // alloca 镜像同步（env 已写穿，镜像供 body 后续读取）。
        let ptr = self.local_ptr(place);
        self.emit(&format!("store {store_ty} {store_val}, ptr {ptr}"));
    }

    /// RFC 009 M3：spilled local 的 LLVM 堆槽类型（供 `getelementptr …, ptr null,
    /// i32 1` 尺寸技巧与 memcpy 使用）。仅 struct / variant 值类型可超过
    /// SPILL_THRESHOLD；其余类型返回 None（不会进入 spill_set）。
    fn spill_slot_type(&self, ty: &TypeId) -> Option<String> {
        match ty {
            TypeId::Named(name) => {
                if self.layouts.structs.contains_key(name) {
                    Some(format!("%struct.{name}"))
                } else if self.layouts.variants.contains_key(name) {
                    Some(format!("%variant.{name}"))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// 发射 env struct type 定义。
    fn emit_env_struct_type(&self, env_type: &str, fields: &[TypeId]) -> String {
        let tys: Vec<String> = fields
            .iter()
            .map(|ty| llvm_type_of(ty, self.layouts))
            .collect();
        format!("{env_type} = type {{ {} }}\n", tys.join(", "))
    }

    /// 发射 resume 函数：switch state；state 0 发射完整 CFG。
    fn emit_resume_function(
        &mut self,
        name: &str,
        resume_name: &str,
        env_type: &str,
        dtor_name: &str,
    ) -> String {
        let n_awaits = self.sm_await_count;
        // Zero-cost EH milestone ②：try/catch 可能出现在 async body 内，经
        // emit_try_catch_seh 在 resume 函数中发射 invoke/catchswitch，故 resume
        // 函数须随 async 函数 may-throw 状态附加 uwtable + personality。
        // RFC 016：resume 函数级 EH cleanup pad 存在时（有 env 唯一 owner 的
        // class 局部），**强制**附加 uwtable + personality——WinEH 要求
        // cleanuppad 所在函数必须带 personality（否则 clang 报
        // "CleanupPadInst needs to be in a function with a personality"）。
        let may_throw = self.callee_may_throw(name);
        let has_cleanup = self.has_env_owned_class_local();
        let mut attr_str = String::new();
        let mut eh_suffix = String::new();
        if self.is_windows && (may_throw || has_cleanup) {
            attr_str.push_str(" uwtable");
            eh_suffix.push_str(" personality ptr @__CxxFrameHandler3");
        }
        self.output.push_str(&format!(
            "define i32 @{resume_name}(ptr %env_ptr, ptr %waker){}{}{} {{\n",
            attr_str,
            eh_suffix,
            self.dbg_attr()
        ));
        self.output.push_str("entry:\n");

        // Func/Action 形参运行时为 arc_closure*（与 emit_fn 一致）。状态机 resume
        // 路径缺此标记时，`await f()` 的 IndirectCall 会把 arc_closure* 当裸函数
        // 指针调用（`call ptr %f()`）→ 0xC0000005（async Func 方法组缺陷根因）。
        for (i, (_, ty)) in self.cfg.params.iter().enumerate() {
            if is_delegate_type(ty) {
                self.closure_locals.insert(LocalId(i as u32));
            }
        }

        // 为每个 local 创建 alloca（复用 local_ptr 语义，使现有 emit_stmt 无需修改）
        let locals_for_alloca: Vec<(LocalId, TypeId)> = self
            .cfg
            .locals
            .iter()
            .map(|(id, (_, ty))| {
                let field_ty = if matches!(ty, TypeId::Void) {
                    TypeId::Int
                } else {
                    ty.clone()
                };
                (*id, field_ty)
            })
            .collect();
        for (id, field_ty) in &locals_for_alloca {
            let ptr = self.local_ptr(*id);
            let ty_str = if self.is_byref_captured_local(*id) {
                "ptr".to_string()
            } else {
                llvm_type_of(field_ty, self.layouts)
            };
            self.emit(&format!("{ptr} = alloca {ty_str}"));
            if ty_str == "ptr" {
                self.emit(&format!("store ptr null, ptr {ptr}"));
            }
        }

        // RFC 016：从 env 字段 load 初始值到 alloca——**仅 env 局部**（参数 ∪
        // 捕获 ∪ 跨 await 存活 ∪ spill），且为**纯镜像 load（不 inc）**。
        // env 唯一 owner：class 局部 +1 只由 env 字段持有；alloca 仅为当前值镜像，
        // 供 body 读取。未存活普通局部零 env 字段、零装载（段内自建自毁）。
        // 捕获局部（ByRef 槽地址借引用）也纯 load——槽地址跨 await 恒定。
        let locals_for_load: Vec<(LocalId, TypeId)> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, (_, ty))| {
                !matches!(ty, TypeId::Void) && self.sm_env_local_index.contains_key(id)
            })
            .map(|(id, (_, ty))| (*id, ty.clone()))
            .collect();
        for (id, ty) in &locals_for_load {
            let field_idx = self.local_env_field_index(*id);
            let field_ptr = self.fresh_temp();
            let ty_str = if self.is_byref_captured_local(*id) {
                // ByRef 捕获镜像：alloca 存「外层权威槽地址」，恒 load/store ptr。
                "ptr".to_string()
            } else {
                llvm_type_of(ty, self.layouts)
            };
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
            ));
            let loaded = self.fresh_temp();
            self.emit(&format!("{loaded} = load {ty_str}, ptr {field_ptr}"));
            // 纯镜像：不 inc/dec（env 唯一 owner）。body 内对 env-owned class 局部
            // 的 Assign/await 提取走 `emit_env_owned_class_assign` 写穿到 env 字段，
            // alloca 同步镜像；此处 resume 装载同步 env→alloca。捕获局部为借引用
            //（外层 env 持所有权），同样纯 load，不改变所有权。
            let local_alloca = self.local_ptr(*id);
            self.emit(&format!("store {ty_str} {loaded}, ptr {local_alloca}"));
        }

        // Buffer the post-entry tail (switch + state 0 CFG + default): hoisted
        // entry allocas accumulate while the CFG blocks are emitted and must be
        // spliced into the entry block before the switch terminator.
        let entry_prefix = std::mem::take(&mut self.output);

        // switch state
        // 0 → 完整 CFG；1+i → sm_resume_i；1+n+i → sm_reenter_i（抢占重入 await）
        let state_ptr = self.fresh_temp();
        self.emit(&format!(
            "{state_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        let state_val = self.fresh_temp();
        self.emit(&format!("{state_val} = load i32, ptr {state_ptr}"));
        self.output
            .push_str(&format!("  switch i32 {state_val}, label %sm_default [\n"));
        self.output.push_str("    i32 0, label %sm_state_0\n");
        for i in 0..n_awaits {
            let resume_state = 1 + i;
            self.output
                .push_str(&format!("    i32 {resume_state}, label %sm_resume_{i}\n"));
        }
        for i in 0..n_awaits {
            let reenter_state = 1 + n_awaits + i;
            self.output
                .push_str(&format!("    i32 {reenter_state}, label %sm_reenter_{i}\n"));
        }
        self.output.push_str("  ]\n");

        // RFC 016：resume 函数级 EH cleanup pad。CFG body 发射期间设为 `Some`，
        // 使区域外（不在任何 try/finally/catch 内）的 may-throw 调用
        // （faulted await 的 rt_throw / 异常传播）经 invoke 落入本 pad，cleanup
        // 一次性调用 dtor 释放 env 持有的 class 引用 + 释放 env。仅当存在
        // env 唯一 owner 的 class 局部时才需要（否则无 class 引用需释放）。
        let cleanup_label = "sm_cleanup".to_string();
        if has_cleanup {
            self.sm_cleanup_label = Some(cleanup_label.clone());
        }

        // state 0：整图 CFG（与同步函数同构）
        self.output.push_str("sm_state_0:\n");
        self.emit(&format!("br label %bb{}", self.cfg.entry.0));

        let mut blocks: Vec<MirBlock> = self
            .cfg
            .blocks
            .values()
            .filter(|b| !is_dead_block(b))
            .cloned()
            .collect();
        blocks.sort_by_key(|b| b.id.0);
        for block in &blocks {
            self.emit_cfg_block(block);
        }

        // default: 非法 state（腐败/竞态）。RFC 016 下 save_locals 为纯值回写
        //（env 唯一 owner，无所有权转移），ret 前幂等同步 env 字段。
        self.output.push_str("sm_default:\n");
        self.emit_sm_save_locals(env_type);
        self.emit("ret i32 0"); // RT_TASK_READY

        // RFC 016：resume 级 EH cleanup pad（深层 unwind 路径，正常路径不可达）。
        // 任何未捕获的异常（faulted await 的 rt_throw 未落入本函数 catchswitch、
        // 用户调用抛异常且无 try 接住）unwind 到此处 → 调 dtor 释放 env 的 class
        // 引用 + free(env) → cleanupret unwind to caller。与正常 return 路径的
        // rt_task_release → dtor 互斥（要么正常完成、要么 unwind），env 恰释放一次。
        if has_cleanup {
            self.output.push_str(&format!("{cleanup_label}:\n"));
            let cp = self.fresh_temp();
            self.emit(&format!("{cp} = cleanuppad within none []"));
            // WinEH 要求 funclet 内的 call 带 funclet token（否则 clang -O0 丢 call）。
            self.emit(&format!(
                "call void @{dtor_name}(ptr %env_ptr) [ \"funclet\"(token {cp}) ]"
            ));
            self.emit(&format!("cleanupret from {cp} unwind to caller"));
        }
        // 发射结束后清除，避免影响后续函数（同 FnEmitter 复用）。
        self.sm_cleanup_label = None;

        let tail_out = std::mem::take(&mut self.output);
        self.output = entry_prefix;
        // Hoist expression-temp allocas into the entry block before the switch.
        self.flush_entry_allocas();
        self.output.push_str(&tail_out);
        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }

    /// 在状态机路径内发射单个 await（由 `emit_stmt` 在 `in_state_machine` 时调用）。
    pub(super) fn emit_sm_await_site(
        &mut self,
        place: LocalId,
        task: &MirRvalue,
        await_idx: usize,
    ) {
        let env_type = self.sm_env_type.clone();
        let n = self.sm_await_count;
        let resume_state = 1 + await_idx;
        let reenter_state = 1 + n + await_idx;
        self.emit_sm_await(
            place,
            task,
            &env_type,
            await_idx,
            resume_state,
            reenter_state,
        );
    }

    /// 发射状态机 await 逻辑。
    ///
    /// - `await_idx`: 全局 await 序号（标签后缀）
    /// - `resume_state`: suspend 时存入 env->state，switch → `sm_resume_{await_idx}`
    /// - `reenter_state`: 抢占 suspend 时存入，switch → `sm_reenter_{await_idx}` 重跑本 await
    ///
    /// 完成后落入 `sm_after_await_{await_idx}`，由同块后续语句继续。
    fn emit_sm_await(
        &mut self,
        place: LocalId,
        task: &MirRvalue,
        env_type: &str,
        await_idx: usize,
        resume_state: usize,
        reenter_state: usize,
    ) {
        let place_ty = self.local_type(place);
        let task_expected = TypeId::Task {
            inner: Box::new(place_ty.clone()),
        };

        // 抢占重入与顺控共用入口：先终止上一基本块，再进入 sm_reenter。
        self.emit(&format!("br label %sm_reenter_{await_idx}"));
        self.output.push_str(&format!("sm_reenter_{await_idx}:\n"));

        let (_, task_val) = self.emit_rvalue_typed(task, &task_expected);

        /* RFC 009 M2：await 边界前抢占检查。
         * 获取当前 worker ctx，检查 preempt_requested 标志。
         * 若已设置：清除标志 → 保存 locals 到 env → 设置 state=reenter
         * → 返回 RT_TASK_PENDING。下次 resume 从 sm_reenter_i 重跑本 await。 */
        let no_preempt_label = self.fresh_label();
        let preempt_suspend_label = format!("sm_preempt_suspend_{await_idx}");
        let worker_ctx = self.fresh_temp();
        self.emit(&format!(
            "{worker_ctx} = call ptr @rt_threadpool_current_worker_ctx()"
        ));
        let preempt_flag = self.fresh_temp();
        self.emit(&format!(
            "{preempt_flag} = call i32 @rt_worker_preempt_check(ptr {worker_ctx})"
        ));
        let is_preempted = self.fresh_temp();
        self.emit(&format!("{is_preempted} = icmp ne i32 {preempt_flag}, 0"));
        self.emit(&format!(
            "br i1 {is_preempted}, label %{preempt_suspend_label}, label %{no_preempt_label}"
        ));

        /* 抢占 suspend：清除标志 → 保存 locals → 设置 state → 返回 PENDING */
        self.output.push_str(&format!("{preempt_suspend_label}:\n"));
        self.emit(&format!(
            "call void @rt_worker_preempt_clear(ptr {worker_ctx})"
        ));
        self.emit_sm_save_locals(env_type);
        let preempt_state_ptr = self.fresh_temp();
        self.emit(&format!(
            "{preempt_state_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        self.emit(&format!(
            "store i32 {reenter_state}, ptr {preempt_state_ptr}"
        ));
        self.emit("ret i32 1"); // RT_TASK_PENDING

        /* 无抢占：继续正常 await 逻辑 */
        self.output.push_str(&format!("{no_preempt_label}:\n"));

        // 存储 task 到 env->awaiter (field 1)
        let awaiter_ptr = self.fresh_temp();
        self.emit(&format!(
            "{awaiter_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 1"
        ));
        self.emit(&format!("store ptr {task_val}, ptr {awaiter_ptr}"));

        // poll task
        let status = self.fresh_temp();
        self.emit(&format!(
            "{status} = call i32 @rt_task_poll(ptr {task_val})"
        ));
        let pending = self.fresh_temp();
        self.emit(&format!("{pending} = icmp eq i32 {status}, 1"));

        let suspend_label = format!("sm_suspend_{await_idx}");
        let resume_label = format!("sm_resume_{await_idx}");
        self.emit(&format!(
            "br i1 {pending}, label %{suspend_label}, label %{resume_label}"
        ));

        // suspend: 注册 waker → 保存 locals → 设置 state → ret PENDING
        self.output.push_str(&format!("{suspend_label}:\n"));
        let outer_ptr = self.fresh_temp();
        self.emit(&format!(
            "{outer_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 2"
        ));
        let outer_val = self.fresh_temp();
        self.emit(&format!("{outer_val} = load ptr, ptr {outer_ptr}"));
        self.emit(&format!(
            "call void @rt_task_register_waker(ptr {task_val}, ptr {outer_val})"
        ));
        self.emit_sm_save_locals(env_type);
        let state_ptr = self.fresh_temp();
        self.emit(&format!(
            "{state_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        self.emit(&format!("store i32 {resume_state}, ptr {state_ptr}"));
        self.emit("ret i32 1"); // RT_TASK_PENDING

        // resume: re-poll 提取（非配对唤醒防御，与协程路径 emit_coro_await 同构）。
        // 历史教训：此处曾「零 re-poll 直达提取」——await_waiting 守卫位可被
        // 非配对 coro_wake 清除（complete/register 时序交错、slab 指针复用交叠），
        // EventLoop 合法推进挂起帧时 inner 可能仍 PENDING → ptr_result 空 →
        // await 得 null（l2_net_batch accept-null 实证）。resume 后先 re-poll：
        // READY 直下提取；PENDING 重新登记 waker（幽灵唤醒会消费 waker 槽，inner
        // 真完成前必须重新挂上）+ 存 state + ret PENDING——下次 resume 再入本块
        // 重 poll（状态机无 re-entrant suspend 限制，天然回环）。
        self.output.push_str(&format!("{resume_label}:\n"));
        let awaiter_ptr2 = self.fresh_temp();
        self.emit(&format!(
            "{awaiter_ptr2} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 1"
        ));
        let task_val2 = self.fresh_temp();
        self.emit(&format!("{task_val2} = load ptr, ptr {awaiter_ptr2}"));
        let st_repoll = self.fresh_temp();
        self.emit(&format!(
            "{st_repoll} = call i32 @rt_task_poll(ptr {task_val2})"
        ));
        let pending_repoll = self.fresh_temp();
        self.emit(&format!("{pending_repoll} = icmp eq i32 {st_repoll}, 1"));
        let resume_wait_label = self.fresh_label();
        let resume_go_label = self.fresh_label();
        self.emit(&format!(
            "br i1 {pending_repoll}, label %{resume_wait_label}, label %{resume_go_label}"
        ));

        // re-poll PENDING：重新登记 waker → 存 state → ret PENDING（回环再入）。
        self.output.push_str(&format!("{resume_wait_label}:\n"));
        let outer_ptr2 = self.fresh_temp();
        self.emit(&format!(
            "{outer_ptr2} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 2"
        ));
        let outer_val2 = self.fresh_temp();
        self.emit(&format!("{outer_val2} = load ptr, ptr {outer_ptr2}"));
        self.emit(&format!(
            "call void @rt_task_register_waker(ptr {task_val2}, ptr {outer_val2})"
        ));
        self.emit_sm_save_locals(env_type);
        let state_ptr2 = self.fresh_temp();
        self.emit(&format!(
            "{state_ptr2} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        self.emit(&format!("store i32 {resume_state}, ptr {state_ptr2}"));
        self.emit("ret i32 1"); // RT_TASK_PENDING

        // task 已完成，提取结果到 local alloca
        self.output.push_str(&format!("{resume_go_label}:\n"));
        // Zero-cost EH milestone ⑦ (async 协作 · §2.5): faulted Task 的异常在
        // await 提取点 rethrow。若处于 try/finally 区域，`rt_throw` 经
        // emit_call_may_throw 发 invoke 落入本区域 catchswitch/cleanup funclet。
        let fault_label = format!("sm_faulted_{await_idx}");
        let extract_label = format!("sm_extract_{await_idx}");
        let faulted = self.fresh_temp();
        self.emit(&format!(
            "{faulted} = call i32 @rt_task_is_faulted(ptr {task_val2})"
        ));
        let faulted_b = self.fresh_temp();
        self.emit(&format!("{faulted_b} = icmp ne i32 {faulted}, 0"));
        self.emit(&format!(
            "br i1 {faulted_b}, label %{fault_label}, label %{extract_label}"
        ));
        self.output.push_str(&format!("{fault_label}:\n"));
        // ⚠️ 已知缺陷 A2（RFC 016 范畴 · 2026-08-07 审查标记）：
        // 此路径 rt_throw 前不调 save_locals，resume 入口对 class alloca 的 inc
        // 在异常**未捕获**传播出 resume 函数时泄漏（+1 永不 dec）。不能简单补
        // save_locals——若异常被本函数 try/catch 捕获，catch body 的 Assign 会
        // 与 save_locals 的 dec 形成 double-dec → UAF。正确修复需 resume 函数级
        // EH cleanup pad（unwind 时 save_locals），属 zero-cost EH 与 async SM
        // 交互的复杂 codegen，RFC 016 审计立项。此路径泄漏 class local +1（非
        // 返回值），与 A1/A4 的 ptr_result 泄漏独立。
        let exc = self.fresh_temp();
        self.emit(&format!(
            "{exc} = call ptr @rt_task_get_exception(ptr {task_val2})"
        ));
        // RFC 016 子项 M2：异常所有权统一转移。Task 持唯一引用（rt_task_fault 转移
        // throw 在途 +1 / FromException 发射处 inc），rt_task_release 对 FAULTED
        // dec ptr_result。必须先 inc（授予 catch 独立副本）再 release（归还 Task
        // 所有权）：顺序颠倒会在 release dec→0 后对已释放异常 UAF。
        self.emit(&format!("call void @rt_arc_inc(ptr {exc})"));
        self.emit(&format!("call void @rt_task_release(ptr {task_val2})"));
        self.emit_call_may_throw("void", "@rt_throw", &format!("ptr {exc}"), true, None);
        self.emit("unreachable");
        self.output.push_str(&format!("{extract_label}:\n"));
        // 提取 inner Task 结果到 result 局部 + 释放 inner Task（与 coroutine 路径共用）
        self.emit_await_extract(place, &task_val2);
        let after_label = format!("sm_after_await_{await_idx}");
        self.emit(&format!("br label %{after_label}"));

        // 同块后续语句从此继续
        self.output.push_str(&format!("{after_label}:\n"));
        // 变量捕获同步：await 期间 lambda 可能经槽写回被捕获局部（env 字段），
        // 而本函数 alloca 仍是 resume 入口 load 的旧值。await 完成后重载
        // 被捕获局部（RFC 016 下为**纯镜像 store**——env 唯一 owner，alloca 不持
        // 所有权，故不做 ARC 覆写），使后续读取看到 lambda 写回的新值
        //（stream_events `fail:invoked` 实测）。
        // 仅重载 **ByRef（变量捕获）** 局部：lambda 经槽写穿到 env 字段，外层
        // alloca 需要同步。ByValue 捕获是副本，lambda 无法写回，重载无意义。
        // 按局部 LLVM 类型过滤——ByRef 均为指针（class/string/object/array）；
        // 对值类型（i32 等）做 ptr load/store 会把相邻字段/alloca 一起读入或
        // 覆盖（async_lambda_e2e multi-capture 实测：baseValue/multiplier 的
        // i32 槽被 8 字节 ptr 覆写，腐蚀相邻 ctr 槽）。
        // 跳过本函数**自身**的捕获局部（变量捕获借引用）：async lambda 的捕获
        // env 槽存外层变量槽地址，对其 inc/dec 会腐蚀外层 env 字段；槽地址跨
        // await 恒定，也无需重载。
        let own_captured: HashSet<LocalId> =
            self.cfg.captures.iter().map(|(id, _, _)| *id).collect();
        for id in self.captured_outer_locals() {
            if own_captured.contains(&id) {
                continue;
            }
            let ty = self.local_type(id);
            if matches!(ty, TypeId::Void) {
                continue;
            }
            if llvm_type_of(&ty, self.layouts) != "ptr" {
                continue;
            }
            // RFC 016：仅重载 env 局部（非 env 局部无 env 字段，段内自管理）。
            if !self.is_env_local(id) {
                continue;
            }
            let field_idx = self.local_env_field_index(id);
            let field_ptr = self.fresh_temp();
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
            ));
            let loaded = self.fresh_temp();
            self.emit(&format!("{loaded} = load ptr, ptr {field_ptr}"));
            // RFC 016：纯镜像 store（env 唯一 owner）。
            let local_alloca = self.local_ptr(id);
            self.emit(&format!("store ptr {loaded}, ptr {local_alloca}"));
        }
    }

    /// 提取 inner Task 结果到 `place` 局部并释放 inner Task。状态机与 coroutine
    /// 路径共用：`in_state_machine` 决定 env 唯一 owner 分支（coroutine 为 false，
    /// 恒走 C11 alloca 覆写分支，帧槽语义）。
    pub(super) fn emit_await_extract(&mut self, place: LocalId, task_val2: &str) {
        let place_ty = self.local_type(place);
        if !matches!(place_ty, TypeId::Void) {
            let ptr = self.local_ptr(place);
            let slot_ty = llvm_type_of(&place_ty, self.layouts);
            match &place_ty {
                TypeId::Int
                | TypeId::Short
                | TypeId::Byte
                | TypeId::Char
                | TypeId::Bool
                | TypeId::UInt
                | TypeId::UShort
                | TypeId::SByte => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_task_result_int(ptr {task_val2})"
                    ));
                    // bool 槽位是 i1，rt_task_result_int 返回 i32：store 前 trunc。
                    if slot_ty != "i32" {
                        let narrowed = self.fresh_temp();
                        self.emit(&format!("{narrowed} = trunc i32 {result} to {slot_ty}"));
                        self.emit(&format!("store {slot_ty} {narrowed}, ptr {ptr}"));
                    } else {
                        self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                    }
                }
                TypeId::Named(n) if self.layouts.enums.contains(n.as_str()) => {
                    // 枚举是 i32 值类型：await 返回枚举的 Task 须走
                    // `rt_task_result_int` 提取并 store i32，不能落入下方
                    // `TypeId::Named(_)` 的 ptr 提取路径（A1 修复根因）。
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_task_result_int(ptr {task_val2})"
                    ));
                    self.emit(&format!("store i32 {result}, ptr {ptr}"));
                }
                TypeId::String
                | TypeId::Object
                | TypeId::Named(_)
                | TypeId::Array { .. }
                | TypeId::Task { .. } => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_task_result_ptr(ptr {task_val2})"
                    ));
                    if self.is_env_owned_class_local(place) {
                        // RFC 016：await 结果是 task 的「借引用」。env 唯一 owner：
                        // 写穿到 env 字段并强制 retain 授予 env +1（仅状态机路径）。
                        self.emit_env_owned_class_store(place, &slot_ty, &result, true);
                    } else if Self::arc_class_place(&place_ty, self.layouts) {
                        // C11：await 结果是 task 的「借引用」，此处覆写须 dec 旧值、
                        // inc 新值（授予 alloca 所有权），与后续 suspend 的 dec 配对。
                        let old = self.fresh_temp();
                        self.emit(&format!("{old} = load {slot_ty}, ptr {ptr}"));
                        self.emit(&format!("call void @rt_arc_inc(ptr {result})"));
                        self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                        self.emit(&format!("call void @rt_arc_dec(ptr {old})"));
                    } else {
                        self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                    }
                }
                TypeId::Long | TypeId::Float | TypeId::Double | TypeId::ULong => {
                    let size = match place_ty {
                        TypeId::Long | TypeId::ULong => 8,
                        TypeId::Float => 4,
                        TypeId::Double => 8,
                        _ => unreachable!(),
                    };
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = alloca {slot_ty}"));
                    self.emit(&format!(
                        "call void @rt_task_result_value(ptr {task_val2}, ptr {tmp}, i32 {size})"
                    ));
                    let result = self.fresh_temp();
                    self.emit(&format!("{result} = load {slot_ty}, ptr {tmp}"));
                    self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                }
                _ => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_task_result_int(ptr {task_val2})"
                    ));
                    self.emit(&format!("store {slot_ty} {result}, ptr {ptr}"));
                }
            }
        }
        // M5.2：await 的 inner Task 结果已提取，立即释放回 slab。
        self.emit(&format!("call void @rt_task_release(ptr {task_val2})"));
    }

    /// 变量捕获语义（C# 闭包）：当前函数中被 lambda 捕获的局部 id 集合。
    ///
    /// 遍历当前 MIR 的所有操作数，收集 `MirOperand::Closure { env }` 的 env
    /// 中被捕获的 `Local`（外层局部 id）。这些局部经闭包 env 槽读写，await
    /// 期间 lambda 写槽后本函数 alloca 需同步重载（见 `emit_sm_await` 的
    /// sm_after_await 段）。与 emit_closure_value 的槽地址捕获配对。
    fn captured_outer_locals(&self) -> HashSet<LocalId> {
        let mut out = HashSet::new();
        for block in self.cfg.blocks.values() {
            for stmt in &block.statements {
                collect_captured_locals_stmt(stmt, &mut out);
            }
            if let MirTerminator::Return(Some(op)) = &block.terminator {
                collect_captured_locals_operand(op, &mut out);
            }
        }
        out
    }

    /// 保存 env locals（非 Void）到 env 字段。在 suspend / return / preempt /
    /// default 前调用。
    ///
    /// **RFC 016（env 唯一 owner）**：save_locals 退化为**纯值回写**——env 字段
    /// 持有 class 局部唯一 +1，body 内对 env-owned class 局部的 Assign/await 提取
    /// 已写穿到 env 字段（见 `emit_env_owned_class_assign`），此处仅把 alloca 的
    /// 当前值同步回 env 供跨 await 读取（对 class 为幂等镜像，对值类型为必需）。
    /// **不**做任何 ARC 覆写/所有权转移（C11/C12 手工配对全部移除）。释放由
    /// dtor + EH cleanup pad 统一完成。
    ///
    /// 仅处理 **env 局部**（参数 ∪ 捕获 ∪ 跨 await 存活 ∪ spill）；未存活普通
    /// 局部零 env 字段、零 save（段内自建自毁）。
    fn emit_sm_save_locals(&mut self, env_type: &str) {
        let locals: Vec<(LocalId, TypeId)> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, (_, ty))| {
                !matches!(ty, TypeId::Void) && self.sm_env_local_index.contains_key(id)
            })
            .map(|(id, (_, ty))| (*id, ty.clone()))
            .collect();
        for (id, ty) in &locals {
            let field_idx = self.local_env_field_index(*id);
            let field_ptr = self.fresh_temp();
            let ty_str = if self.is_byref_captured_local(*id) {
                // ByRef 捕获镜像：alloca/env 字段均存「外层权威槽地址」，
                // 恒 load/store ptr（按值类型 i32 截断槽地址 → 腐败）。
                "ptr".to_string()
            } else {
                llvm_type_of(ty, self.layouts)
            };
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 {field_idx}"
            ));
            let local_alloca = self.local_ptr(*id);
            let loaded = self.fresh_temp();
            self.emit(&format!("{loaded} = load {ty_str}, ptr {local_alloca}"));
            if self.spill_set.contains(&(id.0 as usize)) {
                // RFC 009 M3：spilled local —— env 字段是堆槽指针（不覆写）。
                // 把 local 当前值（ptr to struct/variant）的字节拷贝进堆槽，
                // 使跨 await 的读走槽获得最新数据。spilled 均为值类型，无 ARC。
                let slot = self.fresh_temp();
                self.emit(&format!("{slot} = load ptr, ptr {field_ptr}"));
                let slot_ty = self
                    .spill_slot_type(ty)
                    .expect("spilled local must have a struct/variant slot type");
                let slot_size = self.fresh_temp();
                self.emit(&format!(
                    "{slot_size} = ptrtoint ptr getelementptr ({slot_ty}, ptr null, i32 1) to i64"
                ));
                self.emit(&format!(
                    "call void @llvm.memcpy.p0.p0.i64(ptr {slot}, ptr {loaded}, i64 {slot_size}, i1 false)"
                ));
            } else {
                // RFC 016：纯值回写。class 局部 env 已持所有权（body 写穿），此处
                // 幂等镜像；捕获局部（ByRef 槽地址借引用）同样纯 store（槽地址跨
                // await 恒定）；值类型为必需的 env 同步。不做 ARC 覆写——对槽地址
                // inc/dec 会把外层 env 字段首 4 字节当 refcount，腐蚀 class 指针。
                self.emit(&format!("store {ty_str} {loaded}, ptr {field_ptr}"));
            }
        }
    }

    /// 发射状态机 return 逻辑（由 emit_terminator 在 in_state_machine=true 时调用）。
    pub(super) fn emit_sm_return(&mut self, val: &Option<MirOperand>, env_type: &str) {
        let inner_ty = self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void);

        // C12 ptr_result 借引用模型（2026-08-07 审查修复 A1/A4）：
        // ptr_result 是「借引用」——task 不持独立 +1，返回值的 +1 完全由 env
        // （save_locals 的 ARC 覆写维护）持有，dtor 释放。set_result_ptr 裸
        // store 借用指针；外层 await 提取方独立 inc/dec 自平衡。
        //
        // 历史 C11 inc 已移除：原 inc 企图让 task 持独立引用以防 save_locals
        // dec 后 free，但 save_locals 的 inc-before-dec（L972 inc / L977 dec）
        // 已保证对象存活到 store env 后，C11 inc 是冗余的。更致命的是
        // rt_task_release 不 dec ptr_result（结果为借引用），C11 inc 的 +1
        // 永不释放 → class 返回值引用计数永久 +1 泄漏。删除 inc 后全链路平衡：
        //   save_locals: inc alloca → store env → dec alloca（env 持 +1）
        //   set_result_ptr: 裸 store 借用指针（task 不持 +1）
        //   dtor: dec env.X（释放 env 的 +1）
        //   外层 await: inc（外层 alloca +1）→ scope dec（释放）
        let is_class_ret = val.as_ref().is_some()
            && !matches!(inner_ty, TypeId::Void)
            && Self::arc_class_place(&inner_ty, self.layouts);
        let ret_slot = if is_class_ret {
            let (ty, val_str) = self.emit_operand(val.as_ref().unwrap());
            Some((ty, val_str))
        } else {
            None
        };

        // 保存 locals 到 env（确保跨 return 的 local 状态可被外部观察，如 Task.Result 后续扩展）
        self.emit_sm_save_locals(env_type);

        // 设置 state = -1 (Completed)
        let state_ptr = self.fresh_temp();
        self.emit(&format!(
            "{state_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        self.emit(&format!("store i32 -1, ptr {state_ptr}"));

        // 读取 task_ptr (field 2)
        let task_ptr_field = self.fresh_temp();
        self.emit(&format!(
            "{task_ptr_field} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 2"
        ));
        let task_ptr = self.fresh_temp();
        self.emit(&format!("{task_ptr} = load ptr, ptr {task_ptr_field}"));

        // 写入 result 到 Task 句柄
        if let Some(op) = val {
            if !matches!(inner_ty, TypeId::Void) {
                let (ty, val_str) = if let Some(slot) = ret_slot {
                    slot
                } else {
                    self.emit_operand(op)
                };
                // 通道按值实际发射的 LLVM 表示选择（emit_task_set_result_abi，
                // 唯一权威）：inner_ty 推断残缺（lambda 块体 return 局部在
                // 宿主 ctx 不可解析 → Bool→Int fallback）会误报 Int 族，
                // set_result_int(ptr) 截断引用（ptr_result 恒 null，外层
                // await 按引用提取得 null——嵌套 async lambda 引用丢失）。
                //
                // RFC 009 §结果所有权（强持有，2026-08-22 收敛）：class 结果
                // 走 rt_task_set_result_class 置 ptr_is_class=1，task 统一
                // dec（string/array 仍走 emit_task_set_result_abi 借用路径，
                // immortal 语义）。**无条件 inc**：inc 与 release 的 dec 严格
                // 配对（净平衡），不触碰 env/dtor 既有镜像平衡——借用源
                // （字段/参数）由 inc 授予 task +1；env-owned 局部 env 另持
                // +1（dtor 释放），task 的 +1 独立成立。
                if is_class_ret {
                    self.emit(&format!("call void @rt_arc_inc(ptr {val_str})"));
                    self.emit(&format!(
                        "call void @rt_task_set_result_class(ptr {task_ptr}, ptr {val_str})"
                    ));
                } else {
                    self.emit_task_set_result_abi(&task_ptr, &ty, &val_str);
                }
            }
        }

        self.emit("ret i32 0"); // RT_TASK_READY
    }

    /// 发射状态机 return 逻辑（由 `emit_stmt::Return` 在 `in_state_machine=true` 时调用）。
    /// 与 `emit_sm_return` 相同，但接收 `MirRvalue` 而非 `MirOperand`（stmt 内 return
    /// 值尚未 lower 为 operand）。
    pub(super) fn emit_sm_return_stmt(&mut self, val: &Option<MirRvalue>, env_type: &str) {
        let inner_ty = self.cfg.ret.task_inner().cloned().unwrap_or(TypeId::Void);

        // C12 ptr_result 借引用模型（同 emit_sm_return，A1/A4 修复）：
        // 不 inc——返回值 +1 由 env（save_locals）持有、dtor 释放。
        let is_class_ret = val.as_ref().is_some()
            && !matches!(inner_ty, TypeId::Void)
            && Self::arc_class_place(&inner_ty, self.layouts);
        let ret_slot = if is_class_ret {
            let (ty, val_str) = self.emit_rvalue_typed(val.as_ref().unwrap(), &inner_ty);
            Some((ty, val_str))
        } else {
            None
        };

        self.emit_sm_save_locals(env_type);

        let state_ptr = self.fresh_temp();
        self.emit(&format!(
            "{state_ptr} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 0"
        ));
        self.emit(&format!("store i32 -1, ptr {state_ptr}"));

        let task_ptr_field = self.fresh_temp();
        self.emit(&format!(
            "{task_ptr_field} = getelementptr {env_type}, ptr %env_ptr, i32 0, i32 2"
        ));
        let task_ptr = self.fresh_temp();
        self.emit(&format!("{task_ptr} = load ptr, ptr {task_ptr_field}"));

        if let Some(rv) = val {
            if !matches!(inner_ty, TypeId::Void) {
                let (ty, val_str) = if let Some(slot) = ret_slot {
                    slot
                } else {
                    self.emit_rvalue_typed(rv, &inner_ty)
                };
                // 通道按值实际发射的 LLVM 表示选择（emit_task_set_result_abi，
                // 与 emit_sm_return 同因同则：不信任 inner_ty 推断残缺副本）。
                //
                // RFC 009 §结果所有权（强持有，2026-08-22 收敛）：class 结果
                // 无条件 inc（与 release 的 dec 严格配对）+ set_result_class
                // 置位；string/array 走借用路径（immortal）。
                if is_class_ret {
                    self.emit(&format!("call void @rt_arc_inc(ptr {val_str})"));
                    self.emit(&format!(
                        "call void @rt_task_set_result_class(ptr {task_ptr}, ptr {val_str})"
                    ));
                } else {
                    self.emit_task_set_result_abi(&task_ptr, &ty, &val_str);
                }
            }
        }

        self.emit("ret i32 0"); // RT_TASK_READY
    }

    /// 发射构造函数：malloc env + 初始化 state=0 + 存储参数 + rt_task_from_state_machine。
    fn emit_sm_ctor(
        &mut self,
        internal: &str,
        resume_name: &str,
        env_type: &str,
        dtor_name: &str,
    ) -> String {
        let is_lambda =
            internal.starts_with("__async_resume___lambda") || internal.starts_with("__lambda");
        let param_strs: Vec<String> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (_, ty))| {
                let param_ty = if is_lambda || matches!(ty, TypeId::Ref { .. }) {
                    "ptr".to_string()
                } else {
                    llvm_type_of(ty, self.layouts)
                };
                format!("{} %arg{i}", param_ty)
            })
            .collect();

        self.output.push_str(&format!(
            "define {}ptr @{internal}({}){}{} {{\n",
            self.linkage_prefix(),
            param_strs.join(", "),
            self.comdat_attr(),
            self.dbg_attr()
        ));
        self.output.push_str("entry:\n");

        // calloc env（零初始化，state=0 即 Start）
        self.emit(&format!(
            "%env_size = ptrtoint ptr getelementptr ({env_type}, ptr null, i32 1) to i64"
        ));
        self.emit("%env = call ptr @calloc(i64 1, i64 %env_size)");

        let params: Vec<(String, TypeId, bool)> = self
            .cfg
            .params
            .iter()
            .enumerate()
            .map(|(i, (pname, ty))| (format!("%arg{i}"), ty.clone(), pname.as_str() == "__env__"))
            .collect();
        // RFC 016 配对面：ctor 仅对 **env 唯一 owner** 的 class 参数 inc（env 获取
        // 独立 +1），与 dtor 的 dec 集合逐字一致（env ∪ 非 capture ∪ 跨 await 存活
        // ∪ class place）。非 owner 的 class 参数为借用引用（caller 持有），env 字段
        // 仅存借指针、不 inc——否则其 +1 无人释放（body 不 Drop 参数，dtor 又非 owner
        // 不 dec）→ 泄漏 / 引用计数永久抬高。
        let capture_local_ids: HashSet<LocalId> =
            self.cfg.captures.iter().map(|(id, _, _)| *id).collect();
        for (i, (arg_str, ty, is_env_param)) in params.iter().enumerate() {
            if matches!(ty, TypeId::Void) {
                continue;
            }
            let field_idx = 3 + i;
            let field_ptr = self.fresh_temp();
            let ty_str = if (is_lambda && !is_env_param) || matches!(ty, TypeId::Ref { .. }) {
                "ptr".to_string()
            } else {
                llvm_type_of(ty, self.layouts)
            };
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env, i32 0, i32 {field_idx}"
            ));
            if is_lambda && !is_env_param {
                let loaded = self.fresh_temp();
                self.emit(&format!("{loaded} = load {ty_str}, ptr {arg_str}"));
                self.emit(&format!("store {ty_str} {loaded}, ptr {field_ptr}"));
            } else {
                self.emit(&format!("store {ty_str} {arg_str}, ptr {field_ptr}"));
                // C12: env 持独立所有权——**env 唯一 owner** 的 class 参数须 inc
                //（env 获取 +1，caller 仍持自己的 ref），与 dtor 的 dec 配对。
                // Ref 参数为借用（arc_class_place 返回 false）；非 owner 的 class
                // 参数（未跨 await 存活 / 被捕获）为借用，不 inc。
                let local_id = LocalId(i as u32);
                let is_env_owned = !capture_local_ids.contains(&local_id)
                    && self.await_live_locals.contains(&local_id);
                if Self::arc_class_place(ty, self.layouts) && is_env_owned {
                    self.emit(&format!("call void @rt_arc_inc(ptr {arg_str})"));
                }
            }
        }

        // RFC 009 M3：为每个 spilled local 分配零初始化堆槽，槽指针存入 env 字段。
        // 仅大值类型（struct/variant）可 spill；spilled 均非 param（MIR 分析排除
        // params，ctor 已拷贝 param 值进 env）。防御性支持 param：拷贝 arg 值进槽。
        let spill_slots: Vec<(LocalId, TypeId)> = self
            .cfg
            .locals
            .iter()
            .filter(|(id, _)| self.spill_set.contains(&(id.0 as usize)))
            .map(|(id, (_, ty))| (*id, ty.clone()))
            .collect();
        for (id, ty) in &spill_slots {
            let Some(slot_ty) = self.spill_slot_type(ty) else {
                continue;
            };
            let slot_size = self.fresh_temp();
            self.emit(&format!(
                "{slot_size} = ptrtoint ptr getelementptr ({slot_ty}, ptr null, i32 1) to i64"
            ));
            let slot = self.fresh_temp();
            self.emit(&format!(
                "{slot} = call ptr @calloc(i64 1, i64 {slot_size})"
            ));
            let field_idx = self.local_env_field_index(*id);
            let field_ptr = self.fresh_temp();
            self.emit(&format!(
                "{field_ptr} = getelementptr {env_type}, ptr %env, i32 0, i32 {field_idx}"
            ));
            self.emit(&format!("store ptr {slot}, ptr {field_ptr}"));
            // 防御：spilled param 的值拷贝进槽（MIR 当前排除 params，此分支不触发）。
            if (id.0 as usize) < self.cfg.param_count {
                let arg_str = format!("%arg{}", id.0);
                self.emit(&format!(
                    "call void @llvm.memcpy.p0.p0.i64(ptr {slot}, ptr {arg_str}, i64 {slot_size}, i1 false)"
                ));
            }
        }

        if is_lambda && !self.cfg.captures.is_empty() {
            let env_param_field_ptr = self.fresh_temp();
            self.emit(&format!(
                "{env_param_field_ptr} = getelementptr {env_type}, ptr %env, i32 0, i32 3"
            ));
            let capture_env_ptr = self.fresh_temp();
            self.emit(&format!(
                "{capture_env_ptr} = load ptr, ptr {env_param_field_ptr}"
            ));
            let captures_ref: Vec<&ast::LambdaCapture> =
                self.cfg.captures.iter().map(|(_, _, c)| c).collect();
            let capture_env_ty = self.env_struct_type(&captures_ref);
            let captures: Vec<(mir::LocalId, usize, ast::CaptureMode, ast::TypeId)> = self
                .cfg
                .captures
                .iter()
                .map(|(id, idx, c)| (*id, *idx, c.mode.clone(), c.ty.clone()))
                .collect();
            for (local_id, field_idx, mode, ty) in captures {
                let src_field_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{src_field_ptr} = getelementptr {capture_env_ty}, ptr {capture_env_ptr}, i32 0, i32 {field_idx}"
                ));
                let field_ty = match mode {
                    ast::CaptureMode::ByRef => "ptr".to_string(),
                    ast::CaptureMode::ByValue => llvm_type_of(&ty, self.layouts),
                };
                let loaded = self.fresh_temp();
                self.emit(&format!("{loaded} = load {field_ty}, ptr {src_field_ptr}"));
                let dst_field_idx = self.local_env_field_index(local_id);
                let dst_field_ptr = self.fresh_temp();
                self.emit(&format!(
                    "{dst_field_ptr} = getelementptr {env_type}, ptr %env, i32 0, i32 {dst_field_idx}"
                ));
                self.emit(&format!("store {field_ty} {loaded}, ptr {dst_field_ptr}"));
            }
        }

        self.emit(&format!(
            "%task = call ptr @rt_task_from_state_machine(ptr %env, ptr @{resume_name})"
        ));

        // C12: 登记 dtor_fn——rt_task_release 在释放 env 前调用它 dec 所有
        // class env 字段 + free(env)。未登记则 rt_task_release 直接 free(env)，
        // env 持有的 class 引用泄漏（rc 永不归零）。
        self.emit(&format!(
            "call void @rt_task_set_dtor_fn(ptr %task, ptr @{dtor_name})"
        ));

        let task_ptr_field = self.fresh_temp();
        self.emit(&format!(
            "{task_ptr_field} = getelementptr {env_type}, ptr %env, i32 0, i32 2"
        ));
        self.emit(&format!("store ptr %task, ptr {task_ptr_field}"));

        self.emit("ret ptr %task");

        self.output.push_str("}\n");
        std::mem::take(&mut self.output)
    }
}
