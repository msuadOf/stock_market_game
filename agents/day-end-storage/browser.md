# 浏览器 IndexedDB 日终存档实施记录

## 范围与领域边界

- 按 ADR-0025／0029／0033，只有完整成功自然日日结候选可自动保存；交易日与休市自然日使用同一 `CivilUpdate` 日结通知，不把 PriceTick 当日终。
- 本轮不改变沪深 A 股的资金分、股份股、T+1、撮合、交易日历、费用与披露规则；依据沿用 `docs/trading-rules.md` 已登记材料，不冒称本轮重新核验交易所规则。
- 浏览器 WASM 本地局使用访问者 IndexedDB；Native／Remote 前端不写本地游戏数据库，Native SQLite 持久化由后台自己执行，前端只保留明确授权的 JSON 文件操作。
- 不新增存档版本字段、旧格式迁移、LocalStorage fallback、日内订单落盘或 WAL。IndexedDB 技术结构仅首次创建三个 ObjectStore；非当前数据库结构明确拒绝。

## 实施接口与接线

- `save/archive-store.ts`：统一异步 `ArchiveStore.list/load/select/rename/delete/copy` 及严格 `ArchiveMetadata {slot_id,name,civil_date,tick}`；元数据查询不加载 Engine。`BrowserArchiveStore` 另含日终 `save`、`newSlot` 和取消在途事务。
- `save/indexeddb-save-repository.ts`：原生 IndexedDB，`archives`／`metadata`／`selection` 三个 ObjectStore。日终内容、元数据及 current 指针在同一事务更新；配额错误、非法结构与不完整日终候选显式失败，事务失败保留原档。
- 槽位选择 epoch 与 `DayEndPersistence` generation 双重隔离；新局分配新的槽位 ID，旧异步候选不能更新新局。读取默认槽的晚响应也不能改回新局目标。删除当前槽后不自动加载另一槽，下一日终才创建新档。
- `App.tsx` 日结写协调器按 `HostCapabilities.persistence` 选择浏览器自动写；Native／Remote 无授权文件时不请求候选、不重复持久化。日内策略按钮只说明日终机制，JSON 文件授权／导入保留原用例。
- `useSessionHostLifecycle.ts` 只在 WASM 首次启动读取浏览器档，显式新局不重读；WorkerHost 只在 WASM 分支动态 import。Tauri 首次恢复标志在 StrictMode 探测性启动期间保持，实际恢复 setup 来自 Native `startupContext`，不误查询新局分配。
- `ArchiveManager.tsx`／`App.css` 提供实际可见的多槽管理面板，执行读档、重命名、复制和确认删除；失败显示操作、原因及反馈提示。列表和管理操作不重载正在运行的市场。
- Tauri／Remote `archiveStore` 按 Native owner 约定的 IPC／REST 路由适配，明确选槽传给 `host.load(slot, archiveSlotId, onRestored)`；后台成功恢复才切换写入目标。

### 明确选槽元数据

root 明确：selected archive slot 属于非经济进度元数据。明确选档应立即事务保存选择，供下次打开游戏恢复；这不是日内市场存档。读取档案、Engine 成功 restore、保存选择元数据是三个有区别的操作，不能把元数据失败冒充整局未加载或整体回滚。

