# L03：公司信息与 NPC 意图计划独立生产链复核

## 基线、范围与阅读记录

- 目标产品源码基线：`08e4fc7`（`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`）。该 worktree 的 HEAD 为 `a7c7ce3`；`git diff --stat 08e4fc7 HEAD` 仅见十份 `agents/implementation-audit/` 文档差异，产品源码无差异。结论针对指定源码基线，不把审计文档变更算入产品行为。
- 已全文阅读根 `AGENTS.md` 123 行、`docs/principles.md` 92 行；正式计划 `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` 700 行（分段覆盖 1–700 至 EOF）；镜像计划 `.omo/plans/company-information-npc-intentions.md` 713 行（分段覆盖 1–713 至 EOF）；ADR-0016 119 行；ADR-0023/0024/0025/0026 全文；既有 `sweep03.md`、`reaudit-engine.md` 和会计契约复核。
- 只读跟踪生产 caller、owner、consumer 与存档面；没有改产品、做 Git 写操作或运行测试/构建/长验收。本文行号以当前 worktree 的产品文件和正式计划为准。符号存在、测试代码或计划 `[x]` 均不等于运行通过。

## 决策优先级复核

- 正式计划 K1–K7 位于正式计划:69–180，42 项任务位于:267–658，F1–F4 位于:664–679，DoD 位于:690–700。ADR-0016:97–117 明确计划契约取代早期候选范围和推进顺序。
- ADR-0023:15–27 将开局前历史定为虚拟日 K、首个游戏交易日起撮合生成行情；C06 真实市场校准为不适用。任务 38 的多 seed、敏感性、撮合和量额对账仍适用，不能宣称真实市场统计拟真。
- ADR-0024:20–31 撤销投资者资金循环要求；公司商业收付、商业借款、利息、税费仍须独立记账，不向证券账户注资、不承诺持续成交。
- ADR-0025:12–33 改定只在成功自然日日结生成持久存档，不持久化日内委托/冻结 envelope；跨日计划、历史成交、经历和 RNG 仍是恢复事实。它覆盖计划任务 27/30–32 中任何日内存档暗示，不覆盖 session 内存快照或原子失败回滚。
- ADR-0026:9–18、41–63 固定机构失败/压力反馈及暂停只买不强卖；真实失败衰减和经历只改信心，不改预测/估值。机构这条较新实现不能抵散户 G08。
- ADR-0019/0021 及正式计划:155–180 已撤统一留现金比例、个股上限及数量配额契约；镜像计划仍有旧文字（如 `.omo/plans/...`:约 145–190），不得按旧镜像要求恢复限额或将其列遗漏。

## K 契约 caller 矩阵

