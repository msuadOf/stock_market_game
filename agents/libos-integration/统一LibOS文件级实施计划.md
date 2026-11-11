# 统一 LibOS 文件级实施计划

日期：2026-11-11；进度更新：2026-11-12。状态：**用户已要求实施；本文不代表实现已经完成**。

## 1. 目标与硬约束

1. 以 `vendor/web-thread/upstream` 的源码为基础，追溯移植线程及通知机制，保留完整原始快照、许可和修改来源。不得把仅改编 Parker 说成已完成整个上游线程层的接入。
2. Native 与 WASM 的正式运行都经同一套 LibOS API；LibOS 以上的 World、Group、Desk、Session 使用同一份执行代码。同一 API 允许 Native 和 WASM 在 LibOS 内有不同底层实现；浏览器 Worker bootstrap 是 WASM 宿主适配，不能包含游戏业务分支。
3. 删除旧 watchdog、spin、sleep 轮询策略和正式 feature 选择；不得通过改名、包装或 fallback 保留旧等待路径。必须保留有界失败检测及错误上下文，但其语义由新 LibOS 契约明确实现。
4. 保留 ADR-0043 的 G 个组、每股一个交易部、实体 move、FIFO、收束、失败原子性；保留 ADR-0044 的死亡先通知、身份、tombstone、hard trap 边界。不能为了性能放松资金、股份、交易、日界或存档语义。
5. 目标是产品实际游戏时钟加速约 3000×、WASM 性能接近 Native；完整产品路径与核心推进分别报告。当前尚未约定“相近”的数值门槛，不自行宣称验收通过。
6. 符号标记：**现有**表示当前代码中已核对存在；**拟新增/调整**是待审核设计，不代表已经实现。函数签名是评审草案，实施时如需变化须保持下文语义并更新计划。

## 实施进度补充（2026-11-12）

- 已取消通用资源/测试时限的强制规则；现有 runner 自身仍可能中断进程，使用其结果时须按实际行为报告。
- `Admission` 已补并发清理者、双 Permit 收束、时钟回退及锁竞争/中毒诊断测试；真实 Native engine 测试二进制通过定向验证。WASM 生产目标及带 tombstone 替身的隔离测试模块编译通过；全引擎 WASM 测试编译的已记录尝试失败，真实浏览器运行、跨 Worker 时钟精度与 hard trap 仍需解决，不能视为完整生命周期闭环。
- 上游 spawn/join 和业务载荷归属已有初步对照，见 [上游符号来源对照](上游符号来源对照.md)、[业务载荷归属初查](业务载荷归属初查.md)；二者均经独立复核，但尚非逐分支完整证明。
- 正式 `World` handle 尚未移交 receiver；资源对账、浏览器故障和性能验收仍待实施。第一批尚未完成，不能切换正式默认通道。

## 2. 已知事实与实施顺序

