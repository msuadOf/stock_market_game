# 重集成战役总体计划（main 线退役工作回归 std-thread 架构）

> 2026-10-19。规划 agent 只读分析产物，未改任何入库文件、未跑 cargo。
> 输入：`agents/main-line-reintegration/` 四件（retired-manifest / reint-company-coupling /
> reint-web-features / slice1-review-record）+ `agents/std-thread-migration/plans/
> view-campaign-design-brief.md` 必留清单 + `packages/engine/src/runtime/` 现状核对。
> 锚点：退役终态 `18725d07`，分叉点 `a6001494`，批次 1 已合并（`003c4b5d`）。

---

## 0. 现状快照（规划事实基线）

| 项 | 状态 |
|---|---|
| 引擎架构 | std-thread 所有权模型+发布协议（`runtime/publication.rs` `PublishedTick`/`ViewPublication`）+ Seal 已合并；视图内生 D1-D6 已删 |
| 旧管线 | 仍为 `ENGINE_STEP_RUNTIME=0` 逃生门（批 9 未退役）；`step_frame` 生产路径已走 runtime 分叉（`session/protocol/commit.rs:103`；server actor `actor.rs:1145` 同入口）——远程登录的入口前提**已满足** |
| 公司日终通道 | `AdminRight` 仅 3 变体（`runtime/protocol.rs:228-256`）；`WorldSpec.rights_computer` 预留（`world.rs:229`，`bridge.rs:111` 恒 None）——通道在、未接线 |
| 日结编排 | `RuntimeDriver::day_end`（`drive.rs:1164-1196`）：`run_day_end_public` → `session.end_civil_day()`（唯一权威）→ 双侧回滚编排；日结后 `force_full_accounts=true`（全量回写过渡窗口） |
| 分钟读面现状 | desk 已维护 `minute_closes: Vec<(绝对分钟, 收盘)>`（`desk.rs:90`，`:2304` 回填）并已随每拍安装进 session（`drive.rs:2220-2229` → `failure.rs:2160`）；日结清空（`drive.rs:1190`）。**只有收盘价，无 OHLC/量额/笔数** |
| 查询 API 现状 | `query_stock_history`（含 history_reads 副作用）仍在树——R5 锚；`MarketHistory*`/`MinuteBar`/`CurrentMinuteHistory*`/`PersonalTradeHistory*`/`IntradayAverage*` 查询面 HEAD 全灭；`session/protocol/civil/session.rs` 在树但查询面被削（diff 1201 行）；`intraday_average.rs` 纯函数在树 |
| 事件面 | `Event::Trade{seq, code, price, qty, maker, taker: AccountId}` 每拍经 event_bridge 投影（`event_bridge.rs:25`，全局 seq 先加后取）；`DeskTradeFact` 含双腿费用+订单序号（`protocol.rs:855-878`）——**事件不带费用分解，事实带** |
| web 批次 1 | 11 文件三组（G5/G3 组件半/G7）已合并；复核阻断 0/建议 2（旧档格式拒绝口径、基线口径勘误） |

## 1. 批次地图与排序（数据驱动）

### 1.1 批次一览

| 批次 | 内容 | 档位 | 依赖 | 风险 | 收益面 |
|---|---|---|---|---|---|
| **W2** | web 小公共 diff：K线周期（G1）+均线接线（G3 另半） | S | 无 | 低 | G1/G3 完整化，立即可见 |
| **B2** | 引擎查询 API（分钟K/保留历史/个人历史/日内均价） | L（含设计门） | 无 | 中 | 解锁 4 个 web 功能组；远程查询复用 |
| **W3** | web 查询面接线（G2 分钟K/G5 数据面/G6 个人历史/G8 保留历史） | M | B2 | 低 | 4 组面板出数 |
| **B3** | 远程登录（server 12+native-store 5+web 14） | M-L | 无硬依赖 | 中 | Stage 2 能力（联机/持久化） |
| **B4a** | 公司纯域闭包回归（~64-68% src+纯域测试） | L（机械，可分包） | 无 | 低 | 素材归位，公司线地基 |
| **B4b** | SessionSetup/save/构造期装配+spine 公司状态 | M | B4a | 中 | 含公司配置的新局可构造/存读 |
| **B4c** | 日终 12 步链嵌入（过渡态）+desk 除权基准消息 | L | B4b | **高** | 公司行为按日推进跑通 |
| **B4d** | AdminRight 扩展+rights_computer 接线+回购专户+认购命令通道 | XL（含设计门） | B4c | **高** | 终态落账通道 |
| **B4e** | 公司查询/提案/披露协议面（引擎） | M | B4b（查询）；B4d（动账命令） | 中 | host ~40 pub 方法面 |
| **B4f** | web G9（~80 文件）+G10 重生成（~88）+server 公司契约 | L | B4e | 中 | 公司系统全链可见 |
| **B5** | 收尾：重集成基线重立+文档同步（trading-rules 简化登记/roadmap/ADR） | S-M | 全部 | 低 | 台账清账 |

