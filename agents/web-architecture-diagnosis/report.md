# Web 运行架构诊断

日期：2026-11-10。范围为现有代码只读审计与浏览器观察；未修改产品或测试代码，未构建，未运行 Native 同条件基准。交易制度没有变更，本报告不对沪深规则作新判断。

## 结论

Native 与浏览器使用同一个 Rust `engine` 和 `World` runtime，但连续运行的外围编排并不统一。Web 当前逐 tick 同步构造完整 `TickFrame` 并跨 WASM 边界，JavaScript 再批量传送；server 连续运行先提交核心 tick，再把待消费事实合入 `DeferredInterval`，由外围消费者异步构造结果。Web UI 的消费与确认会对推进产生背压，除此之外，逐 tick 投影、序列化、宿主调度及 WASM 平台本身也仍处于测量路径中。

**不能把 UI ACK 或 React 认定为主要倍率损失。** 去掉 React、Redux、图表与自动存档后的隔离宿主样本约 265.49 tick/s，仍远低于 1000 tick/s。它保留完整 `step`、投影、序列化、消息和宿主处理。后续同入口 WASM A-B-A 实验证实等待轮询有显著成本；剩余 Native/WASM 差距仍未分段测明。

## 路径与边界

- Web 的 `wasm.step(session)` 是对 [web-wasm 导出](/Users/asuad/workplace/stock_market_game/apps/web-wasm/src/lib.rs:668) 的调用，不是独立的 JS 市场引擎：绑定持有 `engine::ProtocolSession`，日界时提交 `CivilUpdate`，其余时候调用 `step_frame` 并组装单帧 `TickBatch`，见 [绑定实现](/Users/asuad/workplace/stock_market_game/apps/web-wasm/src/lib.rs:385) 和 [Worker 循环](/Users/asuad/workplace/stock_market_game/apps/web/src/host/wasm-tick-loop.ts:128)。
- server 的普通连续路径调用 `advance_core_tick`，在可交换槽累计并异步消费区间，按顺序登记并发布，见 [server actor](/Users/asuad/workplace/stock_market_game/apps/server/src/actor.rs:2245) 和 [核心入口](/Users/asuad/workplace/stock_market_game/packages/engine/src/session/protocol/civil/session.rs:1118)。两种入口都使用 runtime 时，交易核心是共同的；投影时机、外围失败和交付时序不同。诊断 feature 会逐拍同步消费，server 的 host-parity 显式单步也用 `step_frame`，不能把每种 Native 用法都称为异步管线。
- runtime 选择本身仍有平台分支：Native 默认启用，可用 `ENGINE_STEP_RUNTIME=0` 回退；WASM engine 未设置 override 默认串行，Web 产品显式启用 runtime，见 [runtime 选择](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/mod.rs:214) 和 [Web 初始化](/Users/asuad/workplace/stock_market_game/apps/web/src/host/facade-engine-initialize.ts:175)。`World` 共同通过 [thread facade](/Users/asuad/workplace/stock_market_game/packages/engine/src/thread_facade.rs:22) 创建线程，但构造、watchdog、关闭等还有平台分支，例如 [World 构造](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/world.rs:1052) 和 [watchdog](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/watchdog.rs:20)。`ComputeContext::available` 在 WASM 固定为 1，在 Native 读取可用核数，见 [compute.rs](/Users/asuad/workplace/stock_market_game/packages/engine/src/compute.rs:500)；这不表示 World 只启动一个 Worker。Native 正确不能独自证明 WASM 线程桥或浏览器验收已通过。
- Native 也不是无限制推进：命令前等待消费完成，批次每拍检查日界和命令，日界等待消费后处理，槽有 Mutex，见 [actor 主循环](/Users/asuad/workplace/stock_market_game/apps/server/src/actor.rs:2412) 和 [批次循环](/Users/asuad/workplace/stock_market_game/apps/server/src/actor.rs:2487)。这些约束保护顺序与事务，不能直接视为可去除的开销。

## 已观测数据

| 样本 | 测量区间 | 结果 | 限制 |
| --- | --- | --- | --- |
| 带消息观察及主线程 profiler 的 UI | tick 1425→14649，59.799 秒 | 221.14 tick/s | Vite 开发 UI；探针和 profiler 有影响，随机 seed |
| 无消息包装、无 profiler 的 UI | tick 1482→8985，30.094 秒 | 249.32 tick/s | 独立 Vite 开发 UI，随机 seed 不同；不是严格 A/B |
| 不挂载 React 的 WorkerHost | tick 1391→8039，25.040 秒 | 265.49 tick/s | seed 42；仍执行产品 WASM step、投影、序列化、消息和 ACK；不是纯核心 |