| 契约与计划原文 | 当前生产 caller → owner → consumer | 复核结论 |
|---|---|---|
| K1，正式计划:69–80；任务 4/5/28/34/37 | `GameSession::end_civil_day`（`session.rs:2073–2128`）→ `CivilClock`、`CompanyOperationsClockWiring` → 日终事件/存档/宿主 | 公历与自然日主链存在；跨宿主完整复放未执行。未知假日必须保持模拟标签，见旧 G15/Q03，不据静态读码宣称官方日历覆盖无误。 |
| K2，:82–93；任务 6–8/14/27/28 | `end_civil_day_after_session_check` (`session.rs:2100–2124`) → `CompanyOperations`、各行业 Books/Journal → 报表、Disclosure、严格 restore | 会计内核与现金拒绝主链存在；支付失败日报未进入持久业务状态/公告，见 L03-N1。日常工商闭环仍缺 G35。 |
| K3，:95–110；任务 8–13/41 | `publish_scheduled` (`session/disclosures.rs:155–201`) → ClosingEngine/四行业报表与 consolidation → PublicLibrary/查询 DTO | 四行业纯账套/报表能力存在；生产定期披露硬设 Standalone，合并生产调用缺失 G28。自定义非工商 CompanySpec 的会话闭环缺 G36。 |
| K4，:112–123；任务 14–16/25/28/33/36 | `end_civil_day_after_session_check` → 日经营/公告 → accepted institution root 的 `record_acquisition` (`roots.rs:344–395`) | 公开信息与本人获知有明确分层；经营失败材料被丢弃 N1；淡出后的历史 belief 仍会继续获知/决策 N3。延付独立机制仍待澄清 Q1。 |
| K5，:125–136；任务 17–20/25 | Retail：`npc_decisions` → `ZiNoiseStrategy::decide_with_experience`；Institution：accepted attention → `InstitutionDecisionRoot::observe` (`roots.rs:306–476`) | 机构五路 profile 链与散户行为链分离，散户财报分析接线缺 G07；散户日期衰减缺 G08；价格记忆上限已有 owner 但生产未调 prune，见 L03-N2。 |
| K5a，:138–158；任务 18/22/23/26 | 年报获取 → `BeliefBook::apply_cause` → `fundamental/valuation.rs` → `assess_candidates`/lifecycle | 现金流 DCF 与中期材料更新缺口仍为 G06/G09；更正/违约 cause 原语有而 root 无分发（Q11）。机构个人代价经历更新按 ADR-0026 已接，不得误说修改预测。 |
| K6，:160–170；任务 21–25 | root candidates → lifecycle/PlanBook → `allocate_child_quote_budgets` → parent order / 真实路由 receipts | 计划/实际 fill/预算/执行链存在；分类在生产请求全是 ExistingPlan，G38；遗留 belief 绕开 watchlist 使淡出无效，N3。T+1、费用、合法订单路由未发现本批变更。 |
| K7，:172–180；任务 27/29–35/37/40 | 日终候选 → persistence；公开查询 → EngineHost/页面；debug trace → collector | 新格式和日终保存依 ADR-0025 收窄；公共查询/三个宿主基础存在。DEV trace 订单事件实际传空，G37；完整制品、UI与三宿主证据未执行。 |

## 42 项任务 caller 对照

“有入口”只表示找到了生产机制，不代表任务验收通过；被现行 ADR 取代的范围明确标出。

