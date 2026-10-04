# Host controls 缺口实施记录

## 范围与依据

- 范围：G18、G19、G40、G51、G52，以及 Tauri G04 的恢复时旧 baseline 重送。
- 已读 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0010 与总账对应原证据。
- 本批只修改宿主交付、控制确认与公开错误诊断，不修改撮合、委托受理、T+1、涨跌停或集合竞价规则；`CommandQueued` 仍只代表入队，不代表接受或成交。
- UI 与 Remote consumer 由各自 owner 同步接线，本记录不替代其验收；Server actor 原有控制 oneshot 已表示实际应用，本批没有重复改写 Server actor。

## 实施

| 编号 | 实施与边界 |
| --- | --- |
| G18 | Worker 按 16ms 目标聚合完整提交，单个 transport batch 上限 64 个完整提交、最多两个未接纳 batch；不删除提交、fact、seq 或分钟点。容量到达时暂停生产，匹配 generation/deliveryId 的消费者 ACK 才恢复。ACK 是同步 consumer 接纳完成，不是 React render 或 requestAnimationFrame；最快循环仍逐 task yield，不等待绘制。单批容量上限可使非常高倍率在 16ms 前发布，目标频率不是硬实时承诺。暂停与基线查询先发布尾批次；旧 generation ACK 与旧 timer 不释放 credit 或复活循环。 |
| G19 | Desktop 固定倍率 timer 使用 `max(tick_interval, 16ms)`，依据尚未消费的现实 interval 算出到期 tick，在 Rust `run_cycle` 聚合后跨 IPC；仍保留 14ms CPU budget、命令优先和完整协议提交。计时债务只按实际完成的 tick 扣减，冷启动与 budget 截断不丢债务。Fastest 原有 CPU 批次不被冒充为固定倍率实现。 |
| G40 | `EngineHost.start/stop/setSpeed/dispose` 返回 Promise；Worker 控制请求带 requestId/generation，应用后才回执，旧代响应拒绝。Desktop actor 的 running、speed、pause preferences 与 shutdown 新增实际应用 oneshot；无效 speed 显式拒绝。Tauri 等待 invoke 回执，确认后修改 running，并检查销毁/旧代。请求 scope 在终止时立即取消，不等待原超时。UI/Remote owner 已传播同步 consumer 接纳 boolean。 |
| G51 | Tauri dispose 释放监听后等待 stop_session；IPC rejection 既走原 fatal callback，又向调用方 reject。不能再由一个丢弃的 Promise 静默吞错。晚到 restore/refresh 不得复活已销毁的 baseline。 |
| G52 | coordinator 保存已知 actual generation/tick/seq 区间、expected generation/tick/seq 与当前 cursor；未知或双 variant 不猜 expected tick。真实 buildErrorFeedback 精确放行公开游标字段，kind 字符串只允许 TickBatch/CivilUpdate，其它私密字段和任意字符串仍脱敏。 |

## TDD 与短验证

