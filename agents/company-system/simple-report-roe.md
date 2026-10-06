# Simple 报告期间 ROE 接线记录

日期：2026-10-06。

## 依据与口径

- Q14 第二章第 3、5 节要求按同期间、同范围的利润与平均权益计算 ROE；权益变动须反映时点，平均权益非正时明确不可用。报告期流量按窗口汇总、余额取期末、比率按报告窗口重算；不能累加各期 ROE，也不能把年度金额当作细期间金额。
- 官方法源核验见[公开披露 ROE 法源核验记录](official-roe-research.md)。证监会公告〔2010〕2号规则9第4条的一般公式使用月份加权权益并将普通归母净利润 `NP / 2` 计入分母；权益事项发生月不计变动后权重，次月起至期末计月。该一般公式不等同于 `period_roe` 的自然日时间加权分析指标。
- Simple 当前没有扣非归母净利润事实。因此 `adjusted_roe` 显式为 `Unavailable(MissingNonRecurringIncomeFacts)`，不复制普通净利润、不填零。合并报告当前缺少归母权益事件逐项事实，及未知类别的报告期间权益分录显式返回带原因的 `Unsupported`。

## 接线

- `IncomeStatement.report_period` 从会计窗口 `movement` 生成，独立于原有 `quarter`（当季）及 `cumulative`（年初至今）列。季报窗口等于当季列；半年报/年报窗口等于年初至今列。
- `ReportSet.roe` 只保存派生的精确结果，不增加第二个净利润或权益事实 owner。Simple 报告通过 `Books` 窗口期初归母权益、报告期利润与可分类的权益 journal 事件，调用 `accounting::disclosure_roe` 的同一规则9一般加权公式。
- 当前适配器只将 `CompanyDividendDeclaration` 中确认的负权益净变动分类为减少事项。其余报告期间权益变动不静默忽略，而使 ROE 明确 `Unsupported(UnclassifiedEquityEvent)`。有重述调整的报告标注权益事件历史不可得。
- `ReportSet.validate` 校验报表窗口与种类匹配、报告期间利润表勾稽及归属净利润一致、ROE 有理数十进制格式和正分母、普通 ROE 与归母净利润/平均归母权益的精确交叉乘积，并拒绝缺少扣非事实时的可用扣非 ROE。
- Engine 公开查询从真实 `PublishedReport.reports` 投影 `PublicReportFinancials.roe`、`income.report_period` 与摘要 `report_period_net_income`；查询层不重算 ROE。

## 验证状态

- 先增加 Simple 月报、季报、年报窗口及比率测试和 ROE 篡改/扣非伪造拒绝测试；root 观察到初始 placeholder 下两条真实行为 RED，记录见 `roe-consumer-red-0/1.log`。该阶段编译问题另见 root 工作日志，不能计作行为 RED。
- 新增规则9事件分类短测，覆盖分红发生月次月起权重及未知权益事件不被漏算；Engine 公共财务 JSON 测试核验 report-period 流量和 ROE 原样投影。
- root 提供的最终 Engine 定向日志：`latest-finance-period-green.log` 10/10、`latest-roe-green.log` 3/3、`latest-official-roe.log` 13/13、`latest-consolidated-window-final.log` 10/10、`public-projection-assert-green.log` 5/5。合并回归覆盖 Q2 同窗归母净利及缺少必需同窗归母事实时拒绝；公开投影覆盖新增 DTO 字段与精确序列化形状。初次投影断言误将 serde tagged variant 的 `{reason: ...}` 对象当字符串，已修正为精确字段断言，最终 5/5 通过。
- 原两项独立复核发现均已修复，非作者复核通过，详见 `simple-report-roe-review.md`。以上动态日志由 root 执行/提供，不是复核者运行。
- root 报告最终 TS typegen 140 项与 Web TypeScript 检查通过；最终跨域 gold/存档 fixture 刷新仍由 root 统一处理，当前不宣称 Web/Engine gold、存档或完整跨层链路完备。未运行完整回归。