| 任务 | 当前生产入口 / 消费方 | 独立判断 |
|---|---|---|
| 1 | `baseline-run.mjs` → fixture/release 示例 | 工具存在；原 before 证据不回写，本轮未运行。 |
| 2 | `docs/company-accounting.md`、policy fixture → 经营/报表配置 | 策略矩阵存在；逐条官方原文和有效期证据不能由静态调用链替代。 |
| 3 | `session/pipeline/`、`session/decision_chain/` → tick 与 root | 职责接缝存在；旧全局逐字回放语义按现行接收顺序契约复核，不将模块移动视为缺失。 |
| 4 | `SessionSetup` → `CalendarPolicy` → `CivilClock`/存档 | 主链存在，G15/Q03 仍需分别审查。 |
| 5 | `end_civil_day` → civil clock/经营/披露 | 主链存在；日报告被忽略是 N1，而不是整个日结未接。 |
| 6 | 行业过账 → `Books::post_batch` → Ledger/Reports | 原子批次及 checked amount 存在；不扩大成任意跨子账操作的统一事务保证。 |
| 7 | `session/company_assembly.rs` → CompanyRegistry/operations → Session | 默认上市公司装配固定 Industrial（:331–345）；四行业配置/开局主线不等于通用自定义会话接通，G36。 |
| 8 | `operations/industrial.rs:43–179` → 工商账套 | 现金不足可捕获为日报告；该日报告上游丢弃 N1；折旧、所得税、商业债务付款的日/期末链缺 G35。 |
| 9 | `operations/bank.rs` → BankBooks/报表 | 银行内核存在；自定义会话闭环列 G36。 |
| 10 | `operations/insurance.rs` → InsuranceBooks/报表 | 保险内核存在；自定义会话闭环列 G36。 |
| 11 | `operations/real_estate.rs` → RealEstateBooks/报表 | 地产内核存在；自定义会话闭环列 G36。 |
| 12 | `consolidation::consolidate` → worksheet/report set | 合并纯函数存在；生产披露未请求合并 scope G28；往来申报超过两方实际账面余额可通过等额配对，见 G28 补充边界。 |
| 13 | `ClosingEngine`/`ReportSet` → PublicLibrary | 五表产物存在；G28 的生产范围、G36 自定义行业接线不能被模块存在核销。 |
| 14 | `CompanyOperations::advance_civil_day` → `CompanyDayReport` | 四行业日循环存在；报告被 session 忽略 N1；工商期末事项缺 G35；ShockKind 不适用分支见 G36。 |
| 15 | `DisclosureDispatch::run_day_end` (`disclosures.rs:99–151`) → PublicLibrary | 定期及新 shock 公告存在；支付失败无公告输入 N1；集团 scope 缺 G28。 |
| 16 | `roots.rs:344–395` → `NpcInformationState::record_acquisition` → ObservationContext | 机构本人的报告获知存在；G09 中期信息不进估值更新，不能用“已获知”核销。 |
| 17 | StrategyFactory → AnalysisProfile/BeliefBook → institution root | 工厂/profile 存在；Retail root 不消费 G07。 |
| 18 | root → AnnualFacts/valuation → belief estimate | 三估值方法存在；G06 数学错误、G09 年报限制、Q11 cause 分发仍在。 |
| 19 | root → `PersonalPriceMemory::observe_price`/technical signals | 记忆观察及信号存在；历史读取 Q02、条目修剪 N2。 |
| 20 | Retail/Institutional settlement 与 observations → experience | 机构 ADR-0026 已接且限定只改信心；Retail `observe_position`/fill 写入旧入口，日期衰减 G08。 |
| 21 | decision lifecycle → TradingPlan/PlanBook | 状态机和实际 fill 事实存在；跨日日终仅订单失效遵循 K6/ADR-0025。 |
| 22 | `decision_chain.rs:968–979,1232–1243` → allocator grants | allocator 能排序类别；生产输入全 ExistingPlan，G38；不恢复现金留底。 |
| 23 | `decision_chain/urgency.rs` → quote policy / route | 紧迫度与保护报价存在；ADR-0026 机构压力只暂停买入，不强制卖出。 |
| 24 | PlanBook/parent lifecycle → order route/receipts | 实际提交/撤单/fill 有接线；日内委托持久化要求由 ADR-0025 取代。 |
| 25 | accepted attention → watchlist/`root_candidate_codes` | watchlist 自身修剪存在；belief keys 仍会恢复淡出股票候选 N3，个人价格记忆另有 N2。 |
| 26 | GameSession → company assembly/decision roots → matcher | 共同 V 路径已删；零售分析 G07、G08、G09、G36 等局部断链仍在。 |
| 27 | 日终 save candidate → strict persistence/restore | 新格式存在；只保存日终事实，活动委托/冻结不进存档（ADR-0025），个人计划保留。 |
| 28 | `company_scenarios` → GameSession/ledger/orderbook | 受控场景入口存在；本轮未运行，源码测试不证明所有场景/金额对账通过。 |
| 29 | `company/query.rs` → Rust DTO/TS bindings | 公共查询基础存在；页游/报告数据的实际精度及跨端 E2E 未运行。 |
| 30 | `web-wasm` bridge → Worker/company coordinator | 查询桥存在；真实 WASM 集成与迟到响应边界未运行。 |
| 31 | Server routes/actor → PublicLibrary | 查询/鉴权入口存在；HTTP/WS 与故障恢复验收未运行。 |
| 32 | Tauri IPC/actor → report query | 查询入口存在；真实 IPC/应用启动验收未运行。 |
| 33 | host event stream → `company-slice`/query coordinator | 公共更新层存在；无交易披露、旧 generation 等 E2E 未运行。 |
| 34 | CompanyPanel/StartDateInput → EngineHost | UI 组件存在；四行业生产数据受 G28/G36 上游限制，视觉矩阵未运行。 |
| 35 | `record_plan_root_diagnostics` → trace collector/inspector | collector 存在；订单 events 传 `&[]`（`decision_chain.rs:753–770`），预算字段由 terminal plan 派生（:812–817），G37。 |
| 36 | pipeline/root → causal diagnostics | 指标入口存在；G37 订单关联缺失；诊断输出/零样本证据未运行。 |
| 37 | WASM/Server/Tauri adapter → host parity | 当前验收入口未运行；旧“自由调度下整局字节完全相同”已收窄，余下频率/恢复/真实宿主证据仍未核销。 |
| 38 | baseline runner → after/sensitivity report | ADR-0023 核销 C06 外部数据条件；10 seed 比较、跨年与参数敏感性仍是有效验收。 |
| 39 | scale/restore probes → serialized state/resource report | 测试入口存在；规模/长局实际证据未运行。 |
| 40 | release artifacts → runtime capability probes | 发布隔离工具入口存在；真正制品探测/行为一致未运行。 |
| 41 | 文档/ADR/工作状态 → 实现和证据链接 | `.omo` 镜像对限额/留底等旧契约漂移应依正式计划和 ADR 修正；正式当前状态待证据同步。 |
| 42 | acceptance runner → suite output/source identity/manifest | 完整批次未执行；不得用旧密封证据、短测或 `[x]` 声明 DoD 完成。 |