- G18 credit 反例：两次生产后 timer 仍存在，红；消费者 credit 修复后绿。
- 旧 timer 反例：stop 后 restart，晚到旧 timer 推进一次 tick，红；timer epoch 修复后绿。
- G40/G51 反例：start 返回 undefined 而非 Promise、dispose rejection 无出口，均红；等待确认与显式 rejection 修复后绿。
- G52 context 反例：actual/expected 为 undefined，红；首次 context 实施绿。独立复核再发现真实复制路径把字段脱敏，新增 buildErrorFeedback 反例红；精准白名单与未知类型边界修复后绿。
- 首次 Rust ordinary binary 验证发现旧固定 singleton 断言与 G19 新契约冲突，以及冷启动耗尽 14ms budget；经 root 明确认可，固定 singleton 断言改为逐帧严格 tick 连续，保留完整日 30 ticks、seq、runtime delta 与真实 ReplayGuard 校验。稳态多帧验证明确先 warm 一个 tick，再新增未 warm 的冷启动债务验证，不冒充冷启动总能发布多帧。
- Node 命令均使用外部 `timeout 10s`、`--test-timeout=10000`。loop/coordinator/error-details 使用 `--test-concurrency=8`；Worker/Tauri 的全局 Worker/window/IPC mock 必须进程内隔离执行，使用 concurrency 1，相关套件总耗时约 0.2s。
- 最后相关短测：Worker host 17/17；WasmTickLoop 11/11；Tauri host 11/11；coordinator 7/7，coordinator 与 error-details 联合 15/15。真实 coordinator bridge 中间提交失败时，已接纳的首提交保留、尾部不消费、不 ACK、pending 控制拒绝、Worker 只终止一次。
- `tsc -p apps/web/tsconfig.app.json --noEmit` 最后通过，约 6.4s；相关源文件 oxlint 与 diff whitespace 检查通过。
- Rust 纯编译准备与测试严格分开：`timeout 300s flock /tmp/stock-game-implementation-audit-cargo.lock cargo test -p stock-market-game --lib --no-default-features --no-run -j16` 最后通过，34.61s。默认 custom-protocol 构建因本 worktree 没有 frontendDist 失败，未伪造 dist；普通 actor binary 使用 no-default-features，不代表桌面发布包验收。
- Rust ordinary binary：`timeout 10s target/debug/deps/stock_market_game_lib-517b418f5f5599ac actor::fatal_tests:: --test-threads=8`，11/11，0.34s；同 binary `actor::protocol_tests:: --test-threads=8`，4/4，0.29s。
- 未执行完整回归、长矩阵、真实浏览器/WASM 高倍率吞吐或真实 WebView 刷新实测；16ms 是目标，不能从常量或短 fixture 声称实测达到 60Hz。

## 独立复核

- `review_host_controls` 未参与实施，复核完整宿主 diff，并独立执行最终 Rust ordinary binaries：fatal 11/11、protocol 4/4。G18/G19/G40 的本 owner 部分/G51 通过；Remote 部分仍以独立 Remote review 为准。
- 已修复其有效发现：consumer failure 不能 ACK 后继续消费；coordinator failure latch；标准 Worker fatal 必须切外层 terminal 并忽略晚到 protocol；实际提交消费与 UI 绘制分离；冷启动与两窗口债务测试。
- `web_gap_review` 的 G52 反馈出口、未知/双 variant 诊断发现均已修复并请求最终复核。最终结论由 root 汇总其独立报告，不在本记录提前宣称整体完成。
- G52 增量：`onApplied` consumer 抛错时不得使用已推进 cursor 冒充本次输入的 expected；新增真实反馈反例先红（错误显示 tickFrom=2、seqFrom=1），在 try 前 pin attempt cursor 后绿（本次输入 tickFrom=1、seqFrom=0）。coordinator/error-details 联合 16/16，通过；再次交由 `web_gap_review` 独立复核。
- 最后跨链增量：Tauri cached baseline consumer 返回 false 时，start 必须 reject、清除 consumer 且不得 resume actor 或标记已交付；新增反例先红（未 reject），修复后 Tauri 12/12 通过，tsc 通过。显式重试 start 时仍必须交付该未接纳的 baseline；交由独立 host reviewer 增量复核。
- 同一消费拒绝契约补齐 Worker cached baseline：false 明确 reject、清 callback、不发送 start、不 ACK、不标记 baseline 已交付；显式重试仍交付 baseline。首次无 actor mock 回执的红测被外部 10s deadline 终止，随后测试补齐真实意外 start 的 actor 回执，确定性红为“缺少 rejection”；修复后 Worker 18/18、Tauri 12/12 联合 30/30，211ms，tsc 通过。双 host 反例验证旧 host 的 started/protocol 不得确认新 host 的在途 start，也不得产生旧 ACK 或改变新 host baseline；请求独立增量复核。
- 最终闭环：`web_gap_review` 确认 G52 三项有效发现均已修复，最终增量 PASS；`review_host_controls` 对最后 Worker/Tauri cached baseline 增量独立执行两文件短测及 diff 检查，PASS，并追加 `host-controls-review.md`。之前“请求复核”的描述保留为过程历史，不代表当前门禁仍未完成。
