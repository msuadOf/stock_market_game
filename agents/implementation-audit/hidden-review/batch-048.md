# 隐藏扫描批次 048（owner 3）

## 范围与读取证据

- 计划来自 caller 的 `agents/implementation-audit/hidden-review/scan-plan.json`，batch 48，owner=3；源基线 `43b1aa5`，source root `/data1/baiyifan/workplace/stock_market_game`，caller `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 三份来源都从首行连续全文读取到 EOF；计划/实测行数及 SHA-256 一致，各自 `aliases=1`。首次读取计划时工具输出截断，但用 `jq` 单独读取了 batch 48 条目；web-01 曾发生展示截断，之后按行段连续补读至第 314 行。章节清单和逐源完整性见 `batch-048.json`。
- 已读 caller `AGENTS.md`（123 行）和 `docs/principles.md`（92 行）。现行决策核对 ADR-0004、0007、0010、0017、0018、0025、0027 及 `docs/trading-rules.md`；交易/存档约束以这些现行决策为准。来源中的“候选设计，未实施”、reader instructions、历史验证陈述只作审计史料，不代表当前状态。

| 来源 | 计划/实测行数 | SHA-256 | 全文章节核销 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/tooling-05.md` | 158/158 | `355cb23b14e9c20fa902f6cdb45cf6418826a71db62e1a85d6697f5ecb7ab5b5` | 完整至 EOF；总论、11 个 simulation 文件逐项核销、跨模块边界、最终动作分类。 |
| `agents/oop-refactor-audit/exhaustive/modules/tooling-07.md` | 31/31 | `e5a96a91ec8d93ce2f1c13c6ab08ffdf9ced4ff2b5e3716009e778f48da6ab9b` | 完整至 EOF；WASM 验证脚本的对象归属、流程、调用契约、迁移边界及两个非 OOP 风险。 |
| `agents/oop-refactor-audit/exhaustive/modules/web-01.md` | 314/314 | `bf4bab880b04584e14dc4ee56c36577312ed8086afccc1580bb9c45d4b685ded` | 完整至 EOF；28 个 Web/UI/测试文件逐项核销、App 与图表候选对象的状态/API/依赖/迁移/验证边界、TradeMarketControls 类别风险及未实施声明。 |

## 当前调用链与旧候选核验