- 已有基础包括成对 `Admission` / `downlink`、`worker_exit::Exit`、有类型的 `OwnedWorkerExit`、共用 `ConstructionGuard`，以及部分未送达 Final 和 panic 载荷保留；这些能力尚未共同接入正式 World 的 receiver 收束和业务资产对账。
- `World`、`ConstructionGuard`、prepared stop 和 verification 的 handle 仍为 `JoinHandle<Result<(), RuntimeError>>`；`OwnedWorkerExit` 当前只由定向测试使用。将其接入时须同步覆盖正常停止、fatal cleanup、构造失败和 Drop，保留 receiver 直至显式收束。
- Native 构造 guard、部分构造失败和停止路径有定向测试；默认及实验 WASM 配置有编译证据。新 guard 的真实浏览器 mode 4、WASM 故障资产回收和正式产品性能尚未验证，不能用编译或旧产物代替。
- 原第一批剩余退出协议工作与第二批下行切换、故障对账连续推进：在真实 join 可保留 receiver、已准入发送可收束且清理路径可持有消息之前，不单独把成对通道设为正式默认。停止时先完成协议仍需的 Abort / Shutdown 发送，再按经测试证明的安全次序关闭准入、join 和 drain。
- `libos::transport::downstream_channel` 当前在 Native 与默认 WASM 仍创建 std mpsc；只有 WASM `libos-worker-experiment` 走 mailbox。`WorkerInbox::receive` 的 std 实现仍调用 `worker_recv` / `worker_recv_spin`。
- `libos::downlink::{Sender, Receiver}` 已具备成对 `Admission`，但尚未接入正式 World。此前直接接线被撤回：`Receiver::drop` 不等待已有 `Permit`，不能证明消息或资产回收。
- `worker_exit::run` 能在 Rust unwind 下返回 receiver；WASM hard trap 不承诺运行 `catch_unwind`、析构或 Rust 返回路径。`GroupFinalUndelivered` / `DeskFinalUndelivered` 目前只在部分 Rust 退出路径保留，wire 中 `ShutdownWorkerResult::structured_error` 被 `serde(skip)`。
- 顺序为：契约和失败测试 → 上游源码映射与平台封装 → 携带 receiver 的退出及清理 → 上下行正式接线和统一上层 → 删除旧策略 → 真实浏览器及跨端正确性 → 性能 → 独立复核。**退出协议未打通前，不允许将成对通道单独接成正式默认并声称完成。**
- 仓库有大量既有未提交修改。实施前逐文件核对现有 diff，保留工作树，不重置、不覆盖其他任务；每批遵循 TDD，独立审查并修复发现。

## 3. API 草案和状态机

### 3.1 消息与等待

以下为拟定逻辑接口，允许按 Rust 泛型和可见性细化，不要求新增无用包装：

```rust
// 拟议契约示意；不是现有代码。
fn downstream_channel<T>(domain: &FaultDomain)
    -> Result<(DownSender<T>, DownReceiver<T>), ChannelError>;

impl<T> DownSender<T> {
    fn send(&self, message: T) -> Result<(), SendError<T>>;
    fn wake_for_stop(&self);
}
impl<T> DownReceiver<T> {
    fn receive(&self, stop: &StopToken) -> Result<Option<T>, ReceiveFailure>;
    fn try_receive(&self) -> Result<Result<T, TryRecvError>, ReceiveFailure>;
    fn close_and_wait(&self, deadline: Instant) -> Result<Closed<'_, T>, AdmissionWaitError>;
}
```

- `send` 的 `Err` 必须返回原 `message`；成功仅表示进入队列，不表示业务已处理。已获得 `Permit` 的发送可与故障并发完成；清理者必须先关闭准入并等待所有许可释放，才可把空队列视作已收束。
- `receive` 返回 `Some(message)` 才表示所有权交给 Worker；`None` 仅表示已经观察到停止。故障与出队竞态时，消息必须由接收端保留给清理路径，保持先于后续队列的顺序。
- FIFO 应明确为每个 sender 的发送顺序；不同 sender 的交错按既定业务同步协议约束，不能凭 mpsc 推断全局确定顺序。
- `Closed::drain` 返回的 `Drain { messages, state }` 只代表消息移交；`Drained` 不代表业务处理、资金股份对账或宿主 child 已退出。`TimedOut` / `BudgetExhausted` 必须保留已取消息及未取队列，不允许伪装成功。
- 拟引入 `StopToken` 作为 LibOS 层持有/检查停止状态与唤醒的共同抽象；如复用现有 `AtomicBool` 可满足同一契约，则不额外建立包装。期限使用绝对 `deadline`，等待算法和平台原语由 LibOS 实现，不暴露旧 spin 配置。

### 3.2 Worker 生命周期与故障

```text
创建 channel/登记故障域 → spawn 并取得 handle → 运行
→ 发布 stop/fault 并唤醒 → 禁止新发送 → 等待在途发送许可收束
→ join 并取得可移交的 receiver / Final / 原错误
→ 分批取回消息 → 业务对账 → 成功收尾或明确不可恢复终态
```

