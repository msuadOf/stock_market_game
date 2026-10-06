# Web 公开报告 ROE consumer 接线

日期：2026-10-06。

## 范围与依据

- `apps/web/src/save/schema/company/reports.ts` 将 `ReportSet.roe`、`income.report_period` 与 `income.report_period_net_income_to_parent` 作为严格必填字段解析；校验精确有理数格式、窗口利润勾稽、scope／nullable 对应、权益表同窗归属净利润及 ROE 交叉一致性。Consolidated 不支持可用 ROE；`NonPositiveAverageEquity` 与 `MissingNonRecurringIncomeFacts` 仅可用于具体指标不可用，不能作 unsupported basis 原因。也严格解析 `Shock` 与 `CashDividend` typed 公告内容。
- `apps/web/src/host/public-report-normalize.ts` 严格校验公开报告 DTO、报告窗口及摘要交叉一致性。Public DTO 的 `income.report_period_net_income_to_parent` 是 required-nullable：Standalone 为 `null`，Consolidated 为同报告窗口归母净利润；scope/nullability、ROE 及权益表利润均勾稽该窗口值，不回退到 YTD `net_income_to_parent`。
- 公司面板使用真实报告窗口列，呈现普通／扣非 ROE 与不可用原因；百分比展示使用精确有理数与 half-even 舍入。
- 领域依据为 `docs/decisions/ADR-0035-company-system-simple.md` 与 Q14 第二章；ROE 法定计算依据及适用边界见 `agents/company-system/official-roe-research.md`、`agents/company-system/official-roe-review.md`。Simple 缺少扣非事实时显示明确不可用原因，不复制普通净利润或伪造零值。
- Engine public query 从已发布报告投影 `PublicReportFinancials.roe`、`income.report_period`、同窗归母利润及摘要 `report_period_net_income`；Web 不重算 ROE，也不改交易制度或 Engine 计算。

## 验证

- `apps/web/node_modules/.bin/tsc -b --pretty false` 通过。直接调用项目安装的 tsc；当前 Node 25.8.2 下 `corepack pnpm` 启动时报动态 import callback 错误。
- 本任务由 `apps/web` 目录经外部 10 秒 deadline supervisor 运行 save/schema/public-financials batch：**40/40 通过**，包含四档真实 gold、手写 schema fixture、typed 公告、scope/nullability、窗口归母与 ROE 边界；后续报告夹具单文件增量 `reports.test.ts` 为 **4/4 通过**。
- root 随后运行 `final-web-roe-and-belief-short.log`：combined batch **48/48** 通过，另含其他 owner 的 belief-scope 7 项；该 7 项不归因于此 Web consumer。`final-web-roe-and-belief-types.log` 记录 TypeScript 检查通过。
- 旧 35/36、36/37 失败记录均为 gold/parser 契约更新前的历史结果；新正式 gold 已安装，严格 parser 与手写 fixture 均同步。历史日志保留为 TDD 证据，不代表当前测试状态。
- `reports.test.ts` 与 public normalizer 的 Consolidated+Available ROE 负例先观察到 `Missing expected exception`；修复后仅接受 `ConsolidatedAttributionFactsUnavailable` 且拒绝可用合并 ROE。Unsupported basis 两种非法 reason 负例也先观察到 `Missing expected exception` 后修复为两层 parser 均拒绝。
- 初版 Web diff及最终 strict restore 增量均获非作者 Luna 限定 PASS。复核确认 Consolidated + 可用 ROE 拒绝、Unsupported reason 限制、required-nullable/scope 与同窗口权益勾稽符合 Engine `company/query.rs`、`reports/validate.rs`；二月夹具符合月报窗口与 YTD 差异语义，不伪造 ROE。未宣称 Web/Engine 全链或完整 A 股 ROE 特殊口径已实现。`git diff --check` 通过。

## 边界

- 现阶段不宣称 Web/Engine 全链验收完成；Web gold、聚焦短测和本批独立复核已通过。源码已冻结，未 stage／commit。
- 不更改 `corporate-actions.ts`、Engine、生成类型或正式 gold；不接受旧裸 `event` 公告形状作为兼容回退。
