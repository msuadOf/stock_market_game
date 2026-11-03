# WASM Worker session slot create 修复 · 第一轮独立复核

- 复核对象：`git diff`（detached 于 `main 7fabfcd9`），改动文件 4 个，共 +107/−13
- 复核角色：未参与本次实现的独立 subagent
- 复核日期：2026-11-03

## 结论

未发现阻塞缺陷。两个目标路径（重复 create 覆盖旧句柄、create FFI 失败后重试）都被真实修复，
create 参数合同收紧为 `type` + `seed`，错误显式回传，`restoreInitial` / `restore_prepared`
的初始档语义未被破坏。改动范围与需求一致，没有无关扩大。

只发现两条低严重度的测试固化缺口（不改变实现正确性），以及一条既有、非本次引入的防御性观察。
建议补齐测试后再合入，或由实施者明确接受当前覆盖。

## 逐项回答

### 1. 是否覆盖两个泄漏/非法重试路径，是否最小

覆盖。

- `apps/web/src/host/wasm-session-slot.ts:50` 新增 `createAttempted`；
  `:84` 在调用真实 `wasm.create_session` 之前锁存；
  `:81-83` 对重复 create 与失败后重试统一抛错，FFI 不会再进入第二次。
- 唯一生产调用点是 `apps/web/src/host/wasm-worker.ts:384`（全仓库 `rg` 确认），因此不存在绕过槽位锁存的第二个 create 入口。
- 首次 FFI 失败时 `handle` 仍为 `null`、`generation` 仍为 0，槽位保持「不可用但状态明确」；后续请求通过 `requireHandle` /
  `requireGeneration` 显式报错，不会出现「失败的第一次 + 成功的第二次」这种非法双会话状态。
- 重复 create 被拒绝后旧 `handle` / `generation` 保持不变，旧会话仍可用（测试 `wasm-worker-ownership.test.ts:576-592`
  用 `civilDate` 验证，并用 `drop` 验证丢的是旧句柄 7），符合「保留旧会话、拒绝覆盖」的意图。
- 改动只涉及槽位锁存、create 分支字段校验、注释和对应测试，没有顺手改动 restore/shutdown 等相邻语义，范围最小。

### 2. create 参数合同、错误传播、旧会话保留语义是否自洽

自洽。

- `wasm-worker.ts:383` 用既有 `exact({ ...message }, ["type", "seed"], "Worker create")` 收紧合同：
  `setup`、`initialArchive`、`requestId` 等额外字段都在 `slot.create` 之前被拒绝，不再静默忽略；
  缺少 `seed` 同样会被拒绝（`exact` 的必填分支）。主线程实际只发送 `{ type, seed }`
  （`apps/web/src/host/worker-host.ts:412`），合同与调用方一致。
- 合同/模式/参数校验都在锁存之前：`wasm-worker.ts:380-383` 的 `requireConfigured`、wrong-mode、`exact`，
  以及 `wasm-session-slot.ts:76-78` 的 bindings / seed 类型校验，均先于 `:84` 的锁存。
  因此被拒绝的非法请求不会消耗唯一创建机会。
- 错误传播走既有 `catch` 通道：无 `requestId` 时发 `failure`，带数字 `requestId` 时发 `operationError`；
  新增测试分别断言了两种形态（`wasm-worker-ownership.test.ts:619-637`），首次 FFI 失败保留原始
  `create failed` 文案（`:594-617`），没有吞错。

### 3. restoreInitial / restore_prepared 兼容是否被破坏

未被破坏。

- `create` 的 wrong-mode 拒绝位于 `wasm-worker.ts:381`，早于 `slot.create`，因此不会置位 `createAttempted`；
  随后 `restoreInitial` 仍能执行并消费 `restore_prepared`。
  `wasm-worker-ownership.test.ts:640-652` 在真实 Worker 路径上验证了「wrong-mode create 不调用 FFI →
  restoreInitial 成功 → 第二次 restoreInitial 不再消费」。
- `wasm-session-slot.ts:90` 只在原有守卫上增加 `createAttempted`；未尝试 create 时行为不变，
  `restore_prepared` 仍只在 `initialRestoreAttempted` 首次置位时消费一次。
  `wasm-session-initial-restore.test.ts` 三个用例均通过。

### 4. 测试是否真正覆盖真实 Worker 路径与 slot 路径，是否有遗漏或弱断言

两条路径都覆盖到了，且是有效覆盖：

- slot 路径：`wasm-session-slot.test.ts:25-63` 覆盖重复 create、首次 FFI 失败后不可复用、
  `restoreInitial` 安装的会话不可被 create 覆盖，并断言了 `generation` / `handle` / FFI 调用次数。
- 真实 Worker 路径：`wasm-worker-ownership.test.ts:576-637` 通过 `registerHooks` 加载真实
  `wasm-worker.ts`（只替换 `import.meta.env` 与 WASM bindings），断言了 FFI 调用次数、`failure` /
  `operationError` 形态、旧 generation 请求失效、旧会话仍可用、drop 命中正确句柄。