- `ArchiveStore.select(slotId,isCurrent)` 新增统一接口，成功返回 `true`，过期选择返回 `false`。Tauri `archive_select` 与 Remote `POST /api/archives/select` 携带实际 generation；Native 后台另有 writer lease 守卫，数据库逻辑由其作者实现。
- Browser 选择事务写 `selection/current`，不修改 `archives` 中经济事实；加载后内存目标先固定到 chosen 槽。选择写入失败保留原数据库 pointer，**不**改回已成功加载的内存目标；下一成功日终继续写 chosen 槽。
- `useSaveCommands` 成功 restore 后先同步实际 setup、历史与委托等 UI，再执行选择。元数据失败明确提示“市场已加载；启动槽选择保存失败”，保留当前局，不调用假回滚／baseline 同步冒充失败恢复。
- Browser epoch 与 isCurrent 在事务创建、读取、写入确认各处校验。旧代选择不可覆盖当前选择或当前目标；删除已选槽只清空 pointer，不默选第一个槽或 `current`。
- Native 无 ID JSON 导入保留其当前 writer target。原有 UI 无 ID 数据库读档按钮由 Remote UI 作者改为打开**实际**多槽管理器，明确选择 ID 后再 restore／select，避免从相同经济 JSON 猜槽位或为旧快捷按钮另增冗余 selected RPC。

### 生命周期与提交确认

root 精准恢复工作改动后，本 group 的六个文件有真实冲突。仅使用 `apply_patch` 消解，不操作 index／commit。融合后的公共 `host.load` 为 `(slot, archiveSlotId?, onRestored?)`，数据库明确选槽传 ID 为第二参数，JSON 文件传 `undefined`，提交确认回调为第三参数，不建立旧二参兼容。

- 保留 UI 主线 `configureMarketTiming`：Engine 已提交 restore、即使后续恢复运行失败，也在提交 callback 中安装新 setup／时间比例／公开频率；提交前拒绝不改这些参数。callback 幂等，成功返回可再次确认而不重复安装。
- 保留授权文件选择后才执行 `beforeRead`：`loadFromFile` 先显示文件选择／授权，再等待选择完成前已提交的日终写入；取消选择不等待、不取消当前日终。快速槽／明确 DB 槽位读取前也等待已提交写入，写入失败显示原因而不加载旧档冒充成功，显式重试允许读上次有效档。
- `DayEndPersistence.beforeRead` 保留主线实现及专属测试；只等待调用前队列，不被后来日终无限延长，不把 rejected write 的错误静默转换成读档成功。
- 生命周期使用 Native／Remote 的 actualSetup 同时配置时间；Remote 仅订阅现有市场，不读取浏览器资金档、不以客户端 speed／pause preference 覆盖共享市场，也不在离开时暂停全市场。WASM 保留懒加载，仅受控 E2E 明确 threadCount=2，生产继续能力探测。
- pause preference 以当前 host 及 market generation 校验过期确认；输入的 UI 时间／业务资金仍不混用。
- 保留双方有效测试：首次写入前无档、等写成功后再读、写失败不加载旧槽、重复加载屏障、旧宿主隔离、文件取消、Native 选槽及失败 metadata 诚实提示、Remote 控制和 restore 提交确认；新增源字段只按 root 授权给**手构初始** `currentSaveFixture` 加 `retained_market_history: []` 与 `runtime_state.active_minute_history: {}`。没有修改 clock、生成过往分钟、真实 JSON 或生成类型。

### Native generation 边界

- 已核读 Native `lib.rs` 的 `archive_rename`／`archive_copy`／`archive_delete`／`archive_select` 命令及 Actor `mutate_archive_metadata` 接口：四类修改必须提交规范 generation 字符串，由 Actor writer owner／generation 校验，不能直接绕过 Actor 改数据库。
- `tauri-host.ts` 在操作调用开始捕获实际 `timeline.captureGeneration()`，四种修改 IPC 均透传 generation；等待响应后仍执行原 `assertResponseCurrent`，旧代异步错误不会变成成功或静默默认。

## TDD 与短验证

### 早期适配器证据

以下日志对应各批执行时源码，保留实际红绿历史，不冒充当前融合源码全部重验。

所有普通 Node case 使用 `--test-timeout=10000`，整条命令由 `scripts/run-with-deadline.mjs 10000` 外部监督。Node 套件以 4 个进程并发；Browser 脚本以 3 个独立页面并发，整批也受同一十秒外部上限。

