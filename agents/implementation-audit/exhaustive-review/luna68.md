# Luna68：Pipeline continuous/core/projections 实施记录全文复核

基线按父任务指定：产品 `08e4fc7`（当前 checkout `a7c7ce3` 为 merge 同产品）。只读核对三个指定实施记录及当前 engine 生产调用链；只新增本报告，不改产品或 Git，不运行测试/长验收。未重新核验外部交易所规则；交易语义依据仅沿用正式 ADR 与既有规则记录，不声称本轮重新认证。

## 全文 EOF 与章矩阵

三份原文均逐行连续读至 EOF，行数合计 121。矩阵记录原文条款范围、当前实际 owner/caller 和复核结论。

| 原文文件/行 | 章节或承诺 | 当前代码 owner、生产 caller、边界复核 |
|---|---|---|
| `pipeline/continuous/implementation.md:1–3`（44行） | 标题、日期、范围及动作组合 | 范围是 A01、N01、R2-N04/N06 及可选 E03。没有将测试 helper `fill_receipts`重复计作另一项生产迁移。 |
| `continuous/implementation.md:5–13` | A01/E03/N01/N06/N04 方法表 | `continuous_matching.rs:250` 由真实 `process_continuous_stock_step_inner` 构造并运行 `ContinuousStockRoundProcessor`；`:1016` 内部真实调用单 Place `ContinuousFillReceiptProjection`，`:1035` 的同名外层 helper 明确 `cfg(test)`。生命周期路径由 `project_continuous_retail_lifecycle` 到 `order_lifecycle_events`/`ContinuousLifecycleEventBatch` 后再写 session。coordinator `incremental_continuous_stock_shadow.rs:265–271` 并行逐股调用 consuming `apply_round`，`:349–379` 在 route 排空后 consuming `finish_for_tick`。连续日终 caller `continuous_tick_finalizer.rs:212–228` 先 LifecycleProjection 再共同 `TradingDayEndTransition`。生产连接均存在。 |
| `continuous/implementation.md:15–25` | ADR、失败边界、首错、DayEnd顺序、跨组 state/getter | coordinator `:174–188` 的 round error 标记整体失败；worker `:384–400` 消费 market/ledger，错误无局部恢复入口；finish `:353–373` 拒绝失败 coordinator、每股 finish、核对事实数。day-end `:457–474` 在 `end_of_day()` 清簿前冻结 last/depth，随后创建 DayEnd release。finalizer `continuous_tick_finalizer.rs:287–340` 按 causal→parent→NPC→retail→checked event index 顺序处理，局部写入仍处于可丢弃 candidate。实现没有增加重试、P4 释放回补密封 P1 等旧文未承诺行为。Money/qty和现行 ADR 语义延续；文档没有提供新的官方规则依据。 |
| `continuous/implementation.md:27–40` | 新增/既有短 filters 与诊断 filters | 对应源码测试仍可检索，例如 `continuous_matching_tests.rs`、`continuous_lifecycle_projection.rs`、`continuous_tick_finalizer.rs`、`incremental_continuous_stock_shadow_tests.rs`。只能证明测试源码存在，不能将本轮静态复核写作测试通过。既有 reports 的测试运行证据也不作为本轮重跑。 |
| `continuous/implementation.md:42–44` | rustfmt/diff-check及待 root 验收 | 属实施时状态。后续独立 continuous review 与 summary 已记录验证流程；因此旧的“待 root”不是当前实现遗漏，但本轮不复述为本轮运行证据。 |
| `pipeline/core-implementation.md:1–3`（17行） | 标题、范围与状态 | 明确对应 PlanChainFactConsumption、NpcOrderLifecycleBook、ExpiryOutput、state/getter 迁移；当时编译/测试与审查待 root，不可单凭该记录判当前缺失。 |
| `core-implementation.md:5` | 四类消费集合、prepare/commit时机 | `adaptive_plan_chain.rs` 中四类身份集合和 prepare 增量承载操作/receipt/candidate/sealed identity；prepare 路径按 identity 后 payload 检查，receipt 检查晚于 facts。实际 coordinator 在完整 market、parent、计划投影与同步成功后 `commit_round`，失败增量不被写为已消费事实。与先前 `reaudit-pipeline-contracts.md` 的阶段事务核对相符。 |
| `core-implementation.md:7–9` | NPC lifecycle 借原 Vec；release累计局部失败边界 | `quote_expiry.rs` 的 lifecycle manager 借用 session 原 Vec，注册/移除由 `session.rs` caller 接入，DayEnd clear 由日终 caller 接入；不是新序列化副本/索引。ExpiryOutput 先更新 checked aggregate 再 append，维持记录所述溢出局部状态，不扩张为新原子承诺。正式旧 review 已更正 duplicate lifecycle panic 是旧 invariant，不能归为本重构新问题。 |
| `core-implementation.md:11–17` | getter/state测试迁移、filters、静态验证、大A依据 | caller 和现有测试迁移能在当前 source 定位；默认及诊断 feature 的编译运行属于父级验证记录。本轮不执行。文件再次声明沿 ADR-0017/0018，不改制度，故未要求凭本 OOP receiver 迁移重查外部 A 股制度。 |
| `pipeline/projections/implementation.md:1–3`（60行） | 标题、范围状态 | 四动作及其实现完成不等于验收完成；该记录直说测试与独立复核待 root。 |
| `projections/implementation.md:5–14` | A01/N11/N05/N13 定义、依据、单位/T+1/P1 | 对照当前 `retail_projection.rs`、`decision_snapshot_capture.rs`，对象是投影/capture内的临时状态。无真实 Settlement/T+1解锁由经验临时 `Position` 代行，也没有把 SelfView replaceable cash 当 P1 预算。历史 ADR 已决定局部受理边界；本轮未自行改成交易优先级语义。 |
| `projections/implementation.md:16–29` | owner/caller矩阵及 API/state/容器迁移 | `project_institutional_receipts` 把 moment 绑定为 `ExperienceUpdateMode::InstitutionalFacts(moment)`（`retail_projection.rs:328–338`），`AccountFillProjection::apply_order` 消费它（`:529`）；retail 分支仍调用 Retail writer（`:519`）。真实 receipt 投影经 settlement pipeline 消费（先前审计定位 `account_settlement.rs`），capture 在 `decision_snapshot_capture.rs:171–205` 真实调用。Captured观察是本轮临时输入/输出，没有存档契约变动。candle/scheduler/state 移动已按实际 owner 表核，不依赖只搜 `session` receiver。 |
| `projections/implementation.md:31–38` | characterization测试和 Settlement既有测试 | 断言保留/测试源码可查不代表本轮执行。报告诚实地区分“先写 characterization”与“观察到新增用例 red”；没有伪称 red/green。实际费用、Settlement仍由权威账务路径负责。 |
| `projections/implementation.md:40–50` | 精准 filters | 对应投影与 capture 测试，以及 institutional fee/settlement case 在源码存在。命令过滤器是验证提示，不是功能 owner，也不能由其存在推出长验收通过。 |
| `projections/implementation.md:52–60` | 精确静态检查、EOF清单、待核对 | 静态格式/差异检查是原记录证据。本报告独立完成三篇实施记录 EOF 检查，不冒称重新执行 cargo，也不把后续 review 当本轮执行。 |

