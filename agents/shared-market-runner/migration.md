# 共享市场调度循环迁移

**重构范围：Native Server、Tauri Desktop 和浏览器 WASM 一起迁移。** 先冻结下面的框架和验收基线，再整批改完；不提交过渡实现，也不在中途逐端编译或跑测试。本文是实施蓝图，不表示现有产品已经迁移。

依据 [ADR-0005](../../docs/decisions/0005-unified-engine-three-deployments.md) §1、§4、§6、[ADR-0010](../../docs/decisions/0010-unified-host-protocol-and-local-refresh.md) §1–2、[ADR-0032](../../docs/decisions/0032-session-ingress-receipt-order.md)。市场成交、结算、日历、账户、委托受理与保存语义仍由同一个 `ProtocolSession` / `World` 执行，不更改 A 股规则或交易数据口径。

## 所有宿主调用同一轮推进

- `engine` 中的宿主无关 Rust runner 独占调度状态与规则：固定倍率的到期、迟到与重置语义，MAX 的时间预算、硬上限与让位，已受理命令优先，市场批次边界、日界暂停，以及失败时已提交前缀与首错的交付顺序。共享 Rust 发布侧统一批次封装、容量和缺口策略、游标校验、通知、基线和重同步；不允许由宿主各自实现解读规则。以输入时刻、就绪状态和注入的推进接口进行决策，不在引擎里建计时器或选择平台。
- **只有 LibOS 实现平台差异。** 时钟、计时等待、任务/Worker、邮箱、信号、IPC/网络传输与存储机制通过通用 LibOS API 暴露；LibOS 不知道速度档、tick、成交事实、日界、会话或重同步策略。宿主入口仅做部署组装及 ABI 接线，不自行探测平台或实现市场、发送或存档策略；界面只渲染共享发布契约。
- 迁移以 Server 的单次有界批处理为基准，不能为了复刻 WASM 当前一任务一 tick 而降低 Native 吞吐。固定倍率须先冻结晚醒策略：Server 原本不补齐错过的间隔，而 Desktop 会按已过去的时间计算欠下的 tick；不能在统一后仍保留两种算法。同一策略不保证不同机器每个 MAX 批次 tick 数相同，但相同输入、时间和命令就绪序列应有相同决策和轨迹。

## 两条独立执行路径

```
控制入口 → 有界命令邮箱 → 市场 runner → 权威提交/归档日志
                                         └→ 有界发布交接 + 唤醒 → 发布任务 → 观察者
```

- **生产者与发送者并行、独立调度。** 市场 runner 在专用执行上下文持续推进，前端发包任务在另一独立上下文运行；同一 Worker 内串行执行 runner、JSON 编码/postMessage 不算完成。runner 只做有界、非阻塞的交接（尽量共享对象或传句柄）和主动唤醒；不执行发送，不等待编码、socket、Tauri emit、UI ACK、观察者重同步或观察者邮箱容量。没有订阅者、观察者卡住/退出与渲染暂停均不改变权威 tick 推进。市场 runner 仍可因显式控制、引擎故障或权威提交/归档机制失败而暂停/停止，不把这些错误伪装成 UI 背压。
- **交接有界，缺口必显式。** 发布交接满时不覆盖权威事实；共享 Rust 按序登记观察者缺口，记录 `generation` 和已交付/缺失的 `seq` 范围，通知存活的观察者。若数据队列已满，缺口标记走独立状态/唤醒而非只能排进该队列。旧增量/ACK 不得跨 generation 重新入流。共享 Rust 从可用的权威日志游标重放，或取带 `generation + seq` 的一致基线后从后继序号恢复；超过可靠恢复能力则显式失败，禁止悄悄跳过。
- **权威事实不依赖观察者队列。** 已接受委托、日界存档候选、可回放的已提交事实有独立保留/持久化契约、可验证的提交游标及失败状态。区分市场状态已提交与日志已耐久，不能在权威日志丢失时伪报归档完成；处理独立的存储上限或 I/O 失败，不通过无限保留或前端确认解决。通知按写入后发布、睡前复查的顺序，覆盖空转竞态、满队列合并缺口、关闭唤醒、旧 generation 清理和发送者崩溃；具体渲染速率不影响此契约。

## 接线位置与约束

