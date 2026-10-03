# frontend host 对象提取记录

状态：实施与精确短测通过，交由未实施的 reviewer 做完整 diff 独立复核；本记录不代替复核门禁。

## frontend-N06：WorkerRequestScope

- 唯一 owner：`worker-request.ts::WorkerRequestScope` 绑定一个 Worker port，私有 `sequence` 与 `pending: Map<requestId, PendingRequest>`；每项拥有原有 listener、timeout、resolve/reject 与幂等 cleanup 的 settled 标识。
- 方法：`nextRequestId()`、`request(request, successType, timeoutMs)`、`pendingCount()`。`requestWorker()` 保留独立请求兼容入口，但 WorkerHost 所有生产请求使用同一个 scope。
- 生产 caller：`createWorkerHost` 构造唯一 scope；`setPausePreferences/readSpeedMetrics/playerWorkingOrders/npcDecisionTrace/calculateIndicators/stepOnceForE2E/submitIntent/civilDate/endCivilDay/save/refreshBaseline/load/queryPublicReports/publicReportById` 从其分配 ID 并发送请求。六个现有 helper（metrics/orders/indicators/step/restore/refresh）接收同一 scope；原有 async wrapper 与解析顺序保留。
- 删除的旧 owner：WorkerHost 的 `requestSequence`。`currentGeneration`、`baselineEpoch`、callback、session lifecycle 仍在 WorkerHost。
- 等价边界：逐请求 listener 仅接受同 requestId 与该请求 generation；success/operationError/timeout 只 settle 一次并回收 listener/timer/registry。ID 仍在原 caller 位置分配，错误内容与 10000ms timeout 不变。
- `worker-lifecycle.ts` 无需改动：pause/dispose 仍只负责线程生命周期。scope 不接入新的 dispose/fatal 立即 reject；同步 postMessage 抛错后仍等待原 timeout 清资源，restore 也不主动取消旧请求。动作原文明确将这些行为修复另批实施，本批只完成等价 ownership 提取。
- 测试：`worker-request-scope.test.ts` 新增连续 ID、逆序响应、错误 generation 忽略、重复响应、operationError/短 fake timeout 资源回收、同步 postMessage 抛错后原 cleanup 时序；`worker-request.test.ts` 七个既有关联用例保持；`worker-host.test.ts` 保留基线/save/order/helper 用例并增加 dispose/fatal 不立即 settle 与同一 scope 跨命令连续序号；`worker-lifecycle.test.ts` 保留生命周期用例。
- 红绿证据：新 scope 测试在未实施时两个 assertion 失败，实际值 `undefined`、期望 `function`；既有四个 host/request/lifecycle/Tauri 文件通过。加入 scope 后新测试和既有用例通过。Tauri 新行为用例先在旧实现运行通过，作为等价提取的 characterization 基线。

## frontend-R2-N03：TauriTimelineState

- 唯一 owner：`tauri-timeline-state.ts::TauriTimelineState` 私有 `timelineId/generation/cachedBaseline/baselineEpoch`。初始 generation 为规范十进制 string `"1"`。
- 方法：`setInitialTimeline/installInitialBaseline/replaceRefreshedBaseline/replaceRestoredBaseline`；`matchesTimeline/currentGeneration/baselineForDelivery/baselineForRead`；`captureGeneration/captureQueryCursor/assertGeneration/assertQueryCursor`；`clearForDispose`。`nextTauriGeneration` 用原有 BigInt 精确递增。
- 生产 caller：初始化 create_session 后先设临时 timeline，再校验 initial baseline 并先写 timeline、后 parse/install snapshot；两类 listener 查询 timeline；start/snapshot/tick/day 读取唯一 cache；save 只 capture/assert generation；orders/NPC 额外 capture/assert baselineEpoch；refresh/load 调同步替换方法后，由原 facade 执行 callback/resume。
- 删除的旧 owner：createTauriHost 的四个局部字段。sessionId、running/disposed、callback/fatalCallback、unlisten、capability 与 invoke/listen 保持 facade 唯一拥有。
- 安装失败边界：restore 先写 generation/timeline，parse 失败保留旧 cache/epoch；refresh 先写 timeline，parse 失败保留旧 cache/epoch。callback 抛错发生在新 cache/epoch 已安装之后，load 不继续 resume。restore/resume 失败不顺带改 running 或自动补 resume。
- 查询边界：save 保留 disposed/generation guard，无 epoch guard；orders/NPC 保留 response generation/current generation/epoch guard；civilDate/publicReports/byId 无新增完成后的统一 guard。clearForDispose 只清 timeline/cache，generation/epoch 不重置；晚到 refresh/load 保留原有安装行为。
- 测试：`tauri-host.test.ts` 新增真实 mock event 的旧 timeline 过滤、callback-before-resume、同 generation refresh 使旧 orders/NPC 失效而 save 有效、restore snapshot 解析失败、callback 抛错、restore/resume 失败、dispose 后晚到 refresh/load/civilDate/save 差异、initial generation mismatch 与非 nextGeneration。`tauri-timeline-state.test.ts` 增加 Number.MAX_SAFE_INTEGER 之后精确递增、refresh parse failure 的旧 query epoch、dispose 不重置 generation/epoch 与旧 refresh 拒绝。既有 event-coordinator 契约测试保持。
- 静态契约断言：`tauri-startup-contract.test.ts` 随真实 owner 更新读取文件位置，保留 initial baseline 校验、snapshot parse、string/BigInt、Rust actor restore 顺序断言；App lifecycle 接线按 App agent 新模块检查返回原 createTauriHost Promise，再由 lifecycle await createHost，及 document.hidden getter 后 start/stop 顺序。

## 领域与验证范围

仅迁主线程请求资源与 Tauri IPC 消费者时间线 authority；不修改 A 股 Money（分）、shares（股）、T+1、交易阶段、委托、披露、日终 SaveCandidateKey、generated Rust 契约或 engine 权威状态。依据现有 ADR-0007、ADR-0010、ADR-0025 与项目工程原则；不裁决开放的历史/持久化能力。

Node：`v25.8.2`（项目要求 >=24.18.0）。普通测试实际使用四个并行 file worker、case timeout=10000ms、外置进程树 deadline=10000ms：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=4 apps/web/src/host/worker-request-scope.test.ts apps/web/src/host/worker-request.test.ts apps/web/src/host/worker-host.test.ts apps/web/src/host/worker-lifecycle.test.ts apps/web/src/host/tauri-host.test.ts apps/web/src/host/tauri-timeline-state.test.ts apps/web/src/host/tauri-startup-contract.test.ts apps/web/src/host/tauri-event-coordinator.test.ts
```

结果：8 文件通过，default isolation 的 file 级报告 duration 502ms；无 skipped/cancelled。另以 `--test-isolation=none` 核对 scope/timeline/startup 的 12 case 详情通过，不冒充完整回归。

精确 lint：`run-with-deadline.mjs 10000 -- apps/web/node_modules/.bin/oxlint --threads=4`，列出本批 9 个改动 TS 源/测试文件；通过。`git diff --check` 对本批已跟踪改动文件通过（只读检查）。

未运行全回归、build、E2E、tsc；未执行 Git 写入或提交。独立 review 由父 agent 统一安排，发现需修复并复核后才可宣称整批完成。
