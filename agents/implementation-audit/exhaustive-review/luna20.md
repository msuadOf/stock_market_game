# Luna20：策略实现稿与连续盘口稀疏反馈复核

## 范围与口径

- 基线：产品 `08e4fc7`，当前 `HEAD` 与请求指定的 `a7c7ce3` 相同（完整 SHA `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`）。本工作树除本文件外只有既存的未跟踪审计目录；未写产品代码、未做 Git 操作、未运行测试或长验收。
- 全文 EOF 阅读：`docs/superpowers/specs/2026-06-29-strategy-impl-design.md` 183 行；`docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` 30 行；`.omo/plans/escrow-parallel-engine.md` 362 行。另读根 `AGENTS.md` 与 `docs/principles.md`。
- 判定顺序：当前退役/收尾决定与现行 ADR、当前生产 caller 和实现优先于历史计划中的旧 API/全局调度假设。计划记载的测试数字、性能数字只作为原文证据，不是本轮重跑结果。

## 全文章节矩阵

| 文档章节/行 | 原文主张 | 当前对应实现或裁决 | 复核结论 |
|---|---|---|---|
| strategy design §1–2，1–24 | 首批 ZiNoise/Value/Momentum；多股视图、`Vec<Intent>`、隐藏 V、滚动价格、注入 RNG | 当前 `strategy/mod.rs` 的 `MarketView` 有股票 map、交易 tick/minute 与风险/量价字段；Intent 用 `LimitPrice` 表达固定/符号价；机构已发展为 `BeliefInstitutionStrategy` 与 belief 决策链 | 设计稿是早期 API 基线，不可用旧 `ValueStrategy`/仅三字段 StockView 名字判当前缺失。当前大 A 报价由会话权威校验与市场受理，不由策略快照替代 |
| strategy design §3，25–62 | `StockView`、`MarketView`、`SelfView`、`Intent` 与 `Strategy::decide -> Vec` | `packages/engine/src/strategy/mod.rs:72–153,194–285` 保留多股模型和 Vec Intent；结构已扩展 symbol price、交易时间及策略决策元数据 | 核心多股/Intent 契约保留；字段变化是后续需求演进，不是接口丢失 |
| strategy design §4–5，63–112 | 散户随机报价、机构 V 估值、游资趋势、工厂采样，玩家返回 None | `StrategyFactory::build*` 仍对 Player 返回 `Ok(None)`，NPC 参数非法返回 `Err`（`strategy/factory.rs:15–49` 及各分支）；机构和热钱具有当前策略专属执行与报价链 | 原策略具体算法已演进，不能沿用原文的基础 V/last-price 算法作为现行约束；公开创建边界仍显式失败/无策略 |
| strategy design §6–7，113–137 | 意图可行性由编排层校验；非法参数显式拒绝；策略与撮合/存储分层 | P3 `AccountValidatorDriver` 校验候选，P4 股票 worker 校验盘口/笼子等；`StrategyFactory` 返回 `StrategyError`；生产 Strategy 状态由 `StrategyState` 编解码 | 层边界和错误约定在当前代码仍清楚；不可行意图按业务事实投影处理，不等于策略 API 返回错误 |
| strategy design §8–11，138–183 | serde、三策略边界、非法参数、工厂与 DoD 验收 | `tests/strategy_state.rs` 有完整状态往返、已知参数篡改和未知 wire variant 测试；`state.rs` 每变体验证具体状态；保存 v2 重新构造并核对身份 | 原列出的早期 `strategy.rs` test matrix 不可当作当前 exact suite；代码有现行覆盖源码，但本次不声称测试已运行 |
| sparse plan 当前判断，1–7 | 共享 Rayon，不另造池/请求配额；原整簿按每轮与续行次数复制，单次性能基准 | `incremental_continuous_stock_shadow.rs` worker 从 post-P0 初始化，后续持有同一 shadow；`ContinuousStockProjection` 只带 `MarketDelta`；性能表只作单次报告且明确跨运行留簿不同 | 本轮新增的整簿 clone 已移除；原性能数据不代表当前稳态或稳定加速 |
| sparse plan 代码范围，9–13 | ID 索引；增量变更和已成交事实；候选盘口增量应用；连续/盘前共用，竞价独立，失败不回退复制 | `OrderBook` 同时维护价格时间档与 `live_by_id`（`orderbook.rs`）；`MarketDelta` 携带股票/last price/订单变更及簿序（`market.rs:68,368–401`）；candidate 调 `apply_changed_orders`（`adaptive_plan_chain.rs:695–717`）；错误映射成不变量失败，无整簿 fallback | 与设计吻合。OrderId 用于定位；价格/时间顺序仍为 orderbook 的 `(price, seq)`，没有拿 ID 排序交易 |
| sparse plan 验收/报告，15–30 | 代表性短测、普通/诊断编译、1/8 worker 进程外性能采集；单次结果不能证明稳定收益 | 原文称订单簿、增量轮次、计划链及入口短测和编译已通过；数据明示 NPC/玩家留簿数不同、1 worker 本版比旧本慢、不得宣传稳定提速 | 仅可引用为计划当时记录，不是本轮验证。留簿差异与现行自由并发受理契约相容，不能用该性能样本判语义错误 |
| escrow plan 头部/Scope，1–53 | 旧托管并行目标，9 分歧、单一并行路径、全 tick shadow、测试/文档门禁 | 文档头 3–7 行明确 2026-09-30 核销：固定来源类序及跨 worker 整局相同已被 ADR-0017/0018 取代，旧语料栈删除，Task 9 旧三重门禁不再按原文执行；`work-status.md` 与现行 ADR 是后续状态来源 | 只能用于定位实现背景与守恒/回滚等仍有效要求；不能依 Scope 的旧全局同序、全 artifact 相等或旧语料矩阵重开已退役验收 |
| escrow plan 验证策略与 Execution/runtime，54–136 | 预算、多层验证、反馈 typed outcome、固定预算跨轮、真实 roots、一次 finish/commit、原子回滚 | `continuous_tick_transaction.rs:181–271` 组装生产路径；`ready_stock_stream.rs:52–84` 按 identity 取 P4 typed outcomes 并推进真实 coordinator；candidate shadow 最后 `commit_tick`；明确的连续反馈测试见 `incremental_continuous_stock_shadow_tests.rs` 和现行 plan-chain suites | Runtime 的根计划/预算/反馈约束仍可作为调用链检查表，但旧 source class / canonical 全局类序不成立；看当前实际受理 gate，不推断全局来源优先 |
| escrow plan 四组件/审查/排程，137–174 | A–D 单 owner、独立审查与验证职责、冻结输入/多核资源 | 属历史协作规范，不是生产运行时实现；已由用户要求本轮仅静态审计 | 不把旧代理排程要求转换成实现缺口；当前 task 的限制遵循用户明确范围 |
| escrow plan Todos 1–8，175–252 | ADR/阶段、守恒、账户校验、结算、订单簿、失败/宿主、存档契约 | ADR-0017/0018 与 `packages/engine/src/session/pipeline/` 当前实现已有相应模块；各任务原文可作历史契约索引。保存策略状态见 `session/persistence/v2.rs` | 不逐项重新验收或重跑；旧文中策略 profile/state 语义应由当前 sealed `StrategyState` 约束复核 |
| escrow plan Todos 9–12，253–287 | 原语料等价、K7 94 次、performance、diagnostics、文档核验 | 任务 9/10 在 294–295 行明确 BLOCKED，Task 11/12 是历史 PASS；303–308 行用户明确不迟补 Task 9 历史见证，Task9/10 与 F1–F4 维持未勾选 | 退役/收尾决定优先：不能要求恢复旧工具或把历史 BLOCKED 改成新产品缺口；当前工具本身若仍违背 ADR 的比较契约，应作为独立工具问题重新证明 |
| escrow plan CivilUpdate、F1–F4、commit/success，310–362 | 宿主屏障、终审、流程、最终成功标准 | 324–341 行仍是条件性终审清单；352–362 行是原计划目标，不覆盖 303–308 收尾决定及 Task9/10 阻塞事实 | 原终审不能冒充通过；无新执行证据，本轮不复核其 UI 或性能验收 |

