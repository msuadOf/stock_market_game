# N2b：玩家提案 + 偏好局内编辑 台账

- **日期：** 2026-10-08。
- **worktree：** `wf_95bfe91e-44b-1`（基线 main `b7ace30a`；开工时分支落后 main 158 个提交，经 `merge --ff-only main` 快进到 `b7ace30a`——分支无独有提交，快进非破坏性）。
- **决策：** 用户 2026-10-08 verbatim：「玩家 UI 可发起提案 + 玩家控制偏好参数」「持仓即可、直接生效——玩家对自己有持仓的公司可直接发起提案（分红/送转/配股/增发/回购/拆股），走既有 approve 同一制度校验，通过即生效；无额外治理模拟，trading-rules 登记为游戏化简化」；「偏好局内编辑即时生效、下周期评估」（coordinator 细化项）。

## 语义定位（大 A 语义门禁）

**游戏化简化，显式登记**：真实 A 股的利润分配、配股、增发、回购、拆股决议由发行人依治理程序（董事会预案、股东大会决议）作出，证券投资者不能发起公司决议。本批开放玩家发起通道，不模拟任何治理程序（无投票、无最短持有期、无持股比例门槛），登记于 `docs/trading-rules.md`「玩家提案与偏好局内编辑」节；ADR-0037 追记「玩家提案追记（2026-10-08 N2b）」，把第 4 条「公司决策不由投资者发起」修订为真实 A 股语义的登记性表述。

制度校验**零旁路**：提案只新增一条前置规则（账户持仓 > 0，用账户持仓判定、名册外持仓也算持有），其余全部委托既有 `approve_cash_dividend` / `approve_stock_distribution` / `approve_rights_offering` / `approve_issuer_repurchase` / `approve_share_split`（可分配利润、法定公积金、面值权威链、同除权日碰撞预检、新局开关、10% 上限、唯一未完成回购方案、名册/法定事实前置等原样执行）。偏好自动提案仍是 NPC 公司的发起路径，与玩家提案共用同一状态机（ADR-0037 第 3、5 条不变）。

## 实现

### 引擎（`packages/engine/src/session/player_proposals.rs` + `company/system.rs` + `session/protocol/civil/session.rs`）

- `PlayerCompanyProposal`（ts-rs 导出，六变体最小参数集：分红每股红利 / 送转种类+比例 / 配股价格+比例+缴款期 / 增发价格+股数（定向提案玩家本人，`lock_until=None`，缴款期固定 1 个交易日）/ 回购价格上限+额度+数量上限+窗口交易日数+用途 / 拆股方向+整数比例）。
- `GameSession::propose_company_action(account, proposal) -> Result<PlayerProposalReceipt, PlayerProposalError>`：
  - 前置：账户存在、公司已知且已上市、账户对该上市证券持仓 > 0（`Account::position`，非名册判定）→ 否则 `NoHolding`。
  - 日程推导复用 `company::simple::preferences::{preference_ex_dates}`（公告日≠登记日、除息/除权=登记日次一交易日）——保证 typed 公告通道照常发布；配股经 `RightsOfferingEventPlan::derive_schedule`、回购窗口起点=公告日后首个交易日再推进 `window_trading_days−1` 个交易日。
  - 现金分红决议的 `total_gross` 按名册非库藏股计算（与 approve 口径逐字一致；approve 受理时按同口径复核，任何漂移显式拒绝不静默错账）；`distributable_amount` 取当前 `available_for_distribution`；送转 `approved_total_new_shares = 非库藏股 × 比例 / 10^6`。
  - 方案身份 `player-proposal:{公司}:{类别}:{日期}:{序号}`（序号=该机制既有账簿计数，账簿只增、恢复保留，不重推导）。
  - 错误分类：构造期参数域违反 → `InvalidInput`；新局开关关闭 → `UnsupportedOperation`（按当前开关状态分类，非错误文本启发式）；既有 approve 拒绝 → `BusinessCondition`；结构性内部不一致（日历映射缺失等）→ `SystemState`。
