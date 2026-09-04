# RFC 048：命名管道与本机进程间通信（IPC）体系

状态：草案 → 评审中（2026-09-02 评审轮：性能/稳定性论证）
关联：RFC 025（网络协议层）· RFC 014（运行时 ABI）· RFC 016（验证式 FFI）· RFC 038（异步异步面）· RFC 046（通道，进程内 MPMC）· RFC 009（Reactor）
落点：`crates/runtime/rt_pipe.c` + `crates/runtime/platform/pipe_{windows,posix}.c` + `std/Net/Pipes/`

## 1. 动机与本位

本机多进程协作（工具链多进程编译、Agent 子进程协作、服务-客户端本机通信）需要比 TCP 更低开销、比匿名管道更灵活（跨无关进程、命名寻址）的传输面。当前 runtime 仅具备**子进程 stdio 重定向**用的匿名管道（`rt_proc.c`，`.ani` FFI，仅限父子关系）；命名管道（跨无关进程、命名寻址）完全缺位。

本 RFC 定义：**runtime 命名管道 ABI（双后端：Windows** **`\\.\pipe\name`** **/ POSIX FIFO）+** **`std/Net/Pipes/`** **门面（对齐** **`Arc.Net`** **家族范式）**。定位为**本机 IPC**：不做跨主机、不做消息边界语义（统一字节流模式）。

**跨平台是硬要求而非可选项**：runtime 已明确多平台（`rt_env_platform` = Windows/Linux/macOS/Android/iOS/OHOS；Reactor 四后端；抢占式调度 SIGURG）——命名管道作为传输面家族成员，**公开面与语义全平台一致，平台差异全部吸收在 rt 层**（Arc 用户代码零平台分支，单一惯用法）。见 §5。

**与 RFC 046 Channels 的分层关系**：Channels 是**进程内** MPMC 数据结构（内存队列 + 监视锁）；命名管道是**跨进程**字节传输（OS 内核缓冲）。二者互补不重叠——组合范式（管道收 ⇄ Channels 进程内分发）见 §8。

## 2. 基础面事实（决策依据）

- **Reactor 异步基建齐备**（IOCP / io\_uring / kqueue / poll 四后端），`rt_socket_*` 已走通 PENDING + RtIoCompletion 模式；POSIX 侧 io\_uring/kqueue 均可承载 FIFO fd 的异步读写。

- **rt\_socket 家族是 Core ABI 直射先例**（`rt_abi.h` + `[Builtin]` codegen 拦截）；`rt_proc_*` 走 `.ani` FFI。命名管道属长驻传输面，**选择 Core ABI 直射**（与 socket 同族，codegen 一致性）。

- **平台拆分两种既有约定**：reactor 走 `platform/` 多后端文件；`rt_proc.c` 走单文件 `#ifdef _WIN32`。管道双后端语义差异较大（Windows 连接状态机 vs FIFO 对组装），**采用 reactor 约定**：`rt_pipe.c`（公共语义/状态/名字规范化）+ `platform/pipe_windows.c` + `platform/pipe_posix.c`。

- **两套流契约并存**（既有债）：`Arc.IO.Stream`（字节抽象，Stable）与 `Arc.Net.StreamTransport`（string 面）。

- **已登记债务**：`rt_socket_accept_async` 的 accept-null 竞态（RFC 009 域，Reactor 深水区）——管道异步面设计必须吸取同族教训。

- 进程模型（`Arc.Diagnostics.Process`）已齐备，但**无句柄传递模型**——本 RFC 不做句柄传递，登记后续方向。

## 3. runtime ABI 面（`rt_abi.h` 单一声明，双后端实现）

句柄 `rt_pipe_t*`（不透明指针，同 `rt_socket` 家族风格）。**ABI 面全平台唯一**：