共 **11 批**（含 2 个 web 适配批）。

### 1.2 排序论证

推荐执行序：**W2 ∥ B2 设计门 → B2 → W3 → B3 → B4a → B4b → B4c → B4d → B4e → B4f → B5**

- **W2 最先**：零引擎依赖（turnover.ts 已入 manifest——slice1 勘误证实；只剩 calendar-candles/ChartPeriodTabs 两个小 diff + basis 参数裁决），风险最低、收益立即可见。可与 B2 设计门并行（不同层，worktree 分离）。
- **B2 先于 B3**：解锁面数据说话——引擎查询网一次重建带 4 个 web 功能组（G2/G5 数据面/G6/G8，web 盘点 §0 结论 2），是剩余工作里**单位风险解锁面最大**的一批；B3 与 B2 无硬依赖边（远程上下文走 RuntimeDelta/EngineUpdate，不走 MarketHistory 族）。
- **B3 先于公司线**：31 文件 vs 公司线 400+ 文件，自包含（server 入口统一前提已满足）；且 B3 与 W3 都碰 `App.tsx`/`engine-host.ts` 公共 diff，按 W3→B3 串行落地减少公共文件冲突。
- **B4a 可提前并行启动**（自己 worktree 取回+纯域测试，不碰 runtime），但落地窗口建议在 B3 前后——纯增量文件+lib.rs mod 接线，冲突面小；提前启动缩短公司线关键路径。
- **公司线内部硬序**（coupling 盘点 §5 原序）：纯域 → setup/save/装配 → 日终链过渡态 → AdminRight 终态 → 查询/协议 → web。B4e 的纯查询部分理论上可与 B4c 并行，但 civil 协议面同时承载两类，串行更省复核成本。

---

## 2. 批次 2 专节：引擎查询 API——发布协议读面 vs 专用查询通道

任务书点名要求方案对比。三个候选：

### 方案 A：会话侧事件流累积（旧形状直搬）

`RetainedMarketHistory::record(events)`（18725d07 `session/retained_history.rs:141-229`）逐拍消费 `Event::Trade`/`AuctionCompleted` 聚合分钟K，`SharedHistoryDay(Arc<…>)` 冻结日快照。

- 复用度：retained_history.rs（1,226 行）+ live_minute_history.rs（342 行）约 80% 直搬，落地最快。
- **硬伤 1（架构相悖）**：视图内生战役刚删掉 D1-D6"为镜像而上行/安装"，方案 A 等于往会话步尾重新加 O(成交数) 累积税，与"零会话层税"（战役目标陈述 1）和"GameSession 退位为发布版本的读者"（目标陈述 2）直接冲突。
- **硬伤 2（与现状冲突）**：分钟收盘读面**已经走发布链**——desk `minute_closes` 每拍随发布/安装进 session（`drive.rs:2220-2229`）。方案 A 造出第二份并行累积，两源必然漂移。
- **硬伤 3（数据不全）**：`Event::Trade` 不带 `FeeComponents` 费用分解，个人成交确认（G6）无法只靠事件重建。

### 方案 B：desk 原生分钟K观察面 + PublishedTick 读面（推荐，市场域）