- `worker_exit::Exit<R, E> { outcome, receiver }` 和 `Outcome::{Returned, Panicked}` 是现有类型；计划接入正式 Worker join。拟新增区分 Group/Desk 的有类型持有结构 `OwnedWorkerExit`，不能用字符串代替 receiver 或原错误。
- 上述是逻辑次序：实现需避免先等待只能由 Worker 释放的许可而死锁；停止、关闭准入、join、drain 可按测试证明的安全顺序编排，但 `Drained` 前必须证明发送已收束。
- Native panic 与受控可返回错误尽量取得 `Exit`；WASM hard trap 无 Rust 返回保证。对于 trap 时不可安全取得或信任的状态，标为 `Unrecoverable`，停止实例、禁止发布失败 tick / 存档，仅允许使用先前已验证恢复点。不得伪造 Final、资产回收或 tombstone。
- `primary_error` 保留首次错误；`cleanup_errors` 记录后续失败，保留 Worker 身份。诊断中的未送达快照不是第二份权威资产，不可与账户、订单簿或恢复消息重复计数。

## 4. 文件和符号计划：合并为三个实施批次

三个批次各自完成测试和独立复核；批次内按下列小节的依赖顺序推进，小节不是独立交付批次。第一批建立可安全接线的底座，第二批完成正式切换与故障闭环，第三批完成跨端验收和性能优化。

### 第一批：设计、LibOS 底座与 Worker 退出协议

#### 1.1 设计、源码来源及依赖清单

| 文件 | 符号 / 具体工作 |
| --- | --- |
| `docs/decisions/0043-std-thread-ownership-runtime.md` | 审阅拓扑、`Entity` move、FIFO、tick 收束和异常章节；保留既定业务语义，只记录执行基础设施迁移对实现描述的影响。 |
| `docs/decisions/0044-wasm-panic-tombstone.md` | 核对 `DEAD`、child 绑定、TLS、tombstone、hard trap 和 `last_committed_tick=unknown`；明确 LibOS 适配必须保留这些约定。 |
| `docs/decisions/0045-web-thread-vendor-admission.md` | 更新“仅源码准入、未接生产”的阶段性陈述：记录待接入范围、上游 commit、许可、隔离快照与适配的区别；生产接入状态只按实施证据更新。 |
| `vendor/web-thread/upstream/src/thread/atomics/parker.rs`、`channel.rs`、`spawn/`、`src/thread/mod.rs` | 建立逐符号来源表，比较 Parker 状态机、发送/唤醒、spawn/join 与本仓 `Signal`、`SlotArena`；保留原始快照，不直接修改上游文件。对不能直接复用的上游代码写明原因和差异。 |
| `agents/libos-integration/现状与验收.md`、本计划 | 更新入口清单、已复用和待移植上游代码清单、每批测试结果及未完成项，不把工作记录当正式 ADR。 |

验收：可逐项找到上游源码、移植代码、许可证和业务约束；不将 API 名称统一等同于生产接入。

#### 1.2 统一 LibOS 基础设施

| 文件 | 现有符号与拟议修改 |
| --- | --- |
| `packages/engine/src/libos/mod.rs` | 将 `transport`、`thread`、`fault_domain`、`downlink`、`worker_exit` 定为 runtime 唯一可依赖的执行边界；整理模块可见性，避免业务层直接使用平台内部结构。 |
| `packages/engine/src/libos/thread.rs` | 保留公共 `Builder`、`JoinHandle`、`available_parallelism` 语义，在 `wasm_impl::{init_wasm_facade, child_trampoline}` 接入并核验上游线程实现与 `SlotArena`、`SpawnJob`、`join_typed` 的关系；Native 可静态使用原生实现。不得破坏 `init_tombstone_primary`、`bind_tombstone_ingress`。 |
| `packages/engine/src/thread_facade.rs` | 查全现有调用者；将真实平台实现收归 `libos::thread`，兼容导出若仍被公共 API 使用则只作无条件重导出，不在此重新选择平台；核对源码兼容性。 |
| `packages/engine/src/libos/mailbox.rs` | 以现有 `Signal::{notify, wait}`、`Sender::{send, wake}`、`Receiver::{try_recv, retain_interrupted}`、`receive_interruptible_when` 对照上游 Parker；确认 token、最后 sender 断连、伪唤醒、消息/故障竞态及内存序。原队列若继续使用 std mpsc，明确它仅是 LibOS 内部队列而非旧等待策略。 |
| `packages/engine/src/libos/fault_domain.rs` | 保留并检验 `FaultDomain::{register, channel, abort}`、`GlobalFault::{initialize, host_layout, host_is_ready, is_failed}` 和 `instance_failed`；两端对可发生的故障使用一致外部语义。Native 不得把可发生的本地故障静默视为成功。 |
| `packages/engine/src/libos/transport.rs` | 将 `DownSender` / `DownReceiver` 切到成对 downlink 的统一公开类型；改造 `downstream_channel` 的登记流程，避免直接用实验 feature 切换类型。`upstream_channel`、`receive_up`、`try_receive_up` 提供共同语义，平台选择留在 LibOS 内。 |
| `packages/engine/src/libos/wake.rs`、`worker_wait.rs` | `WorkerWake::wake_for_stop` 对 Native/WASM 均真正唤醒，不通过插入业务消息制造唤醒；`receive` / `try_receive` 统一 stop、断连、故障及取件竞态处理。消除 std 专属旧策略分支。 |