## 实际代码摘录与语义核对

以下摘录直接核对生产调用与失败/时序承诺：

```rust
// continuous_matching.rs
ContinuousStockRoundProcessor::new(input, next_trade_event_index, prior_ledger)?.run()

// continuous_tick_finalizer.rs
ContinuousDayEndLifecycleProjection::new(session, &transaction.receipts, day_end_event_base)
    .apply()?;
TradingDayEndTransition::new(session).apply()?;

// retail_projection.rs
enum ExperienceUpdateMode {
    Retail,
    InstitutionalFacts(crate::experience::ExperienceMoment),
}
```

生产调用把 receiver 接进原入口；日终投影与共同交易日转换依序调用；机构 moment 随模式绑定，Retail 仍走自己的旧 writer。`CapturedExperienceObservation::capture` 先更新候选 experience、形成 `self_positions`，`consume` 先 build SelfView 再构造 risk（`decision_snapshot_capture.rs:171–201,288–370,373–422`），保持原文所述错误先后与观察同源。`SelfViewCashReservations::available_cash` 是 `raw_cash - reserved + replaceable`（`:430–445`）；阶段/window 决定是否可替换（`:457–505`），仅用于 SelfView观察。该量没有通往 P1 reservation snapshot 的写入调用。`AccountFillProjection::finish` 延后保存 final-position mismatch（`retail_projection.rs:623–645`），caller先收集各账户即时 Result，再检查 deferred 错误（`:405–430`），与记录中的优先级相符。

