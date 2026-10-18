# 公司线引擎耦合面盘点（重集成设计输入①）

> 生成：2026-10-18。方法：只读分析——对 `agents/main-line-reintegration/retired-files.txt`
> 中全部 116 个 engine 侧退役文件逐个提取 `18725d07` 版本的 `crate::` 依赖与行数，
> 结合旧 session.rs / pipeline / market 与新 `packages/engine/src/runtime/`（std-thread）
> 的源码对照。未跑构建、未改任何入库文件。
> 素材锚点：退役清单 [`agents/main-line-reintegration/retired-manifest.md`](../agents/main-line-reintegration/retired-manifest.md)；
> 分叉点 `a6001494`，main 侧终态 `18725d07`。

## 0. 一句话结论

公司线的**领域建模（名册/税务/分红/配股/回购/送转/账务分录）基本是纯域代码**，与旧并行
计算符号（`ComputeContext`/`ParentOrderPlan`/`PendingNpcBatch`）**零引用**；它的真实耦合
集中在**session 状态形状**（中央账户表 + 日终编排链 + market 除权字段 + 意图受理域）。
新架构已预留 `AdminRight`/`rights_computer` 公司日终权益通道（未接线），重集成的主战场
是把旧"中央 `&mut accounts` 直改"改写为"spine 计算 + 组端可逆落账"。

## 1. 总量盘点

| 分类 | 文件 | 行数 | 说明 |
|---|---|---|---|
| 退役 engine 侧合计 | 116 | 60,876 | manifest §引擎面 |
| 其中非测试 src | — | 30,039 | 重集成的实际对象 |
| 其中测试 | — | 30,837 | 纯域测试 16,040 行，可随域直接回来 |

非测试 src 按依赖纯度分类（依据逐文件 `crate::` 引用扫描，见 `.tmp/reint-scan/file-class.txt`）：

| 类别 | 行数 | 占比 | 判定 |
|---|---|---|---|
| 纯域（仅 company/accounting/calendar/money/account） | 9,406 | 31.3% | 近直接复用 |
| 纯域 + orderbook serde 助手（`js_safe_u64`/`canonical_u64_decimal` 属性） | 9,683 | 32.2% | 同上（助手仍在树） |
| 实际跨模块接线 | 10,950 | 36.5% | 需重写/重接线 |

> 注：`company/system.rs`（1,341 行）对 information 的全部耦合是 1 处
> `AccountingPolicyRef` 字面量，若归入纯域则纯域 ≈ 20,430 行（**68%**）。
> 纯域占比取 **64%~68%** 为可信区间。

跨模块接线 src 文件 TOP（重写面排序）：

| 文件 | 行数 | 耦合模块 |
|---|---|---|
| `session/corporate_actions.rs` | 4,430 | experience、orderbook（实质是 session 状态形状） |
| `company/system.rs` | 1,341 | information（1 处字面量） |
| `session/retained_history.rs` | 1,226 | experience、orderbook（支撑面：日级历史） |
| `company/simple/preferences.rs` | 856 | market（1 个纯函数 `price_bound_cents`） |
| `company/report_correction.rs` + `session/company_corrections.rs` | 1,349 | information、session |
| `session/company_contract_views.rs` | 508 | orderbook、session（host 查询面） |
| 其余（memberships/trade_confirmation_query/intraday_disclosures/exchange_calendar/operations-restore 等） | ~2,800 | 混合 |

## 2. 耦合面提取

### 2.1 符号面（退役代码绑定的外部符号）

**旧并行计算符号：零引用。** 全部 116 文件对 `ComputeContext`/`ParentOrderPlan`/
`TickShadow`/`PhaseInput` 引用为 0；`PendingNpcBatch` 仅 4 处 incidental 出现在
支撑文件（retained_history / live_minute_history / memberships_tests /
issuer_repurchase_transaction_probe_tests）。且这些符号在当前树**仍存在**（旧串行
管线是 `ENGINE_STEP_RUNTIME=0` 逃生门，批 9 才退役）——公司线不构成对旧管线的
符号级依赖，重集成不必等旧管线退役，也不被其拖住。

**模块级依赖**（引用文件数，`crate::` 扫描）：accounting 73、company 67（闭包内部
互引）、calendar 60、orderbook 23（绝大多数仅 serde 助手）、money 23、account 19、
session 18（几乎全在 session/ 下的接线文件与测试）、information 14、strategy 5、
experience 4、plans 1、market 1。**全部 12 个被引模块在当前树仍存在**——不存在
"模块整层消失"的问题，缺的是模块内部的 main 侧新增符号。

