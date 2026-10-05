# Simple Web 接线冻结与归属

本批冻结生产源码，后续只在根任务正规生成 fixture 后补定向存档测试证据；不操作 index、Cargo 或 generated bindings。股本行为偏好及完整公司行为不在本批完成范围，报告可用性查询由独立 Host owner 接线。

## 初始化链路

`App.tsx` 在预览前确定 seed，`createSeedDraft` 调用 `createPriceAnchoredCompanyConfig`；`CompanySystemInput` 展示同份 JSON 与 seed。新局命令直接解析展示值，将同份公司配置及 seed 原子交给创建端，生命周期消费已选 seed，不另抽取。Remote 创建／重置同样传完整配置；恢复使用实际存档的 setup 和 seed，不做价格反推。反推 helper 只读 `StockSpec.initial_price` 与总股本，不读取运行期成交价格。custom JSON 不因 seed 编辑自动覆盖。

当前定向初始化短测 13/13 通过，证据为 `.tmp/company-system/initial-preview-final-green.log`；并发 4，case timeout 与外部命令 deadline 均为 10000ms。新增实际新局正例对照每家公司的 seed 初始化营收、固定开支与平衡权益，确认展示预览被原样消费；未运行完整回归。

## 可归属文件

独立新增或本主题主要维护文件：

- `apps/web/src/config/company-initial-preset.ts`、对应 `.test.ts`。
- `apps/web/src/config/seed-draft.ts`、对应 `.test.ts`。
- `apps/web/src/components/company/CompanySystemInput.tsx`、`apps/web/src/app/company-config-commands.test.ts`。
- `apps/web/src/save/schema/company/annual-growth.ts`、`period-generation.ts`、`simple-finance.ts`、`system.ts` 及各自 `.test.ts`。
- `apps/web/src/save/schema/company/simple-values.ts`、`simple-finance-config.ts`、`system-config.ts`。
- `apps/web/src/save/schema/company/simple-chart.ts` 的四类精确基础科目表与五个独立虚拟汇总科目，供严格恢复和短测复用。

共享文件仅按本主题 diff 归属，不可按整个路径盲目提交：

- `apps/web/src/App.tsx`、`app/StartupScreen.tsx`、`app/useSaveCommands.ts`、`app/useSessionHostLifecycle.ts` 与其 seed／新局定向测试；Remote 登录屏的配置与 seed 消费增量。
- `apps/web/src/config/defaults.ts` 的 Simple 配置与价格初值；`components/company/ReportNotes.tsx`、`public-report-fixture.ts` 和公开来源渲染测试。
- `apps/web/src/host/public-report-normalize.ts` 的完整报告 `source`；`save/schema/company/reports.ts` 与 `report-corrections.ts` 的完整报告来源／Simple 更正关联及短测。
- `apps/web/src/save/schema/company/accounting/journal.ts` 的 `SimplePeriodSummary` 与复用导出；`books/industrial.ts` 的共享税务 parser 导出。
- `apps/web/src/save/schema/root.ts`、`market.ts` 的 Simple 配置／状态／日期／发行人／公开材料交叉校验。
- `apps/web/src/save/complete-save-schema.test.ts`、`day-end-archive.test.ts` 的当前契约迁移。

共同生成 bindings、真实 JSON fixture、底层财务、税务、Session、三宿主 Rust 与股本行为均归根任务或对应 owner，不应计入本批独立文件集合。工作记录与复核记录统一在 `agents/company-system/`；已有月度旧方案记录保留历史范围，不作为最终期间合同通过证据。

## 待验收

根任务已正规生成真实 `current-schema-save.json` 和 `current-closed-day-save.json`，完整存档与休市日短测 15/15 通过；旧 fixture 导致的红证据保留，未手补财务字段或日期。完整存档保留 Simple 状态拒绝断言，行业 flow／inventory 拒绝断言保留在低层独立 parser；日终原活动订单、冻结资源、输入与父单负例全部保留。休市日档实际日期为 2026 年元旦，日期精确预期已与真实 generator 对齐。

低层 fixture 与完整存档分离后，另有五份公司 parser 测试 22/22 通过；本批增量可归属 `company-slice-test-fixture.ts`、`company-contracts.test.ts`、`company-schema.test.ts`。它们保留底层 CompanyOperations、groups 和所有行业参数的明确独立契约，不作为 Simple 兼容层。源码 public type 导出及两个测试类型边界已做最小修复，完整 TypeScript 检查仍由根任务统一验证。

类别根修已让公司配置 `kind`、发行人 `kind` 与财务状态 `kind` 严格一致，并逐科目核验各行业基础表加独立汇总科目；默认预设显式 Industrial，编辑 JSON 可选择其余类别。初始化汇总权益改用跨类别通用的 `simple_receivable`，不把汇总经营当作贷款或保费。根任务 fresh70 正规生成真实 bindings 与两份存档后，本批必要短组同一命令 45/45 通过：完整存档／日终 15、类别与 seed／新局 27、初值 helper 3；日志 `.tmp/company-system/web-final-kind-fresh-green.log`。外部 deadline 与 case timeout 均 10000ms，并发 4；未修改 JSON 或 generated，仍不宣称完整回归或全项目类型检查通过。