- desk 已在同一循环里维护 `volume_today`/`turnover`/`trade_count`/`minute_closes`（绝对分钟键）——扩为 per-minute OHLC+量+额+笔数桶是**既有聚合循环的自然延伸**，且 desk 知道自己的竞价/连续相位（集合竞价事实在 `protocol.rs` 批 8f 族），比旧会话侧"从 AuctionCompleted 事件反推相位 565/900 分钟"更干净。
- `StockPublic` 扩分钟K字段 → 随 `DeskShard`（`publication.rs:32`，`Arc` 共享免深拷贝）进 `PublishedTick`；查询方法=发布版本薄读者（`publication().load()` 一次 clone 得一致读点）。
- 跨日留存（近五交易日）：日结静点把当日 bars 冻结为 `Arc<RetainedHistoryDay>` 不可变日快照（旧 `SharedHistoryDay` 模式直搬），desk 侧 5 日环形窗或日结拉取到 session 冻结集——**设计门内二选一**（倾向 desk 侧留存：所有权在 desk，查询仍走发布/静点拉取）。
- 成本：desk 热路径小扩展 + `DeskCandles` save/restore 种子扩字段 + wasm 串行路径"单 owner 退化发布"复用（战役风险表既有要求）。

### 方案 C：查询时 report_log 重放派生（纯 R5 字面最大化）

- 零常态维护成本；但每次查询重放成本 O(跨度)，近五日分钟K不可接受；且 report_log 全量事实保留本身待批 9 裁决。**仅作对照，不推荐**。

### 推荐：B（市场域）+ 事实派生 receipts（个人域）的混合

- **市场域（分钟K/保留历史/日内均价）走方案 B**：价格/量额权威在 desk（owner 原生观察面=终态，避免过渡态返工）；`intraday_average.rs` 纯函数在树，输入对上分钟K即可复用。
- **个人域（成交确认/个人历史）走事实派生**：数据源=驱动器已汇集的 `DeskTradeFact`（带双腿费用+订单序号），session 侧 per-account `AppendOnlyHistory` 累积 receipt 日志；**receipt_id = trade_seq*2 + leg 位**（event_bridge 已分配全局单调 seq——确定性、全局唯一、免新号段；设计门内核对与旧 receipt_id 语义差异）。个人域归属账户，desk 不宜持有跨股 per-account 日志。
- **副作用查询核对**（开工 checklist，对齐战役 Q3 实验 2 全覆盖要求）：逐查询对照 18725d07 核对是否记 history_reads/其他副作用——已知 `query_stock_history` 记（在树，不动）；`query_market_history_for` 等新面是否记读经历须逐个核对，不许凭印象。
- **分页一致性**：旧 `as_of_receipt`/`before_receipt` 游标语义（读己之写快照）在新读面上重立，契约测试对照旧实现。

### B2 验收判据

1. 语义等价：对照 18725d07 的 session 级契约测试重立基线（分钟归一含竞价分钟 565/900、分页游标、availability 四态 Traded/NoTrades/Closed/BeforeStart/NotEnded、settled_through）。
2. ts_rs ~20 功能线类型重生成（不 checkout G10——覆盖新架构生成物是事故）；web `tsc -b` 错误集不扩大（基线 32）。
3. save/restore roundtrip：active 分钟K、retained 日快照、receipt 日志三面。
4. 发布版本滞后显式失败路径（照 `collect_resting_facts` 同式，`drive.rs:1219-1224`）。
5. **性能门**：B 探针 300/1000 tick 净机窗配对测（基准独占全机）；desk 扩展劣化超阈值→负结果走归因流程（CLAUDE.md 性能闭环），不空口关闭。
6. wasm 编译 + 串行退化发布语义不破。
7. 副作用核对清单全项落档。

### B2 风险

| 风险 | 对策 |
|---|---|
| desk 热路径扩展回退 1000x 主线 | 桶更新并入既有 volume/turnover 循环（不加第二遍遍历）；基准门禁前置 |
| `FeeComponents` 现居 `session/pipeline/conservation.rs:70`（旧管线） | 与批 9 排序交互：B2 内先裁决 import 路径（搬家到中立位置 or 依赖逃生门），登记 |
| receipts 数据源二选一（DeskReport 事实 vs DeskTradeFact 扩字段） | 设计门内定，配等价性测试（费用逐位） |
| 与 slice1 取回组件的显示口径 | W3 接线时统一验收 |