**已灭符号清单**（当前树 grep 为 0，重集成需随文件取回或重建）：
`CompanySystem`、`SessionCorporateActions` 及其 5 个 registry（dividends /
rights_offerings / issuer_repurchases / share_splits / stock_distributions）+
`dividend_tax_books`/`split_unit_context`、`IssuerRegistry`、`ShareRegistry`/
`HolderId`/`ShareLot`/`AcquisitionSource`、`CashDividendTaxMode`、
`ExReferencePrice`、`CompanyReportCorrection`/`ReportCorrectionEpoch`、
`taxpayer_identity_of_kind`/`TaxpayerIdentity`、`AccountKind::IssuerRepurchase`
（account.rs 公共文件改动，diff 349 行内）。

### 2.2 接入点（旧 session 管线挂钩）

**构造期（pre-spawn，串行，无并发问题）**
- `company_assembly::assemble_companies` → `CompanySystem` 装配；
- `assemble_corporate_foundation`：seed_float 后按分配结果构建全流通名册
  （`ShareRegistry::new` + `define_dividend_legal_facts` bind-once）；
- 回购专户开立（`AccountKind::IssuerRepurchase` 账户）；
- `SessionSetup` 公司字段族（`company_system`、`dividend_tax_mode`、
  `rights_offering_enabled`、`issuer_repurchase_enabled`、
  `auto_corporate_foundation`、`report_frequency`、`flat_withholding_bp` 校验链）。

**tick 起点（旧 `pipeline/mod.rs::plan_tick` 的 `shadow.execute` 内，2 个钩子）**
1. `prepare_ex_references_for_current_date()` —— 除权除息基准价写入 market
   （旧 market.rs 的 `cash_ex_reference_pending_trade`/`last_cash_ex_reference` 字段，
   `continuous_matching.rs` 消费）；
2. `place_issuer_repurchase_orders()` —— 回购执行器经
   `receive_private_intent` 受理域向订单簿放真实委托（每日至多一单，ADR-0032/0017）。

**日终链（旧 `session.rs::end_civil_day_after_session_check`，12 步顺序，全部
`Arc::make_mut(&mut company_system)` + `&mut self.state.accounts` 中央直改）**
1. `close_registries_through`（名册日终推进，入参=全账户持仓快照+个人贸易确认+回购户）
2. `apply_report_corrections_at_day_end`
3. `process_dividends_on_day_end`（含 Flat 简税代扣上下文）
4. `process_stock_distributions_on_day_end`（送转）
5. `settle_share_splits_on_day_end`（拆/缩股入账相位）
6. `auto_cancel_completed_repurchases`（完成即注销）
7. `register_share_splits_on_day_end`（R 日登记相位）
8. `process_rights_offerings_on_day_end`（配股）
9. `collect_issuer_repurchase_fills` + `process_issuer_repurchases_on_day_end`
10. `corporate_actions.validate`（公司行为与 Simple 账务勾稽）
11. `company_system.advance_day`（+结算周期判定→`process_simple_preference_proposals`）
12. `disclosures.run_simple_day_end` → `deliver_public_information` →
    `record_civil_day_events`

**host 驱动的公共 API（~40 个 pub 方法）**：审批/认购族（`approve_cash_dividend`/
`approve_share_split`/`approve_rights_offering`/`subscribe_rights_offering`/
`approve_issuer_repurchase`/`execute_issuer_repurchase_cancellation`/
`configure_cash_dividend_tax_book`/`define_dividend_legal_facts`）、查询族
（`account_dividend_tax_status`/`dividend_tax_outstanding_views`/
`company_capabilities`/`owner_rights_offerings`/`company_period_explanation`/
`corporate_actions`/`query_public_reports` 等）、`player_proposals` 提案面。
其中**认购/审批类直接动玩家账户现金**（中央直改），是新架构下必须重新设计通道的部分。

### 2.3 事件 / 协议面

- `Event::CompanyDisclosurePublished` + `CompanyDisclosureKind` + seq 统一编号：
  **当前树仍在**（session.rs:376/450，事件枚举未被合并波及）——事件出口免建。
- save schema：旧 `SaveState` 携带 `corporate_actions: SessionCorporateActions`、
  `company_system: CompanySystem`（ts_rs 导出到 web 存档）——新 save 面需扩展。