| ABI                                                       | 语义                                          | Windows 实现                          | POSIX 实现                               |
| --------------------------------------------------------- | ------------------------------------------- | ----------------------------------- | -------------------------------------- |
| `rt_pipe_server_create(name, max_instances) → rt_pipe_t*` | 创建服务端（不监听）                                  | `CreateNamedPipeW`（BYTE 模式、duplex）  | `mkfifo` 双 FIFO + open 记录（§5）          |
| `rt_pipe_server_wait_connect(h) → bool`                   | 阻塞等待接入（**同步面**）                             | `ConnectNamedPipe`                  | 读端 `open` 阻塞                           |
| `rt_pipe_submit_wait_connect_async(h) → bool`             | Reactor 异步等待（PENDING → RT\_IO\_OP\_CONNECT） | OVERLAPPED ConnectNamedPipe         | poll POLLHUP 探测 / io\_uring            |
| `rt_pipe_client_connect(name, timeout_ms) → rt_pipe_t*`   | 客户端接入                                       | `CreateFileW` + `WaitNamedPipeW` 重试 | `open` 轮询至 timeout                     |
| `rt_pipe_read(h, buf, len) → int`                         | 阻塞读（0 = 对端有序关闭）                             | `ReadFile`（ERROR\_BROKEN\_PIPE → 0） | `read`（0/EPIPE → 0）                    |
| `rt_pipe_write(h, buf, len) → int`                        | 阻塞写（短写补写至尽）                                 | `WriteFile` 循环                      | `write` 循环（EPIPE → 0，见 §3.1-1）         |
| `rt_pipe_submit_read/write_async(h, ...)`                 | Reactor 异步读写（RT\_IO\_OP\_READ/WRITE）        | OVERLAPPED                          | io\_uring / kqueue EVFILT\_READ / poll |
| `rt_pipe_server_disconnect(h) → bool`                     | 断开当前连接并复用实例                                 | `DisconnectNamedPipe`               | close 双端 + 重建 FIFO（§5）                 |
| `rt_pipe_close(h)`                                        | 关闭                                          | `CloseHandle`                       | `close` + 陈旧 FIFO 卫生                   |
| `rt_pipe_is_connected(h) → bool`                          | 连接状态                                        | 状态记录                                | 状态记录                                   |

约定：**字节流语义（无消息边界）**——Windows 用 BYTE 模式而非 MESSAGE 模式（单一惯用法，两侧行为一致）；「读到流尾」为全平台统一 EOF 表达。

### 3.1 评审补强项（2026-09-02 评审轮：性能/稳定性论证产物）

1. **SIGPIPE 防护（稳定性关键）**：POSIX 下对已关闭读端的 FIFO `write()` 会触发进程级 SIGPIPE（默认杀死进程），且 `write` 无 `MSG_NOSIGNAL` 等价物——runtime 现状仅 socket 侧用 `MSG_NOSIGNAL` 自保、未全局忽略 SIGPIPE（已核实：`rt_socket.c/rt_net.c` send 走 MSG\_NOSIGNAL；全 runtime 仅 rt\_preempt 设 SIGURG handler）。**处置**：rt 层 POSIX 初始化点统一 `SIG_IGN(SIGPIPE)`，`write` 返回 EPIPE 映射为 0（EOF）——与「读到流尾」语义闭环。该初始化属 runtime 全局行为变更，随本 RFC 一并落地并在 M0 冒烟批断言（写端写入已关闭读端 → 返回 0，进程存活）。
2. **缓冲大小调优面（性能关键）**：Windows `CreateNamedPipeW` 的 nIn/OutBufferSize 与 POSIX 内核 pipe 缓冲（Linux 默认 64KB，`F_SETPIPE_SZ` 可调）——**默认 64KB 对齐**（`rt_pipe_server_create` 默认值；显式参数留调优口）。大吞吐场景增大缓冲可减少系统调用次数；小消息场景默认值已足够。
3. **单写者配对约束（稳定性关键）**：POSIX FIFO 仅保证 ≤ PIPE\_BUF(4096) 的写原子性；并发多写者的大数据写会**交错**（Windows BYTE 模式同理无消息边界）。**处置**：文档化默认语义为单写者/单读者配对；多写者场景由上层组合（Channels 进程内扇出/汇聚，见 §8）承担——与「字节流 + 上层组帧」的单一惯用法一致。

## 4. std 门面（`std/Net/Pipes/`，namespace `Arc.Net.Pipes`）

对齐 `Arc.Net` 家族范式（`[Builtin(ABI = "rt_pipe_*")]` 门面 + 纯 Arc 组合面）：

- **`NamedPipeServerStream : Stream`**——`NamedPipeServerStream(string pipeName, int maxInstances = 1)`；`WaitForConnection()` / `WaitForConnectionAsync(CancellationToken)`；`Disconnect()`；`IsConnected`；继承 `Stream` 字节契约。