先写测试：先通知后等待、检查与睡眠间通知、重复通知、最后 sender drop、故障与出队竞争、停机无业务消息、时钟回退。每个平台执行共同契约；真实 Chromium shared-memory Worker 补同样的关键交错测试。

#### 1.3 准入、退出、receiver 移交与清理

| 文件 | 现有符号与拟议修改 |
| --- | --- |
| `packages/engine/src/libos/admission.rs` | 检验 `Admission::{acquire, close, is_closed_and_quiescent, wait_closed}` 与 `Permit::drop` 的 acquire/release 顺序；关闭后拒绝新许可，持有许可的发送完成前不得报告收束；超时不得丢失后续清理能力。 |
| `packages/engine/src/libos/downlink.rs` | `Sender::send_with_fault` 返回原载荷；`Receiver::{run, close_and_wait}` 与 `Closed::drain` 接入真实生命周期。保留 `DrainState::{Drained, BudgetExhausted, TimedOut}` 和 `Drain.messages`；审查 receiver drop 与在途发送竞争，不把 drop 当清理。 |
| `packages/engine/src/libos/worker_exit.rs` | 把现有 `Exit<R,E>`、`Outcome::{Returned,Panicked}` 用于真实 `Builder::spawn` 的闭包返回；在 WASM 无 unwind 的配置明确返回不可恢复诊断，不能假设 `catch_unwind` 覆盖 trap。 |
| `packages/engine/src/runtime/world.rs` | 修改 `World::spawn` / `spawn_with_calendar_source` 内两处 `Builder::spawn`：分别包装 `group_main_borrowed`、`desk_main_borrowed`，保留真实 `GroupIn` / `DeskIn` receiver 和 `WorkerId`。拟增加 `OwnedWorkerExit::{Group, Desk}` 或等价类型，确保 join 后有类型地收口，而非擦除 receiver。 |
| `packages/engine/src/runtime/world/native_construction.rs` | 审查现有 `NativeConstructionGuard` 和已启动 handle：将与平台无关的部分构造失败收尾提升为公共 `ConstructionGuard`（拟议名）；覆盖 spawn 成功 0..G+N 个后失败，保留首错、已启动 handle、receiver 和发送端归属。WASM 构造也使用同一协议。 |
| `packages/engine/src/runtime/world/shutdown_completion.rs` | 修改 `World::shutdown_with_completion`、`ShutdownWorkerResult` 的收口；保留 `retained_group_final`、`retained_desk_final` 的诊断语义，显式区分 `receiver_recovered`、`messages_drained`、`assets_reconciled` 和 `child_released`，不得用 join 数或 Final 数推断其他阶段完成。公开 Rust 结构体新增字段的源码兼容性需检查。 |
| `packages/engine/src/runtime/protocol.rs` | 核对 `RuntimeError::{GroupFinalUndelivered, DeskFinalUndelivered, HostAborted}`；拟增结构化不可恢复/清理失败表示，保留原错误、worker 身份及阶段。将业务 phase 从旧 watchdog 模块解耦，而非简单删除信息。 |

先写失败测试：两个 sender 持有 `Permit` 与 close 并发；receiver 正常返回、显式 Err、Native panic；join 后部分 drain、到期重试；部分构造失败；Final 及原错误同时保留。资产测试必须区分人工载荷与真实业务资产。