- host 协议：退役 `session/protocol/civil/*`（公司查询、个人贸易、day-end NPC 协议）、
  `company_contract_views`、`trade_confirmation_query`——需在新 session views/protocol
  上重建（web/server 侧另有 269/12 文件配套，不在本输入范围）。

### 2.4 公共文件接线面（不能 checkout、只能 rebase diff）

| 共享文件 | 与 18725d07 的 diff 行数 | 内容 |
|---|---|---|
| `orderbook.rs` | 732 | 分支侧重构 + main 侧受理域扩展 |
| `market.rs` | 571 | main 侧除权字段族（cash_ex_reference…） |
| `account.rs` | 349 | `AccountKind::IssuerRepurchase` 等 |
| `information/publication.rs` | 120 | 披露发布面 |
| `money.rs` | 94 | 数值助手 |
| `calendar.rs` | 1 | 无实质冲突 |

另有 `session/disclosures.rs`（旧 443 → 新 227）、`session/company_groups.rs`
（428→7）、`session/company_assembly.rs`（49→27）为**同路径不同实现**，重集成时
以新树为底、按 diff 搬 main 侧语义。

## 3. 新架构映射表（旧接入点 → `runtime/` 等价面）

| # | 旧接入点 | 新等价面 | 判定 |
|---|---|---|---|
| 1 | 日终权威入口 `end_civil_day` | `RuntimeDriver::day_end`：`world.run_day_end_public()`（Desk Drain→组 DayEnd→DayEndCandidate 校验）+ `session.end_civil_day()` 权威推进 + 双侧回滚编排（drive.rs:1158） | **等价已建**，公司日终步骤嵌入 `session.end_civil_day` 内即可保序 |
| 2 | 日终账户权益落账（`&mut accounts` 直改） | `DayEndDirective.rights: Vec<AdminRight>` + `World::rights_computer(day, group)` 钩子（world.rs:1808；组端可逆日志落账） | **通道已预留、未接线**（bridge.rs:111 恒 None）。`AdminRight` 仅 3 变体（CashDividend/ShareSplit/WithholdingTax——注释明确为送转个税设计），**需扩展**：配股缴款（现金+股双腿）、碎股、回购户结算 |
| 3 | 名册推进的全账户持仓快照 | `DayEndCandidate.groups: Vec<(GroupIdx, Vec<GroupAccount>)>` / `World::snapshot_accounts(_sparse)`（drive.rs:646/653） | **等价已建**（只读面） |
| 4 | tick 起点 `place_issuer_repurchase_orders`（意图受理域放单） | `GroupIn::ExternalPlan { ops: Vec<PendingOp>, cancels }` + `World::broadcast_external_plan/send_external_plan_grouped`（玩家意图路由已翻转至此） | **近等价可复用**；回购专户需名册落位（见 #9） |
| 5 | tick 起点 `prepare_ex_references_for_current_date`（market 字段） | 无等价：价格/涨跌停基准归 desk（`DeskStockSpec`、`StockPublic.daily_upper/lower_limit` 基于前收） | **需新建**：`DeskMsg` 增除权日基准消息（或 Start 附带覆盖）；纯函数 `ex_reference_price.rs`（520 行）可直接复用 |
| 6 | `Event::CompanyDisclosurePublished` 等事件 | 当前 `Event` 枚举原样保留 | **免建** |
| 7 | 公司 spine 状态（CompanySystem/名册/税账/Books）推进 | `session.end_civil_day` 内 `Arc::make_mut` 推进——新树该函数已含公司 seam（pre-fork `operations`/`closing`/`disclosures` 即在此推进） | **位置等价**：新 session state 增字段即可，日终步骤链照搬 |
| 8 | Simple 日终披露 `disclosures.run_simple_day_end` | 新 `disclosures.rs::run_day_end(DayEndDisclosureCtx)`（pre-fork 版，结构已在） | **需融合**：Simple ctx（system+5 registry 入参）在新 ctx 上扩展分支 |
| 9 | 回购专户（中央账户表里 `AccountKind::IssuerRepurchase` 账户） | 无等价：运行期账户权威在组线程（`GroupAccount`），spine 账户表仅为构造投影/回写镜像 | **需设计**：专户入某组名册（走 ExternalPlan+AdminRight）或 spine 影像户+日终对账，二选一待重集成设计 |
| 10 | 玩家公司命令动玩家现金（`subscribe_rights_offering` 等） | 玩家账户在组（`AccountId(0)` 投影惯例位，bridge.rs:174）；写路径=意图或日终权益 | **需重设计通道**（审批类只动 spine 公司状态的部分可直搬） |
| 11 | 构造期装配/名册/法定事实 | `WorldSpec` 构造前串行代码（`accounts_projection_from_session` 之前的 session 侧阶段） | **近直接复用**（pre-spawn 无并发） |
| 12 | `SessionSetup` 公司字段族 | 新 `SessionSetup` | 恢复字段+三态校验（接线级） |
| 13 | save schema 公司字段 / host 协议 civil 查询 | 新 save/views | **需重建**（web/server 配套另计） |

