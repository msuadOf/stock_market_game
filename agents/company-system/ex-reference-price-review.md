# 除息参考价模块独立复核

复核日期：2026-10-06。复核人未参与实现。范围仅含 `packages/engine/src/company/ex_reference_price.rs`、同目录 `ex_reference_price_tests.rs` 与 `company/mod.rs` 对该模块的导出；不涵盖其他并行改动。本轮仅静态审查，未运行 Cargo 或回归，动态验收结论待实现者提供 fresh 短测真实日志后补记。

## 领域依据与范围

已完整读取 `agents/remaining-questions-and-features/cash-dividend-implementation-preparation.md` 和 `dividend-research.md`。并核对 `.tmp/corporate-actions-research/sse26-docx.txt` 第 4.3.1—4.3.3 条、`.tmp/corporate-actions-research/szse26-pdf.txt` 第 4.4.1—4.4.3 条及 5.2.3 条：两所 2026 年修订规则均规定权益登记日次一交易日除权除息；纯现金、无股份变动时公式化简为前收盘价减现金红利。该参考价用于除息日行情前收盘价/涨跌幅基准。规则允许发行人申请并经交易所同意、公布后调整公式。

当前代码仅为整数分 `Money` 的纯现金公式基础模块，不接 Session/Market、不改历史成交或行情，不处理少于一分每股金额，也不计算经批准的公式调整。此边界不能被表述为完整股息、全市场除息接线或小于一分精度支持。

## 静态检查

- 公式实现使用 `previous_close.sub(pre_tax_cash_per_share)`，与纯现金简化式相符；checked 减法将溢出显式传播为 `Arithmetic`，拒绝非正前收、非正红利及非正参考价。
- 登记日先验证交易日，再由同一显式 `CalendarExchange` 查询 `next_trading_day`；没有把自然日加一误作次交易日。日历越界错误保留为类型化 `Calendar` 错误。
- `Money` 精度为整数分，计算没有引入额外舍入；这符合本模块被限定的整数分基础，不覆盖研究材料指出的不足一分每股股利。
- 当前测试覆盖沪深整数分结果、周五至周一交易日推进、非交易登记日拒绝及结果非正拒绝。尚待作者短测日志；边界覆盖可再检查零/负输入、checked 溢出和日历上界错误。
- 初审 finding 已关闭：实现者新增必传 `CashDividendFormula`（`StandardCashOnly` / `ExchangeApprovedAdjustment`）；后者在计算前返回 `UnsupportedApprovedAdjustment`。函数文档同时限定标准 API 不接收/不代表获批特殊公式，调用方必须先排除该类事件。对应拒绝测试覆盖此语义。当前源码已复核，待短测日志。
- 新增 `docs/trading-rules.md` 公司行为章节：规则条文和施行日期对应已读取的官方原文；明确标准纯现金和市场日历、批准调整时拒绝、不足一分不擅自舍入、非正参考价拒绝、当前未接市场/不代表完整结算。另以 ADR-0035 与 `docs/company-actions-design.md` 核对 `Simple` 展示现金、真实投资者到账/认购/回购边界及未获中国结算指南确认事项，文案与既有决定一致。文档链接目标存在。未发现过度核销或 A 股语义漂移。

## 当前结论

## 最终限定复核

已亲读实施者提供的 `.tmp/company-system/ex-reference-short.log` 原始输出：9 项目标测试通过、0 失败，耗时 0.01 秒。实施记录给出的实际命令为 `node scripts/run-with-deadline.mjs 10000 -- target/debug/deps/engine-eb8b0dcc57b8995e company::ex_reference_price --nocapture`，退出码 0；由 deadline runner 对测试 binary 进程树施加 10,000ms 上限，单 binary 执行。此前失败仅为测试日期 fixture 落在模拟春节假期/预期日期错误，实施记录未将其冒称为需求红测；早期编译失败涉及并行模块，当前限定结论不使用该失败作证。

结论：**限定 PASS**，仅通过 `ex_reference_price` 标准纯现金整数分基础模块的静态语义复核及本次 9 项短测。参考日期只按调用者提供的 `TradingCalendar` 计算；当前默认日历对未有已核实官方覆盖的年份会用模拟假日规则，因此这不是实际未来交易日历完整性证明。该模块未接入真实市场，不支持不足一分/股、获批调整公式计算、股东登记/税务/付款、行情前收或涨跌幅基准，也不代表完整公司分红或公司行为已完成。未运行完整回归。