## 旧缺口复核：G06–G09、G28、G35–G38

- **G06：成立。** `valuation.rs:129–177` 的 DCF 循环按年更新 FCFE，但 `:159–165` 每年的 present value 都只除一次同一折现因子；应为随年数的折现。终值单独折现五次 (`:166–170`)。这不是无调用的死代码：个人估值 consumer 由 `roots.rs:407–476` 组装到 candidate assessment。错误影响方法数值；没有运行金样。
- **G07：成立。** Institution 决策 root 用 BeliefBook/Profile 五路评分 (`roots.rs:400–476`)；生产零售仍从 session NPC pipeline 调 `ZiNoiseStrategy::decide_with_experience`，该消费不是 institution belief chain。`AnalysisProfile` 有测试/工厂，不代表散户获得财报驱动。需保留身份/基本面能力解耦契约，但不能给所有零售强加同一种模型。
- **G08：成立（散户部分）。** `decision_snapshot_capture.rs:345–348` 仍调用 `observe_position`（只给 market minute）；真实零售成交在 `retail_projection.rs:519–528` 调 `record_fill_with_order`。日期版 `observe_position_dated` 在 `experience/feedback/lifecycle.rs:335` 有定义，但搜索到的生产 consumer 不含它；`failure_influence` 也仅在 `experience/feedback/inputs.rs` API 层可用。机构的真实失败、衰减及信心已由 ADR-0026/`roots.rs:464–465` 消费，不应再报机构缺失或把失败衰减重复应用。
- **G09：成立。** `roots.rs:344–397` 只把 Annual 加进 `new_annual_reports`；:430–447 唯一调用 `BeliefCause::NewMaterial`。`fundamental/update.rs:91–105,242–260` `ensure_annual_for` 拒绝非 Annual，facts 也是年度提取。季报/半年报能公开并被获知，但不更新盈利/估值预测。
- **G28：成立，并有独立金额边界。** `disclosures.rs:155–200` 对每个公司生成 `ScopeId::Standalone`。没有相应集团 report registration/publication。纯 consolidation 位于 `accounting/consolidation/mod.rs:84–113`，不改变该生产 caller。额外检查 `eliminate.rs:84–105,107–139`：申报双方只要成员/科目合法且金额彼此相等即可形成抵销，未与各自实际科目余额比对；不得将纯函数的等额约束误说为抵销上界验证。
- **G35：成立。** `operations/industrial.rs:43–179` 日常流程含减值迹象、生产、赊销、补货、管理费、坏账准备；没见日常折旧、所得税估算/支付或商业贷款本金/利息偿付。利息应计已存在：`operations/dispatch.rs:33–56`；工商 AR 到期则在 `dispatch.rs:76–85` 无条件 collect。`session.rs:2182–2232` 期末结账不能替代未调度的折旧/税/偿债事件。准确结论为“工商期间业务不完整”，不是“完全没有会计/利息”。
- **G36：成立（生产会话闭环）。** `company_assembly.rs:331–345` 所有上市证券都构造成 Industrial；通用行业 books/flow 值对象存在但这不构成自定义四行业 SessionSetup/close/disclose/query 闭环。其 `operations/config.rs:36–45` `books_mut` 对非工商 `unreachable!`，当前默认装配前置条件下不单列为默认 panic。另 `events.rs:12–20,209–236` 将某些 shock 适用范围写为工业/地产，但 `operations/day.rs:135–145` 对所有公司抽样激活、没有以 kind 过滤，`disclosures.rs:112–124` 会对 active shock 生成公告；银行/保险可能公布无效的生产中断/资产减值信息。不能把这解释为股票交易错误。
- **G37：成立。** writer 可提取订单 ID，但唯一生产调用 `decision_chain.rs:764–770` 将 `&[]` 传入，因而 trace 没有真实 order IDs。:812–817 的 budget_constraints 只从 terminal plans 的 status 得出，不消费实际 `AllocationConstraint`。这是诊断信息缺接，不影响下单/成交行为，不表示 release 隔离本身缺失。
- **G38：成立。** `decision_chain.rs:968–979,1232–1243` 的生产 AllocationRequest 都写 `class: ExistingPlan`；仓内 allocator 的 `NewOpportunity` 排序分支不会在生产请求中出现。它影响 K6 新机会/续行的分配优先语义，不等于账户之间应共享预算。`AllocationExperience::default()` 不是单独缺陷：机构经历已上游消费，重新在 allocator 惩罚可能双重计算。