第一批完成条件：设计与上游来源可追溯；统一基础设施和 receiver 移交协议通过定向测试，已知 trap 限制明确。此时不宣称正式 World 已完成切换。

### 第二批：正式切换、删除旧策略与故障资源闭环

#### 2.1 接入真实 World 下行并统一上层

| 文件 | 现有符号与拟议修改 |
| --- | --- |
| `packages/engine/src/runtime/downstream.rs` | `channel<T>` 无条件调用新的 `libos::transport::downstream_channel`；不自行选择 std / WASM feature，错误映射保留方向和根因。 |
| `packages/engine/src/runtime/upstream.rs` | `channel`、`recv_up_deadline`、`try_recv_up` 仅做业务相位映射；删除按平台或旧 feature 选择等待方式；`external_fault` 改为调用统一故障能力，不把 Native 故障固定忽略。 |
| `packages/engine/src/runtime/worker_inbox.rs` | `WorkerSender` 不再为 std sender 和实验 mailbox 提供正式分支；`WorkerInbox::receive` 删除 `WatchdogConfig` 与 `Option<SpinConfig>` 参数，按统一 stop/receive 契约调用 `DownReceiver::receive`。测试替身仅限 `cfg(test)`，不形成产品策略。 |
| `packages/engine/src/runtime/worker_context.rs` | 从 `WorkerContext` 移除 `watchdog: WatchdogConfig`；检查 `up_tx`、`abort`、`shutdown` 是否可分别用 LibOS 的故障/停止句柄表达，避免同一状态出现多个来源；保留 `capture` 诊断绑定。 |
| `packages/engine/src/runtime/group.rs` | 修改 `group_main`、`group_main_borrowed` 的 `rx.receive(... Some(spin_config()))` 为统一 receive；删除 `spin_config` 和相关旧注释。审计 `GroupState::run_round`、`final_state` 及 `undelivered_ops`：失败载荷和尾部不丢，Final 是一次性移交，诊断不能重复入账。 |
| `packages/engine/src/runtime/desk.rs` | 修改 `desk_main`、`desk_main_borrowed` 的收件入口；审计 `DeskState::flush_batches`、`final_state`、`undelivered_batches`、退款及成交回传，保留失败目标和尾部，并明确现有诊断副本不是权威资产。 |
| `packages/engine/src/runtime/worker_stop.rs`、`libos/wake.rs` | `wake_all` 保留“先发布停止再唤醒所有等待者”的统一顺序；删除为旧 std 通道生成业务 shutdown 消息的替代策略，验证空闲和互持 sender 时 join 不阻塞。 |
| `packages/engine/src/runtime/world.rs`、`world/wasm_shutdown.rs`、`world/prepared_stop.rs` | 将 `World::spawn`、`shutdown_message_protocol`、`shutdown` 与 handle join 的正式平台分支迁入 LibOS；相同业务代码处理 Group 和 Desk 的 stop、Final、join。平台宿主专有细节留在 LibOS/宿主适配。 |
| `packages/engine/Cargo.toml`、`apps/web-wasm/Cargo.toml`、相关构建脚本 | 移除 `libos-worker-experiment` / `libos-upstream` 作为正式实现选择及其使用者；保留与业务实现选择无关的诊断开关时明确其用途。全仓搜索构建引用，不能只删除 Cargo feature 定义。 |

验收：默认 Native、默认 WASM 都实际使用新上下行与同一业务循环；读取编译产物和测试配置确认未启用隐藏旧路径。新通道的生命周期与第一批退出协议及本批资源清理一起闭环，不能先上线类型替换、后补故障清理。

#### 2.2 删除旧等待策略与平台泄漏