| 宿主 | 替换点 | 保留职责 |
| --- | --- | --- |
| Native Server | `apps/server/src/actor.rs`：`run`、`fresh_interval`、`tick_and_broadcast`、`run_fastest_batch`、`run_protocol_batch`、`prepare_civil_updates` 所含调度、分批和日界判断改用 runner | 并行的市场任务和发布任务通过 LibOS 通用端口运行；权限、控制路由、归档、投递规则来自共享 Rust。迁移 `routes.rs` 与 `publisher.rs` 的业务规则；WebSocket/HTTP 只搬运输入输出 |
| Tauri Desktop | `apps/desktop/src-tauri/src/actor.rs`：`run`、`fresh_interval`、`tick_and_emit`、`run_fastest_batch`、`run_cycle`、`publish_protocol_cycle`、`prepare_civil_updates` 改用相同 runner，不再用 UI 刷新间隔算到期 tick | Tauri IPC/emit 通过 LibOS，在发布任务而非市场任务执行；发布和存档语义共用，界面仅消费 |
| 浏览器 WASM | `apps/web-wasm/src/lib.rs`：每个 `SessionRegistry` 会话附带 runner，新增每次唤醒驱动一轮的批量导出；手工 `step` 继续遵守原单次调用契约。`apps/web/src/host/wasm-tick-loop.ts` 的 `stepOnce`/`frame` 自动策略与 `wasm-update-delivery.ts` 的暂停谓词由共享 Rust 替代；`wasm-worker.ts` 按返回计划调用新入口 | Worker、postMessage、IndexedDB 等机制收进 LibOS；引擎与发送者分别调度。删除 JS 推进等待 `uiFrame` 的机制，发布及重同步由共享 Rust 决定。保留已有共享 ingress 的受理边界 |

- **命令在批中到达：** 同步 WASM 调用不会处理新的 MessagePort 回调。不能把 Native 每个 tick 检查 `cmd_rx` 与 WASM 只在任务间处理消息冒充成同一合同。保留现有顺序的方案是：跨线程控制端按 generation 发布可观察的单调命令序号，runner 在下一 tick 前发现挂起序号即让位，控制端处理到该序号后确认唤醒；取消/恢复后拒绝旧 generation，禁止过期 ACK 放行。须覆盖「已写信号但消息尚未到」的竞态和多个待处理命令。玩家委托由共享 ingress 保有现存受理与截止语义，不把尚未成功入队的消息称为已受理。
- **日界与失败：** 成功市场前缀先进入权威输出路径，再处理日界；每个成功的 `CivilUpdate` 连同对应的存档候选按顺序提交到该路径，不能先越过后续日界再回头抓候选。这里不要求等待网络或 UI 消费。统一 pause 谓词，保留 `CorrectionPrepareRejected` 可恢复分类、fatal 首错、未交付范围、单步与恢复的 generation 语义。后续日界失败不得擦除已成功的前缀。
- **优先排除的危险点：** WASM `wasm-tick-loop.ts` 的 pending/outstanding 与 `wasm-worker.ts` 的 `uiFrame` 门控，要改成纯观察者流控；`lib.rs::step` 的逐 tick Rust→JS 序列化不许留在 runner。Server `actor.rs::publish_civil_update` 现同步 `persist_day_end`，要捕获稳定候选并交给独立持久化路径，不能因异步而伪报成功；`routes.rs::run_ws` 的长批发送和广播接收共用任务，`publisher.rs::ClientFrameBuffer` 原有规则需共享，不能把 socket 阻塞挪到权威任务。Desktop `actor.rs::emit_update` 当前同步调用 Tauri emit，迁到发布端，且不要保留 UI 节拍驱动市场。检查停止旧 sender 并启动新 generation、客户端收到旧增量之后立即换基线的竞态。

## 验收门槛

1. **契约测试：** 假时钟覆盖开跑、暂停/恢复、倍速、晚醒、MAX 预算/硬上限、命令压力和重入；输出轨迹覆盖市场批次、日界暂停/顺序、可恢复与致命错误、成功前缀、手工单步和恢复后迟到的唤醒/ACK。用快照和有序事实断言，不只对比计数。
2. **接线测试：** 三端以同一组 clock/命令/事件就绪轨迹验证同一路 runner；真实 Chromium 验证批次中控制、恢复、关闭及停发 ACK 仍持续提交权威 tick；Native/Tauri 注入阻塞发送者与慢观察者，证明 runner 继续推进、容量有界、缺口被发现且能重同步。验证日志保留和跨 generation 失效，不能仅以界面跳数为证。静态门禁拒绝 LibOS 外的目标分支、平台运输机制及宿主重复实现推进/发布策略。普通测试 ≤10 秒，长验收统一 ≤300 秒并独立复核。
3. **性能验收：** 固定种子、负载、市场阶段、分块宽度与 CPU 配额。分别测共同 `step_frame()` 提交、runner/本地发布、编码/投递、端到端 UI；验证 Native 在同负载下无可辨识回退并报告控制延迟与批次波动。WASM 对比每批跨边界次数、等待成本和实际稳定推进；Server WS 重同步跳变不算吞吐。Linux Desktop 用真实图形会话，不用无窗口样本替代。性能没有对比分层数据前，不宣布「无额外开销」或「三端已统一」。

