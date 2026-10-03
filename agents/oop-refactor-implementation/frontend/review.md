# frontend OOP 独立复核记录

## 范围与限制

- baseline/当前 HEAD：`b89afb3346743a4b4fccf26c9ac9ff108595f696`；审查包含未跟踪新文件全文，不能只依赖 `git diff`。
- reviewer 未实施源码或测试，只写本复核记录；按 owner 指令不运行回归、build、E2E 或 tsc，不创建子 agent。
- 已读 `AGENTS.md`、`docs/principles.md`、`docs/testing.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0007、ADR-0010，以及 assigned-actions 对应条目。其余 owner 在途，当前不批准全量 frontend 完成。

## R2-N01 / R2-N04 首轮复核

七文件全文与 baseline diff 已读至 EOF，2026-10-03。未发现必须修复的缺陷。

1. **A 股语义**：只迁移异步 MACD/KDJ 请求身份及公开报告 ticket；公式、价格输入、DTO、披露日期/revision、公共 ID、宿主与 Redux 权威未改变。无新增交易制度，不需要为对象提取重新臆造官方规则依据。原 `candles?.length ?? 0` 是可选 candles 合同，并非掩盖异常的新 fallback。
2. **必要与最小范围**：请求对象不拥有 React setter/第二份 state；registry 不复制 coordinator generation/disposed/cache 权威。每次 effect 共用挂载 gate，registry 的 sequence clear 后继续递增，符合冻结动作范围。没有 retry、timer、AbortController、新依赖或业务行为扩展。
3. **边界与复杂度**：旧 generation 成功/失败、引用身份、parser/reject 错误、force 替换旧 finally、clear 后 ticket 不复用和 capability 清理均有相应测试。hook fixture 修改 React 私有 dispatcher，因此只证明所模拟的 render/commit/cleanup，不能视为真实 React DOM/StrictMode/E2E 验收；`request-owners.md` 已明确此限制。未见跨层语义漂移，未发现要求扩大本批范围的遗漏。

### 内容 SHA-256

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/components/indicator-results.ts` | `934e5d0d5ff68e6863077ee8c111429353e59cd1405cf879fd66fe3a1c58e108` |
| `apps/web/src/components/useIndicatorResults.ts` | `29f03aafa5c176905e08e8836be83405693897f7d157de329dbf7f0e0615bba2` |
| `apps/web/src/components/indicator-results.test.ts` | `117ee311e578a67470c902a2b21ed2515609aa8bce70c36cf4ae169301db4e27` |
| `apps/web/src/components/use-indicator-results.test.ts` | `ff961b15db03332d46c38db9c9bd618a745d5d9eaa198c7627086badc55ae053` |
| `apps/web/src/host/company-request-registry.ts` | `6d77620e4f8d2a001aabf4d6020191baeb24d59f70cdba577bfdbc5b48f2636b` |
| `apps/web/src/host/company-query-coordinator.ts` | `733e770c13e633f20b33fb318776b2c403ae93df0a005fb892275a71c1f1936c` |
| `apps/web/src/host/company-query-coordinator.test.ts` | `42662e54bfca81b8e11e4ba41e6a80fb4b5ed3760610c2b9b413307d3b671446` |

## 后续范围

mobile N08/N09 与原型 web-08-A01/E03 开始审查；最终其余 owner 稳定后继续完整 apps/web + design diff，不能把上述局部结论作为全量批准。

## mobile N08/N09 / web-08-A01/E03 复核

