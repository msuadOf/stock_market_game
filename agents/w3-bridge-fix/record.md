# W3 遗留断裂修复批：postIngressBridge 对非共享 Memory 的 DataCloneError

- 分支：`fix/w3-ingress-bridge-serial-crash`（最初基于 main @ a6911c52 提交
  29e192d4/4f1f6a9b；后按基线通报 rebase 到 main @ fd5f210a，重写为
  c59d001e/f0ff51dc，无冲突——B4a 批对修复面 6 文件零改动，`git diff
  a6911c52 fd5f210a -- <修复面>` 为空，main(fd5f210a) 的 postIngressBridge
  仍是第 133 行 `...ingressBridge` 直传形态，断裂原样存在，红证等效成立）
- 任务：修 `apps/web/src/host/wasm-worker.ts` 的 `postIngressBridge` 把非共享
  `exports.memory` 直接 postMessage 回主线程导致的
  `DataCloneError "#<Memory> could not be cloned"`（main 本地 wasm 起局即崩，
  P0 批实证档案：`.worktree/wasm-p0/agents/wasm-p0-web-recovery/record.md`
  §"main 既有断裂"）。

## 修法（一句话）

ingress 桥按 substrate 显式分派：initialize 时探测 wasm memory 是否为
SharedArrayBuffer 背衬且环境暴露 SAB 全局（=可跨 worker 共享），共享形态保持
原样（postMessage module+memory 给主线程，ingress worker initSync 后
`ingress_enqueue` 直写）；串行形态只上报 `{type:"ingress", shared:false}`
形态标记，主线程不 spawn ingress worker，`submitIntent` 降级走 engine
worker 主通道 `enqueue`。

## 两形态兼容论证

- **串行（非共享 memory，现行 main 官方构建——`scripts/frontend-build-plan.mjs`
  稳定工具链串行 wasm）**：非共享 `WebAssembly.Memory` 不可结构化克隆，
  ingress worker 依赖的"跨 worker 共享内存直写 engine 队列"前提不成立。
  降级走主通道零语义损失：engine 侧 `enqueue(handle, intent)` 与
  `ingress_enqueue(token, intent)` 殊途同归于
  `enqueue_player_intent(AccountId(0), intent)`
  （`apps/web-wasm/src/lib.rs` 649/663 行），单机纯前端、player 固定
  AccountId(0)。
- **共享（真线程，未来形态）**：memory 为 SharedArrayBuffer 背衬且环境暴露
  SAB 全局（跨源隔离）时才走原共享路径——shared memory 与
  WebAssembly.Module 均可结构化克隆（Node 实测 + 测试 fixture 断言），
  ingress worker initSync 同一 module+memory 后直写共享队列，路径原样保留。
- 判据两条件缺一不可：非隔离环境即使 wasm 是 shared 构建也无法跨线程共享
  SAB（postMessage 被浏览器拒绝），且非隔离上下文不暴露 `SharedArrayBuffer`
  全局——探测必须先 `typeof` 守卫再 `instanceof`，否则探测本身抛
  ReferenceError（浏览器非隔离部署实测，见下"三层断裂"第 2 层）。

## 实际发现的三层断裂（逐层 TDD）

1. **DataCloneError（任务主对象）**：`postIngressBridge` 回传非共享 memory。
   红证：`wasm-worker-ingress-bridge.test.ts` 用例 1（fixture 非共享 memory +
   postMessage 内置 `structuredClone` 复现浏览器 structured clone 语义 →
   actual `{type:"failure", message:"#<Memory> could not be cloned."}`）。
2. **探测自身 ReferenceError（修复首版引入，浏览器端到端暴露）**：首版探测
   `exports.memory.buffer instanceof SharedArrayBuffer` 在非跨源隔离 worker
   里抛 `ReferenceError: SharedArrayBuffer is not defined`（SAB 全局不暴露）。
   红证：用例 4（delete `globalThis.SharedArrayBuffer` 后 create 必须正常降级）。
   修复：`typeof SharedArrayBuffer === "function" && ... instanceof`。
3. **smoke-pages 裁剪与 initialAllocation expectedCodes 错配（既有基建缺陷，
   被 1 修复后首次暴露）**：smoke-pages 劫持 `Worker.prototype.postMessage`
   把 create.setup.stocks 裁成 2 只（7a94c401 引入），但
   `worker-host.initialAllocation` 的 expectedCodes 取自主线程
   `createWorkerHost` 闭包的完整 setup（5 只）——两者必然错配
   （"初始分配响应遗漏本局证券"）。该裁剪自引入后从未被端到端执行过
   （一直被第 1 层挡住）。处理：去掉 stocks 裁剪（npcs/history/ticks 裁剪
   保留，提速大头），`scripts/smoke-pages.mjs` 内注释说明。
   Node 直调 wasm 佐证（200 个 seed）：裁剪小局 `initial_allocation` 恒返回
   2 只，engine 侧无缺股——错配确在主线程 expectedCodes 侧。
   归因勘正（复核发现）：stocks 裁剪实由 6ac55d83（10-02）引入、7a94c401
   （p0 支线，非 main 祖先）只是保留；实质论断（从未端到端执行、今日错配
   确定性成立）不变。

