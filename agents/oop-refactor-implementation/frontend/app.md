# App 生命周期与命令实施记录

## 范围与状态

本 worker 实施 `web-01-A06` 全部、扩展 `frontend-E01` 和 `frontend-E02`，包括可选拆分。生产实现已落盘，定向测试通过；完整批次的独立审查和统一类型检查由父任务安排，本记录不将尚未收到的审查结果写为完成。

开工读取根 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md` 和 ADR-0004/0010/0025；原 `App.tsx` 分段全文读至 EOF，再实施迁移。领域基线复用 `docs/trading-rules.md` 已登记的规则：金额用分、输入价格用元、数量用股；卖出预检扣除 T+1 锁定和未成交卖单占用；活动 setup 的 category 决定单笔数量上限。此次未改变交易制度，也未将旧规则核对记录冒充当日官方重新核验。

## web-01-A06

### SessionHostLifecycle / useSessionHostLifecycle

落点：`apps/web/src/app/useSessionHostLifecycle.ts`。

- 同一 effect 闭包对象拥有 `cancelled`、`ownedHost`、`unsetIndicatorCalculator`，并提供 `start`、`stopCurrentSession`、`releaseOwnedHost`、`dispose`。
- `AppShell` 仍唯一持有 `hostRef`、coordinator refs、update/fatal bridges、stop ref、全部跨 callback 的失效令牌。`connectProtocol` 和 `disconnectProtocol` 仍在 Shell 接线，ProtocolCoordinator 的 baseline/applied/failure callbacks 没有迁移成第二套 authority。
- caller：AppShell 调用 `useSessionHostLifecycle`；重选 UI 经原 `stopStartupRef.current` 调用同一对象的 stop；effect cleanup 调用同一对象的 dispose。
- 启动顺序仍为 WASM 环境校验 → 一次读取 source → 按档案 setup/seed 创建 host → load → 回写 setup/注册 calculator/接线 coordinators → 校验 delivery capability → 初始化原 manager ref → 设置 speed/pause preferences → start → 后台补 stop → 完成 source/ready。
- 默认 host factory 为普通 function，直接返回 createTauriHost/createRemoteHost/createWorkerHost 的原 Promise；不增加额外 async/await continuation。
- malformed protocol fixture 使用惰性 getter，求值仍在 host.start 后；共享 setup/ref/setter 都是注入借用。
- stop 仍先使 selection/replacement/player-order 令牌失效，再 stop，并在 finally 释放；dispose 仍按日终失效、selection/replacement 失效、E2E 清理、registration/coordinator/manager/host 释放、player-order 失效执行。

测试：`session-host-lifecycle.test.ts` 五个行为 case，覆盖完整初始化顺序、后台补暂停、清理顺序/旧消息丢弃、StrictMode 异步创建后取消、存档配置/大整数 seed、初始化失败显错；`app-startup-wiring.test.ts` 与 `app-persistence-wiring.test.ts` 保留原接线断言并读取新生命周期模块。

### SaveCommands / useSaveCommands

落点：`apps/web/src/app/useSaveCommands.ts`。

- 门面方法：`recoverFromFile`、`noticeSavePolicy`、`load`、`selectFile`、`loadFile`、`newGame`。每次 render 的门面借用原 refs、当前 setup 草稿、setters 和 storage/file adapters，不拥有新的存档/会话状态。
- 六个 UI caller 全部迁移，AppShell JSX 通过原 handler 名的别名调用；外层 App 仍唯一持有 InitialSaveSource、DayEndPersistence 与 sessionSetup。
- recover/newGame 等原日终 idle 屏障，再通过 setSessionSetup 请求重建；load/loadFile 更新当前 Shell 的 active setup/drafts/history，不写外层 setup 或 ready。
- 两个 load 路径与测速 hook 共用原 request gate/load-in-progress ref，保留进入、finally、baseline recovery 的失效顺序。文件选择共用原 selection generation 与 dayEndFileTargetRef，旧响应不能改目标。
- noticeSavePolicy/selectFile 不生成或写入日内存档；自然日日结 onApplied 的保存仍在 Shell 并委托原 DayEndPersistence/writeDayEndTargets。

测试：`save-commands.test.ts` 六个行为 case，覆盖保存政策/授权无日内写入、旧文件选择/取消、两种 load 的写集和共享令牌、失败同步/fatal/旧宿主响应、恢复屏障及首次 source、日期校验与新局重建。保留 `startup-recovery.test.ts`、`startup-save-source.test.ts` 的外层 App 行为验证。

### TradingCommands / useTradingCommands

落点：`apps/web/src/app/useTradingCommands.ts`。

- hook 唯一拥有原委托/条件单表单、queried player orders、canceling IDs；返回 `form` 只读视图、窄 setters、`buildIntent/submit/cancelPlayerOrder/addAuto/refreshPlayerOrders` 和稳定 `clearPlayerOrders/clearCancelingOrderIds`。
- caller：AppShell 下单/撤单/添加条件单/选股表单 caller 全部使用同一 hook；ProtocolCoordinator callbacks 和 SaveCommands 使用返回的清空方法/refresh 方法，不另建列表。
- playerAccount、activeSetup、protocol working orders 来自原 Shell selectors；Redux、AutoOrderManager、EngineHost 和共享 PlayerOrderRefreshGate 不新建第二份。
- A 股类别数量校验、T+1/已挂卖单扣减、符号限价建 intent、确认只表示提交、撤单入队失败和当前订单查询都保留原实现。

测试：`trading-commands.test.ts` 五个行为 case，覆盖同一表单的 Highest/T+1/冻结预检、创业板市价数量上限/缺失配置、稳定清空取消状态/失败释放、旧查询/protocol 列表接口、manager/Redux 条件单登记。`trade-input.test.ts` 与 `symbolic-limit-order.test.ts` 只更新对应生产模块读取位置，并保留原规则断言。

## frontend-E01

落点：`apps/web/src/app/useSpeedMetricsPolling.ts`。

- `createSpeedMetricsPolling` 每次 effect 只拥有 cancelled/timer 和 poll/start/dispose；AppShell 继续独占 metrics/error/polling generation，并提供 load 共用 gate/ref。
- caller：AppShell 原 polling effect 改为 `useSpeedMetricsPolling`；依赖仍为 ready/speed/running/pollingGeneration，立即读取及成功/失败后 1000ms 重试保持原语义。
- 测试 `speed-metrics-polling.test.ts` 三个行为 case：立即采样/唯一重试/cleanup、失败恢复、load-in-progress/gate invalidation/晚到结果丢弃。

## frontend-E02

落点：`apps/web/src/app/usePausePreferences.ts`。

- `createPausePreferencesLifecycle` 只拥有 loaded；loadOnce/synchronize 使用原配置 adapter。Redux settings 为唯一偏好值 authority，AppShell readiness 和用于 barrier 的 latest ref 保留。
- caller：AppShell 调用 `usePausePreferences`；首次 effect 和两项偏好变化 effect 的依赖/host-null/E2E skip 保持原样。初始 host.setPausePreferences 仍属于 SessionHostLifecycle。
- 惰性 StorageSource 将 sessionStorage 属性 getter 的异常留在显式错误边界；synchronize 仍先调用 host 再尝试 storage，不能因为 getter 抛错跳过宿主同步。
- 测试 `pause-preferences-runtime.test.ts` 四个行为 case：首次加载一次/window 缺失、加载失败不伪造 ready、host 未就绪/E2E/两类同步失败、storage getter 异常与 host-first 顺序。

## 验证与证据

精确环境：Node `v25.8.2`，满足仓库 `>=24.18.0`。普通命令均经 `scripts/run-with-deadline.mjs 10000`，case 均使用 `--test-timeout=10000`；显式 `--test-concurrency=2`、`--test-isolation=none`。最终两个互不冲突的短批次由 Promise.allSettled 并发执行，进程级最大并行 2。没有运行全回归/build/E2E/tsc，也没有 Git 写入/提交。

- E01/E02：先写六个行为 case 和明确未迁移入口，实际红灯均为「待迁移…生命周期」错误；实现后六 case 变绿。之后追加 sessionStorage getter case。
- 原 App/source guards：迁移后运行，11 个 case 因生产位置改变失败；按新 owner 更新读取位置、保留原语义断言并增加 Shell 注入检查，最终 17/17 通过。没有删除或弱化规则断言。
- 新增五个行为套件最终 23/23 通过：命令 wall 约 0.41s，runner duration 约 226ms。
- 原 startup/persistence/utility 精确四文件最终 17/17 通过：命令 wall 约 0.27s，runner duration 约 95ms。
- App error/startup recovery/save source 加原 startup/persistence 五文件 16/16 通过：Vite SSR 命令 wall 约 3.65s，runner duration 约 3.30s。这不是浏览器验收。
- 统一类型检查指出测试 fixture 结构不完整后，修正 readonly archive 构造、完整 EngineHost fixture、AccountSnap 的 invested_cents/recovered_cents 和未用局部量。相关四文件再次 22/22 通过（wall 约 0.41s）。
- 新 `command-host-test-fixture.ts` 提供完整具名 EngineHost，未授权方法显式 throw；日终档经过真实 parseSaveSlot。`hook-test-runtime.ts` 只模拟 React state/ref/memo/callback，不宣称覆盖 DOM 或浏览器提交周期。
- `git diff --check` 本 worker 已跟踪修改通过。

## 文件与协调

生产：App.tsx 和五个 use* 模块。测试：五个新增行为 suite、两个新增测试 helper，以及 app-startup-wiring/app-persistence-wiring/trade-input/symbolic-limit-order 四文件。`host/tauri-startup-contract.test.ts` 的 App 生命周期读取位置已告知 host worker，由其维护；本 worker 没有写该文件。

独立 reviewer 由父任务安排；收到有效发现后须修复并复核，不凭本记录宣称全批次完成。未运行的浏览器、完整市场/宿主矩阵和统一类型检查由父任务如实报告。
