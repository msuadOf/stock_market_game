# Simple 报告期间 ROE 独立复核

日期：2026-10-06。复核者未参与实现。范围限于本轮 ROE Engine 接线和必要的 ReportSet fixture；公告 owner 的并行 diff 不在本次语义审查范围内。

## 依据与结论

- 已阅读 `docs/principles.md`、Q14 财务模型第二章，以及 `official-roe-research.md`。证监会公告〔2010〕2号规则9第4条一般公式按报告月数加权、变动次月起计权，并在平均权益分母纳入普通归母净利润 `NP / 2`；同一控制合并及比较期间另有特殊处理。
- 公式 helper 的月份算法、精确整数有理数、负/零平均权益不可用、同一控制/比较期间拒绝，与已记录法源一致。归母利润/归母权益范围在合并资料不完整时显式标 Unsupported；扣非事实缺失不伪造扣非值；未分类权益分录亦不静默忽略。
- 公开查询从 `PublishedReport` 直接映射 ROE 和 report-period income；该部分没有发现重算/口径漂移。`ReportSet::validate` 的有理数交叉乘积避免浮点误差，普通 ROE 与利润/平均权益相互勾稽。

## 有效发现

1. **已关闭：季度/半年合并报告混用 YTD 与窗口归母利润。** 实现新增必填可空字段 `report_period_net_income_to_parent`，由 `ConsolidationFacts.window_ni_to_parent` 生成，并用于窗口勾稽和 ROE 数学校验；已有 `net_income_to_parent` 继续保留 YTD 语义。`consolidated_quarter_validation_uses_report_window_parent_income` 使用 Q2 期间且断言 YTD 与窗口金额不同，再要求校验成功，针对原失效条件形成回归测试。缺少该合并同窗口事实时 `validate_parent_income_source` 显式拒绝。未发现归母口径或窗口漂移。

2. **已关闭：2199 年 12 月期间边界。** `period_dates` 现在直接用 `calendar::days_in_month(2199, 12)` 构造 `2199-12-31`，不再要求表示 `2200-01-01`。新增 `final_supported_month_has_a_representable_month_end` 明确锁定最大月末。

## 尚未执行

- 复核者未运行 Cargo。root 后续提供的最终动态证据为 `latest-finance-period-green.log` 10/10、`latest-roe-green.log` 3/3、`latest-official-roe.log` 13/13、`latest-consolidated-window-final.log` 10/10、`public-projection-assert-green.log` 5/5。Q2 同窗归母回归、缺失必需同窗事实拒绝、最大支持日期及 DTO 精确结构均有对应测试通过。
- 2026-10-06 增量静态复核关闭两个 finding：required-nullable Engine field 通过 `deserialize_with` 保持必填键并允许显式 `null`，公开 DTO 原样投影窗口事实；未发现新的 A 股口径、跨层或范围问题。动态测试日志由 root 执行/提供，不能表述为复核者亲自运行。
- root 报告最终 TS typegen 140 项与 Web TypeScript 检查通过；复核者未执行这些命令。跨域 gold/存档 fixture 刷新仍待 root 统一完成，因此本记录不宣称 Web/Engine gold、存档或完整跨层验收完成。