七文件全文及 baseline diff 已读至 EOF。未发现阻断问题。分/元、股/手、null 竞价价保留累计量、阶段独立量尺、算术均价和统一 tradeTime 既有简化保持；MA/KDJ 全历史后切窗、fixed slots 与 code-period key 保持。ADR-0009 的对称分时轴与午休坐标保持，收盘竞价过滤仍在上游。周/月 5/20 游戏交易日聚合原算法未改。prototype 初绘后 DOM 搬移、book 派发 click、未知视图 no-op、按钮引用身份和标题分支顺序保持；固定样例不被当作规则。新增 SSR 覆盖实际 caller，独立 VM fixture 只模拟所用 DOM 操作，未运行真实浏览器。新空 Kline 值对象行为不改变生产空数据分支。

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/mobile/MobileStockDetail.tsx` | `77b666477d51c29b18bb2728ca571824b76ac3eec57b161aa765d47ba52e2f40` |
| `apps/web/src/mobile/market-model.ts` | `8b04e3b448bb29148b401f3de7360ceafb2aa621e28694676ed81cf9320cdaeb` |
| `apps/web/src/mobile/mobile-intraday-projection.test.ts` | `de7139ffc70d355dd242efeca3455edf3b8c2070b9638f0a5677061eda011efd` |
| `apps/web/src/mobile/mobile-kline-projection.test.ts` | `c6cdd4cecdbbd872d1f43f6c2cc02bb6fdd7f8fffaa4d8075a33f7e1ddb13685` |
| `apps/web/src/mobile/mobile-trading-concept.test.ts` | `a68c26c250264c3b760ffd0a1edf92d03c81cbf8516256ca7f2821b559e4122d` |
| `apps/web/src/mobile/mobile-component-render.test.ts` | `1e341489f57178e201c2966c7828241c63552364a8ac1131416e89c4607b0617` |
| `design/ui/mobile/mobile-trading-concept.html` | `0665fbe6caf7dfb96c8d98dc9a8cc31b0b94a6618c79c4a8c16c978f79150ba9` |

## remote N01/N07/R2-N02 复核

九文件全文及 baseline diff 已读至 EOF。未发现阻断问题。fail 清 socket/waiter 后 reject、close、commands、fatal；baseline epoch/report/cache 后 callback/waiter；resync send 后登记 waiter；mode identity 与 awaiting 状态原顺序保持。save 只 pin generation，而 orders/diagnostics 加 baselineEpoch 的不同 guard 保持。opaque ID 不转 number、页验证先完成再登记、CommandQueued 只表示入队，符合 ADR-0010 与既有跨层合同。三个窄 owner 没有侵占 running/disposed/HTTP/callback 编排，没有重试或新配额。测试明确覆盖旧 socket、单槽 waiter、同步 send 失败与 callback 重入；fail 保留 cache、旧 onerror 和 dispose 不 settle command 的既有缺陷保持，不据此宣称这些缺陷已解决。

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/host/remote-host.ts` | `a3a32aa2eeac33ce4c8f65837fb3a14cd6609bb85ae2d437b20825c38ab913af` |
| `apps/web/src/host/remote-company-reports.test.ts` | `9c2c3f38505ce7813a4e0d958a7f4a304a0fcd509cdb4c24639a2f69894068bf` |
| `apps/web/src/host/remote-state-contract.test.ts` | `7da1a9714e2501937f0f98a07bfce18709072f55d5babbb1c862ce3f165a114c` |
| `apps/web/src/host/report-query-context.ts` | `e54fc63389d196fa42646027169ec25e6179834f53ebec0f7acea82d37474fd3` |
| `apps/web/src/host/report-query-context.test.ts` | `6668ca8d1f1e6c2b638789bf134b0e2c8daa084b1ceac313e96e66b70ad58182` |
| `apps/web/src/host/remote-command-registry.ts` | `13078e0c46f18bf956df2e2ffcf3b429919113446e3bf9dd6ec6101b25409d90` |
| `apps/web/src/host/remote-command-registry.test.ts` | `4a3a02a54816fa00b07e78fbe6d90f19c1e0e9feaffc48954f1cd59e77d38887` |
| `apps/web/src/host/remote-publisher-state.ts` | `2893b9f5b5ee7ada9ea747b424c3c9d4babff82607027b8e625fa9a3c4ea7add` |
| `apps/web/src/host/remote-publisher-state.test.ts` | `cebf8df9b16eac3900d238fbc9ee0627fb882cc475db84c904bcc37dc0da999d` |

## WASM N04/N05 复核