## 连续盘口与反馈调用链

1. 输入股票簿在 `IncrementalContinuousStockCoordinator::from_post_expiry` 一股初始化一次（`incremental_continuous_stock_shadow.rs:112–177`）；每轮 `apply_round` 按 code 分组，仅将有操作的股票 shadow 移交 Rayon worker（`:179–332`）。股票内沿输入操作次序串行处理；worker 结果的股票排序只用于稳定合并/报错，不作跨股交易优先级。
2. `ContinuousStockStep::finish` 在 `continuous_matching.rs:610–619` 由 `original_orders` 对被操作的 incoming/resting ID 形成 delta。`book_state.rs:31–45` 只比较这些身份的 before/after 与成交归属；未扫描并复制全簿。插入、成交部分改量、成交完毕、取消路径各有 ID 索引与 side-level 盘口维护。
3. `adaptive_plan_chain.rs:695–717` 消费每个 stock projection，先通过候选市场当前 last price/last close 校验版本，再 `OrderBook::apply_changes`。应用前逐项校验 before order、终态 ID、filled/live 排斥；错配返回 `ProjectionMismatch` 映射的 fatal。没有按旧整簿 clone 的静默兼容分支。候选收到该轮变更后才由 plan synchronization 投影 typed 事实。
4. `ReadyStockStream::continuous_progress`（`ready_stock_stream.rs:52–84`）从 pending 表按 `candidate_key` 取出对应 typed outcomes，校验股票和 round，计划类结果走 `advance_after_typed_outcomes`，其他事实只投影一次，然后刷新 unfinished route 并发现可续候选。P3 rejection 走 typed outcome 续行；股票 worker 完成结果不可用事件展示顺序猜测。
5. P3 持续资金/股份预算与身份游标留在 tick coordinator/validator，P4 释放不回补预算；调用链最终在 continuous flow 排空后 `stream.finish`、`chain.finish`、一次 `finish_continuous_shards` 和 `finalize_continuous_tick`（`continuous_tick_transaction.rs:225–271`）。candidate commit 才更新权威状态。任务级运行要求具体引用 `.omo/plans/escrow-parallel-engine.md:70–77` 的延续部分，但该计划中的退役来源序不可复用。