| 文件 | 具体工作 |
| --- | --- |
| `packages/engine/src/runtime/watchdog.rs`、`runtime/watchdog/wasm.rs` | 全仓梳理 `WatchdogConfig`、`SpinConfig`、`spin_config`、`worker_recv`、`worker_recv_spin`、`recv_deadline` 和 `Phase` 调用；把仍需的业务阶段与期限契约改由中立类型提供，删除旧等待、轮询、spin 与相应配置；没有剩余职责后删除文件。 |
| `packages/engine/src/runtime/wasm_wait.rs` | 查明全部调用，使用 LibOS 消息通知及故障唤醒取代 `try_recv` + sleep 方案；无调用后删除模块。不得将相同轮询搬进 LibOS 冒充新实现。 |
| `packages/engine/src/runtime/coordinator.rs`、`world.rs`、`drive.rs` 和引用旧 `WatchdogConfig` 的测试 | `recv_deadline`、`Phase::TickConverge` / `Seal` / `Drain` 等调用迁至统一期限与业务 phase；错误类型保留相位、worker 和耗时上下文。清理旧配置参数和仅用于选择旧算法的变量。 |
| `packages/engine/src/runtime/watchdog_spin_tests.rs`、`watchdog_verification.rs`、`watchdog_wait_verification.rs`、`runtime/mod.rs` | 将仍有价值的顺序、超时和竞争断言迁到 LibOS 契约测试；删除只验证被移除算法的测试和模块声明，不弱化业务断言。 |
| `packages/engine/src/lib.rs`、`thread_facade.rs`、`runtime/` 全目录 | 审计 `cfg(target_family = "wasm")`、`std::thread`、`std::sync::mpsc`、旧 feature 名和 `wasm_wait`。逐项分类：业务执行平台分支必须下沉；测试/诊断/公开兼容导出列证据，不以简单搜索零结果冒充架构审计。 |

验收：正式运行无旧等待算法、无旧/新策略选择；正常、断连、stop、故障、deadline 测试通过，原有有效行为断言保留。

#### 2.3 真实业务资源对账和失败传播

| 文件 | 现有符号与拟议修改 |
| --- | --- |
| `packages/engine/src/runtime/protocol.rs` | 清点 `GroupIn`、`DeskIn`、`UpMsg` 中所有携带现金、股份、委托、成交、退款和回购的 variant；列出发送前、队列、处理中、回传、提交各状态的权威拥有者。 |
| `packages/engine/src/runtime/group.rs`、`desk.rs` | 为 `run_round`、`flush_batches` 和 `final_state` 建立失败时的权威状态/诊断清单；已发送与失败及未发送尾部分开处理，禁止把 Final 诊断副本当新资产。覆盖真实冻结、委托、退款、成交与回购。 |
| `packages/engine/src/runtime/world/shutdown_completion.rs`、`world.rs` | 拟增加内部 `reconcile_worker_exit`：消费取得的 receiver 和业务状态，按消息 variant、身份和阶段进行一次性对账；只有已证明的收束结果可用于确认成功，不能恢复时产生不可恢复终态。具体业务映射先依据 ADR-0043 和现有事务边界写失败测试再固定。 |
| `packages/engine/src/session/failure.rs`、`session/shutdown/tests.rs`、`session.rs` | 保留原始故障与清理失败；审查 `ShutdownFailure` 经 Session 的字符串化和 `serde(skip)` 边界。对外只提供安全、必要的结构化诊断，不把私有资产快照当可重放 wire 数据；失败状态不得发布为成功 tick 或合法新存档。 |
| `packages/engine/src/runtime/world/shutdown_completion/tests.rs`、`runtime/desk/borrowed_receiver_tests.rs`、现有 Group/World 测试 | 添加真实 World 对账测试：组扣资后下行失败；交易部退款/成交回传失败；发送成功与失败目标混合；stop 与发送并发；部分构造；显式 Err 与 panic。每例对账户现金、持仓、在途实体、订单簿和事件分别核对。 |

验收：已可恢复场景逐个证明守恒、恰好一次归属、FIFO 与失败不发布；未覆盖或无法恢复的 hard trap 明确标为终态，不把人工合成退款测试报告为真实成交验证。

#### 2.4 浏览器宿主及真实 WASM 故障