---

## 3. 各批明细（范围/前置/验收/风险/档位）

### W2 — web K线周期+均线接线（档位 S）

- **范围**：`KlinePeriodSelector.tsx`+`kline-periods.ts` 取回；`utils/turnover.ts` 取回（manifest 已登记，slice1 勘误）；`calendar-candles.ts`/`ChartPeriodTabs.tsx` main diff 应用（**裁决**：basis 参数去留——main 版删 basis 换季/年K+tradeStats，HEAD 版有 basis；倾向 main 版语义=五档周期+成交额聚合，basis 若有消费者需保留双参数）；`useMovingAverageSettings.ts`+`indicator-source-policy.ts` 取回+`chart-settings-slice`/`store.ts`/`PriceChart.tsx`/`MarketKlinePanel.tsx` diff 应用。
- **前置**：无。
- **验收**：取回/接线文件相关测试失败=0；既有失败集不扩大；tsc 错误集不变；季K/年K聚合对照旧 main fixture 黄金值。
- **风险**：低。basis 裁决是唯一决策点。
- **交互点**：无引擎交互（数据面 HEAD `wire-values.ts` 已解析 trade_stats）。

### B2 — 引擎查询 API（档位 L，含 Sol 设计门）

见 §2 专节。

### W3 — web 查询面接线（档位 M）

- **范围**：G2 分钟K面板+host 层（`host/market-history.ts` 78 行/`current-minute-history.ts` 34 行）；G6 `host/personal-trade-history.ts`（65 行）+`private-history-query.ts`+面板；G8 保留历史面板+model+schema；G5 数据面接 TradeConfirmationTable（批次 1 已取回）；`host/intraday-average.ts`（96 行）；`engine-host.ts` main diff（视图类型族）+`LocalRefreshViews.tsx`/`MobileStockDetail.tsx` 接线；ADR-0041 对齐（`engine-host.ts` 视图类型按 Coded\* 词表——MarketHistory 系走六位 code 串不受影响，逐点核对）。
- **前置**：B2 落地+类型重生成。
- **验收**：四面板出数；分页/游标交互测试；ADR-0041 词表核对记录；批次 1 组件（TradeConfirmationTable）首次出数验证。
- **风险**：低（组件层已就绪）；公共文件 diff 冲突面与 B3 串行规避。
- **交互点**：纯 web；消费 B2 发布读面导出的 ts_rs 类型。

### B3 — 远程登录（档位 M-L）

- **范围**：`apps/server` 12 文件（identity_routes/identity/archives 路由）+`packages/native-store` 5 文件+web 14 文件（RemoteLoginScreen/remote-auth/credential-store/remote-membership 等+7 test）；引擎侧 **membership 状态重建**（`session/memberships.rs` 已灭，`MarketMembership*` 类型随 G10 重生成）；`remote-membership.ts` 对齐 Coded\* 词表（ADR-0041 明确影响面）；公司配置耦合裁剪（seed-draft 的 `parseCompanySystemConfig` 依赖降级为可选——远程登录本质只需 `parseSessionSeed`+setup）；`App.tsx` diff（RemoteLoginScreen+ArchiveManager 挂载）。
- **前置**：无硬依赖（server 入口已统一 runtime `step_frame`——本计划 §0 已核实）；建议在 W3 后（公共 diff 串行）。
- **验收**：远程登录/登出/会话恢复 e2e；token 头契约测试（server 域契约测试取回重立）；归档路由读写 roundtrip；旧 main 格式存档**显式拒绝**行为保持（schema/root.ts:51-55，slice1 复核建议 1——不放宽校验；如需读旧档先做迁移工具，另行裁决）。
- **风险**：native-store 跨平台编译（desktop/Tauri 面）；runtime 分叉下远程会话 checkpoint/fork_for_verification 等价性；中风险。
- **交互点**：server actor 每 tick `step_frame`+broadcast EngineUpdate（既有）；归档走日结 save_candidate（civil session 在树）；不碰发布协议新增面。

### B4a — 公司纯域闭包回归（档位 L，机械可分包）