## 历史结论复核

### G16

需要区分两个 clone 命题。sparse plan 中的“每次计划反馈复制整只股票簿”已被 `MarketDelta` 路径消除；当前证据支持有界改动完成，不应继续把它计作 live-book clone 缺口。implementation-audit 的 G16 原义却是 `RootReadContext::capture` 仍复制完整历史 `PlanBook`：`roots.rs:5–14,17–29`，尤其 `:25`。该复制与本轮股票盘口 delta 无关，现行代码仍保留，G16 仍成立但只能表述为 PlanBook 所有权/复制目标；未运行测量，不能宣称实际瓶颈或量化成本。依据旧 G16 定义见 `implementation-audit-2026-10-02.md:67,133`、`reaudit-engine.md` 的 G16 段及 `luna08.md:40`。

### G38

当前分配器仍按 `RiskReduction → ExistingPlan → NewOpportunity` 排序（`plans/allocation.rs:115–126`），但生产买/卖计划请求分别在 `decision_chain.rs:968–979`、`:1232–1243` 明写 `AllocationClass::ExistingPlan`；现有新机会优先级没有生产输入。范围仅同账户子单软预算分类，不代表跨账户或跨股票的撮合优先级。卖方生产请求 `fee_reserve=0`，不会从当前请求造成卖方费用现金预算占用；不得借 G38 改写 A 股可卖股/T+1或共享账户资源语义。G38 仍成立；`AllocationExperience::default()` 不单列，避免重复上游经历惩罚。与 `implementation-audit-2026-10-02.md:96`、`luna08.md:41` 一致。

### G39

