# Engine 契约 checkpoint 独立复核

## 范围与依据

复核者未参与本批实现。以 `HEAD` 到当前 worktree（含 staged 与 unstaged）的差异为准，审阅 `packages/engine/Cargo.toml` 及 `packages/engine/src` 除 `company/`、`session/`、`information/` 外的全部变更；另完整阅读范围内未跟踪的 `packages/engine/src/strategy/fundamental/monthly_baseline_tests.rs` 和 `packages/engine/test-support/simple_company.rs`。对有变更的源文件按完整 diff 阅读，没有以局部搜索代替。未编辑产品代码、未运行 Cargo、未检查 index 或宣称产品整体验收。

核对项目根 `AGENTS.md`、`docs/principles.md`、`docs/error-handling.md`、`docs/architecture.md`、`docs/trading-rules.md`、ADR-0031、ADR-0035、ADR-0036；并阅读 `agents/remaining-questions-and-features/q22-shared-host-ingress.md`、`q23-tax-idempotency.md`，以及此前对 CompanySystem Simple core、Web 接缝和 fixture 的独立审查记录。官方 A 股规则基线沿用 `docs/trading-rules.md` 当前记录；本范围没有改变申报、撮合、T+1、费用或证券制度。

## 发现

- **P2：`AccountId` 全 u64 字符串契约已获批准，但正式 ADR 原先未记录。** `agents/shared-market-accounts/engine-memberships.md` §范围与契约记载根因是 MAX `AccountId` map key 与真实金融事实链在序列化／hash 失败，用户已批准为必要根修：经济账户身份独立于控制主体，完整 u64 十进制字符串，无旧格式兼容或迁移。因此不应恢复到 `js_safe_u64`。原问题是 ADR-0031 的“ID 不因字段邻近改变格式”表述与已批准的 `AccountId` 特例之间缺少正式衔接；本 checkpoint 后已在 ADR-0030 明确仅 `AccountId` 的完整 u64 wire 规范，并在 ADR-0031 澄清该独立边界。两份 ADR 的该项更新仍需独立复核。

  独立复核补记：未实施者完整复核两份 ADR 的更新及 `engine-memberships.md` 对应契约，确认范围、规范格式、拒绝规则、无兼容迁移，以及与 ADR-0031 的衔接均一致；未发现有效问题或不必要扩展。

## 其余复核结论

- `Money` 仍按 ADR-0031 保持有符号分；累计成交额改 `u128` 非负规范十进制字符串，计算使用 checked arithmetic，避免合法多笔成交累计超过 `u64` 时截断。`IntradayAverage` 的输入输出单位仍为分与股，没有在 engine 提前舍入。
- 诊断拒绝公开匿名化事件进入要求真实账户事实的内部基线；策略仅过滤非交易证券，没有替代订单簿的权威开闭市校验，也没有改变 A 股同价时间优先或 T+1 规则。
- `PriceMemory` 将证券交易所观察分钟与共享账户接触分钟分开单调校验，避免跨交易所时钟误比较；历史读取仍不能凭空创建个人价格锚点。
- 财务窗口重述按成员、来源和有效期间投影，并保留现金流按实际过账日归属；合并仍检查期间覆盖，账簿投影未写回成员原账。`ClosingEngine` 仍在账簿事务成功后发布版本和重述状态。未发现当前范围内金额精度、现金回溯、原子提交、隐私泄漏或交易规则偏移的其他 P1/P2 问题。
- 新增 `num-bigint` 直接依赖与 ADR-0036 授权用途相符；此前 review 记录仅作为对应实现范围的审查证据，不外推为公司系统、Session 或全产品验收。

## 文件清单

`git diff HEAD` 范围文件清单：

- `packages/engine/Cargo.toml`
- `packages/engine/src/account.rs`
- `packages/engine/src/accounting/closing/mod.rs`
- `packages/engine/src/accounting/consolidation/aggregate.rs`
- `packages/engine/src/accounting/consolidation/eliminate.rs`
- `packages/engine/src/accounting/consolidation/minority.rs`
- `packages/engine/src/accounting/consolidation/mod.rs`
- `packages/engine/src/accounting/consolidation/sale.rs`
- `packages/engine/src/accounting/error.rs`
- `packages/engine/src/accounting/journal.rs`
- `packages/engine/src/accounting/reports/bank.rs`
- `packages/engine/src/accounting/reports/consolidated_window.rs`
- `packages/engine/src/accounting/reports/insurance.rs`
- `packages/engine/src/accounting/reports/mod.rs`
- `packages/engine/src/accounting/reports/real_estate.rs`
- `packages/engine/src/behavior/decision.rs`
- `packages/engine/src/behavior/heuristics.rs`
- `packages/engine/src/calendar.rs`
- `packages/engine/src/calendar/date.rs`
- `packages/engine/src/diagnostics.rs`
- `packages/engine/src/experience/price_memory.rs`
- `packages/engine/src/experience/watchlist.rs`
- `packages/engine/src/intraday_average.rs`
- `packages/engine/src/lib.rs`
- `packages/engine/src/orderbook.rs`
- `packages/engine/src/strategy/fundamental/interim_window_tests.rs`
- `packages/engine/src/strategy/fundamental/mod.rs`
- `packages/engine/src/strategy/fundamental/update.rs`
- `packages/engine/src/strategy/hot.rs`
- `packages/engine/src/strategy/institution.rs`
- `packages/engine/src/strategy/mod.rs`
- `packages/engine/src/strategy/retail.rs`
- `packages/engine/src/strategy/sizing.rs`
- `packages/engine/src/strategy/zi_noise.rs`
- `packages/engine/src/verification_evidence.rs`
- `packages/engine/src/verification_evidence/phase_timing_tests.rs`
- `packages/engine/src/verification_evidence/tests.rs`
- 未跟踪完整源：`packages/engine/src/strategy/fundamental/monthly_baseline_tests.rs`、`packages/engine/test-support/simple_company.rs`

明确排除 `company/`、`session/`、`information/` 的实现复核；其余 sibling work、宿主同步情况、构建、运行时与完整回归均不在本结论内。