- **`NamedPipeClientStream : Stream`**——`NamedPipeClientStream(string pipeName)`；`Connect(int timeoutMs = -1)` / `ConnectAsync(int timeoutMs = -1, CancellationToken)`；`IsConnected`。

- **`NamedPipeTransport : StreamTransport`**——string 面适配器（`ReadLine/WriteString`），桥接 `Arc.Net` 既有 string 契约消费方。

- 文本行协议等更高层封装**不在本 RFC**（字节流 + 上层组帧，避免双轨）。

**继承** **`Arc.IO.Stream`** **而非 StreamTransport**：管道本质是字节流（与 FileStream 同族）；string 面经 `NamedPipeTransport` 显式适配。

## 5. 跨平台硬要求

### 5.1 语义收敛契约（全平台一致，测试锁定）

以下行为在 Windows/Linux/macOS **逐字一致**，由同一套 L2 批（§6）在双平台过门锁定：

1. 字节流、无消息边界；`Read` 返回 0 = 对端有序关闭；短写对调用者透明（补写至尽）。
2. 阻塞读/写、`Connect(timeoutMs)` 超时语义（-1 = 无限等待）。
3. **名字规范化**（Arc 逻辑名 → 平台物理名，规则唯一且公开）：

   - Windows：`\\.\pipe\{name}`

   - POSIX：`$XDG_RUNTIME_DIR`（回退 `/tmp`）`/arc-ipc-{sanitized(name)}.in|.out`

   - sanitize：仅保留 `[A-Za-z0-9._-]`，其余折叠为 `-`；冲突即创建失败（不静默复用）。
4. FIFO 生命周期卫生（POSIX）：`unlink` 责任归**创建者进程退出/`rt_pipe_close`**；陈旧 FIFO（无监听者）在 `client_connect` 时以 ECONNREFUSED 呈现——统一映射为「接入失败」，不残留文件锁语义。
5. 权限：POSIX `mkfifo` 0600；Windows 默认 DACL（当前用户私有）——两平台同为「默认仅创建者可访问」。

### 5.2 平台映射（差异吸收在 rt 层，Arc 面零分支）

| 差异点  | Windows 原生               | POSIX 映射策略                                           |
| ---- | ------------------------ | ---------------------------------------------------- |
| 双工   | 单内核对象 duplex             | `name.in`/`name.out` 双 FIFO 组装（名字映射对用户透明）            |
| 多实例  | `maxInstances` 并行接入      | 「accept 后立即重建 FIFO」循环；`maxInstances > 1` 为串行排队语义（登记） |
| 异步   | OVERLAPPED + IOCP        | Linux: io\_uring；macOS: kqueue EVFILT\_READ；退化: poll |
| 断连感知 | ERROR\_BROKEN\_PIPE      | read 0 / EPIPE——统一映射为「读到流尾」（含 §3.1-1 SIGPIPE 防护）     |
| 断开复用 | `DisconnectNamedPipe` 原语 | close 双端 + 重建 FIFO                                   |

### 5.3 实现组织与验证矩阵

- **文件组织**（对齐 reactor 约定）：`rt_pipe.c`（公共状态机、名字规范化、缓冲语义）+ `platform/pipe_windows.c` + `platform/pipe_posix.c`；内部共享结构经 `rt_pipe_internal.h`（不入 `rt_abi.h` 公共面）。

- **验证矩阵**：M0 起，同一套 L2 批（case 零平台分支）在 Windows 与 Linux 双平台过门；macOS kqueue 面随 M2 异步面补齐。构建配置沿用 runtime 既有跨平台编译脚本（`rt_library` / 各 platform 产物）。

- **平台专属行为仅允许出现在 rt 层**；`std/Net/Pipes/` 与用户代码不得出现 `rt_env_is_linux()` 分支——违者按「单一惯用法」纠正。

## 6. 性能/稳定性论证（评审结论）

### 6.1 性能定位（机制级）

