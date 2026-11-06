# WASM App 日终候选捕获修复记录

日期：2026-10-30（Asia/Shanghai）  
Worktree：`<WORKSPACE>/.worktree/wasm-app-save-capture-2026-10-30`  
Branch：`codex/wasm-app-save-capture-20261030`  
Base：`8e058acae3ffa498714dc54d810c00ee5c315278`

## 改动

- `worker-host.save` 的两种请求形态都使用独立 60,000ms deadline；普通请求默认值与 WorkerRequestScope 未变。中文注释说明默认大档日终候选约 15 秒的实测背景。
- App 使用同一个经过校验的 candidate Promise 交给 `DayEndPersistence.completed`。仅当 Promise resolve，且 host 仍是当前宿主、ProtocolCoordinator 仍为 ready 且 generation 匹配时，才刷新 player orders。保存拒绝继续进入原 completed rejection notice，不触发刷新。
- 因 AppShell 闭包难直接用现有行为 harness 渲染，抽取三参数 `captureCivilDayEndCandidate`，由 App 实际调用，并以 deferred Promise 行为测试覆盖等待、resolve、reject、host/generation 变化。
- 未更改 Civil reduction 的 `playerOrdersReady: false` 或 Worker 协议语义。

## TDD 证据与定向验证

红测：临时恢复 `worker-host.save` 默认 10 秒后运行：

`<WORKSPACE>/.tmp/node24-env/node <WORKSPACE>/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs 10000 -- <WORKSPACE>/.tmp/node24-env/node --test --test-timeout=10000 --test-concurrency=4 --test-name-pattern='WorkerHost save remains' apps/web/src/host/worker-host.test.ts`

结果：exit 1，`Worker save 操作超时（10000ms）`，发生在 mock clock 10,000ms；原始日志 `red-worker-host.log`。

绿测使用相同 Node 24.18.0、外部 deadline supervisor、10,000ms 单测与命令 deadline、`--test-concurrency=4`。三个互不冲突 suites 并发运行：

- `worker-host.test.ts`：35 passed，包含 15,202ms 成功、save 60,000ms 显式超时、WorkerRequestScope close 即时取消并忽略晚到响应、普通 `civilDate` 仍按 10,000ms 超时；`green-worker-host.log`。
- `civil-day-end-capture.test.ts`：3 passed；`green-candidate-capture.log`。
- `app-startup-wiring.test.ts`：8 passed；`green-app-wiring.log`。

`git diff --check` 通过。未运行 build、Cargo、浏览器或长验收。

## 当前状态

未提交、未 cherry-pick，等待独立完整 diff 审查。该批只处理已授权的 save deadline 与 Civil 候选 resolve 后刷新门控；其他 busy worker 查询顺序不在本实现范围。