六文件全文及 baseline diff 已读至 EOF，另阅读全文未改的 wasm-restore-transaction.ts。未发现阻断问题。handle/generation 单归 slot，timer/节拍/暂停策略/测速单归 loop，Worker 保留 bindings 异步初始化与消息协议。restore candidate snapshot、交换 handle、旧 drop、finally microtask restart、generation 推进、prepare 的顺序保持；响应旧 generation 与新 baseline 分离。市场 TickBatch 才计 tick，CivilUpdate 不计；protocol 先发再 barrierPaused、Infinity 每 task 一步与有限倍率单步追赶保持。新增实际 Worker listener fixture 覆盖接线；单 slot 与 loop fixture 覆盖失败和 timer 边界。重复 create 泄漏、旧 drop 失败部分 authority、prepare 失败新 generation、candidate cleanup 失败遮蔽原错误等现有缺陷保持，未伪称 restore 绝对原子。

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/host/wasm-worker.ts` | `b52692d191a0efedacd3c719229ec3e412d7fee3c8cf53b0e59ba638abbca754` |
| `apps/web/src/host/wasm-session-slot.ts` | `f4cd028d99bc944392e45e41f3a99583878c71a897366245ecb52e1b24ca67a9` |
| `apps/web/src/host/wasm-tick-loop.ts` | `62e9f8fb145d8907cf446c6ed02b9d29787ba0561cd4d2532cac485a015ae089` |
| `apps/web/src/host/wasm-session-slot.test.ts` | `414c080122cc4090c061aaf7f8080a57216d109918ba34ef20b1b0e34d4ad298` |
| `apps/web/src/host/wasm-tick-loop.test.ts` | `9147fa6f4bb6be25de2a5546767f2fbcf07e82d45879c239267f3d4b8fe81d4a` |
| `apps/web/src/host/wasm-worker-ownership.test.ts` | `1d75c371870defe61e9aabba8009196dfcd60cce165b65a202e629c2ec7324e5` |

## R2-N01 fixture 具名函数修正复核

`use-indicator-results.test.ts` 的 `render` method 变为 `render: function IndicatorFixture`，其方法体不变。已只读复核，未发现行为变化；这是 React rules-of-hooks 静态检查的最小修正。最终 SHA-256：`b1de0402044e7ee74c625eb7345799ea5daa5343e1d98d06bce9318ba9cbefa3`，替代首轮表中的该文件 hash。

## N06 / R2-N03 复核

九文件全文及 baseline diff 已读至 EOF。未发现阻断问题。WorkerRequestScope 接管唯一 sequence 与逐请求资源，所有生产 helper/命令委托同一 scope；requestId 与本请求 generation 关联、dispose/fatal 继续等待原 timeout、同步 postMessage 抛错的后续 timeout 清理保持，不合并 Remote 协议。Tauri 四字段集中但 session/running/callback/IPC 留 facade；generation 用 string/BigInt 精确推进，初始/refresh/load 的 timeline 先写后 parse 部分失败状态、query guard 差异与 callback/resume 顺序保持。真实 mock events 与 IPC、fake timers 和窄值对象测试覆盖关键边界；static startup regex 只是接线守卫，不冒充运行时验证。

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/host/worker-host.ts` | `11139fec6a2a7c104816a6ef7812e49677a47fb032c8281c222de4d4a9988e72` |
| `apps/web/src/host/worker-request.ts` | `963bb337403726de4338b9e8a5473e17b11b218491e98da4edc7a0e2131ff8fd` |
| `apps/web/src/host/worker-host.test.ts` | `cecd53e906f5b7fd2e663ce93767ccdaa94a0356152d9c6860019330b63fed70` |
| `apps/web/src/host/worker-request-scope.test.ts` | `1df10b97d9527e877cac14335f8d69a5d821be78f80ebe9acba5a0cc07451990` |
| `apps/web/src/host/tauri-host.ts` | `1c0a31ad3d3dea667fbe74a979f9866a878b19d9885eae94c0d0f670e49f022b` |
| `apps/web/src/host/tauri-host.test.ts` | `817a606812e21d7c159d90d47e58998c2de0e9b15ce36ae52dd151a91978d190` |
| `apps/web/src/host/tauri-startup-contract.test.ts` | `c004eb73b19ce280c221d487f4e952e16af22404ccf501fccce1c83e625ce2e3` |
| `apps/web/src/host/tauri-timeline-state.ts` | `90c671a14585f035f333f7a6decef47cf6629a5226080313c24a26153ef1a68d` |
| `apps/web/src/host/tauri-timeline-state.test.ts` | `666f911f08f6ece026e5f87ccaa031bde6b91a8ab484b0d8b7f4f0b139f16c41` |