| 文件 | 现有符号与拟议修改 |
| --- | --- |
| `apps/web/src/host/libos-host-fault.ts` | 校验 `parseLibosHostFaultLayout`、`activateLibosHostFault`、`abortLibosHostFault`、`overlapsLibosHostFault` 的地址、容量、重叠和原子通知；保持故障域布局与 Rust 一致。 |
| `apps/web/src/host/engine-supervisor.ts` | 核对 `createEngineSupervisor` 内 `installSubstrate`、`spawn`、`workerError`、`abortFaultWaiters`、`fatal`、`finalizeFatal`、`cleanup`、`dispose`：注册前失败和 child trap 可报告身份；先发布死亡/唤醒，再按证据清理；记录无法取得 Final 的情况。 |
| `apps/web/src/host/wasm-worker.ts` | 审核 `postFailureDetails`、`handleBusinessMessage`、`reportEngineQuiescence`：保留首错及清理错误，只有真正完成 quiescence 才报告；不得把 Rust 私有诊断缺失解释为资源成功回收。 |
| `apps/web-wasm/src/lib.rs`、`packages/engine/src/wasm_tombstone.rs`、`facade_slot/` | 核对导出、`child_trampoline`、child 绑定和受控 std marker；新 LibOS 适配保持 ADR-0044 death/TLS/tombstone 布局与生命周期。 |
| `apps/web/src/host/engine-supervisor-libos.test.ts`、`libos-host-fault.test.ts`、`wasm-worker-failure.test.ts`、`agents/libos-integration/browser-probe/` | 先补 JS 协议和地址校验单测；再扩展真实 Chromium 共享内存 Worker 场景。隔离 probe 可验证原子通知，但最终还需真实产品宿主和正式 WASM 产物的浏览器测试。 |

测试矩阵：spawn 前/后故障、child 未绑定/已绑定、Rust panic、hard trap、消息入队前后故障、正常关闭、多个 Worker 同时死亡、浏览器主线程响应与 child drain。验证失败 tick 不发布、错误有身份且清理有界。

第二批完成条件：两端正式路径已切换、旧策略已删除，真实业务资源及浏览器故障测试按可恢复与不可恢复边界给出证据；任何缺口均不能报告为闭环。

### 第三批：跨端正确性、性能与最终验收

#### 3.1 跨端业务与存档正确性

| 文件 | 符号 / 工作 |
| --- | --- |
| `agents/libos-integration/world-benchmark/canonical-compare.mjs`、`trace-compare.mjs`、`event_projection.rs`、`fact_projection.rs` | 扩展逐项比较：同股 FIFO、每账户有序事实、成交/拒单/撤单非空覆盖、资金股份最终与中间状态；对跨股允许交错的事实明确比较规则，不把全局字节顺序不同无条件判业务失败，也不忽略真实同股顺序错误。 |
| `agents/libos-integration/world-benchmark/restore-run.mjs`、`restore-difference.mjs`、`restore-verify.test.mjs` | 同一真实存档在两端恢复和推进，检查恢复后权威状态、连续事件、最终状态与旧档兼容；固定诊断依赖版本和构建信息。 |
| `packages/engine/src/strategy/sampling.rs` 及概率相关现有实现、`agents/libos-integration/world-benchmark/math_candidate.rs` | 现有验证闭包使用的 `libm` 候选不是正式算法。先确定实际概率计算差异、精度和存档重建影响；如需改变正式实现或新增核心依赖，补 ADR、旧档和性能测试并独立复核，不能将临时注入当正式一致性证据。 |
| `packages/engine/src/runtime/world/committed_calendar_tests.rs`、`packages/engine/src/session/persistence/` | 覆盖跨日、日界失败、恢复和继续推进；断言未提交 tick 和 Calendar 不发布，已提交事实不被错误回滚。 |

验收：同 setup、seed、轨迹、产物和依赖身份；真实非空交易事实及不同阶段均覆盖。记录所有失败与不确定项，不仅比较 hash 和事件数量。

#### 3.2 性能、证据和独立复核

