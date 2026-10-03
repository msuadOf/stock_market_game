# 宿主实现更新复核（2026-10-03）

## 范围与依据

本次以当前 `HEAD` 源码复核历史审计 `implementation-audit-2026-10-02.md` 中 G01–G05、G18–G20，沿 web adapter、worker/server/desktop actor 与前端调用链确认行为。已完整阅读 `docs/principles.md`、历史审计报告及 ADR-0005、ADR-0010。未运行测试或回归；测试源码不作为生产行为存在的证明。本复核只更新本报告，不修改实现。

## 逐项结论

| ID | 当前状态 | 复核结论与当前生产证据 |
|---|---|---|
| G01 | 仍缺，旧结论准确 | WebSocket URL 仍将 token 放 query：`apps/web/src/host/remote-request.ts:10-16`；服务端 WS handler 只从 Authorization header 认证：`apps/server/src/routes.rs:1062-1072`，而浏览器 WebSocket 构造只传 URL（`apps/web/src/host/remote-host.ts:118-121`）。因此浏览器 remote WS 握手未带服务端要求的 token，HTTP 请求带 bearer 不会补上 WS 握手头。服务端会拒绝该握手，远程宿主无法建立协议连接。 |
| G02 | 仍缺，旧结论准确 | Web adapter 对 `/api/speed` 的 GET 未附授权头：`apps/web/src/host/remote-host.ts:197-199`；服务端 GET handler 调用 `authorized_session(... authorization_token(&headers))`：`apps/server/src/routes.rs:907-911`。实际 UI 每秒轮询 host `readSpeedMetrics`：`apps/web/src/app/useSpeedMetricsPolling.ts:20-30`；因此远程该读取请求仍因无凭据失败。 |
| G03 | 仍缺，旧结论准确 | 服务端 pull 模式的 `GetFrame` 消费分支存在：`apps/server/src/routes.rs:1430-1445`；remote adapter 建连后只处理服务端消息，未发现任何 `GetFrame` 发送或定时取帧路径（`apps/web/src/host/remote-host.ts:118-136, 200-208`；文件内唯一客户端 send 为 Resync/SubmitIntent，见 `:68-73, 209-216`）。切换 delivery 仅换 socket；界面可选 pull 不等于 pull 能持续取帧。 |
| G04 | 仍缺；旧报告的调用场景遗漏，Worker 不属于部分修复 | App 的继续按钮明确再次调用 `host.start`：`apps/web/src/App.tsx:362-374`。Remote `start` 每次均重送缓存 baseline：`apps/web/src/host/remote-host.ts:152-162`；Tauri 同样每次重送：`apps/web/src/host/tauri-host.ts:151-158`。这些缓存只在安装/刷新/读档时更新、不随 delta 推进：Remote `apps/web/src/host/remote-publisher-state.ts:52-61`，Tauri `apps/web/src/host/tauri-timeline-state.ts:64-95`；普通协议更新由 `apps/web/src/host/protocol-coordinator.ts:96-103, 117-128` 应用，不写回宿主 baseline cache。因此暂停后继续会向 coordinator 重放旧 snapshot baseline，违反 ADR-0010「baseline 仅初始化、读档、显式重同步」，可回退/重置已推进状态。Worker 不应算部分修复：`deliveredGeneration` 守卫在旧基线 `b89afb3` 已存在（`git show b89afb3:apps/web/src/host/worker-host.ts` 对应 `start` 实现），本轮 `b89afb3..8cf34a1` 仅把它保留在 `apps/web/src/host/worker-host.ts:255-265`，不是本次修复。当前缺口主要是 Remote/Tauri。 |
| G05 | 仍缺，旧结论准确 | server 每 30 秒发 Ping，却没有检查 Pong 截止时间或关闭无响应连接：`apps/server/src/routes.rs:1303-1306, 1389-1402`。Remote 意外 close 会调用 fatal `fail`，没有重连调度：`apps/web/src/host/remote-host.ts:129-135`；`connect()` 只在启动、显式切换模式或 load 无连接时被调用（`remote-host.ts:118-121, 154-160, 201-208, 268-272`）。`capabilities.reconnect: true`（`:153`）与实现不符。手动 `start` 可建连，但那不是自动恢复。 |
| G18 | 仍缺，旧结论准确 | `WasmTickLoop.publish` 每个 step 都立即 `post` 一条完整 protocol 更新（`apps/web/src/host/wasm-tick-loop.ts:66-74, 76-81`）；宿主收到后直接调用 callback（`apps/web/src/host/worker-host.ts:214-221`）。循环的 `setTimeout` 让 Worker 有机会处理消息，但不限制尚未消费的主线程消息数，也没有合并/背压队列。未见生产端 `uiFrame` 协议或等价有界机制。 |
| G19 | 仍缺，部分旧描述需校正 | 固定倍率 actor tick 仍执行 `run_cycle(1)`：`apps/desktop/src-tauri/src/actor.rs:832-838`；该路径的 `publish_protocol_cycle` 立即 emit：`:907-920, 927-939`。所谓 `pending_fixed_events` 未实现聚合，`flush_fixed_events` 只清空：`:894-896`。已有 Fastest 按 `FASTEST_BATCH_MAX_STEPS` 批次运行：`:890-892`，所以准确结论是 Fastest 有批次、固定高倍率生产路径没有 16ms 聚合。此次抽取 `DesktopPacing` 未改变该事实。 |
| G20 | 仍缺，旧结论准确 | 重构后启动逻辑仍以 `initialSlot === null ? DEFAULT_SEED : BigInt(initialSlot.seed)` 选择种子：`apps/web/src/app/useSessionHostLifecycle.ts:90-98`；`DEFAULT_SEED` 固定为 `42n`：`apps/web/src/config/defaults.ts:120-121`。因此普通新局仍固定种子。 |

