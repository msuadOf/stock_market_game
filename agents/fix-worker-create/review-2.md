# WASM Worker session slot create 修复 · 第二轮独立复核（异步 MessagePort 测试）

- 复核对象：当前工作树 `git diff`（相对 `main 7fabfcd9`）；实现文件与第一轮完全一致，
  本轮变化仅在 `apps/web/src/host/wasm-worker-ownership.test.ts` 的 fixture 与三条新增用例
- 复核日期：2026-11-03
- 关联：`agents/fix-worker-create/review-1.md`

## 结论

通过。三条新增用例现在确实经真实 `MessagePort` 异步投递，而不是手工 `dispatchEvent`；
消息跨端口时经过 structured clone，因此覆盖了浏览器 Worker 的队列与克隆边界。
断言与上一版逐条对照没有弱化，三用例在真实异步路径下稳定通过。实现未变，第一轮结论继续有效。

## 是否真的走异步端口

是。

- fixture 的 `business` 是真实 `MessageChannel()`；`business.port1` 作为 dataPort 交给
  `bindEngineTransport`，`business.port1.start()` 在 `wasm-worker.ts` 绑定监听时被调用。
- `sendAsync` 使用 `business.port2.postMessage(message)`（`wasm-worker-ownership.test.ts:174-181`），
  由 Node 的 MessagePort 排队异步投递到 `port1` 的 `message` 监听器；`port1.postMessage` 只被
  重写用于捕获出站消息，不影响入站投递。
- 用例只通过 `await sendAsync(...)` 推进，等待条件都是本条消息才会产生的效果
  （`baseline` / `posted.length > 0` / `civilDate` / `drop:7`），不会依赖手工同步事件。
  首轮 create 前的 `posted` 已清空，所以等待条件不会命中初始化遗留消息。
- `seed: 1n` 能在跨端口后保持 bigint，说明消息确实经过 structured clone。

## 断言是否弱化

未弱化，逐条对照：

- 重复 create：仍断言第二次 create 只产生 1 条 `failure`、消息含「只能初始化一次」、
  FFI 调用次数仍为 1、没有重复 `ingress/created/baseline`；新增等待后旧会话 `civilDate`
  仍能成功，`drop` 命中 `drop:7`。覆盖强于上一版（`drop` 现在等真实处理完成）。
- 首次 FFI 失败：仍断言原始 `create failed`、`generation === 0`、失败后重试报「只能初始化一次」、
  FFI 总调用次数 1、无 `ingress/created/baseline`、`start` 得到「已过期会话」。
- 参数合同：仍逐项断言 `setup` / `initialArchive` / `requestId` 被拒（前两者 `failure`，
  带 requestId 为 `operationError`）、消息含「不是允许字段」、未进入 FFI；随后合法 create
  成功并只调用一次 FFI。
- 断言在 `await` 之后执行，读取的是端口消息处理后的状态；没有把「等待」退化成固定 sleep，
  也没有放宽匹配表达式。

## 残留问题（低严重度，非阻塞）

1. `wasm-worker-ownership.test.ts` 第二条用例末尾：
   `await f.sendAsync({ type: "drop" }, () => f.calls.length > 0)`。
   `calls` 在请求发送前已非空，该等待条件立即为真，实际未证明 `drop` 已被处理；
   因为其后没有断言且失败 create 后 handle 为空、drop 本来就是 no-op，不影响结论。
   若要保留此调用，建议改为等待一个真实可观察量，或删掉该调用。
2. 第一轮登记的两条 P3 测试缺口仍存在：wrong-mode create 后未断言显式失败回包；
   seed 类型错误 / 缺少 seed 不消耗唯一创建机会未固定。本轮未改动相关代码。
3. 等待循环固定 100 次 `setImmediate`，无显式计时；当前每个用例约 14ms，远低于 10s 用例上限，
   在 12 进程并发扫描下也稳定。属于可接受的测试实现方式。

## 验证记录

- `wasm-worker-ownership.test.ts`：38/38 通过；连续重复执行 3 次均为 38 pass / 0 fail。
- `wasm-session-slot.test.ts` + `wasm-session-initial-restore.test.ts`：14/14 通过。
- `src/host/*.test.ts` 140 个文件、12 进程并发：125 通过；15 个失败仍全部是工作树未安装依赖的
  `ERR_MODULE_NOT_FOUND`，无断言失败，与本次 diff 无关。
- 本工作树仍无 `node_modules`，未执行 `tsc`/正式 web 构建；测试使用 Node 类型剥离直接运行。