1. 真实 Browser 红：`browser-red.log`，新增模块未实现时真实 Chromium 动态导入失败，子命令 exit 1。不是手工伪造的失败断言。
2. 真实 IndexedDB／React UI 代表性短验收：`browser-green.log`，三 case 并发执行，覆盖事务 CRUD、完整日终输入拒绝、schema_version 拒绝、旧 DB 结构拒绝、旧代候选／目标 epoch 隔离、事务中注入真实 QuotaExceededError 后完整档案与元数据不变、关闭重开恢复，以及真实 UI 重命名／复制／读档／删除。管理操作在明确点击读档前没有调用加载。
3. `application-short.log`：19 项 source 接线、ArchiveMetadata 和 InitialSaveSource 短测通过。
4. `lifecycle-short.log`：37 项 Lifecycle／SaveCommands／DayEndPersistence／ArchiveStore 短测通过，包含 Native／Remote 不访问 IndexedDB、恢复实际 setup、不误查 initialAllocation、Native 明确槽位恢复，以及启动失败无 host 时新局仍分配新槽。
5. `native-transports.log`：29 项 Tauri 初始化／generation 与邻近传输短测通过；模拟 IPC 的创建响应已同步当前严格对象契约，保留原成功与拒绝断言。
6. `typecheck.log`：`node node_modules/typescript/bin/tsc -b --pretty false` 在十秒外部监督内通过；这不是三宿主构建或发布验收。
7. `lint.log`：对本轮新增适配器／管理组件和接线源码执行现有 oxlint `--deny-warnings`，十秒外部监督内通过；`git diff --check` 通过。

代表性命令：

```sh
node scripts/run-with-deadline.mjs 10000 -- node agents/day-end-storage/browser-indexeddb-short.mjs
node scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/app/session-host-lifecycle.test.ts apps/web/src/app/save-commands.test.ts apps/web/src/save/day-end-persistence.test.ts apps/web/src/save/archive-store.test.ts
```

### 选择及 Native 修改证据

1. `selection-red.log`：真实 Chromium＋IndexedDB 复现选择写入失败后 memory target 被错误回滚，下一日终原目标仍 `tick=0`，预期 `15480`；子命令 exit 1。
2. `selection-command-red.log`：1 项真实 Node 命令用例失败，Engine 已 restore，但 current setup 尚未同步，随后冒充整体读档失败；子命令 exit 1。
3. `selection-green.log`：4 个真实 Browser case 以四页面并发，全部通过，最长 3355ms。新增物理 close／reopen 后仍写所选槽、选择故障旧 pointer 不变且下一日终仍 chosen、旧代 metadata 选择拒绝，以及删除所选后还有其他槽却 load 为 null。
4. `selection-application-green.log`：42 项代表性短测通过，4 进程并发，Node 测试报告 duration 为 351ms，整命令受十秒外部监督；新增 Engine restore 成功但 metadata 失败的诚实状态、旧选择响应不写新局通知、传输选择确认及严格 boolean 响应。
5. 最新 `typecheck.log`／`selection-lint.log` 与 diff check 通过；未新增依赖、未运行完整回归。

- 新增 `tauri-archive-generation.test.ts`。真实红日志 `tauri-mutation-red.log`：`archive_rename` payload 的 generation 为 `undefined` 而非 `'1'`，原 assertion 精确失败。修复后 `tauri-mutation-green.log` 25 项并发短测通过，覆盖四种修改 generation、restore 后旧 rename 响应明确拒绝、新代修改使用 `'2'` 与既有 Tauri 生命周期测试。

### 当前融合 group 证据

