# actors 实施状态

日期：2026-10-03。实施范围为下列四项动作；源码已迁移，统一编译、行为验证与独立复核待 root 安排。没有运行 Cargo、完整回归、E2E 或性能任务；没有 Git 写操作或提交。

## hosts-01-A01：桌面 SessionHandles 命令边界

- 状态：源码实施完毕，待统一验证与独立复核。
- 文件：`apps/desktop/src-tauri/src/actor.rs`。
- 字段与方法：`SessionHandles::cmd_tx` 设私有；`SessionCommand` 仍为 `pub`；既有公开方法及签名保留。
- 真实 caller：`SessionManager::new_session` 仍构造 sender/receiver；Tauri `lib.rs` 原有方法调用无须变更。未发现 actor 模块外 raw sender 的正常调用。
- 语义：桌面 `set_speed` 仍只提交 mpsc，不等待回执；Fixed/Fastest 外部校验与顺序不变；内部非法速度由 actor 记录并忽略。
- 测试：新增外部访问 `cmd_tx` 应失败的 `compile_fail,E0616` doctest；既有 `lib_tests::desktop_speed_protocol_accepts_fixed_and_fastest_json` 及 fatal/protocol 用例保留，均未运行。
- 未完成：doctest 与统一编译/行为验证；独立复核。

## hosts-01-A02：server SessionHandles 命令及订阅边界

- 状态：源码实施完毕，待统一验证与独立复核。
- 文件：`apps/server/src/actor.rs`；相关外部 caller 由父 agent 与 server worker 负责迁移。
- 字段与方法：`cmd_tx`、`event_tx` 设私有，`SessionCommand` 仍为 `pub`；新增 `SessionHandles::subscribe_events(&self) -> broadcast::Receiver<EngineUpdate>`，直接订阅 actor 持有的 sender。
- 真实 caller：routes WebSocket 初次/Resync，以及外部 tests 的订阅已通知所属 worker 迁移；最后工作区检索未发现 actor 外正常 `.event_tx`/`.cmd_tx` 使用。
- 语义：有限正 Fixed 与 MAX_SPEED_MULTIPLIER 校验、InvalidSpeed、oneshot 确认及 Fastest 正无穷哨兵保留；没有加强或改变 server 内部非法速度的不变量错误行为。公开 sender 字段收窄是源码 API 可见性变更。
- 测试：新增两个 `compile_fail,E0616` doctest 分别检查 raw sender 隐藏，新增 `no_run` 正向 API doctest 检查 Receiver 类型；保留命令 burst 顺序与既有速度/订阅测试，均未运行。
- 未完成：统一构建、doctest、baseline/Resync 行为回归；独立复核。

## hosts-N01：组合私有宿主 Pacing

