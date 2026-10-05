# CashDividendBook 独立复核

## 范围与结论

审查 `packages/engine/src/company/cash_dividend.rs`、其全部 `tests.rs` 与 `company/mod.rs` 导出，依据 `docs/principles.md`、ADR-0035、Q14 §4–5、`docs/company-actions-design.md`、`docs/trading-rules.md`、现金分红及股本行为官方规则研究、现有 `share_registry` 与 `cash_dividend_tax` 契约。以未参与实现者身份复核完整最终源码、测试和文档边界。

**限定范围通过。** 未发现当前纯状态机／导出范围内仍未解决的实现缺陷或 A 股语义错误。结论只覆盖现金分红方案、登记权利冻结和外部付款结果事实记录，不代表公司账簿／真实投资者账户支付、税务或完整公司行为已经接线。

## Findings 与边界

- **法源与日期语义：已闭合。** 《公司法》（2023 修订，2024-07-01 施行）第210、212条以及沪深《交易规则》（2026 修订，2026-07-06 施行）登记日次一交易日除息规则已在 `dividend-research.md` 等材料核验；`docs/trading-rules.md` §公司行为与现金除息边界同步了沪深条款、施行日期和来源。`approved_on` 明确表示股东会分配决议日，计划支付日期受六个月期限校验；真实逾期支付不被拒绝或吞掉，而是保留到账事实并在 receipt 标记超期。日期链 `approved_on <= announced_on <= registered_on < ex_dividend_on <= payable_on` 被明确写为方案输入约束，不冒称全部法定时序。中国结算派息到帐／预划与尾差细则仍未取得正文，本模块没有从除息规则推定这些操作细节。
- **方案额度：职责切分成立。** `distributable_amount` 是上层完成弥补亏损、法定公积等校验后提供的授权额度；模块校验总 entitlement 不超额度，不把它当公司现金或可分配利润证明。Simple 展示现金不作为付款预算，此边界与 ADR-0035/Q14 一致；上层决议、利润核验和账簿凭证尚未接线。
- **付款事实：边界清楚、集成未完成。** `settle` 验证并记录调用方提交的 gross payment outcomes，不自行修改公司或账户现金。实施记录与 `current-handoff.md` 明示真实入账须由上层和此状态在同一事务提交。`Paid` 因此只表示该账簿记录调用方报告的成功结果；后续集成仍须以真实 account/company ledger 凭证为依据，并独立接好税务。不得称为端到端分红完成。
- **登记与税务：无跨层冒充。** 登记日快照冻结完整明确的持有人与股份数量，排除发行人自持，拒绝仅有自持股份而无受益持有人的登记；它仍依赖 `ShareRegistry` 提供真实完整名册。未按 `AccountKind` 猜法定税务身份，也未将个人股息税简化进 gross entitlement。税务批次、持有人全集和市场账户接线尚未完成。
- **精度与除息：范围受限。** 每股金额要求可由 `Money` 表示的整数分，不擅自处理不足一分的尾差。市场登记日／次交易日校验使用调用方明确提供的证券交易所和 `TradingCalendar`；恢复后必须调用 `validate_with_calendar`，日历不从存档日期推断。纯现金除息参考价由独立模块负责，本改动没有更新市场价格锚或改写历史成交。
- **可靠性：原复核缺陷已修复。** 支付 retry 使用规范 holder 顺序；JSON paid/failure 使用序列而非 enum map key；required-nullable 字段缺失会拒绝；恢复会重放 receipt 并核对 entitlement、已付／未解失败摘要、状态和日历；已完成阶段 replay 不回退状态；付款日期不允许倒退。连续失败按 holder 更新 unresolved failure 摘要并保留历史 receipt，空失败原因、重复持有人、漏项、超额错额和 treasury-only 快照均有明确处理。

## 验证

- TDD 证据保留在 `.tmp/company-system/checklist-common/`：早期额度失败来自错误复用 `plan-1` snapshot 的无效 fixture，已改用 `plan-low` 并保留超额断言；`cash-retry-red.log` 的有效红测证明连续两次失败会导致自身状态无法恢复，后由按 holder 更新摘要修复。
- 最终 `build-cash-final.log` 显示 Engine lib test 编译成功（有现存 unused/dead-code 警告）；`cash-final-green.log` 显示当前最终 binary 的 CashDividend 定向用例 14/14 通过、耗时 0.02 秒，由 8 threads 及外部 10 秒 deadline 运行。审查者亲读日志但未自行运行测试或完整回归。
- `git diff --check` 对本次源码、测试与导出无输出。