## 新候选及反证

### L03-N1：支付失败业务事实未落入存档/公开风险链（成立）

- 原文：K2:92 要求“资金不足生成 `PaymentFailed/Overdue` 业务状态和公开风险材料”；任务 8:338、14:387 要日常付款失败形成可说明状态。ADR-0024:28–31 仅核销投资者现金循环，不取消公司付款事实。
- Caller/owner：`session.rs:2105–2109` 调 `CompanyOperationsClockWiring::run_day_end`，返回 `CompanyDayReport`；工商处理器在 `operations/industrial.rs:99–105,146–170` 把若干现金不足记入 `payment_failures`；`operations/day.rs:205–220` 形成日报告。
- 断点/consumer：`:2105` 的返回值被直接丢弃；`run_day_end` 虽把报告返回到 session，但 `CivilDayEndReport` 不接收该类型。持久 `OperatingCompany` (`operations/core.rs:19–28`) 没有 failure 状态面；`DisclosureDispatch` (`disclosures.rs:107–124`) 只从 active shock 发公告，没有 payment failure consumer。
- 影响与最小边界：现金不透支、业务日继续是对的，缺的是失败/逾期开项在成功日结后留存并能形成公开风险材料。不要把全部 payment engine 判为未实现，也不要用外部资金补充处理。建议受控日结→保存/恢复→公开查询测试，断言失败信息持续可追溯且公司/投资者现金、成交量不凭空变化。此处没有运行测试。

### L03-N2：个人价格记忆 prune 没有生产 consumer（成立）

- 原文：K5:135、任务 19/25 要个人“持仓 + 8 个未持仓”记忆上限和淡出。ADR-0019 撤销的是世界容量配额，不是个人行为上限。
- Caller/owner：机构 root 对 candidates 调 `PersonalPriceMemory::observe_price`（`roots.rs:321–337`）；owner 的 `prune` 在 `experience/price_memory.rs:182–198`，保护集合为持仓与活动计划。
- 断点/反证：`roots.rs:481–486` 只组 `protected` 并调用 `watchlist.prune`，没有调用 `personal.price_memory.prune`。`rg` 中价格记忆 prune 的其它调用只有测试。保存会保留完整 `PlanPersonalState`，所以生产无限候选时，非持仓且无计划记忆不会按 8 个上限淘汰。
- 验证建议：自定义多股会话逐一实际观察并退出，保护持仓及活跃计划后验证非保护条目至多 8 个；存档恢复后重复断言。这个上限是功能语义，不是撤销的全局数量配额。

### L03-N3：淡出 watchlist 后旧 belief 仍能成为新观察/交易候选（成立）

- 原文：K6:165 限定候选为本人持仓/关注/本轮发现集合；任务 25:490–496 要计划结束后股票可淡出。保留历史信息/信念不代表每轮自动重看或可重开计划。
- Caller/owner：`roots.rs:322` 调 `root_candidate_codes`；定义 `decision_chain.rs:299–310` 将 held、所有 `belief.entry_stocks()`、active plan、discovered 无条件并集。随后 `roots.rs:323–337` 写观察价、:344–395 获取新材料、:468–476 再分析并推送 lifecycle。
- 断点/consumer：`watchlist.prune` 在同一 root 尾部只改变 Watchlist；旧 belief entry 不删，也不在下轮候选入口受 watchlist 限制。lifecycle 可按合法方向重新建 plan。于是已淡出、无持仓/活跃计划且未重新发现的股票仍持续观察、读公告、能新建计划。
- 最小范围/验证：保留已知报告、belief 和历史信息，只过滤本轮候选资格。测试应断言淡出前史仍在，但重发现前不产生新 observation/acquisition/plan；重新发现后恢复资格。此项与 N2 是不同的状态 consumer。