| 文件 | 符号 / 工作 |
| --- | --- |
| `agents/libos-integration/world-benchmark/native-run.mjs`、`run.mjs`、`engine.ts`、`measure.mjs` | 固定同一 setup、seed、release 产物和预热；交错多次采样 Native/WASM。分别记录核心推进及真实产品 UI/存档/跨日生命周期，不把诊断入口称为产品路径。 |
| `agents/libos-integration/world-benchmark/acceleration.mjs`、`acceleration-window.mjs` | 按游戏时钟计算模拟秒 / 实际墙钟秒；扩展跨日与非交易时段时先写失败测试，明确统计起止、交易日历及保存耗时。禁止将 tick/s 无条件换算成倍数。 |
| `agents/libos-integration/world-benchmark/cpu-run.mjs`、`cpu-samples.mjs`、`step-profile.mjs` | 记录可复现 CPU、线程数、等待阶段、耗时分布、产物 hash、机器环境与运行参数；标注采样粒度、插桩开销及不可比较部分，依 profile 优化，不靠静默调整业务参数。 |
| `agents/libos-integration/world-benchmark/试接测量记录.md`、`现状与验收.md` | 报告原始样本、失败、产物身份、性能范围、两端比例、目标达成情况和限制；保留过去实验，不能把实验数据写成正式产物新结果。 |
| 完整 diff 与相关测试 | 每批由未实施的 subagent 按 AGENTS.md 独立检查 A 股语义、必要性、边界测试和跨层漂移；修复有效发现并复核，最终一次审查完整 diff。 |

性能门槛：以产品实际路径约 3000× 为目标；“WASM 相近 Native”尚无明确数值，由用户审核确定。未达目标时继续剖析并优化，不能以减少语义、关闭正确性检查、限定有利窗口或单样本通过掩盖。

第三批完成条件：跨端交易和存档验收通过，真实产品性能测量及完整 diff 独立复核完成；未达目标或证据不足时明确报告未完成。

## 5. 自动化与验收门禁

- **TDD**：每项行为先写失败测试并记录失败原因，再实现、重构；已有测试不可为通过而删弱断言。
- **架构门禁**：给 `runtime/`、`session/` 增加可维护的依赖/条件编译检查（拟议 `scripts/check-libos-boundary.mjs`，以及对应测试），核查正式执行代码无平台线程、通道、等待分支；白名单只允许经审核的测试和诊断，不允许把正式业务文件整体加入白名单。测试脚本本身先验证正反例。
- **队列契约**：发送成功/失败所有权、逐 sender FIFO、stop、断连、通知竞争、超时、fault、准入收束、分批 drain；两端共同执行，加真实浏览器测试。
- **生命周期**：构造 0..G+N 个 Worker 后失败、正常关闭、显式错误、panic、WASM hard trap、Final 丢失、child drain 与期限；分别记录 receiver 可否取得、是否真正对账、是否有已知恢复点。
- **业务**：真实资金和股份冻结、退款、成交、委托与回购；检查同股 FIFO、T+1、整手及既有 A 股规则；无交易制度更改，若实施中触及规则先查现行官方依据。
- **性能**：Native/WASM 相同样本、交错多次，报告中位数及范围、CPU、产物和工作量；核心与完整产品分别计算，纳入跨日及保存。数量、运行参数和达标阈值在执行前固定。
- **资源**：按验证需要选择并记录实际并发参数、命令与运行结果；现有 runner 的内置限制按实际行为如实报告。
- **复核**：每批独立 subagent 复核完整 diff；发现修复后再次复核。受控构建或浏览器验证超时必须记失败/未完成，不借用旧产物冒充当前通过。

## 6. 交付物与审核点

1. 最终代码、正式 ADR 更新、上游来源/许可差异表和测试证据；不改原始 vendor 快照。
2. 文件与符号迁移清单：每个遗留 `cfg`、旧 feature、watchdog 引用有删除或明确归属，不用“基本统一”代替逐项核查。
3. 故障矩阵列明每个注入点的首错、receiver、消息、资金股份、Final、child release 和恢复结论；明确无法恢复与未知项。
4. Native/WASM 正式产品正确性和性能报告，分别给出核心与完整路径，包含未达标和波动。
5. 用户待定：两端“相近”的数值阈值；约 3000× 的完整产品计时起止与样本规模。上述参数未定前可准备测试框架并报告实测数据，但不能宣称最终性能验收通过。
6. 用户已要求按本计划继续实施；保留既有工作树，所有修改均以本计划及已确认的业务约束为界。