隔离样本的回调只记录最后 tick 并返回成功，不执行 React、Redux、图表和自动保存；采用产品默认多 Worker runtime。只运行一个样本，样本尚未到收盘。首轮误用了只反映 baseline 的 `host.tick()`，发现后中断并改用 callback 中的 tick；中断记录保留。详细数据见 [隔离样本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/bare-host-result.json)、[脚本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/bare-host.mjs)、[日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/bare-host.log) 和 [中断日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/bare-host-interrupted.log)。

带探针 UI 样本的普通消息从探针首次观察到消息至发出 ACK 的中位数为 4.5425 ms，p95 为 8.8 ms；探针包装与监听有额外开销，首次观察不等同于 handler 精确入栈时刻。收盘 `CivilUpdate` 接收至 ACK 为 3137.99 ms，当日 save 请求至 saved 响应为 5941.485 ms。同一时段记录到 3740 ms 与 5904 ms 的主线程 long task。tick 15300 消息接收于 82653.28 ms，CivilUpdate 于 85730.975 ms，tick 15301 于 88935.51 ms，tick 15302 于 99366.6 ms；这些时点相关，但不足以逐段证明阻塞原因。详情见 [汇总](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/summary.json) 和 [原始观察](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/observed-run.json)。

观察样本正常窗口 Chrome 进程 CPU 中位数约 247.2%，机器为 10 逻辑核；原始筛选含两个 PPID 1 的 crashpad 进程，它们在该样本为 0 CPU，应排除作为 browser root，实际 browser 树约使用 2.5 核。这是进程汇总而非各 Worker 分析。初始发现的三个 Worker target 只是创建过程中的快照，不代表最终 Worker 总数。结束时主线程 heap 约 1.737 GB，但没有强制 GC，也不是已知峰值，不能据此断言泄漏。

两次 UI 均为 Vite 开发服务器而非正式 production 成品基准。磁盘 `apps/web/wasm-pkg/web_wasm_bg.wasm` 与 `apps/web/dist/assets/web_wasm_bg-DdgnOEx2.wasm` 的 SHA-256 均为 `cbfdbdca109bf67f2e6a69b7ac6e50e1ccc0301d2e4f118df859807f3af1d236`，这只说明这两个文件相同，不消除宿主构建和运行环境差异。隔离样本未验证与其他样本为相同 seed 或完全相同执行条件，不能用吞吐差计算 UI 责任比例。

主线程 profile 前约 10 秒包含开局休市日日终保存，`validateSaveValue`、序列化和 base64 相关调用显著；观察函数 self time 也可能包括惰性结构克隆，不能全部归为探针。未取得 Worker profile，因而没有核心纯计算耗时或 Worker 等待分解。原始 UI 报告因旧日 K selector 失败，见 [失败日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/ui-built-4192.log)；第二次 Worker CDP evaluate 未响应，见 [记录](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/observed-run-v2.log)。后续样本没有 Worker profile，不能视作这些失败已补验通过。

## Native 生产 engine 直跑补测

补充单次较长窗口：预热 1000 tick 后读取 `game.tick()`，从 1000 实际推进至 11000，10000 tick 用时 3.271780 秒，**3056.44 tick/s**，返回 64167 个事件。采样窗口未跨日；进程每 0.5 秒采样包括初始化和预热，最多观测 7 线程、173.9% CPU，不代表窗口平均。诊断 harness 参数化后增量重链（7.15 秒），是与下面短样本不同的二进制，见 [长窗口原始数据](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-long-results.json)、[长短二进制哈希](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-long-hashes.txt) 和 [构建记录](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-long-build.log)。程序在运行前显式检查 Native runtime 已启用，失败即退出。


本次以产品配置模块实际导出的 `DEFAULT_SETUP` 创建 `GameSession`：5 股、20,000 散户、5 机构、2 游资，seed 42，Native 默认 runtime 和 1 组。先处理开局休市日，每次独立新建会话、预热 1000 次 `step`，然后计时 3000 次 `step`（推进区间 1000→4000）；测量区间覆盖开盘竞价后的交易时段，未跨日。三个短窗口的起止 tick 依据 step 调用次数推定，没有单独读取 tick；三次顺序结果分别为 **3396.37、3271.41、3549.27 tick/s**，计时分别为 0.8833、0.9170、0.8452 秒，每次返回 21821 个事件。计时不含创建会话、开局日界和预热，包含每次 `step` 的返回事件计数及释放。