## 重构新增遗漏与契约保留

- 代码重构把 Remote/Tauri/Worker 请求、发布状态和节奏状态拆成 helper，但核心已实现宿主能力仍在：Remote 仍支持 push 发布和服务端命令队列、Resync、`CommandQueued` 回执（`apps/server/src/routes.rs:1414-1450`；adapter `apps/web/src/host/remote-host.ts:68-73, 209-216`）；Tauri 仍使用 timeline id 拒绝旧时间线事件（`apps/web/src/host/tauri-host.ts:113-120`），读档仍递增 generation 并安装新 baseline（`apps/web/src/host/tauri-host.ts:228-239`、`apps/web/src/host/tauri-timeline-state.ts:78-94`）；Worker 仍有 session slot generation 管理和操作请求 ID 关联。未发现因本轮抽取而完全删除的这类独立宿主契约。
- 但 Remote 声称支持的 `pull` 本身从历史到当前都没有消费循环（G03），并非重构新增丢失。Tauri 固定高倍率聚合同理仍未落到生产路径（G19）。
- 本次新增的 `WorkerRequestScope` 为每个请求保存待办表项并在响应/超时后清理（`apps/web/src/host/worker-request.ts:20-70`）；宿主 `dispose` 没有主动取消这些待办项（`apps/web/src/host/worker-host.ts:269-275`），但旧版 `requestWorker` 同样只在响应/超时移除 listener，并无 dispose cancellation（基线实现可由 `git show b89afb3:apps/web/src/host/worker-request.ts` 核实）。这是现存生命周期债，不应误报为此重构回归。
- 尚未逐文件复核本次庞大 OOP refactor 的所有非宿主代码，故“未发现契约丢失”限于上述已跟踪宿主契约及其调用路径；不等同于完整全仓回归结论。

## 总结

G01、G02、G03、G04、G05、G18、G19、G20 均仍缺。G04 在实际暂停/继续按钮路径上可触发：Remote/Tauri 再次 `start` 会重放旧缓存 baseline；Worker 原有 generation 守卫在本次范围前已存在。重构未显示这些宿主契约被删除；它没有修复以上未完成能力。