- **范围**：`packages/engine/src/company/` 域文件+`accounting/`+`calendar` 扩展+纯域测试 16,040 行（coupling §4 表：账务/分录 ~8,500、税务 ~3,200、公司操作域 ~8,800、Simple 周期/装配 ~3,900——含 `ex_reference_price.rs` 520 行纯函数）；`lib.rs`/company mod 声明接线；`account.rs`（`AccountKind::IssuerRepurchase` 等 349 行 diff）+`orderbook.rs` serde 助手 diff（732 行 diff 中的受理域/serde 部分**只取 serde 助手**，受理域扩展留 B4c/d）+`money.rs`（94 行）小步恢复。
- **前置**：无（不碰 runtime；旧并行符号零引用已证实，不被旧管线拖住）。
- **验收**：纯域测试全绿（新立基线计数）；零 runtime 引用（grep 门禁）；`tsc`/wasm 不受影响。
- **风险**：低；体量是唯一成本——可分包并行（多 worktree 取回不同子模块），机械任务可评估 Haiku 分包（ROLE.md 规则）。
- **交互点**：无（构造期前纯域）。

### B4b — SessionSetup/save/构造期装配+spine 公司状态（档位 M）

- **范围**：`SessionSetup` 公司字段族恢复+三态校验链；save schema 扩展（`SaveState` 携带 `company_system`/`corporate_actions` 的 ts_rs 导出面）；`company_assembly::assemble_companies`+`assemble_corporate_foundation`（名册构建+法定事实 bind-once+回购专户开立）挂到 **WorldSpec 构造前 session 侧串行阶段**（`accounts_projection_from_session` 之前，coupling 映射 #11 近直接复用）；spine 公司状态字段进新 session state（#7 位置等价——`end_civil_day` seam 已在）。
- **前置**：B4a。
- **验收**：含公司配置的新局构造+save/restore roundtrip 公司字段逐位；无公司配置的旧局零行为变化（回归对照）。
- **风险**：save 面与 W3/B3 已扩展 schema 的合并冲突——串行落地。
- **交互点**：WorldSpec 构造前串行段（pre-spawn 无并发）；save 面。

### B4c — 日终 12 步链嵌入（过渡态）+desk 除权基准（档位 L，Sol 复核门）

- **范围**：`session/corporate_actions.rs`（4,430 行）编排循环改写为嵌入 `session.end_civil_day` 的 12 步链（状态容器形状可搬，"中央 `&mut accounts` 直改"暂以 **force_full_accounts 全量回写过渡窗口**兜底——`drive.rs:1194` 日结后置位、下一 step 全名册同步，coupling §5 步骤 3 明示此过渡态成立）；名册推进消费 `DayEndCandidate.groups`/`snapshot_accounts` 只读面（#3 等价已建）；desk 除权基准价消息**新建**（映射 #5：`DeskMsg` 增除权日基准消息或 Start 附带覆盖；`ex_reference_price.rs` 纯函数直搬；消费侧涨跌停基准=`StockPublic.daily_upper/lower_limit` 路径核对）；回购执行器放单走 `ExternalPlan`（#4 近等价，`broadcast_external_plan` 已翻转玩家意图同通道）。
- **前置**：B4b。
- **验收**：12 步全序日终黄金用例（对照 18725d07 公司域测试重立）；现金/股份守恒逐位；`day_end` 双侧回滚编排下公司步骤的失败回滚等价（`rollback_day_end` 扩展覆盖）；除权日涨跌停基准用例（分红/送转/配股除权价）。
- **风险**：**本战役最高语义风险**——12 步顺序语义+回滚窗口+过渡态性能（全量回写每日一次，可接受但要测量）。
- **交互点**：日结静点（run_day_end_public→end_civil_day 内嵌）；组端 ExternalPlan（回购放单）；desk 消息面（除权基准，新建）。

### B4d — AdminRight 扩展+rights_computer 接线+回购专户+认购命令通道（档位 XL，Sol 设计门）

