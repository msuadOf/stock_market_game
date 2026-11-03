# WASM Worker session slot create 修复 · 第三轮独立复核（P3 修补确认，最终结论）

- 复核对象：当前工作树 `git diff`（相对 `main 7fabfcd9`）
- 复核日期：2026-11-03
- 关联：`review-1.md`、`review-2.md`

## 结论

**通过，无有效阻塞或新增发现。** 第二轮提出的三项 P3 已全部落实，实现代码自第一轮起未再变化；
两轮登记的正确性结论继续成立，可以按当前 diff 合入。

## 三项 P3 修补确认

### 1) 删除 FFI 失败用例末尾无效的 drop 等待

已删除。`wasm-worker-ownership.test.ts:604-621` 现在在断言 `start` 返回「已过期会话」后结束，
不再有等待条件恒为真的 `drop` 调用，也没有断言因此丢失。

### 2) 初始档测试断言 wrong-mode create 显式失败，restoreInitial 仍成功

已补齐。`wasm-worker-ownership.test.ts:665-680`：

- wrong-mode `create` 后断言 `posted.length === 1`、`type === "failure"`、消息匹配 `/只能 restoreInitial/`；
- 随后 `restoreInitial` 成功，`restore:7` 恰好一次，并收到 `created` / `baseline` generation 1；
- 第二次 `restoreInitial` 后 `restore:7` 仍为一次，`restore_prepared` 单次消费语义保持。
- 该用例使用同步 `send`，但被测路径（wrong-mode 拒绝与失败上报）本身同步且无 `await`，
  生产环境经 MessagePort 进入同一分支，行为等价，不影响结论。

### 3) seed 参数错误不消耗唯一创建机会

已补齐两条路径：

- Worker 真实端口：`wasm-worker-ownership.test.ts:649-663` 经 `sendAsync` 依次发送
  `{ type: "create" }`（缺 seed）与 `{ type: "create", seed: 1 }`（非 bigint），
  均断言 `failure`、消息匹配 `/seed|必填/`、未进入 `create` FFI；随后合法 `create` 成功且只调用一次 FFI，
  最后 `drop` 命中 `drop:7`。
- slot 路径：`wasm-session-slot.test.ts:71-75` 在 seed 类型错误后执行合法 `create`，
  断言调用序列仅 `["create:1"]`、generation 为 1，固定「参数校验先于锁存」。

## 其余复核项

- 两轮已确认的核心行为不变：`createAttempted` 在首次 FFI 前锁存（`wasm-session-slot.ts:81-85`）；
  `exact({ ...message }, ["type", "seed"], "Worker create")` 在 `slot.create` 之前
  （`wasm-worker.ts:383`）；wrong-mode 校验早于锁存（`:381`）；唯一生产 create 入口仍是 `:384`。
- 大 A 语义无关：改动不触及交易制度、撮合、费用、单位或存档交易语义。
- 既有、非本次引入的残余观察仍只有一项：`wasm-session-slot.ts:85-86` 未校验
  `create_session` 返回值是否为合法 u32（`restore_prepared` 在 `:96` 有校验）。
  锁存后该返回值不可能重试，若后续要加固可单独处理；不构成本次需求缺陷。
- 未发现新增 TypeScript/生命周期/并发风险：create 分支无 `await`，端口消息按序处理，
  每个 Worker 模块实例持有独立 `slot`。

## 验证记录

在 `apps/web` 下用仓库相同的 Node flags 执行：

- `wasm-worker-ownership.test.ts` + `wasm-session-slot.test.ts` + `wasm-session-initial-restore.test.ts`：
  **53/53 通过**，连续重复 3 次均 53 pass / 0 fail。
- `src/host/*.test.ts` 共 140 个文件、12 进程并发：125 通过；15 个失败全部为工作树未安装依赖的
  `ERR_MODULE_NOT_FOUND`（如 `@reduxjs/toolkit`、Tauri 包），无断言失败，与本次 diff 无关。
- 本工作树无 `node_modules`，未能执行 `tsc` 或正式 web 构建；合入 CI 时应由完整依赖环境覆盖。
