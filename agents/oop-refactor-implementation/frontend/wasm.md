# WASM owner 提取记录

## 范围与阅读

本分片实施 frontend-N04 `WasmSessionSlot`、frontend-N05 `WasmTickLoop`。已读取根 `AGENTS.md`、
`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`，以及
ADR-0002、ADR-0005、ADR-0010、ADR-0025；改动前完整读取 `wasm-worker.ts`、
`wasm-restore-transaction.ts`、`speed.ts`、`wasm-update-delivery.ts` 与相关现有测试到 EOF。

只修改 `wasm-worker.ts`，新增两个 owner 与三个精确测试文件。`wasm-restore-transaction.ts`、
`speed.ts`、`wasm-update-delivery.ts` 公共实现保持原状；未改协议、存档 schema 或交易规则。

## Owner 与 caller 迁移

- `WasmSessionSlot` 唯一拥有 `handle`、`generation`，通过只读 getter 借用由 Worker 异步初始化的
  bindings。方法为 `requireHandle`、`requireGeneration`、`readGeneration`、`create`、`restore`、
  `drop`，另有供初始化与显式刷新共用的 `prepareBaseline`。restore 继续委托既有事务 helper。
- `WasmTickLoop` 唯一拥有 `timer`、`running`、`speed`、`flushMs`、`lastStepAt`、
  `pausePreferences`、`HostSpeedMeter`。方法为 `start`、`stop`、`setSpeed`、`setFrameRate`、
  `setPausePreferences`、`frame`、`stepOnce`、`isRunning`、`readSpeedMetrics`、`publish`。
  通过 slot 执行 step；clock、timer、port post、failure 为窄依赖注入。
- Worker realm 只创建一个 slot、一个 loop。全部会话命令的 handle/generation 访问、
  create/restore/drop、baseline/failure 输出均迁移；start/stop/调速/帧率/暂停偏好/测速/单步
  全部委托 loop。`endCivilDay` 使用同一 `loop.publish`，drop 先 stop loop，再 drop slot。
- `worker-host.ts` 仍通过现有消息协议间接使用 owner，没有增加句柄或 timer 所有权。

## 保留的顺序与既有缺陷

- 重复 create 直接覆盖旧 handle，不顺带修复旧资源泄漏；create 失败不推进 generation。
- restore candidate snapshot 失败，清 candidate、保留原 handle/generation，并按既有 finally
  路径恢复运行意图；restore 构造失败没有 candidate 可清理。
- 交换 authority 后旧 handle 的 drop 失败，保留新 handle、旧 generation。
- prepare baseline 失败已经处于新 handle、新 generation；没有宣称 restore 所有失败都回滚。
- candidate 清理失败会中断 finally，原有 restart 未执行边界保持。
- Worker restore 回应回显请求旧 generation；紧随其后的 baseline 使用新 generation。
  restart 仍通过 microtask，在回应/baseline 或 operationError 发布之后发生。
- 有限倍率每 frame 至多 step 一次，按旧 lastStepAt 单步追赶；Infinity 每 task 至多一步，
  随后以 0 delay yield。保留 TICK_MS=1000、FRAME_MS=16、UI_TARGET_HZ 的原取值与公式。
- publish 先交付 protocol，再 stop/post barrierPaused；仅 TickBatch 计市场 tick，CivilUpdate
  不推进测速 tick 计数。stop/drop 清待执行 timer，暂停状态旧回调不会执行 step。
- 第一轮精确回归发现 save 分支已有局部 `slot` 名称与新 owner 遮蔽造成 TDZ；改为 `savedSlot`，
  保持 saved wire 字段名 `slot`，现有 stale-generation 断言原样通过。

## 验证证据

Node 实际版本 v25.8.2。已先阅读 `scripts/run-with-deadline.mjs` 和普通 Web runner。
两个独立 Node 精确分片并发执行；每条命令由进程外 `10000ms` deadline 监督，
各 Node 设置 `--test-timeout=10000 --test-isolation=none --test-concurrency=2`。
未执行完整回归、build、E2E 或 tsc。

新增 slot/loop 测试先于实现写入。首次 red：缺少 owner 时明确资源边界断言失败；
其中未初始化 case 的直接动态 import 也报 missing module。该 red 只能证明目标 owner API
尚不存在，不能冒充旧业务行为存在失败。实现后删除了临时文件存在性检查，最终测试均为行为断言。
最初默认 test isolation 的双文件运行只返回 file failure，随后用仓库现行 isolation=none
取得具体输出；未将第一次失败当通过。

最后两个并发分片合计 **57/57 通过**：

1. 43/43：`wasm-session-slot.test.ts`、`wasm-tick-loop.test.ts`、
   `wasm-restore-transaction.test.ts`、`wasm-update-delivery.test.ts`、
   `wasm-worker-failure.test.ts`、`speed.test.ts`、`wasm-save-protocol.test.ts`。
   Node 测试部分 179ms，整条受监督命令约 0.38s。
2. 14/14：`wasm-worker-ownership.test.ts`、`worker-host.test.ts`。
   Node 测试部分 186ms，整条受监督命令约 0.40s。
   新增 Worker 测试实际执行原协议 listener，使用 fake WASM bindings、port 与 timer；
   loader 只替换 Node 缺少的 Vite import.meta.env，验证 restore 回应、baseline、
   microtask restart、失败恢复、endCivilDay barrier、save 与 drop 的真实接线。
3. 仅六个改动代码/测试文件的 oxlint，显式 `--threads=2`，由 10000ms deadline 监督，通过。
4. `git diff --check` 通过；源码检索未残留 Worker 旧 handle/generation/timer/running 等 globals。

## 语义与未完成

本批仅调整宿主资源 owner，不调整现行 A 股账户、委托、价格、股份、费用、T+1、
市场 tick/自然日或日终存档候选语义；没有新增需要重新查交易制度的规则。

独立完整 diff 复核由父 agent 统一安排，本分片未创建下级 agent，也未进行 Git 写入/提交。
在未实施本改动的 reviewer 完成复核前，本记录仅表示实现与精确验证已交付，不宣称整批完成。