| 维度     | 命名管道（本 RFC）                                | TCP loopback                   | UDS                          | Channels（046） |
| ------ | ------------------------------------------ | ------------------------------ | ---------------------------- | ------------- |
| 跨进程    | ✓                                          | ✓                              | ✓（Windows 需 10 1803+）        | ✗（进程内）        |
| 延迟/抖动  | **优**——免协议栈，直达对端内核缓冲                       | 良——全协议栈（头封装/校验/序号/ACK 状态机）为抖动源 | 优（同走内核 pipe 机制）              | 极优（内存级，纳秒）    |
| CPU/消息 | 低——省协议栈处理                                  | 中                              | 低                            | 极低            |
| 大块吞吐   | 良（缓冲 64KB 起步可调，§3.1-2）                     | 良（内核 memcpy 主导，两者接近）           | 良                            | 极高            |
| 确定性/流控 | 内核缓冲满即阻塞写——**天然背压**                        | 拥塞控制/重传干扰                      | 同管道                          | 显式背压模式        |
| 生态对齐   | **.NET System.IO.Pipes 正统 IPC 面**（微软级家族对齐） | —                              | .NET 后加入（System.Net.Sockets） | —             |

结论：优势集中在 **小消息延迟、延迟抖动、CPU/消息、确定性**——本机 IPC 主场景（控制面/事件面/高频小消息）正中靶心；大块流式吞吐与 loopback 接近（诚实边界）。无端口占用/防火墙/地址管理负担为运维面附加优势。

### 6.2 稳定性定位

- **天然背压流控**：内核缓冲满 → 写阻塞——无需应用层流控即可防对端过载（socket 同理，但管道语义更简单确定）。

- **EOF 统一语义**：对端有序关闭 → 读到流尾（0），异常关闭路径已按平台精确映射（§5.2）。

- **SIGPIPE 缺口已闭环**（§3.1-1，评审发现并落文）。

- **同族竞态前置门已闭合**（2026-09-02）：Reactor 域 accept-null 债务根治——await 的「零 re-poll 直达提取」假设被取证证伪（`await_waiting` 守卫位可被非配对 `coro_wake` 清除，EventLoop 合法推进挂起帧时 inner 仍 PENDING → `ptr_result` 空 → await 得 null）。协程与状态机两条 await lowering 均已改为 **re-poll 提取**：resume 后先 poll，PENDING 走第二挂起点（coro：独立 `coro.suspend` 回环 suspend2；状态机：重新 register_waker + 存 state 返 PENDING 天然回环）重等并重登记 waker。`l2_net_batch` 修复前失败率 60%（3/5+3/5 取证轮），修复后 6/6 全绿。M2 异步面的 PENDING/waker 交接回归门随之落位于此形态之上。

- **单写者约束显式化**（§3.1-3）：多写者交错风险不留给用户踩坑，文档化 + 上层组合范式兜底。

### 6.3 选型权衡（FIFO ⇄ UDS，诚实记录）

POSIX 上 FIFO 与 UDS 同走内核 pipe 机制，性能差异很小；Windows afunix（10 1803+）较新。**选 FIFO + Windows named pipe 的依据**：① .NET 家族正统 IPC 面为 System.IO.Pipes（家族对齐优先）；② Windows named pipe 数十年成熟度；③ FIFO 在全部 POSIX 零依赖。**留有余地**：门面为 `Stream` 继承面——若未来实测显示底层替换有显著收益，可在不动公开面的前提下扩展 UDS 后端（已列入不做清单的并行面约束：不混装、独立评估）。

## 7. 安全面（最小集）

默认当前用户私有（见 5.1.5）。自定义 ACL/权限面**不做**（登记后续方向）。

## 8. 与 Channels 的组合范式（仅范式，不在本 RFC 实现）

跨进程分发：接收线程从 `NamedPipeServerStream` 读字节 → 反序列化 → `Channels` 进程内扇出给工作协程；汇聚方向反向。单写者约束（§3.1-3）下，多对一分发 = 多个管道实例（或客户端互斥写）+ Channels 汇聚。组合面留待独立 RFC。

## 9. 里程碑分期

- **M0（rt 层同步面）**：`rt_pipe.c` + 双后端 + ABI 注册 + codegen 拦截 + SIGPIPE 全局防护 + `l2_pipe_smoke` **双平台过门**（含：字节回环、EOF、双工往返、名字规范化、接入超时、**写已关闭读端 → 返回 0 进程存活**）。
  - **验收记录（2026-09-02，Windows 侧）**：`l2_pipe_smoke` 五 case 全批通过（roundtrip / eof / write_closed_peer / connect_timeout / name_normalize）。冒烟牵出并修复两处**共享基建**根因：① pipe 门面未列入 `is_opaque_runtime_handle` ARC 豁免——裸 `RtPipe*` 被 `rt_arc_dec` 当对象头（offset 0 = `is_server`）递减，1→0 走释放分支读 offset 8 当 vtable → async Main 完成回调中 0xC0000005，批测表现为「case 1 PASS 后下一 case BEGIN 前」批进程死亡；② `copy_crypto_native_dll_if_needed` 门卫与 wgpu 版不一致（旧谓词不识别进程内编译 `target=None`）——Arc.Net 包经源码合并编入 TLS 面，产物隐式导入 `crypto_native.dll` 却未落位 → 批进程 0xC0000135（STATUS_DLL_NOT_FOUND）起跑即死，net/noise 等批同受益于修复。测试侧四处死代码兜底 `copy_crypto_native_dll_beside_batch` 随之移除。**Linux 双平台门禁仍欠**（POSIX 后端已备，待 Linux 环境执行同一批）。