- `merge-lifecycle-first.log`：首轮真实失败，synthetic fixture 缺新必填 retained history，在用例入口严格拒绝。root 授权补必需空事实后，`merge-lifecycle-second.log` 为四套件四进程并发 **46/46** 通过，Node duration 353ms。
- `merge-independent-short.log`：DayEnd／pause／ArchiveStore **18/18** 通过。
- `merge-history-boundaries.log`：永久历史 schema／活动分钟拒绝／日终屏障 **15/15** 通过，保留缺字段负控与不删除事实的断言。
- `merge-connection-red.log`：真实 Chromium 新 adapter 对已关闭连接继续 transaction，InvalidStateError。首次启动 Vite 预优化曾触及整命令十秒上限；未放宽上限，优化缓存就绪后重新取得实际红。
- `merge-connection-green.log`：`connection` 与旧数据库 `structure` 两页面并发 **2/2**，最长 2500ms。其余四个旧 Browser case 未重跑成当前验收：新永久历史严格 schema 需要重新核对 synthetic 逐自然日内容，不能把之前绿色日志套到现在源码。
- `merge-lint.log` 与限定 diff check 通过。`merge-typecheck-first.log` 全项目 TypeScript 检查失败于其他恢复中 owner 文件／尚未生成 CurrentMinuteHistoryRequest/Response，无本 group 诊断；不声称全项目类型检查通过。
- 未运行 Cargo、真实 fixture 生成、复杂回归／E2E，未写 index。最终非作者合并增量复核由 root 安排。

## Fixture 来源与未覆盖边界

- Browser 与命令短测使用明确的 `currentSaveFixture()` synthetic source，经 Web 严格 schema 校验，但**不声明 Rust restore 有效**。本轮给该函数补 `report_correction_operations: {}` 与 `runtime_state.personal_trade_confirmations: {}` 两项必需空事实；后续融合又按明确授权补 `retained_market_history: []` 与 `runtime_state.active_minute_history: {}`，不改 clock 或构造过往分钟，未修改真实 JSON。
- Browser 日终协调器测试提交交易日和休市日日期不同、tick 不必变化的 synthetic 候选，验证持久化不会因 tick 相同遗漏自然日；它不等同真实 Engine 日结端到端验收。
- 旧 `CompressedLocalStorageSaveRepository` 尚保留为既有未接入生产的旧测试对象；当前 App 不引用它，也不 fallback／读取／迁移其旧数据。
- 真实当前 Engine SaveSlot 的统一重生成与实际三宿主端到端验收由 root 协调，不能以这里的 adapter／mock 短测冒称全部完成。
- 未执行 Cargo、生成绑定、Git 操作或完整回归；完整 diff 的未实施作者独立审查待 root 安排，尚不宣称最终完成。

### 单一数据库 owner 与退休映射

退役 `save/indexed-db-save-repository.ts` 及其专用 suite，原因是该旧单槽 compressed adapter 含 LocalStorage fallback，与当前 ADR-0033 多槽严格数据库 owner 冲突；App 由 UI 作者仅引用 `save/indexeddb-save-repository.ts`。不是用删除旧断言冒称新的功能全部通过。

| 原 suite 有效要求 | 当前保留／替代的证据 |
|---|---|
| LocalStorage 容量错误不能影响 IndexedDB | 新 adapter 从构造到读取均不访问 LocalStorage；原 fallback 读取和 compressed wrapping 明确退休，不建立兼容 |
| 开库等待时旧 generation 不写 | Browser `transactions`／`selection` 旧异步 generation 与选择 epoch 拒绝，既有 DayEndPersistence case 保留 |
| put 后旧 generation 失效 abort 保旧档 | 多槽 adapter 事务内再次检查 current，完整档与 metadata 不变；既有真实 Browser fault／旧代 case 保留 |
| 配额／commit 失败显错且保旧有效档 | 新 Browser 原生 ObjectStore.put QuotaExceededError，完整 payload、metadata 与 pointer 不变；写失败读档屏障用例保留 |
| 首次无档和明确再读 | InitialSaveSource 一次首读及生命周期 case，明确 DB 读档前 beforeRead 用例均保留 |
| 意外连接关闭后明确操作重新开库 | 新真实 Browser `connection` case；红 InvalidStateError 后新增 `database.onclose` 释放连接缓存，绿色明确重开 |