- `GameSession::set_simple_preferences(company, preferences)`：`CompanySystem::set_simple_preferences`（先 `preferences.validate()` 全域校验，写入 `SimpleConfig.companies[].preferences`，hash 缓存失效）；**同步**更新 `state.setup.company_system` 对应条目——恢复勾稽 `company_system.config() == setup.company_system`（`persistence.rs` 既有校验）要求两侧一致，同一候选事务内完成、任一侧失败不留半配置。语义：任意时刻可改，下一结算周期末日评估生效；同周期已产生提案不回滚（不新增回滚代码，测试锁定）。
- `GameSession::company_simple_preferences(company)` 只读初值查询（未知公司显式报错）。
- `ProtocolSession::propose_company_action` / `set_simple_preferences` 委托包装（wasm 可变入口先例：`configure_cash_dividend_tax_book`；ProtocolSession 只有 Deref 无 DerefMut）。
- 附带：`StockDistributionKind` / `ShareSplitDirection` / `RepurchasePurpose` 补 `ts_rs::TS` derive（typegen 8 个新类型）。

### Web（wasm → worker → host → UI）

- wasm 三导出：`propose_company_action`（owner 固定 `AccountId(0)`；`Result→PlayerProposalOutcome` 转换后序列化返回，业务结果**不抛错**；仅反序列化失败抛错）、`set_simple_preferences`、`company_simple_preferences`；`wasm-pkg.d.ts` 声明同步。
- worker（`wasm-worker.ts`）三消息 `proposeCompanyAction`/`setSimplePreferences`/`companySimplePreferences`（请求字段先校验：requestId、公司身份 ≤64、提案必须是对象）；worker-host（`worker-host.ts`）host 方法 + 过期 generation 防护；`engine-host.ts` 接口三个可选方法（缺失=宿主不支持，UI 显式提示）；`MarketRuntimeProvider` 三个 action（宿主切换/generation 防护同既有模式）；`LocalRefreshViews.ConnectedCompanyPanel` 接线。
- `host/player-proposals.ts`：六类提案构造器（提交前最小参数域预检——金额为元文本两位小数→规范分字符串、比例/股数正整数、天数 1..=255；权威校验在 engine）+ `parsePlayerProposalOutcome` 严格 parser（**三类显式分支**：accepted（kind/identity/批准/公告日）、no_holding（公司/证券/原因）、institutional_rejection（kind/原因/F 批四分类）；未知形状显式拒绝）+ `parseSimplePreferencesValue`（与 engine `validate` 同域）。
- `components/company/PlayerProposalPanel.tsx`：「发起提案」区（六类表单 + F 批能力面前置展示「当前制度条件与 blocker」+ 三类结果显式 + 游戏化简化常驻说明）与 `InGamePreferencesPanel`（局内偏好编辑：初值读取、局部草稿、显式保存、非法输入不写草稿、下一周期生效语义提示）。`CompanyContractPanel.useQueried` 导出复用（触发键纯度沿用 F 修复轮口径）。

## 测试（红→绿）

引擎 `session/player_proposal_session_tests.rs`（红证据=编译期缺失 API：`red-compile.log`；绿 12/12）：

1. 现金分红全链路：受理→公告→登记→除息→派发，玩家按登记持股真实税前到账（+5 分）、状态机到 Paid、日程与偏好推导对拍。
2. 送转全链路：10 送 10 → R+1 Credited、玩家 5→10 股、发行股数守恒演进。
3. 拆股全链路：1 拆 2（面值 2 分整除缩小）→ 除权日 settled、玩家 5→10 股、总股本翻倍。
4. 配股/增发/回购各一例受理：配股（RightsToAllShareholders 比例、R/缴款期日程断言）、增发（独立会话——同日配股+增发推导同除权日被既有碰撞预检拒绝，属既有语义；DirectedPlacement 目标=提案玩家、100 股）、回购（维护价值用途、窗口起点断言）。
5. 无持仓拒绝（三类行为各一例 + 不留账簿）。
6. 名册外持仓仍可发起（模拟净买入后、名册日终更新前的瞬态；日终勾稽使该瞬态不可跨日，测试同日提案）。
7. 制度拒绝显式（每股红利远超可分配 → `BusinessCondition` + 原文含「可分配」）。
8. 开关关闭拒绝（配股+回购 → `UnsupportedOperation` + 「本局未启用」）。
9. 参数域拒绝（0 红利、拆股比例 1 → `InvalidInput`）。
10. 偏好三口径：评估前关闭→本周期不提案；已产生提案不回滚；中途开启→下一周期评估生效（沿用确定性 plan_id）。
11. 偏好域校验与未知公司拒绝。
12. restore 深等：提案受理+偏好编辑→save→restore→再 save 深度相等（两侧配置一致不触发勾稽拒绝）、账簿与偏好保留。

Web：`host/player-proposals.test.ts`（parser 三分支+畸形拒绝、六类构造器域、偏好 parser、worker 请求字段校验 operationError）12 项；`components/company/player-proposal-panel.test.ts`（中文标签、宿主不支持显式提示、SSR 表单渲染+游戏化简化说明+渲染期零请求、偏好面板语义）4 项。