- **M1（std 门面同步）**：`NamedPipeServerStream/NamedPipeClientStream/NamedPipeTransport` + `l2_pipe_echo`（跨进程）+ FIFO 生命周期卫生。
  - **验收记录（2026-09-02，Windows 侧）**：三批全绿（`l2_pipe_contract` 5 case / `l2_pipe_echo` 跨进程 / `l2_pipe_smoke` 5 case）。① **析构契约落定**（rt_pipe.c 状态注释）：模式 A 裸句柄无 ARC 头/析构钩子——显式 `rt_pipe_close` 是唯一收口路径；新增 `RtPipe.closed` 标志（close 幂等早退 + 全方法入口守卫安全返回 0/false），状态块不随 close 释放（泄漏至进程退出，与 Thread/Socket 同策 H1）；**补齐 `Terminate` 的 emit 分派臂**（M0 遗漏——契约测试首跑即以此暴露：Terminate 走 stub 死代码体，WaitForConnection 真 ConnectNamedPipe 阻塞）。② **FIFO 卫生修正**：create 的 EEXIST 由「静默复用（注释与实现矛盾）」修订为 **unlink+mkfifo 残骸接管自愈**（POSIX 无内核生命周期无法判活跃性；§5.1-3 相应修订）；disconnect 去 unlink+mkfifo（对齐 Windows DisconnectNamedPipe 复用语义，消除换 inode 连接撕裂）。③ **NamedPipeTransport**：组合适配器（聚合具体门面类型 + 游标式行缓冲）——模式 A 门面**不可经抽象基类引用虚调用**（裸块无 vtable，RFC 006「基类引用存储」缺口的同型面，实证 0xC0000005），故不继承 StreamTransport（async 抽象面待 M2 真异步 ABI）。④ **跨进程 echo**：spawn 自身（argv 传角色，子进程 `Environment.Exit` 收口防 driver 续跑）+ stdout/stderr 双重定向吸干（防管道满阻塞与 ARC_CASE 标记污染）+ 16 行 UTF-8 往返 + BYE 收束 + 退出码校验。⑤ **新基建**：`rt_env_self_exe`（Windows GetModuleFileNameW / Linux /proc/self/exe / macOS _NSGetExecutablePath）+ `Environment.SelfProcessPath()`（对标 Environment.ProcessPath 正名落位）。⑥ 测试环境注记：批测以 `ARC_HOME` 重定向沙箱放行目录（本机 TRAE 沙箱对 `$ARC_HOME/rt_cache` 的 `<name>-<hash>.o.tmp` 编译中间名禁写，重编静默失败致批进程吃旧 runtime——**部署期症状**，非产物缺陷）。**Linux 双平台门禁仍欠**。

- **M2（Reactor 异步）**：`WaitForConnectionAsync/ReadAsync/WriteAsync` 真 Reactor 面（IOCP/io\_uring/kqueue）+ accept-null 同族竞态回归门（专项审查先行）。

- **M3（组合与压力）**：管道 ⇄ Channels 桥接 idiom 文档 + 多实例压力批（含缓冲调优对比数据，回填 §6.1）。

## 10. 不做清单

- 消息边界 / 帧协议（上层组帧，单一惯用法）。

- 跨主机（TCP 已覆盖）。

- 句柄跨进程传递（独立 RFC）。

- 自定义安全描述符 / 权限面（登记后续）。

- 匿名管道公开面改造（`rt_proc.c` 既有匿名面仅服务 stdio 重定向，维持现状）。

- 域套接字（UDS）并行面：FIFO 已覆盖本机字节流场景；UDS 若未来需要（含 fd 传递能力）另立 RFC，不与命名管道混装（见 §6.3 留有余地）。

