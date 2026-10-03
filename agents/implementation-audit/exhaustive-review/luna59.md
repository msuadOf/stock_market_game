# Luna59：App、图表与宿主实现记录独立复核

## 范围与方法

- 按任务基线核对 `08e4fc7`（合并提交等同），连续读完 `agents/oop-refactor-implementation/frontend/app.md` 82 行、`chart.md` 109 行、`host.md` 41 行，共232行至 EOF；范围包含文件清单、测试限定、未覆盖项和复核状态。
- 对照实现记录逐个追踪真实生产 caller、被迁移 owner、异常和异步 cleanup；以 `UX-CONTRACT.md`、ADR-0010、ADR-0025及既有交易规则为产品语义依据。工作记录仅证明作者宣称的迁移范围，不覆盖正式契约。
- 本文为独立静态复核；没有运行测试、构建或浏览器验收，也没有修改产品文件或执行 Git 写操作。

## 原文条款与调用链矩阵

| 原文章节及全文覆盖 | 对照原文代码/调用链 | 结论与旧发现复核 |
|---|---|---|
| `app.md:1-9` 范围、分层、A股单位与既有规则 | `useTradingCommands.ts:60-67` 解析股数、读取股票类别、扣除 `t1_locked` 与 `reserved_sell_qty` 后校验；提交仍走 `EngineHost`。金额/股数概念未因 UI owner 提取改变。 | 迁移范围符合记录。旧 sweep60 关于 T+1 预检保持成立；这里不重新主张已经核验交易所现行规则。 |
| `app.md:11-23` SessionHostLifecycle 全部生命周期 | `App.tsx` 通过 `useSessionHostLifecycle` 接入；effect 中构造 `createSessionHostLifecycle`，将 factory 返回的 Promise 直接交给 `start`（`useSessionHostLifecycle.ts:218-230`）。`start` 先读初始档，再创建 host；拿到异步结果后先登记 `ownedHost`，若已取消则 dispose（`:87-103`）。启动后的取消守卫、宿主注入、coordinator 接线、偏好同步、`start`、隐藏页补停和 ready 顺序见 `:105-180`。重选调用 `stopCurrentSession`，先 invalidate 三类 gate，再在 `finally` 释放 owner（`:79-85`）；effect cleanup `dispose` 失效日终存档和替换 gate，再释放资源（`:193-201`）。 | caller 和资源 owner 实际存在。cancel发生在 `createHost` 等待期间时，cleanup 尚无 host 可释放；Promise 返回后 `cancelled` 检查释放该 host。load等待期间 cleanup 可直接释放已登记 host；load 返回后被取消的分支退出。未发现取消路径漏掉新建 host。普通新局仍用 `DEFAULT_SEED` 是原 G20 范围，非此次提取遗漏。 |
| `app.md:25-35` SaveCommands 六入口与存档屏障 | `App.tsx` 构造 `useSaveCommands` 并将六个门面操作接回原 UI handler。`useSaveCommands.ts` 的 recover/load/file/new-game 路径共用 `sessionReplacementGateRef`；日终写入先 invalidate selection 并等 `dayEndPersistence.idle()`，load 的 `finally` 清 `speedMetricsLoadInProgress` 并 invalidate metrics request gate；文件选择用 generation 和 `dayEndFileTargetRef` 隔离旧响应。 | 与 ADR-0025 日终持久化相符；授权文件选择与日内政策提示没有写入当前状态。旧 sweep60 的共用门禁和独立写集结论成立。失败分支有显式 notice/fatal 路径，未发现被 facade 静默吞掉。 |
| `app.md:37-46` TradingCommands 表单、列表、撤单与条件单 | `App.tsx` 创建单一 hook；hook 由 `useState` 管理一份表单和取消中 ID，优先消费 protocol orders，否则使用 query 结果（`useTradingCommands.ts:23-34`）；列表刷新用共享 refresh gate 并验证 host 身份（`:36-51`）。submit/cancel 都 await `host.submitIntent`，成功文案表达“已提交”而不是成交（`:74-103`）。 | 迁移已接入真实 caller。旧结论确认：确认语义没有漂移，预检保留 A股单位/类别和T+1。失败 notice 仍为全局 notice，旧 G22 的交互关联缺口没有因对象提取被核销。 |
| `app.md:48-63` Metrics polling 与 PausePreferences | App 的 polling effect 委托 `useSpeedMetricsPolling`；它借用共享 load gate，只拥有 timer/cancel。PausePreferences 只拥有 loaded 标志，Redux 仍为偏好 authority；host 同步发生在 storage 写入之前，getter 异常留在显式错误边界。 | 真实 owner 连接及原错误顺序与既有复核一致；远端测速凭据等 G02 不属于本次提取。 |
| `app.md:65-82` 测试、helper、类型修复、验证声明与文件清单 | 实现记录明确 `hook-test-runtime` 只模拟 React 私有 dispatcher；新 EngineHost fixture 对未授权方法显式失败。历史测试结果限定为对应短测，没有声称浏览器、E2E、全回归或统一 tsc 已过。 | 旧复核没有把 SSR/mock 提升成浏览器证据，结论成立。记录列出的生产、测试和 helper 迁移文件与当前目录 owner 路径一致。 |
| `chart.md:1-11` 图表语义保留声明 | `market-chart-projection.ts:8-16` 将 tick 映射为分钟与元、累计量差，并把 `buy` 固定为 `true`；竞价点保留 null 指示价并过滤收盘竞价（`:19-25`）。UX-CONTRACT.md:47、50另规定阶段独立自适应量轴、竞价槽/PreOpen呈现及无交叉委托空态；此投影文件本身不实现全部坐标与量轴 UI。 | 原有格式/单位保持不等于所有正式 UX 已满足。旧 G10/G11/G12 残余结论仍应由总账承接：累计量差/基线、日界隔离、买卖方向呈现仍需按各自既有证据判断；不把旧算法问题误判为这次 OOP 新回归。 |
| `chart.md:13-33` MarketChartProjection 与 React/Provider caller | `useMarketChartRuntime` 在挂载时惰性建 projection；`MarketRuntimeProvider` 提供 getter；`LocalRefreshViews` 的行情面板/移动详情以及真实 ProtocolCoordinator 更新进入 hook。Projection 的 `upsertFrames/rebuildHistory` 管分时与竞价，snapshot/baseline/reset 管日K；对外 history 仅复制 code 索引（`:56-105`）。 | 真实 caller 存在。浅拷贝只读索引不构成深不可变承诺；selected 数组副本/刷新隔离问题继续按 G31 跟踪。旧 sweep60 所述未新增 Redux/protocol authority 成立。 |
| `chart.md:35-53` PriceChartRuntime 与生命周期、FR01 | `PriceChart` 每个 effect 创建独立 runtime 并 cleanup 同一实例；`PriceChartRuntime.create` 主容器缺失时不建图，构造创建主图、series、副图、observer 和 window listener（`:38-117`）；dispose 顺序可由 owner 调用。指标切换逐个移除并在成功后清句柄（`:165-175`及 `removeIndicatorSeries`），匹配 FR01 修复意图。 | FR01“部分 remove 失败后错误地重复删除已成功句柄”已被后续 review 结论与现行逐项清理逻辑核销。构造中途抛错时对象未返回，effect cleanup 无从执行；这是源码可证的资源边界，但 `chart.md` 明确排除半初始化 rollback，frontend/review 的相关复核也确认不在该实现批次。保留为另批行为边界，不新增本批 G。 |
| `chart.md:55-69` MarketGridRowSynchronizer | `MarketGrid` 创建唯一 synchronizer，在 `onGridPreDestroyed` 清 API 借用；render `recordLatest`、effect/ready `updateLatest/attach`。`attach` 先以新 grid 初始行作 submitted 基线，再 diff 最新行；`submitLatest` 仅在调用不抛后前移提交目标（`market-grid-row-synchronizer.ts:17-46`）。 | 生产 caller、ready/render race 和 destroy 接线存在。`submittedRows` 语义是已提交目标，不是异步事务完成回执；AG Grid 持有在途事务，dispose 不承诺撤销。旧 sweep60 对此反证有效，不新增错误“完成状态”发现。 |
| `chart.md:71-109` 测试、验证、完整清单 | 原记录区分定向测试、lint、统一类型检查、浏览器及全回归；只报告其实际做过的范围。FR01 修复后测试覆盖第二次 `removeSeries` 抛错的重试。 | 无证据将历史验证扩张为当前审计运行结果。文件表述和 review 的复核边界一致。 |
| `host.md:1-14` WorkerRequestScope、命令 caller 与清理边界 | `createWorkerHost` 创建单一 request scope，生产命令通过其 sequence、pending map 注册逐请求 listener/timer；handler 校验 requestId 和 generation（`worker-request.ts:43-69`）。成功、operationError、timeout 走幂等 cleanup；同步 `postMessage` 抛错按既有时序等待 timeout（`:71-72`）。Worker `dispose` 只清回调/cache 并 dispose lifecycle（`worker-host.ts:269-276`），未通知 scope 立即 reject。 | 提取后的唯一 owner/caller成立。dispose/fatal 时 pending 请求不立即失效及同步 throw 延迟回收均在原记录和 review 中明确排除；不是被工作记录隐瞒，也不能仅从新 `pending` Map 推导出本批应改变契约。错误不会永久 pending：保留原 timeout rejection/清理边界。 |
| `host.md:16-25` TauriTimelineState与查询/恢复调用者 | `createTauriHost` 委托 `TauriTimelineState` 管 generation、timeline、cached baseline、epoch。refresh/load/query 是真实调用者（`tauri-host.ts:205-239`）；orders/NPC query 使用 cursor 校验，save 只校验 generation。restore 先推进 generation/timeline 后解析 snapshot（`:78-84`），dispose 清 timeline/cache（`:86-89`）。 | 不同查询的 guard 强度不一致、restore parse 失败部分写入和晚到响应仍按原行为保留。旧结论成立：对象抽取本身不证明这些边界已经原子化或统一取消，需具体契约单独评估。 |
| `host.md:20-23` start baseline 与 dispose 异步调用 | `start` 每次从 `baselineForDelivery()` 交付缓存 baseline 后 fire-and-forget `resume_session`（`tauri-host.ts:151-159`）；`dispose` 触发 unlisten 与 `stop_session` 而不 await（`:165-175`）。 | start 重交付旧 baseline 仍应按 G04 对照 ADR-0010:56；frontend owner 提取不能核销。`stop_session` rejection 没有显式接入 fatal 通道，属于可明确指出的 cleanup 异步错误出口不足；原 sweep60 已记述这一点，不是新候选，也不由同步 `dispose()` 当前签名解决。 |
| `host.md:27-41` A股/日终边界、验证及复核声明 | 文件将金额分、元输入、股数及T+1沿用既有契约；报告精确短测/lint和待独立复核，不称三宿主矩阵已完成。 | 没有发现存档/API/交易字段漂移。异步命令“已提交”语义与 ADR-0010 的受理边界一致；长验收未在本次运行。 |

