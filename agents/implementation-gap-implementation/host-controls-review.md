# Host controls 独立复核

## 范围与依据

非作者 `/root/review_host_controls` 复核 G18、G19、G40、G51；G52、Remote 链分别由其他 reviewer 负责。本记录尚未给出最终完成结论。

已读仓库 AGENTS、工程原则、架构、开放问题、ADR-0010，以及总账与历史宿主证据。复核范围为 Worker loop/request/adapter、Tauri adapter 与 Desktop actor 的完整 diff，并追踪 App 与 lifecycle 的真实消费者。改动调整宿主交付和确认，不新增 A 股交易制度；价格时间优先、T+1、竞价/日界及订单 `CommandQueued` 语义不得改变。

## 初次发现与状态

| 项目 | 状态 | 证据 |
|---|---|---|
| G18 | 待修复后复核 | Worker 两个 outstanding delivery 与最多 64 个完整提交的批次有界；ACK 在同步 callback 返回后发送，不依赖绘制。但 App 的 callback 调用 `ProtocolCoordinator.accept`，该方法吞入协议失败仅通知 `setError`，返回后 adapter 仍 ACK，且后续提交可把 failure 改回 ready。已反馈作者与 root，要求失败锁定与传播、不得 ACK 未接纳提交。 |
| G19 | 实现方向通过，边界短测待补 | 固定 timer 周期为 `max(tick_interval, 16ms)`；elapsed 除以 tick interval 得到 due，Rust 聚合 TickBatch 后 IPC；仅按实际完成 tick 推进时间债务，预算未执行部分不被直接扣除。现有新增测试只断言 frames 大于一，作者补连续窗口及预算尾数覆盖。 |
| G40 | 阻断，等待跨层复核 | Worker start/stop/speed 由带 generation 的 applied 回执完成；Desktop controls 由 actor 应用后 oneshot 回执完成；EngineHost 暴露 Promise。UI reviewer 另发现 pause preferences checkbox/storage 仍乐观更新，正在修复；仅接口变 Promise 不足以核销真实消费者。 |
| G51 | adapter 通过，最终 lifecycle 复核待整体完成 | Tauri dispose 先使会话不可用并移除监听，await stop_session；IPC rejection 通过 Promise 与保留到 finally 的 fatalCallback 显式交付。lifecycle await dispose 并展示 cleanup failure；初始化资源归属 Q24 不混入此项。 |

## 定向验证

执行 `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/host/wasm-tick-loop.test.ts apps/web/src/host/worker-host.test.ts apps/web/src/host/tauri-host.test.ts`，三份测试文件通过，约 314ms。外部进程树 deadline 为 10000ms，case timeout 为 10000ms，最多四个独立测试进程；Node 为 v25.8.2。此结果不替代后续修复的再次短测或 Rust binary 验证；没有运行完整回归、长期市场或发布验收。

改动整体属于已确认缺口的最小宿主范围；新增并发 request 资源取消不施加交易数量配额，不压缩交易事实或丢弃 CivilUpdate。尚不得报告本批全部完成。

## 最终再次复核

作者 ready 后重新审查稳定完整 diff，并交叉读取 [UI consumer 独立复核](ui-contracts-review.md)。初次表格保留作为真实过程证据，以下为最终状态。

| 项目 | 最终判断 | 修复及边界 |
|---|---|---|
| G18 | PASS，可核销本项静态缺口 | `ProtocolCoordinator.accept` 回传接纳结果，failure 锁住后续 protocol；App/lifecycle 同步传递结果，App failure 调 fatal callback 更新 running。Worker 消费者拒绝时不 ACK、不处理批次尾部、立即终止并取消 pending。标准 failure/error 也设置终态、清 callback 与缓存，重复 failure 与晚到 protocol 不再消费或重复上报。完整提交保持顺序；generation credit 与 timer epoch 防旧代 ACK/旧 timer。 |
| G19 | PASS，可核销本项静态缺口 | Rust 固定高倍率按到期 tick 预算聚合，每周期发送完整 TickBatch；14ms 执行预算未完成部分保留时间债务，不额外加固定 tick。冷启动测试检查实际完成数量对应的时间债务，稳态两个窗口检查 seq/tick 连续及实际 completed duration；原 singleton 断言与新需求矛盾，替换为逐帧精确连续性，而非删除保留事实断言。 |
| G40 | 三宿主与真实 UI consumer 联合 PASS，可核销本项静态缺口 | 控制 promise 只在 worker/actor 已应用后完成，不把入队当订单接受或成交。UI 同一串行 owner await 后更新 running/speed，并拒绝旧 host 成功/失败漂移；偏好 await 后更新 checkbox/storage。最后合并 [Remote 独立复核](remote-chain-review.md) 的最终完整结论，真实三宿主链已闭环；未假称本 reviewer 独立重复执行 Remote 测试。 |
| G51 | PASS，可核销本项静态缺口 | dispose IPC rejection 可由调用方 await，监听已释放且 failure 显式展示；lifecycle 启动取消、返回启动页及卸载清理均等待或明确展示 Promise rejection。Q24 初始化资源归属仍不是此项证明。 |