## 4. 建模价值评估

**分层复用判定**（按 src 30,039 行）：

| 层 | 内容 | 行数（估） | 判定 |
|---|---|---|---|
| 账户分录/账务 | `accounting/`（disclosure_roe/period_roe/reports）、`company/simple/finance_*` 全家（postings/state/kind/settlement entry/report validation）、`reports/simple_summary` | ~8,500 | **近乎直接复用**：输入=公司事件+日历，输出=分录/报表，无 runtime 依赖 |
| 税务 | `cash_dividend_tax.rs`(1,615)+子目录、`income_tax.rs`、三行业 income_tax、`bank/income_tax` | ~3,200 | **近乎直接复用**（持有期/差别化扣缴纯状态机；orderbook 引用全是 serde 属性） |
| 公司操作域 | `cash_dividend.rs`(863)、`stock_distribution.rs`(796)、`share_split.rs`(819)、`rights_offering.rs`(1,351)、`issuer_repurchase.rs`(1,106)、`share_registry.rs`(1,217)、`ex_reference_price.rs`(520)、`dividend.rs`、`capabilities`、`identity`、`config`、`correction` | ~8,800 | **核心建模直接保留**：登记/结算状态机、除权公式、lot/限售语义均为纯域；唯**落账尾巴**（写 Account 的少量函数）需改 AdminRight 发射 |
| Simple 周期/装配 | `simple/`（period/growth/environment/state/preferences）+ `opening.rs`/`assembly.rs` | ~3,900 | **直接复用**（preferences 的 market 耦合=1 个仍在树的纯函数） |
| 日终编排 | `session/corporate_actions.rs` | 4,430 | **最大重写面**：状态容器形状可搬，编排循环从"中央直改"改写为"spine 计算+AdminRight 批次" |
| 接线/支撑 | retained_history、corrections、views、memberships、trade_confirmation、intraday_disclosures、protocol/civil、system.rs 门面 | ~4,000 | 中度改写（挂到新 session/views/save 面） |

**比例估计：src 代码 64%~68% 近乎直接复用；~10% 小改（落账尾巴/serde 面）；
~25% 重写（corporate_actions 编排 4,430 行 + 接线面 ~3,000 行）。**
测试 30,837 行中纯域 16,040 行可随域直接回归，session 级契约测试（~14,800 行）需按
新架构重立基线（manifest 已警告不得沿用 1661 基线）。

**最大重写面**：`session/corporate_actions.rs` 的日终落账通道——旧模型"12 步链内
直接 `&mut BTreeMap<AccountId, Account>`"在新模型下不存在（运行期账户权威在组线程，
可逆日志落账）；等价改造 = ①扩展 `AdminRight` 变体集（配股缴款双腿/碎股/回购户），
②`rights_computer` 接公司日终计算，③名册/账务仍走 spine。次要重写面：desk 除权
基准价消息（#5）与回购专户归属设计（#9）。

## 5. 重集成顺序建议（供设计输入②）

1. 先回纯域闭包（company 域文件+accounting+tests，63%+，几乎无冲突）；
2. `SessionSetup`/save/account.rs 公共 diff 小步恢复；
3. spine 公司状态 + `end_civil_day` 内 12 步链（位置等价，#7）——此时回写窗口
   （`force_full_accounts` 全量同步，drive.rs:1189）使 spine 直改在日终同步语义下成立，
   可作为**过渡态**先跑通正确性；
4. 再做 AdminRight 扩展 + rights_computer 接线（性能/一致性终态）；
5. 最后 desk 除权面、回购执行器、host 协议/查询面。

## 附：本盘点的工作文件

- `.tmp/reint-company-files.txt` —— 116 文件清单
- `.tmp/reint-scan/imports-all.txt` / `mod-refs.txt` / `file-class.txt` —— 逐文件依赖与分类台账