- 状态：源码实施完毕，待统一验证与独立复核。
- 文件：`apps/desktop/src-tauri/src/actor.rs`、`actor/failure.rs`、`apps/server/src/actor.rs`；依父授权同步迁移 `apps/server/src/actor/fatal_tests.rs` 三处构造与断言。
- 对象归属：actor 按值独占 `DesktopPacing` 或 `ServerPacing`；Pacing 独占 running、fastest、requested_speed、tick_interval、base_ms 和 SpeedMeter。没有共享锁、跨宿主基类或新的 engine owner。
- 方法：`new`、`apply_speed(speed, tick)`、`set_running(running, tick)`、`reset_after_restore(tick)`、`refresh_metrics(tick)`、`is_running`、`is_fastest`、`tick_interval`；私有采样 helper 为 `reset_speed_meter(tick)`。`pause_at_civil_boundary(tick)` 保留日界的无条件采样 reset，`stop_after_failure()` 只写停止态，不添加原来不存在的采样 reset。
- 真实 caller：两个 manager 初始化、actor run select guard/fresh_interval、SetSpeed/SetRunning/SpeedMetrics、成功 restore、日界 pause、desktop stop_after_host_failure、server stop_with_failure、自动批次后采样均迁到对象方法。
- 语义：desktop 最短固定周期 1µs，server 1ms；desktop 内部非法速度日志与忽略、server 调试不变量、句柄最大倍速/回执差异保留。14ms/步数预算、server worker semaphore、checkpoint/rollback、提交与 generation 仍由 actor 管理；桌面 pending_fixed_events/last_fixed_publish 保留原行为。未改撮合、费用、T+1、交易日、股/分单位或存档契约；依据现有 trading-rules、ADR-0010/0017 的宿主与事务边界，无新增交易制度规则。
- 测试：先写新增短用例，再迁移实现；未运行，不能声称确认过红灯或绿灯。两宿主新增 `pacing_keeps_speed_mode_interval_and_pause_metrics_consistent`、`pacing_restore_resets_sampling_but_fatal_stop_preserves_it`；桌面新增 `desktop_pacing_rejects_invalid_internal_speed_without_changing_state`。覆盖 Fixed→Fastest→Fixed、保留 interval、重复 running 不 reset、采样/暂停/恢复、fatal 停止保留采样、restore reset、非法值不改状态与周期下限。已有 fatal/顺序用例的业务断言保留。
- 未完成：统一编译与测试，尤其 actor fatal/protocol 与 restore 相关已有测试；独立复核。

## hosts-R2-N13：桌面 ActorHarness 测试 fixture

- 状态：源码实施完毕，待统一验证与独立复核。
- 文件：`apps/desktop/src-tauri/src/actor.rs` 的 `cfg(test)` 支持对象、`actor/fatal_tests.rs`、`actor/protocol_tests.rs`。
- 对象归属：`ActorHarness` 持有 mock App、SessionActor、命令 sender 与按需建立的 engine-event/engine-failure receiver；failure 订阅只在 fatal 场景建立，未用空 channel 占位。
- 方法：`new_protocol_actor(game, fastest, pause_preferences, session_id, timeline_id)` 集中构造；`subscribe_engine_events`、`subscribe_failures` 建立并保持订阅生命周期，receiver 始终归 harness。
- 真实 caller：`fatal_tests::assert_auto_step_fatal` 与 `protocol_tests::capture`，无生产 caller。game seed/setup、故障注入、ReplayGuard、ticks 和每个业务断言仍在原场景。
- 语义：保留两个场景原始 running/fastest/requested 初值；fatal 的 sender close 检查仍针对原命令通道；mock App 活到场景结束，receiver 收取顺序与 fatal 后无重复事件的检查保留。没有替代或重建业务状态。
- 测试：现有三个 fatal 用例及四个 protocol 用例均保留，未运行；未为 fixture 增加镜像测试。
- 未完成：统一构建与现有用例；独立复核。

## 已执行的有限检查

- `rustfmt --edition 2021 --config skip_children=true`：仅上述所属 Rust 文件，成功；不是类型检查。
- `git diff --check -- <所属文件>`：成功；不是行为验证。
- 工作区 caller 检索：raw sender 正常使用只存在于 actor 内；仅 doctest 故意访问私有字段。

## 给 root 的代表性短单测 filters

测试必须从统一预构建产物执行，单命令和 case 保留 10000ms 进程树硬限，并显式配置并行线程；下列只提供精确 filter，不在本 worker 运行 Cargo。

| crate / target | 精确 filter |
|---|---|
| `stock-market-game` / lib `stock_market_game_lib` | `actor::tests::pacing_keeps_speed_mode_interval_and_pause_metrics_consistent` |
| `server` / lib | `actor::interval_tests::pacing_restore_resets_sampling_but_fatal_stop_preserves_it` |

补充代表性用例：desktop `actor::tests::desktop_pacing_rejects_invalid_internal_speed_without_changing_state`；桌面全部 `actor::fatal_tests` 与 `actor::protocol_tests`；server `actor::fatal_tests`。公共 API doctest 须由 root 统一门禁运行。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