- **候选计数：** tooling-05 有 1 个对象迁移候选（`EscrowVerificationRun`）；tooling-07 有 0 个；web-01 有 4 个（`SessionHostLifecycle`、`SaveCommands`、`TradingCommands`、`MarketChartProjection`）。这 5 个旧 OOP 候选都能在当前 caller 找到对应实现；tooling-05-D01 是另列的未来安全修复建议，不计入获批 OOP 候选。
- **App 命令与生命周期候选已落地。** `apps/web/src/App.tsx:41-43,212-215,287-300,352-360` 组合 `useTradingCommands`、`useSessionHostLifecycle`、`useSaveCommands`；`useSessionHostLifecycle.ts:55-105,218-232` 拥有单次 effect 的 `cancelled`、`ownedHost` 和资源释放流程；`useSaveCommands.ts:52-89,95-143,161-240` 复用 AppShell 持有的 gates/refs 并保留各存档命令写集；`useTradingCommands.ts:22-140` 管理交易表单和活动委托查询/取消 UI 状态，host/Redux/AutoOrderManager 仍由既有层权威持有。旧 web-01 的 SessionHostLifecycle、SaveCommands、TradingCommands 候选不再是遗漏动作。
- **MarketChartProjection 候选已落地，但其旧证据不是交易语义验收。** `useMarketChartRuntime.ts:15-29,51-101` 以单一 projection 管图表缓存并保留 Redux/React/effect 编排；`market-chart-projection.ts:55-105` 管五组缓存、重建、快照替换与读取；Provider/LocalRefreshViews 接线仍是呈现层消费者。未运行相关测试，故不把源文档中的测试清单当成本轮运行结果。
- **web-01 提及的 A 股 UI 风险仍由 G68 覆盖。** `LocalRefreshViews.tsx:120-129` 的 `TradeMarketControls` 未接收活动 `SessionSetup`，仍从 `DEFAULT_SETUP.stocks` 按类别计算快捷涨跌停；交易命令 `useTradingCommands.ts:58-67,106-118` 则从 `activeSetup.stocks` 校验证券类别。存档加载会更新活动 setup（`useSaveCommands.ts:124-131,190-196`），所以合法非默认局配置存在 UI 展示/输入规则未共同消费活动配置的风险。现行总账 G68 已准确登记“非默认证券规则/选项未消费当前配置”；不因主板类别改动一定导致限价数字变化而夸大结论，也不新建重复 G。
- **tooling-05 的 A01 对象聚合已实现；D01 安全顺序缺口仍在。** `scripts/simulation/run-escrow-verification-matrix.mjs:557-576,809-810` 当前有 `EscrowVerificationRun` 并在公开入口创建/执行；这与候选聚合边界一致。独立 D01 则要求在读内容前 `lstat` 拒绝 symlink/非 regular file。当前 `validateArtifacts`（`:367-418`）先 `safeArtifactPath`，随后 `realpath`、`readFile`（`:393-399`），最后才做 canonical containment（`:402-404`）：路径指向输出目录之外时内容已在拒绝前读取；指向目录内目标的 symlink 也可能通过。该候选在原文中明确列作未来独立缺陷修正，不属于获批 A01 对象迁移承诺，因此记录为仍在的安全线索，不把它升格成已批准承诺遗漏或新 G。新建与 PASS 复用都调用该验证器的事实由 `:660,727` 可见。
- **未把旧对象化调查误作现行 OOP 待办。** `tooling-07` 明确无对象迁移候选；顶层 WASM harness 的异常路径跳过后续 drop、未知查询同时用 `page_size:0` 是源文档标明的行为边界，当前总账没有依据据此宣称为 OOP 迁移缺口或已修复。本轮没有运行其脚本或兼容性验证。

## G 关联、反证与新候选

- **图表现行缺口仍开放。** `G10/G11/G31/G47`（`implementation-audit-2026-10-02.md:61-62,69,104`）分别涉及分时成交量聚合、跨日历史隔离、按当前股票稳定图表引用和竞价 null 断线。`MarketChartProjection` 的存在不能核销这些消费行为；本次静态检查未重判其完整复现条件，也未运行测试。
- **tooling 的矩阵证据不能关闭 G39 或证明长任务门禁。** 当前总账 G39（同文件 `:126`）要求按同一实际受理轨迹验收 K7；tooling-05 中的 matrix 契约、已有测试说明及 17 项运行矩阵不自动证明 K7 fresh after/sensitivity 或现行跨 worker 受理验证闭环。deadline supervisor 属 caller 约束而非 `EscrowVerificationRun` 自己的职责；G61–G63（`:129-131`）各有明确受影响工具/runner，不把本批历史提案并入或关闭它们。本轮没有执行矩阵或回归。
- **没有新的已批准承诺遗漏，也没有旧 G 错误核销证据。** 旧候选目前分别已实现（App/hooks、projection、matrix run object）、属于支撑/保留边界（tooling-07 与多数 UI 文件），或明确是未来缺陷修正（tooling-05-D01）。G68 和图表 G 项继续有效；没有证据表明历史审计错误关闭了这些项。
- **A 股语义范围：** 本批只审查 Web 命令归属、图表展示投影和验收工具，没有修改产品或交易规则。单位、证券类别与活动 setup 的核对只引用项目现行契约/总账，未新增法源主张，未重新联网核对交易所或中国结算材料。

## 验证边界

只完成静态全文、行数/hash、ADR 与调用链核对；未运行测试、构建、回归或 Git 命令，未修改产品代码。批次元数据、章节清单、G 关联、caller 路径和反证见 `batch-048.json`。
