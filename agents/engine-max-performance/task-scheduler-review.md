# 第二批 Worker task scheduler 独立复核

复核范围：`wasm-worker.ts` 的调度器接线，以及新增 `worker-task-scheduler.ts`、`worker-task-scheduler.test.ts`。已阅读 `AGENTS.md`、`docs/principles.md` 与 ADR-0032。未修改实现。

## 结论

当前复核范围未发现阻断问题。调度器仅改变 Web Worker 的 task 唤醒方式，不触及 Engine、订单接收、撮合或账户状态，因此没有改变 A 股交易语义。ADR-0032 的共享 ingress receipt、真实接收顺序与 cutoff 要求不由本次调度器实现或证明；本次接线也未见把 `CandidateSource`、key 或固定来源阶段引入排序。

`WasmTickLoop.frame()` 的最快速度分支仍在每次回调中至多执行一次 `stepOnce(false)`，之后以零 delay 调度下一次 task。零 delay 使用 `MessageChannel`，正有限 delay 仍使用 timer；因此即时 yield 不再依赖嵌套 timer，同时普通有限 delay 保留原 timer 路径。调度器在 callback 开始前从 `pending` 移除任务；取消及 `dispose()` 通过删除 pending 项使已进入消息队列的旧即时回调无效，`WasmTickLoop` 原有 `timerEpoch` 继续隔离 stop/restart 旧回调。信用背压仍由 `WasmTickLoop` 的 `outstanding` 上限控制，调度器没有旁路该检查。

无效 delay、调度器关闭后的 schedule、任务 ID 溢出都会显式抛错；callback 抛错会作为 Worker 未捕获异常由 `worker-host.ts` 的 `error` listener 转成 `WASM_WORKER_SCRIPT` failure，不会被静默吞掉。当前测试覆盖即时 task 不调用 timer、取消已投递回调、dispose 后拒绝新任务及有限 delay 可取消。

## 验证

运行 `node --test apps/web/src/host/worker-task-scheduler.test.ts apps/web/src/host/wasm-tick-loop.test.ts`，两组文件均通过（scheduler 3 项、WasmTickLoop 11 项；Node 汇总显示 2 个顶层文件测试）。耗时约 0.3 秒。

性能目标 `16 核 >=1000x` 不在本次代码复核中确认；此结论不代表整个 Q22 或总体性能目标完成。