## 原始发现 FR01：PriceChartRuntime 的部分 removeSeries 失败记账漂移

- 首轮读取 `apps/web/src/components/price-chart-runtime.ts` SHA-256：`7d4cdc3b6e858b0e4d61b9f01b06e68687a4aa5ffe4548b762abe96c835f15f3`。
- MACD/KDJ 清理从 baseline 的“每项 remove 成功即清该 ref”变为“所有 remove 后批量清字段”。第 2/3 项 remove 抛错时，owner 仍保留第 1 项已被删除的句柄，retry 可能再次删除无效句柄。此为提取新增的部分失败语义差异，属于本批必要修正，不要求顺带半初始化 rollback。
- 已即时反馈 owner，请恢复逐项清字段并补 fake chart 第 2 项 remove 抛错后 retry 测试；待修正复核，不批准 N02。

## A27 / N02 / N03 首轮完整复核

十四文件全文及 baseline diff 已读至 EOF。A27/N03 无新增阻断问题；N02 仅 FR01 待修复。五组 cache 单归 MarketChartProjection；Protocol/Redux/React authority 与 effect/dispatch 顺序保持。readonly getters 冻结的是新建派生点/candle/tradeStats，已阅读全文 kline-sync 确认不冻结借用 snapshot/frame。没有新增 history 永久承诺或改变累计量差值算法。Provider 使用稳定 getter，selected 数组仍复制，完成日 K/未变化 code 复用。MarketGridRowSynchronizer 的 render 登记和 effect/ready 提交保持，reattach 明确按新 grid 初始行做 diff，destroy 只解除 API 借用。PriceChart runtime 的显示/单位/颜色/指标 parser 及 effect 保持，半初始化失败回滚不进入本次范围；remove 部分失败记账见 FR01。测试覆盖真实 ProtocolCoordinator 到 hook 的 SSR probe 与 fake chart/grid；这些不替代真实浏览器 lifecycle/E2E。

| 文件 | 首轮 SHA-256 |
| --- | --- |
| `apps/web/src/app/market-chart-projection.ts` | `878f5ecbcbd20e20191725c04021033ceef88888cafe66ba763b761a3e16efab` |
| `apps/web/src/app/market-chart-projection.test.ts` | `ca17f272d8f7170564653a143671895ce31378e71bcec72d4cb0e617feea3137` |
| `apps/web/src/app/useMarketChartRuntime.ts` | `166db8e85a19a5e1bf6977d633c7822d3a5a6b77793504f3c7b46901147da07b` |
| `apps/web/src/app/MarketRuntimeProvider.tsx` | `15ad134ae9915934c3d36c3fc0df210f02b4c5bc65ba3c1e0fa8e12418c9106d` |
| `apps/web/src/app/LocalRefreshViews.tsx` | `9a3ad9a4ff97b7bbdc349e4d2609b03f29101f1816c5a69c3a1cdd1c4b558f3e` |
| `apps/web/src/app/market-chart-runtime.test-support.tsx` | `c81d084248875fdb0dc179cf966daa0190b8838f4313677623e0a5cd1630eb31` |
| `apps/web/src/app/market-chart-runtime.test.ts` | `4dfd7dcc07dc6f8111fae2dfa7ac6f11acccd8f430f9b868ea941e255c464330` |
| `apps/web/src/components/PriceChart.tsx` | `2bdb388cfe2d9f46e32c045f0eec715f74c70cb3ef254e0f68dd59f2a6880a8b` |
| `apps/web/src/components/price-chart-runtime.ts` | `7d4cdc3b6e858b0e4d61b9f01b06e68687a4aa5ffe4548b762abe96c835f15f3` |
| `apps/web/src/components/price-chart-runtime.test.ts` | `fcfb224551545e25a560b6da956309ea3f9f050a89f0ab1b1b1f533d7d8a95b2` |
| `apps/web/src/components/MarketGrid.tsx` | `ba4d75c85ecc7cd55798c4c1d461530ed664140ca1eff2a6b2c18af466f6275b` |
| `apps/web/src/components/market-grid-row-synchronizer.ts` | `87939159e366760f7c34cc0bda31515b8349e1043ad002c870f629efbf0dca28` |
| `apps/web/src/components/market-grid-row-synchronizer.test.ts` | `a574956628e65de65ece5254c67c787c04e35253ec127b1452c3b387f0fb7a1e` |
| `apps/web/src/components/local-refresh-boundaries.test.ts` | `65e71e18a1a9f383828c96d1a7fef52fa078acff35503d10d1699b8209fecec9` |