### 未升级为确认缺口：Q1 工商客户延付语义待澄清

K4:116 列出“客户延付/信用恶化”。当前事件只有 `CreditDeterioration`（`company/events.rs:209–224`），工商将其加到坏账率（`operations/state.rs:63`、`industrial.rs:174–176`）；到期应收在 `dispatch.rs:76–85` 自动全额收回。形式文档把保险/地产信用冲击列为只记录，但没有清楚说明工商客户延付是独立状态还是信用恶化的替代抽象。故只保留 Q1 澄清，不把风险准备金说成“完全未做”，也不在当前审查擅加业务机制。

## F / DoD 追踪

| 验收项 | 生产 caller 核对 | 结论 |
|---|---|---|
| F1，正式计划:664–667 | 任务矩阵 1–42；K2/K3/K5/K6 caller 如上 | 全文静态核对发现 G06–09/G28/G35–38 与 N1–N3；缺口未修复，不能批准完整合规。 |
| F2，:668–671 | 会计 journal/行业流程/price route/valuation/disclosure | 未重查交易所官方材料，不对合法性来源背书；当前发现属于实现语义/完整性而非新增交易制度。须由法源审查结合 `trading-rules.md` 完成。 |
| F3，:672–675 | WASM、HTTP/WS、Tauri IPC、CompanyPanel/StartDateInput、release trace | 仅静态定位，未运行；三宿主实旅、视觉/键盘、错误态和制品探针仍是待验收项。 |
| F4，:676–679 | 日终 save、静态类别映射、报告/资金链 | 未见需新增股票、派钱、真实行情导入或旧档迁移的理由；不把 ADR-0023/24/25 已核销内容重开。 |
| DoD 1，:692 | K3/G28/G36 | 行业内核有，集团公开报告和自定义四行业生产闭环不足。 |
| DoD 2，:693 | K1 日历/时钟/close | 主干存在；边界与三个宿主证据未运行。 |
| DoD 3，:694 | K5/G06–09、N2/N3、Q02/Q11 | 不足：DCF、散户能力/日期、个人候选淡出、中期报告更新及主动读取边界。 |
| DoD 4，:695 | K6/G38/N3 | 状态机与真实执行存在；预算分类与淡出候选边界未兑现。 |
| DoD 5，:696 | K2/ADR-0024 | 公司/投资者现金隔离原则仍在；N1 仅缺失败事实留存/披露，不应以补钱弥补。 |
| DoD 6，:697 | K7/ADR-0025 | 严格日终存档为当前契约；历史状态/计划保存与会话原子恢复仍需正式验收。 |
| DoD 7，:698 | K7/G28/G36/G37 | 公共查询与调试隔离基础有；公开范围、非工商链、真实 trace 订单及宿主验收不足。 |
| DoD 8，:699 | 任务 1/28/36–42 | 多 seed、四行业长局、scale、真实制品证据本轮未跑，不能据此判通过。 |
| DoD 9，:700 | 任务 1–42 / F1–F4 | 全任务实施/独立复核/证据门槛未满足；历史任务勾选不等于此基线完成。 |

## 结论

在指定源码上，旧 G06–09/G28/G35–38 经生产 caller 复核仍成立（其中 G08 只针对散户；G36 是会话生产闭环问题，不把非工商 `unreachable!` 超出默认前置条件判成必发 panic）。新增确认候选为 N1 经营付款失败状态/风险材料丢失、N2 个人价格记忆 prune 无生产调用、N3 淡出 watchlist 后旧 belief 继续驱动新观察/获知/计划。工商客户延付仅列 Q1 待澄清。没有将此静态复核当作测试或官方 A 股规则复核；在有效发现修复和独立复核之前，不应宣称计划交付完成。