## 独立复核结论（未实施者 subagent，rebase 前版本 a6911c52..4f1f6a9b）

7 项全 PASS，总结论**可合入**。要点：
- 两形态兼容：判据三组合均正确；降级语义等价实证（lib.rs 两路同入
  `enqueue_player_intent(AccountId(0))`；串行构建本无真线程，主通道是唯一
  可行路径，无 A 股语义漂移）。
- 红绿证独立实证：复核者用 a6911c52 旧源码树跑新测试 4/4 红（首例 actual
  为 WASM_WORKER_PROTOCOL failure），修复后 4/4 绿。
- 两条低严重度备注，处置：
  1. 归因措辞（7a94c401→6ac55d83）——已在 smoke-pages.mjs 注释与本档案勘正；
  2. `worker-host.ts` host() 的 `capabilities.sharedMemory: true` 在串行下
    失真——全仓无消费方、属既有代码非本批引入，不扩范围处理，登记为
    后续批顺手项（真线程批收口能力面时一并定真值）。

## 测试

- 新增 `apps/web/src/host/wasm-worker-ingress-bridge.test.ts`（4 例）：
  1. 串行 create 不回传 Memory、无 failure（红→绿）；
  2. 串行 + bindings 缺 ingress_token 仍起局（降级不依赖 ingress 面，红→绿）；
  3. 共享形态仍回传 module+memory+token 且经 structuredClone 可克隆
     （契约锁定，修复前缺 shared 字段红）；
  4. SAB 全局缺失（非隔离）探测不抛 ReferenceError（红→绿）。
  fixture 自持虚拟 wasm-pkg 模块（module hooks 短路），不依赖磁盘 wasm-pkg
  ——裸 worktree 可跑（对比 ownership 测试需真实 wasm-pkg 存在）。
- 更新 `worker-host.test.ts`：现有共享 ingress 测试显式 `shared: true`；
  新增串行降级用例（不 spawn intake worker、submitIntent 走 engine 主通道、
  wire Intent 数值 StockId 词表不变、dispose 只关 engine）。
- ownership 测试（共享 fixture）不破：ingress 消息新增 shared:true 字段，
  既有逐字段断言兼容（wasm-pkg 就位后 2/2 绿）。

## 验证口径（如实）

- 单测：`run-web-tests.mjs` 全批 **977 例全绿**（含新增 5 例；裸 worktree
  需预置 `apps/web/wasm-pkg` 与 `apps/desktop/src-tauri/gen/schemas`，均已
  就位——P0 环境备注已登记的前置）。
- smoke-pages（真浏览器端到端）：**全绿**
  （"Pages smoke passed including cleanup…"，.app-root 出现、无 app-error、
  wasm worker 在场、三断言过）。配套串行 wasm-pkg 为本批用官方命令重建
  （`node scripts/frontend-build.mjs` 的 wasm-pack 段：稳定工具链串行构建；
  tsc 段因 main 既有 company/report 域 31-32 错红，本批不修、任务书明示
  另有批次，vite build 直跑）。
- tsc -b：main 既有红，本批文件零新增错误（未跑完整 tsc，改动文件经
  oxlint --deny-warnings 零告警 + 测试批全绿覆盖）。
- scripts 批：distribution-workflow.test.mjs 全绿（smoke-pages 改动无涉）。

## 环境事件（如实登记）

- 主仓库 `apps/web/wasm-pkg` 当时为 **wasm-bindgen-rayon threads 构建**
  （含 startWorkers glue、shared:true fallback）——与 main 串行代码错配的
  遗留产物（真线程批构建后未还原）；非隔离页面下其 glue 直接抛
  `SharedArrayBuffer is not defined`。本批已按官方构建计划（稳定工具链
  串行）在 worktree 重建。**主仓库该错配产物仍在，建议后续批重建**。
- 会话 cwd 重置导致两次 pnpm install 误装进 `.worktree/reint-b4a`
  （node_modules 为 gitignored 产物，未进 git；如该 worktree 的 agent
  发现依赖变化即此因）。
- engine/server/desktop 零改动；`git diff` 仅 5 文件（wasm-worker.ts、
  worker-host.ts、worker-host.test.ts、smoke-pages.mjs、新增测试与档案）。

## 契约变化（worker 内部协议，两侧同批收口）

- ingress 消息：`{type, generation, token, module, memory, diagnostics}` →
  共享 `{type, generation, shared:true, token, module, memory, diagnostics}` /
  串行 `{type, generation, shared:false}`。
- `submitIntent` 串行路径：engine worker 既有 `enqueue` case
  （`slot.requireGeneration` 代次校验在 worker 侧，主线程响应后再校验代次，
  与共享路径同强度）。