## FR01 修正复核与关闭

已再次阅读全文 runtime/test 至 EOF。`removeIndicatorSeries` 在每次 remove 成功后立即 null 对应字段，MACD histogram→dif→dea 与 KDJ K→D→J 保持 baseline 顺序。测试分别使两组第 2 项删除抛错，再 retry，明确断言首项仅删除一次、第二项两次、第三项一次。修正范围必要且最小，FR01 关闭；reviewer 未自行执行该测试，红/绿执行结果由实施 owner 记录。

| 文件 | 修正后 SHA-256 |
| --- | --- |
| `apps/web/src/components/price-chart-runtime.ts` | `839962174d7d17a350f92de9f96b2609d235fafaa9c44715557ddabcdf349c8a` |
| `apps/web/src/components/price-chart-runtime.test.ts` | `b7b5cb74420d3d27507f51c1727a28abe8a26032a3d7ea10a52b8b7d22e5fd6c` |

## App A06 / E01 / E02 完整复核

App、五个新 hook/闭包 owner、六个新测试/fixture 及四个迁移静态守卫的文件均全文至 EOF，App 与已有测试 baseline diff 完整读取。未发现新增阻断问题。SessionHostLifecycle 的 ownedHost/cancelled/registration 单次 effect owner 与 AppShell hostRef 借用区分明确；ProtocolCoordinator callback、日终 CivilUpdate candidate 与 company/chart authority 留 composition root。factory 直接返回原 Promise，malformedProtocolFixture 惰性 getter 保持原创建后读 URL 时机。SaveCommands 六个 handler 的独立写集、gate 与 idle 屏障、load 失败 resync/fatal、metrics finally 令牌、旧目标拒绝保持；日内保存只提示政策并选择授权目标，符合 ADR-0025。TradingCommands 持有同一表单/取消中/查询 state，清空 callback 稳定；T+1/冻结卖量、板块上限、符号限价仍复用原校验，提交不表示已成交。PausePreferences 只拥有 loaded 标记，偏好仍由 Redux 权威；storage 懒读抛错可见且 host sync 在先。Polling 只持 timer/cancel，借用现有 shared gate，错误与一秒重试时序保持。新增 hook fixture 使用 React 私有 dispatcher，不模拟 DOM/commit，不冒充真实 React lifecycle 或浏览器验收；其余 factory 行为测试与迁移静态接线守卫各有明确证明边界。