`save-storage-keys.ts` 暂保留，只有旧 E2E helper 尚引用。`quick-archive.ts` 与旧 E2E 的 gzip 流延迟／断言须由测试 owner 改为真实新数据库语义；本 group 不伪造 gzip 返回值、不运行复杂 E2E。

## 本轮文件与 hunk 归属

以下仅登记本轮自己的 hunk；同文件既有 Q08／Q23／成员等并行改动保留，不归本轮所有。

| 文件 | 本轮 hunk |
|---|---|
| `apps/web/src/save/archive-store.ts`、`archive-store.test.ts` | 新文件完整内容 |
| `apps/web/src/save/indexeddb-save-repository.ts` | 新文件完整内容 |
| `apps/web/src/components/ArchiveManager.tsx` | 新文件完整内容 |
| `apps/web/src/App.css` | 顶部 `.archive-manager` 三个规则 |
| `apps/web/src/App.tsx` | IndexedDB／ArchiveManager imports、singleton／getter、managedArchives state、日结介质分流、browserLocal command port、管理及策略／读档按钮、管理面板渲染 |
| `apps/web/src/app/useSaveCommands.ts` | browserLocal port、明确 slotId 读取／host.load、browser target select/newSlot/cancelPending 接线；reportFrequency 相关 hunk 不属于本轮 |
| `apps/web/src/app/useSessionHostLifecycle.ts` | 浏览器首读 host gating、动态 WorkerHost import、resumeArchive 标志／createHost 端口、Native actualSetup 与 resumed 初始分配 gating、browser transaction disposal |
| `apps/web/src/app/save-commands.test.ts` | required BrowserArchiveStore mock 方法、browserLocal fixture、本轮存档业务 case与主线beforeRead/callback测试融合 |
| `apps/web/src/app/session-host-lifecycle.test.ts` | cancelPending mock、Native／Remote 不读取浏览器及恢复上下文 case |
| `apps/web/src/app/command-host-test-fixture.ts` | `capabilities.persistence: "browser"` |
| `apps/web/src/save/session-replacement.ts` | shouldResume／explicit selected 标记 |
| `apps/web/src/save/initial-save-source.test.ts` | shouldResume／StrictMode／显式选择 case |
| `apps/web/src/save/current-save-fixture.ts` | 初始两项 facts及本次两项历史必需空事实，仅 synthetic source |
| `apps/web/src/host/engine-host.ts` | ArchiveStore／startupContext／persistence／load archiveSlotId；其他能力 hunk 不属于本轮 |
| `apps/web/src/host/worker-host.ts` | `capabilities.persistence: "browser"` |
| `apps/web/src/host/tauri-host.ts` | 当前 create_session 对象解析／resumeArchive 参数／startupContext、ArchiveStore RPC、native persistence、restore archiveSlotId |
| `apps/web/src/host/remote-host.ts` | ArchiveStore REST／remote persistence／load archive_slot_id；身份 startup 接线待相关 owner |
| `apps/web/src/host/tauri-host.test.ts`、`tauri-initialization.test.ts`、`initial-allocation-transport.test.ts`、`report-correction-transports.test.ts` | 创建会话 mock 严格返回对象及必要 DEFAULT_SETUP import，保留原断言 |
| `apps/web/src/host/company-query-coordinator.manual.ts`、`company-query-coordinator.test.ts` | fixture capabilities 增加明确 browser persistence |
| `agents/day-end-storage/browser-indexeddb-short.mjs`、`browser.md`、七份日志 | 本轮工作与真实验证证据 |

本次融合 group 另含 `usePausePreferences.ts` 的 host/generation 守卫、`wasm-worker-ownership.test.ts` 的完整 capability／civilDate fixture、`DayEndPersistence.beforeRead` 保留，以及旧 `indexed-db-save-repository.ts` 和专属旧 suite 退休；未改变 Native Rust、真实 fixture／generated 或 Git index。