第二次发现：标准 `notifyFailure` 最初只销毁底层 Worker，却未设置 adapter `disposed`、未清 callback，可能消费晚到 protocol。作者已修为一次性终态；真实 failure→late protocol→repeat failure 短测验证消费数、ACK 数不增加，termination/fatal 各一次，start 拒绝已销毁。

最终独立执行四文件 Node 命令（前述三文件加 `protocol-coordinator.test.ts`），外部进程树 deadline 10000ms、case timeout 10000ms、并发 4；exit 0，四份文件全部通过，约 323ms。真实 coordinator 的批次中间缺口测试确认合法 tick1 接纳后，tick3 缺口失败、合法 tick2 尾部未消费，无 ACK，pending speed 被拒绝，terminate 一次。

作者提供当前 worktree 最新 `--no-default-features` test binary 后，独立执行：

- `node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/stock_market_game_lib-517b418f5f5599ac actor::fatal_tests --test-threads=8`：11/11 通过，0.36s。
- `node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/stock_market_game_lib-517b418f5f5599ac actor::protocol_tests --test-threads=8`：4/4 通过，0.29s。
- `git diff --check`：exit 0。

曾对旧 binary 执行 protocol suite，2/4 因旧 singleton 断言失败；已明确反馈并在新源码重编后上述 4/4 再次通过，不隐去失败。编译由共享作者入口 `-j16` 执行，reviewer 未重复编译；本记录只将独立 binary 短测记作独立测试证据。没有运行完整回归、长验收、真实浏览器卡顿或实际绘制频率测量，目标 16ms 不冒充硬实时保证。

本范围有效发现均修复并再次复核，无遗留代码阻断；A 股单位、撮合、日界、竞价、T+1 及订单事实没有改变。改动均为已确认 G 所必需，没有新增交易制度、依赖、持久队列或不必要抽象。

## cached baseline 最后增量复核

收到作者最后 ready 后独立复核 `worker-host.ts/.test.ts`、`tauri-host.ts/.test.ts` 的 cached baseline 接纳增量。两宿主首次 start 的 baseline callback 返回 false 时清除 callback、显式拒绝，不设置 delivered generation，且不发送 Worker start 或 Tauri resume。显式重试仍能重新交付此前未接纳的 baseline；没有将拒绝伪装成成功或永久交付。

Worker 双 host 测试还验证旧 host 的 started/protocol 消息不能确认新 host 的 pending start、不能发旧 ACK、不能改变新 host 已交付 baseline；旧 host 后续显式重试成功只交付一次。改动保持 host 身份隔离，属于既有接纳/确认契约收尾，没有扩大交易制度或资源回收契约。

独立执行 `node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=2 apps/web/src/host/worker-host.test.ts apps/web/src/host/tauri-host.test.ts`：exit 0，两测试文件通过，约 323ms；外部进程树与 case deadline 均为 10000ms，并发 2。`git diff --check` exit 0。此增量 PASS，前述各 G 判断保持不变；没有重复 Rust 编译或扩大至回归。

## G04/G40 全链与文档收敛

最后收到 Remote reviewer 完整 PASS 后，独立核对其最终记录及生产链：Server `SetRunning` / `SetSpeed` 先修改 pacing 再 oneshot reply，handles await reply，routes await handles 成功才返回 HTTP 成功；Remote await requestJson，SessionControlCommands await host 并核对 host identity 后才调用 App ports 更新 Redux。因此 G40 的成功确认是宿主已应用，不是 mpsc enqueue，更不是订单成交；`CommandQueued` 的交易入队语义没有变化。Worker/Tauri/Remote 与真实 UI consumer 联合门禁均 PASS。

G04 联合 PASS：Worker 保持 delivered generation 守卫；Tauri 普通暂停继续不重送已接纳 cached baseline；Remote start 只交付尚未交付且当前 protocol-ready 的 baseline，普通暂停继续不回放旧状态。load、明确 resync 与 reconnect 的新权威 baseline 仍正常交付，消费者拒绝不标记交付完成。上述基线规则与真实 App toggle/start/visibility consumer 已串联核对，不把重连同步误写为从磁盘读档。

总账第 2 节逐行脚本核对：79 个唯一 G，63 行为“已补齐”，剩余 16 行为 G07/G08/G09/G42/G43/G16/G28/G35/G36/G37/G38/G41/G73/G39/G57/G72。不是统计全文重复验收表。本轮宿主 11 项为 G01–G05、G18、G19、G40、G51、G52、G66，加 G17 共 12 项，数字与 README 对应分组一致。G16/G57 虽有实施短测记录，尚未被本轮文档核销，未据临近记录擅自改变剩余数量。

Remote 最终 heartbeat 5 项、WS 10 项通过与 1 个既有 probe ignored 均由该作者记录、Remote reviewer 核对，不假称本 reviewer 重跑；G17 native 数值与 WASM target 编译来自 tick-performance-review。真实浏览器、性能长验收、完整回归与发布验收仍未执行，不因文档收敛扩大这些证据边界。历史 pending/失败段落保留但已被最终结论明确取代，不能视作当前仍待复核。
