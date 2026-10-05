# 共同现金分红基础状态机实施记录

## 范围与依据

- 新增 Engine 纯状态机 `company::cash_dividend`，只依赖显式分红方案、`ShareRegistry` 的 `RegistrationSnapshot`、`Money`、`TradingCalendar` 与调用方付款结果。
- `simple` 的账面现金不是付款预算；方案中的 `distributable_amount` 是上层完成亏损弥补、法定公积等校验后提供的授权额度，不证明实际现金，也不在本模块派生。未引入 `SyntheticFunding`。
- `approved_on` 表示股东会利润分配决议日；《公司法》（2023 年修订，2024-07-01 施行）第212条用于检验计划 `payable_on` 不超过六个月。实际晚到付款事实仍记录，并在 receipt 明确标记 `within_six_month_deadline=false`，不因逾期把已发生事实拒绝或吞掉。
- 2026-10-05 的 `dividend-research.md` 记录沪深交易规则（2026-07-06 施行）规定登记日次一交易日除息。本模块要求调用方显式提供 `CalendarExchange` 和 `TradingCalendar`，校验登记日及下一交易日；未推断市场、自然日偏移或行情参考价。
- 规则依据来源及未取得的中国结算派息细则见 `agents/remaining-questions-and-features/dividend-research.md`、`cash-dividend-implementation-preparation.md`。此记录不是额外的官方规则研究，也不替代正式 `docs/trading-rules.md` 更新。

## 已实现边界

- `CashDividendPlan` 显式保存计划 ID、发行人、证券、交易所、股东会决议日、公告日、登记日、除息日、支付日、税前每股金额及上层授权分配额度；拒绝空身份、非正每股金额、负授权额度、无序方案日期及计划支付超过六个月。
- 日期关系 `approved_on <= announced_on <= registered_on < ex_dividend_on <= payable_on` 是本方案输入约束；只有登记日和除息日关系由对应交易所日历校验，日期顺序不被描述为完整法定时序。
- 登记只接受匹配计划 ID、发行人、证券和日期且通过 `ShareRegistry` 校验的快照；快照冻结为不可变事实，按 holder 汇总 shares 与整数分 gross entitlement，并排除 `IssuerTreasury`。仅发行人自持的快照显式拒绝，不建立零应付的伪成功状态。
- 当前金额基础仅覆盖 `Money` 能表示的整数分每股输入，不进行小于一分的舍入或分配；适用的子分金额/登记结算尾差规则未由中国结算材料证实，必须由上层先解决。模块不生成税务 Profile、税额或扣缴结果。
- 支付由调用方提供覆盖每个未清 holder 的明确 `Paid` 或带原因 `Failed` 结果。记录 API 不转移公司或账户现金；真实入账、公司现金与应付冲减、外部 holder 到账以及税务账需要上层在同一候选事务中接线。部分成功保留应付，已成功 holder 不会被重复付款；失败可用新 payment ID 重试，历史 receipts 保留，当前 failure 摘要按 holder 更新。
- 状态恢复严格校验 entitlement、payment receipts、金额、失败原因、顺序及状态重放。JSON paid/failure 使用序列而非 enum map key。反序列化后须由调用方再调用 `validate_with_calendar`，以提供本局实际日历；状态机不从日期推断或静默选用默认日历。
- 相同 announcement、registration、payable 与 payment event 重放具幂等校验；不同输入复用 payment ID、重复 holder、漏报 holder、金额错配、非显式失败原因和回退的 payment 日期均显式拒绝。实际逾期到账是可恢复的违规事实标记，不宣称合规。

## TDD 与验证证据

- `cash-dividend-first.log` 的旧 7 case 结果为 6 pass / 1 fail；失败来自额度用例误复用 `plan-1` snapshot，产生身份不匹配，属于无效 fixture 红测。已将 fixture 改为 `plan-low`，保留额度超限断言，不把该失败报告为需求红测。
- `cash-retry-red.log` 实际记录连续两次外部付款失败后 `CashDividendBook::validate` 拒绝自身状态：10 case 中 9 pass / 1 fail。修复为按 holder 更新 unresolved failure 摘要，同时保留两条历史 receipt；再补充 Treasury-only、付款日期顺序、阶段后重放与显式日历恢复边界。
- 最终 `cash-final-green.log` 记录 14/14 定向测试通过，耗时 0.02 秒；`build-cash-final.log` 记录当前 Engine library 测试编译成功。命令由 root 使用进程外 deadline 监督；本轮未运行完整回归。
- 两个当前 Rust 文件通过 `rustfmt --check`；未提交，由 root 统一提交。

## 尚待接线

- `CompanySystem` / `SimpleFundamentals` / `CompanySimulation` 尚未创建真实公司决议或将利润、上层授权额度连接到账套；不从账面现金制造付款能力。
- 投资者账户资金流水、外部支付 endpoint、公司应付与现金凭证、错误 UI、失败补付事务及持久化 session 尚未接通；本模块 receipt 是调用方提交的付款结果事实，不是实际入账凭证。
- `CashDividendTaxBook` 的纳税人身份、税 lot 与延期税事实未接线；不得按 NPC 策略猜税务 Profile。
- 市场 ex-reference-price、交易日历策略在存档中的选择及除息行情锚尚未接线；本模块只校验给定日历下登记日的次一交易日。
- 发行人完整股东初始名册、非流通外部收款 endpoint、开局股份取得日期及子分金额/尾差业务规则仍须上层显式提供或拒绝支持。
