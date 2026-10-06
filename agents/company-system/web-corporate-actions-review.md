# Web 公司行为存档独立复核

复核范围：`apps/web/src/save/schema/corporate-actions.ts`、`root.ts`、`save-snapshot.ts`、`company/ex-reference-price.ts`、当前存档 fixture 与 `corporate-actions-schema.test.ts`；并交叉核对 Engine Session 公司行为、股东名册、市场恢复约束。未改实现、未运行测试。本记录基于当前共享工作树；Engine 与 Web 文件仍可能由各自 owner 同时更新。

## 依据与语义边界

- 阅读 `AGENTS.md`、`docs/principles.md`、ADR-0035、Q14、`docs/company-actions-design.md` 及现有股东结算实施计划。Q14/ADR 是产品和架构约束，不是 A 股法源；现有公司行为设计记录公司法第 210、212 条的分配和付款期限依据，并把红利税明确留作未配置。此 Web 增量没有自行推导新的交易制度。
- `ShareRegistry` 全额股本守恒、逐自然日登记、公开市场日净变化与冻结登记快照的方向一致；Session dividend payment 的账户 gross 与 `TreatmentNotConfigured` 必须逐项保留，不应将 gross 当税后净额。
- Web 对 u64 股数、i128 日净变更、严格未知字段、nullable registration、FIFO 处置反向重放、支付回执重放、账户／External 收款凭证勾稽和除息 pending 的正价/最小价位/日期/昨收约束已有相当完整的校验。未发现这些主路径中的明显精度或静默默认问题。
- 按委托只做跨层检查、未改 `FinanceParser`：当前 Engine `SimpleFinanceState` 的 `dividends` 与 required nullable `legal_facts` 均在 Web `parseSimpleFinanceState` 中显式纳入 exact key 和返回对象，且注册资本、Journal 声明/付款来源及余额有勾稽；本项没有观察到 FinanceParser 丢弃这两个新增状态字段。

## 已修发现

- 分红方案 `exchange` 已与 setup 股票交易所绑定，并有错配拒绝测试。
- 名册内 registration `event_id` 已要求唯一，并有重复身份拒绝测试。

## 最新静态结论

- 前轮“旧 MarketSnap 锚点 + 更新 applied group 可通过”的 finding 已修复。parser 按证券选最新组，并要求 MarketSnap 锚点的日期及参考价均精确一致；锚点反向仍要求有对应组。测试同时包含同一双登记计划 fixture 的旧锚点拒绝与最新锚点成功解析正例。
- 首轮两个 finding 也已闭合：分红方案交易所绑定 setup 股票，名册内 registration event id 唯一；均有拒绝用例。`applied_ex_dividend_groups` 现有五个必需数组都各有缺字段测试。
- 新增 `validateCompanySystemSession` 对实际财务分红付款日期不晚于 Session 日期的校验，root 调用其时传入当前日期；新测试通过完整 Session validator 拒绝未来付款。它补的是恢复事实边界，不改变付款规则。
- **新增来源事实复核：** `issuer_repurchase_account` 在 registry 与 immutable RegistrationSnapshot 都是 required nullable；有值时 exact 校验专户标识、source evidence、CivilDate，要求身份/evidence 非空且 `established_on <=` 各自适用的 settled/registration date。每个历史快照的事实与当前 registry 历史按设立日推导出来的 nullable 值精确相等，因此设立前快照必须 null，设立日及之后快照必须完整携带同一事实；检测篡改/追溯回填。与 Engine `ShareRegistry::validate`、`set_issuer_repurchase_account` 及快照校验方向相符。来源记录明确表示“当前登记事实，不替代真实回购成交或过户流水证明”，Web 没有把它扩大成成交/库存股余额证据。
- 新增用例覆盖 registry 和 snapshot 缺字段拒绝、有效的设立前 null/设立后 Some 正例、篡改历史快照拒绝、设立日晚于 registry settled date 拒绝和空身份拒绝。未见遗漏关键适用边界。
- 完整核读 `corporate-actions.ts` 中 HolderId、source、lot/u64、日变化/i128、FIFO disposal replay、registration snapshots、CashDividendBook entitlement 和 payments replay、gross receipts、tax status、applied ex-dividend groups，以及 root／MarketSnap／fixture／相关测试。没有发现剩余静态阻断问题。
- 最新增量静态复核通过；所有先前 Web parser findings 均已关闭。

## 验收边界

- 其余 u64/i128、FIFO 处置反向重放、支付状态重放、tax 未配置和除息价正值/tick/日期/pending 昨收的校验静态看与 Engine 契约一致。`day_market_activity` 的“交易日且 day tick 非零”最终校验在 Engine restore；Web parser 不伪造默认值。
- `web-cross-domain-signed-final.log` 时间晚于最终测试源码，亲读为 40/40 通过：公司 schema 10、corporate-actions 9、company system 14、Money/wire 7；包含最终“五个显式数组”测试和 latest-anchor 双计划正反例。Root 另跑 `full-engine-gold-strict.log`，两份未删字段 Engine 完整存档均通过 Web `parseStrictSaveEnvelope` 并深度相等：`current-schema-save.json`、`current-closed-day-save.json`。
- 正式 ts-rs 绑定生成由 `typegen-review-fix.log` 记录，135/135 绑定测试通过；生成的 `SaveMarketSnap.ts` 含三个除息 required 字段，`SaveSlot.ts` 含 `corporate_actions` typed import。未手改 generated 文件。`workspace-check-final.log` 的全部 workspace targets 与默认 features 构建成功，只有已有/非阻断 compiler warnings。
- 历史 `.tmp/company-system/session-actions/web-cross-domain-final.log` 早于最终 test source，不作为最终证据；现以时间更新的 `web-cross-domain-signed-final.log` 为准。此前实施者 9/9、14/14、TSC 报告也由此 root final 批次补充证据。
- 本次 issuer repurchase source-fact 增量：实施者报告 red 10 cases 中 6 fail（registry exact 拒绝字段、nullable missing case 原先未拒绝等）；最新 corporate-actions 短测 11/11。实施者报告 TypeScript 检查目前有 3 处其它并行改动诊断：`host/public-report-normalize.ts:95` 两处 `unknown`→string、`save/schema/company/reports.ts:72` 的 `string`→`ReportRoeUnavailable` union；这些文件不在本 source-fact diff。没有 source-fact 增量专属的落盘绿测日志供本轮亲读，我未重跑。前序完整跨域 40/40、ts-rs 135/135、full Engine gold strict 2/2 仍是先前契约状态的证据，不把它们冒充成此次增量后的 TSC 结果。
- 本轮静态结论包含 `issuer_repurchase_account` source-fact 增量；未自行运行验证、未改实现、测试、Git index 或生成类型。本轮未新增 A 股交易算法。结论：**Web 公司行为存档 parser 独立复核 PASS（静态、限定范围）**。本增量的 11/11 为实施者报告，TSC 当前共享工作树仍有上述跨任务诊断；不因此声称该全局检查通过。该结论不扩展为完整股本/公司行为功能完成。