- `restoreInitial` 兼容用例覆盖了 wrong-mode create 与 `restore_prepared` 单次消费。

弱断言/缺口（低严重度）：

1. `wasm-worker-ownership.test.ts:640-645`：wrong-mode create 之后只断言「没有调用 create FFI」，
   随后直接 `posted.length = 0`，没有断言该 create 确实回发了 `failure` 或 `operationError`。
   若 create 分支改成静默 `return`，这个用例仍会通过，无法固定「失败必须显式传播」。
2. `wasm-session-slot.test.ts:64-70` 与 `wasm-worker-ownership.test.ts:619-637` 只证明了「额外字段」
   这类校验失败不消耗唯一机会；没有证明「seed 类型错误」「缺少 seed」也不会消耗。
   实现顺序确实正确（`wasm-session-slot.ts:76-78` 先于 `:84`），但合同未被测试钉住。

均属于补断言即可，不需要改生产代码。

### 5. 是否影响大 A 语义

无关。本次改动仅涉及 WASM Worker 会话槽生命周期与消息字段校验，未触及交易制度、撮合、费用、
涨跌停、T+1、板块差异、单位或存档中的交易语义，也没有修改任何规则文档或契约字段含义。

### 6. TypeScript / 生命周期 / 并发风险

- `create` 分支全程同步：`handleBusinessMessage` 的 async IIFE 在 `case "create"` 中没有 `await`，
  `postMessage` 事件按顺序派发，因此两个 create 不会在锁存处交错，锁存顺序可靠。
- `createAttempted` 是 `WasmSessionSlot` 私有字段；每个 Worker 模块实例各自持有一个 `slot`
  （`wasm-worker.ts:112`），不存在跨 Worker 共享状态。
- 锁存不可逆与真实宿主生命周期一致：`createWorkerHost` 每次都会新建 Worker，
  「失败后必须重建 Worker」的提示与调用方行为一致，不会留下无法恢复的合法路径。
- 本工作树没有安装 `node_modules`，无法运行 `tsc`；已用 Node 直接执行测试（类型剥离）。
  新增字段与布尔表达式均为普通 TS 语法，未引入类型系统风险。

## 发现清单

| 严重度 | 位置 | 说明 |
|---|---|---|
| P3（测试缺口） | `apps/web/src/host/wasm-worker-ownership.test.ts:640-645` | wrong-mode create 后未断言实际回发 `failure`/`operationError`，无法防止将来把显式拒绝退化为静默忽略。 |
| P3（测试缺口） | `apps/web/src/host/wasm-session-slot.test.ts:64-70`；`apps/web/src/host/wasm-worker-ownership.test.ts:619-637` | 未固定「seed 类型错误 / 缺少 seed 不消耗唯一创建机会」，实现当前正确但缺少回归保护。 |
| P3（既有观察，非本次引入） | `apps/web/src/host/wasm-session-slot.ts:85-86` | `create_session` 返回值未像 `restore_prepared`（`:96`）那样校验 u32 句柄；若 FFI 返回非法句柄，锁存已消耗且槽位会把它当作就绪。属既有缺口，本次可不在范围内处理，但修复后重试已不可能，建议后续一并显式校验。 |

## 验证记录

在 `apps/web` 下用与仓库测试脚本相同的 Node flags 执行：

- `wasm-session-slot.test.ts`：11/11 通过
- `wasm-worker-ownership.test.ts`：38/38 通过
- `wasm-session-initial-restore.test.ts`：3/3 通过
- 其余 create/restore 相关模板：`wasm-worker-facade-init` 1/1、`wasm-worker-ingress-bridge` 4/4、
  `worker-host-control-worker` 2/2、`worker-host-w1-bootstrap` 11/11、
  `worker-host-initial-x-consumer` 12/12、`worker-host-initialization-failure` 2/2 全部通过。
- 额外把 `src/host/*.test.ts` 共 140 个文件以 12 进程并发跑了一遍：125 个通过；
  15 个失败均为本工作树未安装依赖导致的 `ERR_MODULE_NOT_FOUND`（例如 `@reduxjs/toolkit`），
  与本次 diff 无关，未出现断言失败。

## 建议

1. 在 `wasm-worker-ownership.test.ts:640-645` 补一条断言：wrong-mode create 后的 `posted[0]`
   应为 `failure`（或带 requestId 时的 `operationError`），且消息包含拒绝原因。
2. 补「seed 类型错误 / 缺少 seed 后仍可成功 create」用例，固定「参数校验不消耗唯一机会」。
3. 合入前在完整依赖环境跑一次 `npm run build`（含 `tsc -b`）或仓库正式的 web 测试批次；
   当前工作树缺依赖，只能以 Node 直跑单文件作为验证证据。