## 旧结论复核

- `agents/implementation-audit/reaudit-pipeline-contracts.md` 已沿旧到新 caller 核对连续 P4、DayEnd、结算和候选提交。`exhaustive-review/sweep69.md` 逐项覆盖本次三篇实施记录 121 行，列明精确生产 caller、测试源码位置和旧状态语句的后续证据；本轮检查目标文件 EOF 和当前源码后，没有发现该结论与指定产品基线冲突。
- `exhaustive-review/sweep70.md` 复核 continuous core 独立 review 中唯一漏 caller 已修（`pipeline/mod.rs` 的两个 clear caller 现在使用 state）；因此不能把历史 review 找到的、已修复问题重新报作当前缺口。
- `sweep69/70` 对 N11 两阶段错误优先、N05 SelfView/risk 时序、N13 P1 与观察现金分工的描述，与当前实现一致。既有 review 提到的 G08（Retail dated/decay旧领域缺口）未被这些 receiver迁移修复；这是已有总账项，本报告不重复登记为本批新缺陷。
- 旧 validation 章节的“待 root 执行”描述已被后续验证记录覆盖其历史任务状态；本轮没有运行测试，不能把历史执行结果重述为本轮结果。

## 新候选与反证

| 候选说法 | 反证 / 判定 |
|---|---|
| A01、N01、N04/N06、N11/N05/N13 只有新 struct，没有产品调用 | 每项均可沿上表定位生产入口、消费点和候选/Settlement边界；排除悬空 helper。唯一 `fill_receipts` free helper 限于 `cfg(test)`，实际处理在 Processor 的方法调用中。 |
| consuming stock shadow 报错后必须保留并允许局部重试 | 原实施记录明确失败丢弃整个 tick candidate；当前 `apply_round` 会将 coordinator 标为 failed，`finish_for_tick` 拒绝 failed。添加重试会改变既定失败语义，不是遗漏。 |
| Continuous DayEnd 的生命周期事件有生成，但没跑真实交易日转换 | 当前 finalizer 在 lifecycle projection 后直接调用共同 `TradingDayEndTransition`；逐股 finish 在清簿前冻结 depth、产生 releases，再清簿。生产链接存在。 |
| replaceable reservation 应加进 P1 可交易预算；未加就是少钱 | 它是 SelfView对允许取消报价的观察现金；真实 P1 budget 来自独立资源封存/live reservations。观察不代表撤单执行，不能据此认定遗漏。 |
| capture 的临时 Position 造出不受 T+1 约束的真实持仓 | 临时 position/running positions 仅供经验记录，`PositionView.sellable_qty` 使用权威 `Position::sellable`；没有它触发真实结算或 T+1 解锁的调用。 |
| 三篇记录未运行测试，所以实现尚未完成 | 测试运行是验收证据问题，与 owner/caller 实现缺失不同。旧记录后续有父级验证状态；本轮不执行，也不据此宣称本轮通过。 |
| institution moment 模式已经修复所有账户 experience 日期语义 | Retail 分支仍使用 `record_fill_with_order`，仅 institution 分支绑定显式 moment。不得扩张该迁移声明去核销已有 G08。 |

结论：针对这三篇实施记录全文 EOF 及所承诺的 Pipeline/Projection 资源、经历、结束和受理生产链，未确认新的实现缺失，也未发现交易优先级、资金单位、T+1或日界语义漂移。结论限当前静态源代码和指定范围；无本轮测试/构建结果，无新的官方 A 股规则核验。