- **范围**：`AdminRight` 3 变体→扩展（配股缴款**现金+股双腿**、碎股、回购户结算）；`WorldSpec.rights_computer` 接公司日终计算（`bridge.rs:111` None→实接）；组端可逆日志落账消费 `DayEndDirective.rights`；**回购专户归属设计**（映射 #9 二选一：专户入组名册走 ExternalPlan+AdminRight vs spine 影像户+日终对账——需独立小设计文档+对照测量后拍板）；审批/认购族命令（动玩家现金的 ~7 个 pub 方法）通道重设计（意图受理域 ExternalPlan 或日终权益，逐命令分类）；`corporate_actions.rs` 落账尾巴从"直改"切换到 AdminRight 批次（撤过渡态）。
- **前置**：B4c（过渡态跑通正确性后做终态）。
- **验收**：权益落账守恒逐位；Abort 回滚等价（tick 起终快照哈希一致——R2 锚）；日结 wall 不劣化（权益批次并行度测量）；过渡态↔终态行为等价对照（同种子同结果）。
- **风险**：新通道设计错误会污染组端可逆日志语义；**设计门必须先行**（对照实验数据拍板，不许空口）。
- **交互点**：组端 DayEnd 权益通道（rights_computer→DayEndDirective→组可逆日志）；ExternalPlan（认购命令若走意图）。

### B4e — 公司查询/提案/披露协议面（引擎，档位 M）

- **范围**：host 驱动的查询族（`account_dividend_tax_status`/`company_capabilities`/`owner_rights_offerings`/`query_public_reports` 扩展等 ~30 查询方法）在新 session views/civil 协议重建；`player_proposals` 提案面（命令面——若引入统一 HostCommand 信封需对齐战役 Q4 候选设计）；`disclosures.run_simple_day_end` 与新 `run_day_end(DayEndDisclosureCtx)` 融合（#8 需融合：Simple ctx 在新 ctx 上扩分支）；`Event::CompanyDisclosurePublished` 出口免建（事件枚举在树）。
- **前置**：B4b（查询只读 spine）；动账命令部分依赖 B4d。
- **验收**：三宿主契约矩阵重立（main 侧基线证据在 `agents/company-system/`）；披露 seq 统一编号连续性。
- **交互点**：发布协议只读面（查询走 spine 只读+发布版本）；日结静点（披露融合）；命令面（提案）。

### B4f — web G9+G10 重生成+server 公司契约（档位 L）

- **范围**：web G9 ~80 文件（components/company 18、schema 24 增量、fixtures 14、host 公司视图 8、输入件 8——基座 `save/schema/company/` 在树，冲突面小）；G10 公司 ~88 生成类型**重生成不取回**；`engine-host.ts` 公司查询视图+App 路由接线；server 公司域契约测试。
- **前置**：B4e。
- **验收**：公司 web 面出数；契约测试绿；tsc 基线不扩大。
- **交互点**：纯 web/server 消费面。

### B5 — 收尾（档位 S-M）

- **范围**：重集成后引擎+web 测试基线重立计数（**不得沿用 1661/99/99**——manifest 警告+slice1 勘误双确认）；`docs/trading-rules.md` 公司行为简化项/不支持项登记更新；roadmap 更新；涉及 ADR（0037-0040 实现映射、0043 若 B2/B4d 触及读面/权益语义）同步。
- **验收**：文档-代码-测试三面一致；独立 subagent 终审。

---

## 4. 与现架构交互点总表

| 批次 | 发布协议只读面 | 组端通道 | 日结静点 |
|---|---|---|---|
| W2 | —（纯前端聚合） | — | — |
| B2 | `StockPublic` 扩分钟K桶；查询=publication 薄读者；滞后显式失败 | —（receipts 走 driver 事实，若改组端方案则涉组） | 日结冻结当日 bars 进 retained 窗；对齐 `clear_runtime_minute_history`（drive.rs:1190） |
| W3 | 消费 ts_rs 类型 | — | — |
| B3 | 走既有 TickFrame/EngineUpdate | — | save_candidate/归档路由（日结候选） |
| B4a | — | — | — |
| B4b | —（WorldSpec 构造前串行段） | — | save schema 扩展 |
| B4c | desk 消息面新增（除权基准——发布协议外新消息，非读面） | ExternalPlan（回购放单） | 12 步链嵌入 end_civil_day；force_full_accounts 过渡窗口 |
| B4d | — | **DayEndDirective.rights+rights_computer 接线**；AdminRight 扩变体；组可逆日志 | 权益批次=日终权益通道；Abort 回滚等价 |
| B4e | 查询走 spine 只读+发布版本 | 提案命令面（HostCommand 类候选） | disclosures `run_day_end` ctx 融合 |
| B4f | — | — | — |