退役决策使 escrow plan Task 9 的旧三重门禁、旧语料对等和旧来源顺序不再是当前验收义务（`escrow-parallel-engine.md:3–7,294–308`）；不能把其缺失证据升级为产品 bug。G39 的当前工具问题仍有代码反证：`escrow-verification-contracts.mjs:131,160,186` 要求预算/扰动 artifact receipts 与 reference 全等；`run-escrow-verification-matrix.mjs:583–595,728–735` 比较跨 entry artifact vector。自由实际受理的不同运行不固定实际到达轨迹，完整事件/收据/存档字节相等不再是合理总契约。另 `baseline-run.mjs:1277–1287` 对同 seed 的 finalizer rerun 比完整 stdout SHA，`verify-simulation-artifacts.mjs:423–428` 强制两份摘要完全相等；此入口也跑自由 `step()`，需与固定受理事实重放契约区分。sparse plan 报告本身的 1/8 worker NPC/玩家留簿量不同是具体提醒，不证明具体一次运行必失败。

结论：保留/扩展 G39 为当前工具契约债（包含 escrow verification matrix 与 simulation baseline finalizer/verifier 两条入口）的候选是合理的，但不恢复已经退役的 Task 9 原验收、不判引擎交易规则错误，也不删除有效守恒、订单局部价时、同实体因果和失败原子性校验。当前只静态确认代码，不运行矩阵。旧审计初始两个入口见 `luna08.md:42`；遗漏的 baseline finalizer 已在 `sweep33.md:42–50` 提出，当前源码仍有上述 caller。

## StrategyState 与公开 API 拒绝

- 早期设计稿写 `StrategyFactory.build -> Option<Box<dyn Strategy>>`，本质上 player 无策略。当前返回 `Result<Option<Box<dyn ProductionStrategy>>, StrategyError>`；`Player -> Ok(None)`，NPC 构造非法则 Err。收窄 trait object 与可恢复状态要求一致，不是静默排斥合法内建策略。
- `Strategy` 本身仍公开、可由外部实现，`tests/strategy_state.rs` 定义 `UnknownStrategy` 并确认它可作为 `Box<dyn Strategy>` 使用；因此“策略决策 trait 被全面封闭”是错误结论。
- 持久化和 shadow 所需的生产子集有意封闭：`state.rs:6–34,93–137` 的 private sealed registration 只注册 ZiNoise/Momentum/BeliefInstitution；`Account::set_strategy` 要求 `Box<dyn ProductionStrategy>`（`account.rs:207`），外部不能给 session 安装一个无法完整 serialize/restore 的任意 Strategy。compile-fail doctest直接检查伪造 `ProductionStrategy` 无法实现；未知 serde variant、额外字段、非法参数和身份篡改分别有现行测试源码（`tests/strategy_state.rs`、`session/persistence/v2_tests.rs:1155–1177,1284–`）。这是“公开决策 trait 可扩展、可进权威会话的生产策略注册表封闭”的分层契约。
- 当前 `StrategyState` 三个变体是行为状态本体，`profile/family` 从变体派生；从 trait `state()` 转换时校验 state 的 profile/family 与原策略一致，反向转换做各变体参数校验。此结构符合原文“逐字段状态往返、拒绝未知生产变体”的意图。原稿提的独立 Value/Momentum 分类/DriftUp 参数已被后续机构策略发展覆盖，不能据 variant 数量不吻合判缺。

## 新候选、排除项与边界

- **没有发现新的盘口反馈状态丢失候选。** ID delta 覆盖本轮触及订单，应用前后状态检查与 typed result 按 candidate identity 关联；单轮内匹配创建并全额成交、再次撤单的当前测试源可见于 `incremental_continuous_stock_shadow_tests.rs`（sparse plan 当时记录对应短测已运行）。
- **G39 是明确候选复核，不是本轮修复。** 两类 runner 仍把不同自由调度执行作完整 artifact 相等比较；其正确替代应对相同已受理事实重放作确定性断言，并对自由调度各自核对守恒/价时/依赖/失败原子性。没有在本任务修改门禁或运行它们。
- **不升格为新候选：** `OrderId` 用于订单身份索引而非 price-time key；不同股票结果按 code 收集不定义不同股票交易先后；外部自定义 `Strategy` 不能进入 production `Account` 是封闭存档注册表的预期门禁，而 trait 仍开放；竞价独立阶段及账户预算释放规则不由连续 delta 改写。
- 本文件为静态复核交付。历史数字/测试记录未独立重跑，也未重新取证交易所规则或声称性能收益。