| 文件 | SHA-256 |
| --- | --- |
| `apps/web/src/App.tsx` | `3d84652a168d94c4b37b276830a04316befe2cb17ce7153e8c45ff40b9fa6b92` |
| `apps/web/src/app/useSessionHostLifecycle.ts` | `b74b23deddf1782050c3fd6e95534dc6aa1e7d166a515ed0aedbac6e7e589eb2` |
| `apps/web/src/app/useSaveCommands.ts` | `ce6d3ad6e7ff901ea6342b1b31b483e5e9eeddb4808a20502a1dd4aa03a0a795` |
| `apps/web/src/app/useTradingCommands.ts` | `d95732a0d2d39380ce60787ce56a50232379c77c88f98a1d91d813e38228f4ed` |
| `apps/web/src/app/useSpeedMetricsPolling.ts` | `c6fd5c2cde9062166fb358975195bd2e173b3d1ea37d1be693566dcce1f323df` |
| `apps/web/src/app/usePausePreferences.ts` | `56620b5c9c2e4cfda41bfed008913faaec5dc8d758b7b26bdd624b3ad8887687` |
| `apps/web/src/app/session-host-lifecycle.test.ts` | `b0da1d9ab569149a02c1384cdc5e5bb865fff8d2614cd542942074de0c01f4e8` |
| `apps/web/src/app/save-commands.test.ts` | `04013b4a9cddd9c278654b2586dfd85598962d37718a2a8b2b7e2477716acbfe` |
| `apps/web/src/app/trading-commands.test.ts` | `64316814bd0bf5d8cbd85ee4ad2e38a3110d3184993f90833134587b906e97eb` |
| `apps/web/src/app/speed-metrics-polling.test.ts` | `758dacb029778a95fa92ffd2a230ad30ef354314f691686a3ff7b3bc0006a292` |
| `apps/web/src/app/pause-preferences-runtime.test.ts` | `fa724889142314fd50293749e5c7a738f2b0defeb88f2c383b5cb799fc1c31e5` |
| `apps/web/src/app/hook-test-runtime.ts` | `5aabe94acc18938b593e7755ba7e1a0fd348558ec85a8530854e12c42858f360` |
| `apps/web/src/app/app-startup-wiring.test.ts` | `9ed8b0909289f946d02522c6f0c4b680c210a0c3b1089dd0b103777a0b7b1bdf` |
| `apps/web/src/app/app-persistence-wiring.test.ts` | `340647679377bb83a4676fd3ba7ec6686baa750a9ddbd3d7e30729805a21ee80` |
| `apps/web/src/utils/trade-input.test.ts` | `af1331277ba3765fb32c4cca2d87283ebad9b107d14394ea3e0e62fd2eb5a988` |
| `apps/web/src/utils/symbolic-limit-order.test.ts` | `73c13dd9066ac5c86b0ea483530fd5a36bdb9709b2ca545a4ba0e603e3d0aec3` |

## 类型门禁所需的增量复核

已完整复核新 `command-host-test-fixture.ts`、dev inspector test 全文与单行 baseline diff，重读三个 App fixture tests、两个 Remote tests，并核对 chart 的 resize 端口与 fixture 类型变化。完整 EngineHost fixture 对未声明命令显式 throw，存档 fixture 经真实 parseSaveSlot，账户 fixture 使用 invested_cents/recovered_cents 与 satisfies，未删除/弱化既有行为断言。Remote 仅补 HostFailure.where 和可选方法存在性断言；chart resize listener 的窄类型匹配真实无参数 handler，未改 I/O 顺序。dev createElement 显式泛型只修推断，无组件或领域行为变更。App lifecycle 格式调整、malformedProtocolFixture 惰性 getter 与 static test 删除未使用局部变量不影响已审行为。未发现新增阻断。

| 文件 | 最新 SHA-256 |
| --- | --- |
| `apps/web/src/app/command-host-test-fixture.ts` | `5ce29e0860dbf4463aea28730133e9908d1049a6b9582e506776451450a8136f` |
| `apps/web/src/dev/npc-decision-inspector.test.ts` | `2f6c9d3bc6682df82d2c2d1bd84c0b1b8819c83ac5f58b15e8226ace6914ce5d` |
| `apps/web/src/app/session-host-lifecycle.test.ts` | `b0da1d9ab569149a02c1384cdc5e5bb865fff8d2614cd542942074de0c01f4e8` |
| `apps/web/src/app/save-commands.test.ts` | `04013b4a9cddd9c278654b2586dfd85598962d37718a2a8b2b7e2477716acbfe` |
| `apps/web/src/app/trading-commands.test.ts` | `64316814bd0bf5d8cbd85ee4ad2e38a3110d3184993f90833134587b906e97eb` |
| `apps/web/src/host/remote-command-registry.test.ts` | `afe5f14f14e006b7bb5cacaf31108101f52391eea8a8c180b9a8e87659a5ddbd` |
| `apps/web/src/host/remote-state-contract.test.ts` | `6528de90b58949e88b5130f73da3f7fb46f7651c782cbc5a2c52b6ecd3682506` |
| `apps/web/src/components/price-chart-runtime.ts` | `839962174d7d17a350f92de9f96b2609d235fafaa9c44715557ddabcdf349c8a` |
| `apps/web/src/components/price-chart-runtime.test.ts` | `b7b5cb74420d3d27507f51c1727a28abe8a26032a3d7ea10a52b8b7d22e5fd6c` |