## 验证

| 项 | 结果 | 证据 |
| --- | --- | --- |
| 引擎红→绿 | 红=编译缺失 API；绿 12/12 | `red-compile.log`、`engine-run-3.log`、`final-*.log` |
| 受影响引擎组 | 13 组并发复跑全绿（player-proposals/simple-prefs/company::system/simple-session/corporate-actions/contract-rights/repurchase/split/mechanism-switch/tax-mode/auto-registry/persistence） | `groups/*.log` |
| `session::persistence` 13 败 | **pristine main `b7ace30a` 独立 worktree 复跑逐项一致**（31 过/13 败同名）——既有基线，零新增 | `/tmp/n2b-baseline-persistence.log` |
| `session::corporate_actions` | 单独跑 19/19 绿（7.36s；并行批次内超时 10s 为多组同时启动的编译/启动竞争，非测试失败） | 终端复跑 |
| typegen | 184 项导出全过（新增 PlayerProposalKind/Receipt/Error/CompanyProposal/Outcome + StockDistributionKind/ShareSplitDirection/RepurchasePurpose）；生成文件零漂移（check 仅因未提交报 untracked） | `typegen.log` |
| tsc | `tsc -b apps/web --force` 0 错误 | `tsc.log` |
| oxlint | 本批 12 个 web 文件 0 警告 | `oxlint.log` |
| cargo check | `-p web-wasm` 通过；`--workspace --all-targets --exclude stock-market-game` 通过 | `wasm-check2.log`、`workspace-check.log` |

未运行完整回归与浏览器 E2E（本批验证范围与 N2a/F 批先例一致）；Web 全量套件未整批跑（受影响文件定向复跑，`node_modules` 经符号链接自 N2a worktree——依赖清单与基线逐字节一致已核对）。

## 已知边界与遗留

- **回购提案的日历边界（既有语义，不改）**：`share_registry` 勾稽要求回购专户事实 `established_on ≤ 名册最近结算日`，玩家盘中（已跨当日结算）提案会被该守卫显式拒绝（错误原样上抛）；仅在名册结算当日（含开局装配日）受理。M 批既有行为，本批登记不改语义；若产品要求盘中可提案，需 M 批语义另行决策。
- **配股与增发同日提案互斥（既有语义）**：两者日程推导同口径，同日提案推导出相同除权日、被既有碰撞预检拒绝（同除权日合并口径未核实是已登记的全局守卫）。
- **增发缴款期固定 1 个交易日**（最小参数集口径）；后续需要参数化时扩展 wire 字段即可。
- **局内偏好编辑只覆盖现金分红字段**：送转偏好项在局内面板只提供开关（开启即温和默认值），逐字段编辑留后续（新局入口已有完整编辑）。
- **Tauri/远程宿主**：三个新 host 方法未实现，UI 按既有模式显式提示「当前宿主不支持」。
- 名册外持仓瞬态不可跨日终（日终账户持仓↔名册勾稽），提案须在该瞬态内提交——与二级市场净买入的实际节奏一致（买入当日即可提案）。

## 文件清单

- 引擎：`packages/engine/src/session/player_proposals.rs`（新）、`session/player_proposal_session_tests.rs`（新）、`session.rs`（mod 声明）、`company/system.rs`（set_simple_preferences）、`session/protocol/civil/session.rs`（委托包装）、`company/{stock_distribution,share_split,issuer_repurchase}.rs`（TS derive）。
- Web：`apps/web-wasm/src/lib.rs`（三导出）、`apps/web/src/wasm-pkg.d.ts`、`host/wasm-worker.ts`、`host/worker-host.ts`、`host/engine-host.ts`、`host/player-proposals.ts`（新）+ `.test.ts`（新）、`app/MarketRuntimeProvider.tsx`、`app/LocalRefreshViews.tsx`、`components/company/PlayerProposalPanel.tsx`（新）+ `player-proposal-panel.test.ts`（新）、`components/company/CompanyPanel.tsx`、`components/company/CompanyContractPanel.tsx`（useQueried 导出）。
- 类型：`apps/web/src/types/generated/`（8 个新文件，typegen 正规生成）。
- 文档：`docs/trading-rules.md`（新节）、`docs/decisions/0037-company-action-initiator.md`（追记）、`agents/company-system/{implementation-checklist,current-handoff,player-proposals}.md`。
