# LibOS 析构：当前语义与异步对齐方案

## 当前证据

- 公共 `deferred_drop::defer` 返回 `Submission`。正常负载下 Native 返回 `Queued`，
  可在析构前返回；WASM 返回 `Inline`，调用者等待析构。返回值、时序及析构 panic
  传播范围不同，不能把「都释放了一次」当作语义相同。
- 这不是只有旧管线可触及。`packages/engine/src/session.rs` 的民用日终提交经
  `commit_tick_shadow` → `CommittableSessionState::commit_from` → `defer_swap`
  移交旧字段。WASM 会在同一关键路径析构。Native 不改成同步；
  [旧实验记录](../engine-max-performance/deferred-drop-thread-findings.md) 曾测得
  独立析构线程的正向收益（特定旧负载，不能作为本次开销验收）。
- `verify_reclamation` 两端调用同一个生产入口，记录提交返回时与完成边界后的计数。
  Native 在真正的析构队列头插入栏杆再提交，靠 FIFO 完成 marker 观察实际结束，不靠
  sleep 或「已入队」冒充完成；WASM 端通过隔离页面真实 Worker 执行内联路径。
  本探针不证明析构时序对齐、不覆盖故障/饱和，也不计性能。

## 最小可验收的异步结构（待实现）

1. `packages/libos/src/deferred_drop.rs`：复用现有有界队列／`try_send` 与回压分支，
   将 WASM 实现放在显式的 Reclaimer owner 下。owner 持唯一消费者句柄与 Sender；
   停止边界必须先关闭提交端再 drain/消费 join。消费成功、正常队列满、启动失败和
   异常退出不得合并成一个「成功」。保留 Native 现有后台快路径，不引入热路径锁、
   额外分配、周期轮询或跨 JS 调度。只有数据证明可接受后才切生产。
2. `apps/web-wasm/src/lib.rs`：在完成共享内存/slot/primary tombstone 注册的真实
   主 Worker 初始化链开启 owner。不能用一个永久 static/OnceLock 持有不会 drop 的
   Sender；WASM 槽靠句柄 join 消费，丢弃句柄不会像 Native detach 一样回收。
   现有 `shutdown_session(handle)` 只授权消费该 session，不要顺便终止同一 memory
   的全局 Reclaimer。用独立的、由 memory owner 触发的关闭入口，并保证未启动/
   启动失败不伪造回收证明。
3. `apps/web/src/host/wasm-worker.ts` 与 engine/memory owner：停步进与 ingress，
   完成 session/World 回收及 Reclaimer drain/join 后，才报告 engine quiescence。
   同 memory 的 restore 不关闭 Reclaimer；初始化失败、正常退休、故障超时分别由
   owner 处理。异步 Worker 须计入现有 slot/内存预算、真实退出与 manifest 收敛；
   死亡不能算完成。若已有全局 death 或 hard trap，应沿宿主故障边界报告未证明回收。
4. `packages/engine/src/session/deferred_drop.rs` 和两端测试：复核生产
   `verify_metrics`、计数与退化诊断，不再把 WASM 的 `Inline` 归为 Native
   queue-full。验证同一份非 panic 析构契约（后台成功、FIFO、饱和时无等待回压、
   完成边界仅输出实际释放），以及 panic/硬故障的上层失败类和停止后拒绝提交。
   重跑实局日终及跨存档恢复，不变更任何交易或结算事实。分别用同平台同底座和等价
   负载对照提交 p50/p95、实际释放结束、分配次数、CPU、队列高水位与整局吞吐。
   未做这一步，不能写「仅底座差异」。

### 测试先行：当前红牌与接口建议

- 严格浏览器契约 `run.mjs --deferred-contract` 在现有验收包上返回非零，明确看到
  两次 `Inline`、返回后已完成 `[1, 1]`，不是缺 ABI 或启动错误。当前正向 Native
  测试同时覆盖 FIFO、有界队列满、断开处理和恰好一次。独立 JS 证据检查拒绝伪装
  `Queued`、漏/重复释放、错误 Slot 世代、无退出及未清空资源。
- 建议的 WASM 接口：主 Worker 在共享 facade 和主管确认就绪后调用
  `start_wasm_reclaimer()`；已撤销 Engine/ingress 工作后由同一个 owner 调
  `close_wasm_reclaimer()`。正常完成返回 `true`；从未启动返回 `false`，
  不是回收证明；失败/重复调用应报错。生产不允许不着痕迹地关闭、退回同步或启动第二个
  消费者。直接使用正式 WASM bindings 的其他调用者需要显式遵守同一前置条件。
- 此轮只有测试，不更改上述运行时行为。后续必须让 browser probe 通过真实消费者栏杆
  和 owner close 产生 `Queued/[0,0]/[1,1]`；再加满队列、消费者异常及关闭后拒绝
  测试，原有 Native 生产路径则继续验证不回退。