这是 **Native 生产 engine 直跑**，不是纯 `World::run_ticks`：`GameSession::step` 经 runtime 完成真实 NPC 及交易处理，并包含 session 镜像准备/安装、事实校验与事件投影；不构造 `ProtocolSession` 的 `TickFrame`、不执行 WASM/JS/传输/UI。原始数据见 [三次样本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-engine-results.json)，[短窗口测量程序](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-engine-short-main.rs)、[运行脚本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/run-native.py)、[构建日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/bare-world-build.log) 和 [输入、源码清单及二进制哈希](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/native-hashes.txt)。构建为独立诊断 crate 的 release，`cargo -j8`、外部 300 秒 deadline；其独立 Cargo.lock 固定实际依赖，不能假称与 Web 产物构建环境完全一致。

进程采样每 0.5 秒一次，包含初始化和预热，三次最多观测 7 线程及约 157–166% CPU；每个计时窗口不足一秒，这些采样不能准确表示窗口平均利用率。Web 的 runtime 实际分组及相同并发受理轨迹未在此核定。先前 WorkerHost 265.49 tick/s 包含同步完整 frame、序列化与跨线程消息，不能用两者直接相除并归因为某一平台或组件。后续增加同源 GameSession WASM 诊断测量，见下一节。实际产品 WASM 包的源码一致性也未由二进制哈希单独证明。纯 `World` 同产品负载仍未测；简化 retail 的 `runtime_s5_probe` 不代替本测量。

## 同入口 WASM 与等待参数实验

隔离诊断绑定直接依赖同一 `packages/engine`，创建相同产品 `DEFAULT_SETUP`、seed 42 的 `GameSession`，显式启用 runtime，通过现有 facade 和共享 memory Worker 运行 `GameSession::step`，不调用 `ProtocolSession::step_frame`、不传每拍 frame 给 UI。诊断绑定复制的是 `web-wasm` 外壳以添加测量入口，**交易算法没有复制**；它是实验工具，不是建议维护第二套产品实现。使用 nightly-2026-09-05、受控 tombstone std、atomics、共享内存及与仓库一致的 release `lto=thin`、`codegen-units=16`；构建与哈希见 [日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/wasm-stage-aligned-build.log) 和 [哈希](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/wasm-core-hashes.txt)。独立 Cargo.lock、外壳与原产品包不完全相同，不能称字节等同生产 WASM。

| fine / coarse 等待参数 | 实际采样 tick | 耗时 | 吞吐 |
| --- | --- | --- | --- |
| 500 / 500 µs | 1000→4000 | 11.341775 秒 | 264.51 tick/s |
| 50 / 50 µs | 1000→4000 | 4.290345 秒 | 699.24 tick/s |
| 500 / 500 µs | 1000→4000 | 12.319490 秒 | 243.52 tick/s |

三个独立浏览器实例顺序运行，同一诊断二进制、setup 和 seed，每次预热 1000 tick、采样 3000 tick，每次采样均返回 21821 个事件；只改变 fine 和 coarse 两个等待参数，其他代码与设定一致。事件数量相同不是全部状态或并发受理顺序相同的证明。调整两个参数后吞吐提升约 2.64–2.87 倍、回切下降，支持当前等待机制造成实质性吞吐损失，但该小样本不提供精确占比或普遍性能保证；更短等待也可能提高 CPU 消耗，本实验未测其稳定窗口 CPU。完整记录见 [A-B-A 原件](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/wasm-core-results.json)、[脚本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/run-wasm-core.mjs) 与 [诊断绑定](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/wasm-stage/src/lib.rs)。早期 profile 未对齐的样本单独保留，不与本表混用。

原因有直接代码证据：WASM coordinator 的 [等待循环](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/wasm_wait.rs:70) 先 `try_recv`，空队列后 sleep，再检查；[sleep](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/watchdog/wasm.rs:114) 不与消息队列绑定，因此 sleep 期间消息到达不会提前唤醒。已在队列中的消息可立即消费，所以不能说每次收件固定耗费 500 µs。Native 则使用同队列的 `recv_timeout`，见 [watchdog.rs](/Users/asuad/workplace/stock_market_game/packages/engine/src/runtime/watchdog.rs:87)。锁定的 WASM std 中 `recv_timeout` 所需 `Instant` 不可直接使用，修复不能简单调用 Native 的 API。降低 sleep 参数是诊断手段，不是最终方案。

Native 的 3056 tick/s 与本次 WASM 699 tick/s 仍有显著差距；两平台产物、编译工具链及运行环境不同，剩余差距未分段测明。默认等待的 WASM 直跑约 244–265 tick/s，与先前不含 UI 的 WorkerHost 样本 265.49 tick/s 接近，支持同步等待是重要因素，但不同样本不能证明 UI 成本精确为零。**前端背压仍独立违反“前端不能牵制核心”的架构目标**，见下文；等待瓶颈的发现并不消除这一问题。