## 旧发现复核与新增候选反证

- **已确认旧结论仍适用：** G02 远程测速凭据、G04 Tauri 重启 baseline、G10/G11/G12 行情投影边界、G18/G19 宿主帧率/聚合、G20 默认 seed、G22 notice UX、G31 投影刷新隔离；这些并非此次 owner 提取新引入，未重复登记。
- **已关闭旧发现：** FR01 指标 series 按成功移除逐项清除句柄；失败重试不重复删除已释放项。旧 reviewer 对源码和行为测试的核销与现行调用链一致。
- **另批/明确排除：** PriceChart 构造半途失败回滚、Worker dispose/fatal 立即取消 pending、同步 postMessage throw 立即清理。现行代码确实没有这些行为，但原范围明确列为额外行为修复；错误/资源的现有 timeout 或宿主生命周期边界不能被描述为永不清理。
- **异步 cleanup 复核：** App effect cleanup 对已登记 host 同步 dispose；若 factory 尚未返回，返回后取消分支 dispose。Tauri `stop_session` 和 unlisten 是未 await 的副作用，stop_session rejection 当前无处理；此缺口已见旧 sweep60 的明确记录，应沿既有事项跟踪。Grid destroy 清借用 API 而非取消 AG Grid 队列，是有意契约。没有发现新 owner 遗漏真实 caller 或 cleanup。
- **A股语义与依据：** 本批为 frontend/host ownership 重构，交易金额/输入价/数量与 T+1 逻辑留在原路径；实现记录没有新增交易制度。未以旧工作记录替代官方规则重核，也未发现代码、单位或文案跨层漂移。
- **必要性与复杂度：** 三个实现记录所列 App lifecycle、chart projection/runtime/grid synchronizer、Worker request/Tauri timeline owner 均被真实调用，职责抽取没有复制第二套游戏、存档或交易 authority；测试 helper 只作为观察手段。
- **结论：** 232行全文覆盖至EOF。没有确认 sweep60 之外的新产品遗漏；旧结论按上述现状保留或核销。本文不替代产品测试、真实浏览器生命周期验收或三宿主长验收。