## 最终三门结论（绑定内容清单）

- 完整范围：`apps/web` 与 `design` 相对 baseline 的 32 个已跟踪修改 + 38 个未跟踪源码/测试，共 70 文件；每个文件已阅读全文至 EOF，每个已有文件已审完整 baseline diff。
- 内容清单：`review-inventory.json`；按排序条目规范 JSON 的 SHA-256：`abe7a5142fa745d38e066cc1eda6b97448d9cdc17c986a1825027660395adeaa`。已包含最终 TradingCommands 依赖及身份断言增量；结束时核对各文件最新 hash，无遗漏或漂移。
- **A 股语义门通过**：未改权威交易/费用/结算/披露/存档合同，Cents/yuan、股/手、T+1 可卖量、板块上限、符号限价、CommandQueued、generation/epoch/tick/seq 各概念保持。依据为已登记现行 `docs/trading-rules.md`（2026 版规则生效日期 2026-07-06；正文记录 2026-09-22 至 2026-09-27 的不同条款复核边界）与 ADR-0009/0010/0025；本轮未外部重查交易制度，不伪称费用来源重新访问成功。
- **必要性与最小范围门通过**：16 个 assigned actions 及扩展对应窄 owner，移走数据与共享行为、保留既有权威和分层；类型修正只为受影响验证编译，没有新增依赖、交易制度或修复未授权既有缺陷。
- **边界测试/跨层/复杂度门通过**：FR01 资源记账发现与 FR02 台账 caller 分类发现均已修复并独立复核关闭；16 动作、3 扩展及 39 条需求均已核销，无未解决发现。新增 fixture 与原测试的迁移没有弱化断言。React 私有 dispatcher、SSR、fake chart/grid/IPC 只证明各自边界，不等同真实浏览器或三宿主矩阵。
- reviewer 本轮仅只读源码/diff与工作记录，没有运行普通测试、回归、build、E2E、lint 或 tsc；执行证据由实施 owner/root 的最终定向记录提供，受影响的最后增量测试、TypeScript 与零 warning lint 均已记录通过。任何内容清单变化必须复核，不能沿用当前 hash 宣称新版本通过。

## 需求台账首轮复核：FR02 原始发现

独立逐条核对 `completion.json` 的 16 个 actions、3 个 extensions 和 39 条 requirements；assigned actions 及扩展 ID 集合完全相同，assigned SHA、completion 的 70 文件内容清单与 reviewer inventory 完全匹配；核对实际文件时发现 useTradingCommands.ts 一文件漂移，其余 69 文件一致，已反馈 owner 并追加复核。正文可选项含 N06 nextRequestId、N03 dispose、原型 web-08/E03 与 R2-N01/R2-N03/R2-N04 均已实际接线。N06 dispose/fatal 立即 reject 及同步 postMessage 立即清理为原文明示另批行为变更，scope_exclusions 不属于隐匿未实施目标。

发现 FR02（仅台账准确性，源码无缺陷）：production_callers 混入测试观察、被调用 helper 和共享权威对象；原型还列了没有实际绑定的返回/priceBook/订单按钮。真实原型只绑定 [data-v]、[data-p]、#p；book 导航派发 chart button click。要求把测试观察列 test_callers/observations、callee/共享 owner 列 related_owner，并纠正原型调用方描述。候选原文的返回按钮说法不能凭空作为真实调用证据，也不能借此新增原型行为。已即时反馈 implementation owner；修正台账后复核。

## TradingCommands lint 依赖增量复核

root 要求最小消除 warning 后，refreshPlayerOrders 的 deps 增加两个 AppShell 同一挂载稳定持有的 hostRef/playerOrderRefreshGateRef 对象；没有依赖 .current。已重读 production 全文及新增测试断言；form render、hostRef.current 变化和 gate 失效之后 callback identity 仍相等。此为类型/lint 门禁相关最小修正，不改变生产 lifecycle effect 触发频率或交易语义，无新增发现。reviewer 未执行该测试。