目标是上层只维护一套 Rust runner 和等待状态机，通过统一平台能力接口调用薄的 Native/WASM libOS 适配；适配可处理线程、时钟与通知差异，不复制两套上层业务实现。通知式 mailbox 需要让发送、关闭及失败唤醒等待者，并保持检查顺序、deadline、日界和失败语义；不能简单用短轮询冒充消息到达即唤醒。目标不要求生成相同机器码或两平台吞吐数学相等。

## 数据合并与前端成本

需要的是**单个可累积的差量状态**，不是保留大量已构建完整 frame 的队列。合并应发生在昂贵外围投影和跨边界序列化之前，并明确保留不可覆盖事件与顺序。当前 server 的 `DeferredInterval` 已只保留末版 projection，并合并同股分钟槽末点，但 `parts`、`price_observations` 以及不可覆盖事实仍随区间增长，见 [区间结构](/Users/asuad/workplace/stock_market_game/packages/engine/src/session/protocol/civil/session.rs:167) 和 [累积逻辑](/Users/asuad/workplace/stock_market_game/packages/engine/src/session/protocol/civil/session.rs:270)。因此不能把当前结构误称为完全有界或已经实现所需 mailbox。

Web 的 ACK 只表明交付消费者处理完当前消息，并非核心交易提交的确认；最多两个未确认 delivery 时循环暂停，见 [交付容量](/Users/asuad/workplace/stock_market_game/apps/web/src/host/wasm-tick-loop.ts:102) 和 [ACK 发送](/Users/asuad/workplace/stock_market_game/apps/web/src/host/worker-host.ts:513)。条件单目前在 [客户端执行](/Users/asuad/workplace/stock_market_game/apps/web/src/components/auto-order-manager.ts:2)，依赖逐点价格。若要让 UI 不控制核心推进、同时允许覆盖报价，应将条件判断移到权威执行侧，并明确触发时点、命令顺序、失败与存档语义；不能直接丢掉价格点而改变交易结果。

[reduce.ts](/Users/asuad/workplace/stock_market_game/apps/web/src/host/protocol/reduce.ts:98) 每次复制 `accepted` Map 并保留 canonical 更新字符串，continuation baseline 继续保留，直至非 continuation baseline 重建；每次追加 `intraday` 都复制既有前缀，累计复制量随帧数可达二次增长。同一正常更新在 [reduceEngineUpdate](/Users/asuad/workplace/stock_market_game/apps/web/src/host/protocol/reduce.ts:146) 与 `appliedState` 分别进行 translate 和 normalize。它们是代码层面可见的额外工作，但现有 profile 不足以量化各自耗时。

收盘屏障的 `CivilRefresh` 携带当日完整 `intraday` frames；非收盘屏障的 `intraday` 必须为空（[类型](/Users/asuad/workplace/stock_market_game/apps/web/src/types/generated/CivilRefresh.ts:6)），收盘归一化时逐帧重处理（[normalize.ts](/Users/asuad/workplace/stock_market_game/apps/web/src/host/protocol/normalize.ts:42)）。日终候选另经 [客户端保存链](/Users/asuad/workplace/stock_market_game/apps/web/src/save/client-day-end-persistence.ts:49)：Worker 中保存、normalize 和校验，再跨线程返回，客户端继续验证并写入；其中同步 CPU 工作可阻塞对应线程。现有时间戳不能精确拆分这些阶段，也不证明长任务完全由其中某一步引起。

## 建议边界

1. 在 Rust 中统一连续 runner 和可交换 mailbox：共同维护推进、差量累积、顺序交付、错误及恢复边界；Native 和 WASM 宿主提供线程启动、唤醒、消息运输、存储等平台能力。应先以行为测试固定两端相同输入及已受理顺序下的事实、游标、错误和恢复边界。
2. 定义逐类合并规则：末版状态可覆盖，分钟采样按既定规则合并，成交、委托反馈、披露、错误及其他不可覆盖事实保留身份和顺序；避免将全帧作为累计单位。未消费数据和必要内存上界须明确，不能通过无界队列掩盖慢消费者。
3. 日界、控制命令和 checkpoint 保持权威事务与必要栅栏；日终保存候选从连续行情交付独立处理，确保先后顺序、失败反馈和不重复提交。将条件单迁至权威侧须先定义其领域契约与迁移，不应作为单纯 UI 优化悄然改变行为。
4. 以同一 setup、seed、已受理输入轨迹和计时窗口分别测核心推进、外围投影、WASM/线程桥、序列化、传输和 UI 消费，测试日界及失败。并发实际到达轨迹不由 seed 单独决定；Native 与 WASM 吞吐不必数学相等，但前端消费不应成为权威核心运行的必要节拍。

以上为诊断和方向，不代表方案已经实现或性能目标已经达成。完整观察原件另见 [无插桩样本](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/control-run.json)、[运行日志](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/observed-run-v3.log) 和 [主线程 profile](/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/main.cpuprofile)。