## 模块边界与选择

| 位置 | 接口与责任 |
| --- | --- |
| `packages/engine/src/session/runner/` | `MarketRunner::run_turn`、`Cadence`、`WakePlan`、`CommandEnvelope`、已验证提交批次。Runner 持有 `ProtocolSession`，消费通用邮箱，在每个 tick 开始前读取已发布的命令序号，不识别目标或传输类型。开跑与改速重算到期时刻，固定倍速保留绝对节拍并跳过错过的时隙，不由 UI 节拍补跑；MAX 沿用 14 ms / 100,000 tick 的共同预算，先保证一个不可分割的有效 tick，再在 tick 边界检查预算、控制与日界。手工单步、日界、暂停、恢复与失败使用同一业务状态机。 |
| `packages/engine/src/session/publication/` | `PublicationJournal` 保存有限的已提交观察流；`ObserverCursor`、`DeliveryDecision`、`BaselineFence` 决定有序增量、批处理及显式缺口后的基线或重放。不等待 ACK，不查询任何目标；前端刷新率只影响观察流，不改变市场节拍。共享授权规则由 `SessionPolicy` 统一应用于受信本地主体或远端已认证主体；公共席位和控制权差别由部署配置表达，而非平台判断。 |
| `packages/engine/src/session/checkpoint/` | 日界处捕获并脱离权威存档候选，在提交顺序上关联 day/seq/generation；单独的存储任务处理写入和持久化回执。日志或存档失败显式归类，保留最后可用存档，不宣称未写入内容已耐久。读取和恢复使用同一验证入口。 |
| `packages/libos/src/` 和 `packages/libos/browser/` | 提供任务调度 / 单调时钟 / 非阻塞队列 / 丢唤醒保护 / 独立发送与存储端口。Native 映射到系统调度和相应 I/O 框架，Browser 映射到 Worker + shared memory + Web API。所有适配保留单向依赖，应用启动装配选择部署端口，不在业务分支识别平台。避免在热路径引入动态分发、同步序列化和全量拷贝。 |
| 原产品入口 | 保留对现有 ABI、HTTP/WS、IPC、存档格式的兼容转接；只负责配置和调用共同 API，不决定交易或发布策略。浏览器需要独立发布 Worker 和控制通讯端；不能把同一 Worker 的同步 Rust/JS 处理写成 async 就当作并行。 |

提交者发布事实后推进共享游标；观察者只消费已发布游标。队列写入后更新单调世代并唤醒，消费者睡前读取世代并重查，关闭/恢复使用不同 generation 清除旧消息。慢观察者进入显式 `NeedsBaseline`，不得等待其清空窗口；有日志可重放才重放，否则重新获取一致基线，并且不把快照跳跃计为实际增量吞吐。已经生成的权威事实及真实用户委托回执不能依靠任一观察者订阅存活来保留。

最终 diff 必须同时包含三种产品接线、跨平台底座、行为合同和架构防回归门禁。旧生产自动调度和业务投递控制全部移除，不保留按目标选择的新旧双实现。全部代码就绪后，统一执行编译、端到端、慢消费者故障注入及同负载性能验收；发现细节问题再修并重跑受影响套件。不得把现有 Windows/macOS 打包能力退化成仅 Linux/WASM。发布到 GitHub 另行确认。

### 不可退化的边界

- `ProtocolSession::tick_batch` 会推进全局公开 runtime delta 基线，必须在权威侧冻结已验证输出；不能把投影工作搬到发送者后让其访问正在变化的会话。`prepare_public_baseline` 会清空全局公开基线和待存档候选，不能用于单个慢观察者的重同步。
- 观察者的 fence 使用独立 publication ordinal 加 generation、tick、seq 和公开 runtime revision/origin；日界或状态更新可能共用同一个 tick/seq。基线与第一条增量必须匹配。保留 Server 的按主体裁剪，不能用一个含私有账户的基线给所有订阅者广播。
- 有界队列必须对高权重 `CivilUpdate` 计 bytes 或 fact units，不能只数消息条数。时钟、队列、producer 唤醒不能拿会跨编码、I/O 或 ACK 持有的锁。恢复后旧存档任务不能覆盖更高 generation 或新的归档槽；存储线程拥有 SQLite 事务和相关互斥。
- 对照验证：让某个观察者卡住或退出，权威仍推进且其他观察者有序；卡住者准确收到缺口并从正确基线重连。覆盖独立关闭 + reset 竞态、首包增量不匹配、陈旧控制/ACK、接收方崩溃、存档写入失败与旧写入完成。