| 文件 | 原 SHA-256 | 新 SHA-256 |
| --- | --- | --- |
| `apps/web/src/app/trading-commands.test.ts` | `64316814bd0bf5d8cbd85ee4ad2e38a3110d3184993f90833134587b906e97eb` | `489d36f571a3c6f23490de9965807e269de7c4e5f4554a5a23f2314a0556b9c0` |
| `apps/web/src/app/useTradingCommands.ts` | `d95732a0d2d39380ce60787ce56a50232379c77c88f98a1d91d813e38228f4ed` | `4458169a3debc5f4abe6e406f0995545d05cafd4dff7ce7d890d02073943cfd2` |

本次增量重算独立 70 文件清单 SHA-256：`abe7a5142fa745d38e066cc1eda6b97448d9cdc17c986a1825027660395adeaa`，替代上一轮 ec9c5d 清单；最终 dispose caller 分类与 verification 状态已修正，FR02 已复核关闭。

## FR02 修正复核与需求台账最终核销

已完整读取最终 completion 台账的所有 39 条需求、actions/extensions 元数据与验证记录，阅读全文 verification.md，并阅读全文其七个证据链接记录。逐条映射与此前独立全文源码审查核对；FR02 中测试 caller、callee/related owner 和原型不存在的返回/订单接线均已纠正。N07 dispose 已移到 related_owners 并注明不 settle pending。历史 warning/红灯和最终依赖修正、身份断言、app tsc 与零 warning lint 已分清版本，未重复累加 case 或将历史通过冒充最后版本。FR02 关闭，无未解决发现。

| 核销项目 | requirement 数 | 结论 |
| --- | ---: | --- |
| `web-01-A06` | 4 | owner、methods、真实 caller 与已审源码一致，通过 |
| `web-01-A27` | 3 | owner、methods、真实 caller 与已审源码一致，通过 |
| `web-08-A01` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N01` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N02` | 3 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N03` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N04` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N05` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N06` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N07` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N08` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-N09` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-R2-N01` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-R2-N02` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-R2-N03` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-R2-N04` | 2 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-E01` | 1 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-E02` | 1 | owner、methods、真实 caller 与已审源码一致，通过 |
| `frontend-E03` | 1 | owner、methods、真实 caller 与已审源码一致，通过 |

- assigned actions：16；具名 extensions：3；需求总数：39；ID 集合与原 assigned-actions 完全匹配，正文可选项未仅按标题核销。
- N06 可选 nextRequestId 已实现并接入全部生产命令，pendingCount 是测试观察；dispose/fatal 立即 reject 与同步 throw 立即清理按原文明确另批行为修复。N03 dispose 按实际 grid destroy 解绑；可选原型 web-08-A01/E03、R2-N01/R2-N03/R2-N04 均完成真实提取。
- prototype 四个真实方法为 bindEvents/selectChartView/navigateTo/renderProgress；N08/N09 的 readonly properties 单列 observations，未冒称函数。旧 header 返回/quick 订单按钮没有旧 caller，保留无接线现状属于候选证据纠正，不是偷偷删实现目标。
- 两个 trading 文件增量已复核且相关身份测试已读取；最终 70 个源码/测试/原型 SHA 与 reviewer inventory 和 completion.files 完全一致，无漂移。
- 独立清单 SHA-256：`abe7a5142fa745d38e066cc1eda6b97448d9cdc17c986a1825027660395adeaa`。
- 最终 completion.json SHA-256：`0f02f3f4348756e6d403b4e1515b600901129e298f4811ad6a9acf965ea5edf0`。
- 最终 verification.md SHA-256：`b14dcda5226f1011fc6a8e5cd10f1e509229f3e59d93b62735657e2fcb44c3e7`；已全文重读终稿，三门通过、TradingCommands 最后增量复核通过与 FR01/FR02 关闭状态一致，未以本次文档校正重跑测试或编译。
- 执行门禁按实施方最终记录：trading/startup 精确 11 case 通过，最后 app tsc 6.369s 通过，node/inspector 无受影响增量且保留其稳定通过，69 文件 oxlint threads=4 / 0 warning 通过。reviewer 没有自行运行测试/tsc/lint；独立源码与台账核销均通过，FR01/FR02 均关闭。
