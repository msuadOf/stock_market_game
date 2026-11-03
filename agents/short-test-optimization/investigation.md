# Web 短测超时调查（基线 7d390267）

## 环境

- 独立 worktree：`.worktree/short-test-optimization-7d390267`，detached HEAD `7d3902672b8ea6d936f9efd8e9c9fae66095f24d`
- 当前 shell Node：`v25.8.2`。仓库 `.nvmrc` 为 `24.18.0`；`scripts/run-web-tests.mjs` 的最低版本检查接受 `>=24.18.0`，因此定向测试直接使用当前 Node。
- 默认 `corepack pnpm` 在 Node 25 上启动失败：`ERR_VM_DYNAMIC_IMPORT_CALLBACK_MISSING`。本调查直接运行 `scripts/run-with-deadline.mjs` 和 Node test runner，不依赖 Corepack。
- 独立 worktree 的 `node_modules` 和 `apps/web/node_modules` 仅符号链接到主工作区已安装依赖；未修改主工作区的源码或依赖。
- 每个命令都由 `scripts/run-with-deadline.mjs 10000 -- ...` 约束；Node test runner 使用 `--test-timeout=10000`。

## 基线：单文件顺序运行

| 文件 | wall | 最慢 case |
|---|---:|---:|
| `host/worker-host-initialization-failure.test.ts` | 1390ms | 90ms |
| `host/remote-host.test.ts` | 3135ms | 1862ms（临时 Vite SSR helper） |
| `app/local-amount-render.test.ts` | 5531ms | 4162ms（首次 SSR） |
| `save/complete-save-schema-extra.test.ts` | 6827ms | 5492ms |
| `app/return-to-startup-control.test.ts` | 9062ms | 7966ms |
| `app/startup-recovery.test.ts` | 9037ms | 7939ms（首个 case 包含 before 的 App 模块装载） |

## 失败：并发/分片复现

将上述六个文件同时以各自 10 秒进程树 deadline 启动：

- `return-to-startup-control.test.ts`：exit 1，`bounded command exhausted its execution budget within the total 10000ms deadline`
- `startup-recovery.test.ts`：exit 1，同样的 10 秒进程树 deadline
- 其余四个当时通过；`local-amount-render`、`complete-save-schema-extra` 也已接近上限。

因此这些是资源竞争下真实超时，不是断言失败。`worker-host-initialization-failure` 单独并不慢。

## 已定位慢点

1. `App.tsx` Vite SSR 模块图：诊断脚本测得约 6000–9900ms，模块图 270 个（266 个本地模块）。Vite transform 日志总耗时约 1.9 秒，其中 `LocalRefreshViews.tsx` 单个 transform 约 728ms；其余主要是模块执行、编译与 GC。两个 App 级测试的首个 case 耗时几乎都来自 before hook 的这次装载。
2. `local-amount-render`：Vite 装载五个模块中，`LocalRefreshViews.tsx` 约 4.1 秒；首个 SSR case 约 4.2 秒，后续 case 多为 2–60ms。
3. `complete-save-schema-extra`：fixture 为 9,865,611 字节，`company_state` 独占约 9.1MB；单 case 对 35 个变异各执行一次完整 `parseSaveSlot`。实测一次 fixture 的 `validateSaveValue` 约 107–222ms、`parseStrictSaveEnvelope` 约 307–414ms。
4. `remote-host`：仅一个集成 case 临时创建 Vite 并装载 `market-runtime-gate.test-support.tsx`，模块装载约 0.8 秒，case 合计约 1.9 秒；其余 16 个 case 都在几毫秒内。

## 红绿与实施

### 启动入口拆分

红：先把 `return-to-startup-control.test.ts` 与 `startup-recovery.test.ts` 改为加载新窄入口，并让 `app-startup-wiring.test.ts` 读取新的 `AppRoot.tsx`；实测失败为两个新模块不存在（`ERR_LOAD_URL` / `ENOENT`），不是断言弱化。

绿：

- 新增 `apps/web/src/app/ReturnToStartupControl.tsx`，原控件和行为不变；`App.tsx` 继续 re-export。
- 新增 `apps/web/src/app/AppRoot.tsx`，承载启动选择、重选屏障、失败报告和共享 refs；实际游戏 Shell 由 `renderShell` 注入。
- `App.tsx` 仍以 `MarketRuntimeProvider` + 真实 `AppShell` 注入 `AppRoot`；`app-startup-wiring.test.ts` 明确锁定该装配。
- `startup-recovery.test.ts` 的 4 个行为断言、`return-to-startup-control.test.ts` 的原断言全部保留。测试 harness 只适配注入 Shell 后 render 返回的元素层级。

### 大存档 fixture 收缩

红：`complete-save-schema-extra.test.ts` 先改用 `boundarySave`/`mutateBoundary`，实测失败为 helper 未导出这两个符号。

绿：`complete-save-test-helper.ts` 从 9,865,611 字节的代表性存档投影出约 248KB 的边界测试 fixture，仅保留每类被断言分支的一份代表数据；35 个变异仍各自完整执行 `parseSaveSlot`，原断言逐条未改。另对 mature 和 compact fixture 都做 SHA-256 不变性校验，防止 path-copy 变异污染基准。

## 最终定向验证

- 8 个相关文件（含 `complete-save-schema.test.ts`、`startup-screen.test.ts`）：46/46 通过，整条命令 2.10s。
- `startup-screen` + wiring：11/11 通过；真实 `App.tsx` 仍能装载并保持“先选择后启动”的行为。
- `tsc -b apps/web/tsconfig.json`：通过。
- 变更文件定向 `oxlint --deny-warnings`：通过。
- `git diff --check`：通过。
- Web 真实 runner 第 1 批（40 文件、4 并发分片）：40/40 文件通过，整批 **4.93s**；目标文件实际 wall：
  - `startup-recovery` 1472ms
  - `return-to-startup-control` 1168ms
  - `local-amount-render` 2071ms
  - `remote-host` 839ms
  - `worker-host-initialization-failure` 426ms
- Web 真实 runner 第 10 批（`complete-save-schema-extra` 独立批）：1/1 通过，整批 **1.08s**，case 219ms。

未运行全仓回归（按要求）；未运行 E2E。

## 独立复核与修复

独立 reviewer 未发现生产行为或大 A 语义变化，提出：

- P2：compact fixture 只有负例，若 fixture 本身失效，35 个断言可能因基线先失败而空转。已加入 `assert.doesNotThrow(() => parseSaveSlot(boundarySave()))` 正向控制。
- P3：真实 `renderShell` 装配只用稀疏源码断言。已锁定 `<MarketRuntimeProvider ...>` 必须包住 `<AppShell {...props} />`。
- P3：记录中的变异数量应为 35（原写 33）。已更正。

修复后两个相关文件 9/9 通过（1.04s），`tsc -b`、定向 `oxlint --deny-warnings`、`git diff --check` 均通过；已请同一独立 reviewer 复审。

## 最终复核

- 独立 reviewer 首轮：无生产/大 A 语义问题；提出 compact fixture 正向控制、App 装配断言强度和计数注释三项问题。
- 三项均修复后复审：PASS；全部 finding 闭合。
- 最终 9 个相关测试文件：49/49 通过，整条 2.95s。
- 最终 `complete-save-schema-extra` Web runner 第 10 批：1/1 通过，整批 1.03s，文件 1.01s。
- 最终 `tsc -b apps/web/tsconfig.json`、定向 `oxlint --deny-warnings`、`git diff --check`：通过。