## 5. Sol 复核节奏建议

**所有批次**一律走 CLAUDE.md 强制独立 subagent 复核门禁（未实施者审完整 diff，答三问）。在此之上，以下批次加**Sol 专属终裁门**（架构级决策，前置设计文档+数据）：

| 门 | 批次 | 终裁内容 |
|---|---|---|
| Sol 门 1 | B2 开工前 | 方案 A/B/C 拍板（本计划推荐 B+事实派生混合；含 receipts 数据源、retained 窗归属两子决策） |
| Sol 门 2 | B4c 开工前 | 日终 12 步链嵌入+过渡态兜底方案+回滚编排扩展 |
| Sol 门 3 | B4d 开工前 | AdminRight 变体集+回购专户归属（对照实验数据拍板）+认购命令通道分类 |
| Sol 门 4（可选） | B4e | 若引入统一 HostCommand 信封（对齐战役 Q4 候选 a/b），需终裁 |

**性能基准门**（基准独占全机，其余 agent 暂停）：B2（desk 热路径）、B4c（全量回写过渡）、B4d（权益批次并行度）。

## 6. 跨批次风险登记

| # | 风险 | 对策 |
|---|---|---|
| 1 | 1000x 性能主线互斥（B2/B4c/B4d 触热路径） | 三批各配基准门；负结果先归因（gpt-6-astra high 交叉验证）再决策；基准期全机独占 |
| 2 | 基线口径漂移 | 1661/99/99 均过时；统一口径=「取回文件相关失败=0+既有失败集不扩大」+B5 重立计数 |
| 3 | 批 9（旧管线退役）交互 | `FeeComponents`（pipeline/conservation.rs:70）、`PendingNpcBatch` incidental 4 处——B2 与批 9 排序协调，B2 内先裁决 FeeComponents 归置 |
| 4 | ts_rs 重生成纪律 | G10 全部重生成不 checkout；每批涉及类型变更即重跑导出 |
| 5 | 旧档格式拒绝 | 远程登录读旧 main 档需迁移工具（另行裁决），不放宽 schema 校验（slice1 复核建议 1） |
| 6 | 公共文件 diff 冲突（App.tsx/engine-host.ts/store） | W3→B3→B4f 串行；每批显式 pathspec 提交（禁 git add -A） |
| 7 | 并行纪律 | 主工作区同一时间一个实现 agent；B4a 分包各在自己 worktree；机械子任务可评估 Haiku（ROLE.md） |
| 8 | 大 A 语义门禁 | 每批复核三问（语义依据/最小范围/边界测试）；公司行为规则先查交易所/结算现行规则再实现，简化项入 trading-rules.md |

## 7. 估时档位定义（相对档位，非绝对天数）

- **S**：单实现 agent 一个落地批（取回+接线+测试）。
- **M**：需 2-3 个提交批或含一个小决策点。
- **L**：需设计输入或多个提交批+契约测试重立。
- **XL**：战役级——独立设计文档+对照实验数据+Sol 终裁+多批落地。

---

## 附：本计划的证据锚点

- 耦合映射 #1-#13：`agents/main-line-reintegration/reint-company-coupling.md` §3
- web 三桶与引擎查询网：`agents/main-line-reintegration/reint-web-features.md` §0/§2.3
- 批次 1 先例与勘误：`agents/main-line-reintegration/slice1-review-record.md`
- 必留清单 R1-R5+Q1-Q5：`agents/std-thread-migration/plans/view-campaign-design-brief.md` §2/§3
- 发布协议形态：`packages/engine/src/runtime/publication.rs`（本文行号 2026-10-19 核对）
- 日结编排/回写窗口：`packages/engine/src/runtime/drive.rs:1164-1196`
- 分钟读面现状：`runtime/desk.rs:90/2304`、`drive.rs:2220-2229`、`session/failure.rs:1901/2160`
- 事件/事实形态：`runtime/event_bridge.rs:25-55`、`runtime/protocol.rs:855-878`
