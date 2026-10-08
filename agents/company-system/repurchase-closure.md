# H 批：回购收口 台账（完成即默认注销 + 局中提案可受理）

- **日期：** 2026-10-08。
- **worktree：** `wf_eb0a457b-037-1`（基线 main `071bf263`）。
- **用户决策 verbatim：**「目前暂时回购默认注销，其他方案先不做，可以留一个空的接口」「局中日终可提回购提案」。

## 语义定位（大 A 语义门禁）

- **默认注销**：真实 A 股的处置路径有库存股持有（≤10%、三年内转让或注销）、
  员工持股/可转债转换等（63 号第 17 条）；本批按用户决策**只实现注销**且为
  默认（完成后的首个日终自动执行），其余路径显式不做，枚举留扩展位。自动
  注销复用 M 批手动注销路径（`MovementScope::IssuerRepurchaseCancellation`
  核减专户/总股本/注册资本、面值口径、不除权、N3 分录过账），零复制。
- **局中提案**：`established_on` 取名册最近结算日（`settled_on`）替代原
  `plan.approved_on`——日内恒不晚于结算日，局中任意日可受理（与「持仓即可」
  一致）；名册结算日恰有登记快照时被既有「不得跨登记快照回溯」守卫显式拒绝
  （结构性事实冲突，不做静默回溯）。
- **bind-once 专户事实**：专户事实描述**账户存在性**（同局只创建一次），首次
  批准绑定、此后不随方案重复绑定。原实现每方案重绑且 `source_evidence`/
  `established_on` 必然不同，被「immutable once set」拒绝——**同证券永远只能
  有一个方案**（M 批遗留缺陷，N2b 台账登记的「bind-once 矛盾」）。H 批以
  「存在即跳过重绑」消解；注销终态后新方案可提（测试锁定）。

## 实现

### 引擎（`packages/engine/src/company/issuer_repurchase.rs` + `session.rs`）

- `RepurchaseCompletionPolicy` 枚举（单变体 `CancelOnCompletion`；`serde`
  deny_unknown_fields + `ts_rs` 导出）；`IssuerRepurchasePlan` 新增必填字段
  `completion_policy`（**无兼容旧档**：缺字段/未知变体 serde 与 Web parser 双侧
  显式拒绝；三份存档 fixture 的 `issuer_repurchases` 为空数组，无需重生成）。
- `GameSession::auto_cancel_completed_repurchases(day)`：每个日终在
  `process_issuer_repurchases_on_day_end` 之后、跨域勾稽之前运行；对
  `status == Completed && completed_on < day` 的方案按 `completion_policy`
  分派（当前唯一变体＝注销），**复用** `execute_issuer_repurchase_cancellation`
  （候选事务、名册/账户/账面核减、面值×股数核减注册资本、不除权）。
- 显式边界（代码注释 + trading-rules 同步登记）：
  - **在途委托竞态**：该证券订单簿仍有回购账户在途委托（按账户归属判定，
    不区分方向）→ 该日不注销、下一日终重试。正常时序下市场日终已清空订单簿
    （`Market::end_of_day` 清 book），守卫为防御性；休市自然日不运行市场
    tick，残留委托跨日保留至下一交易日市场日终消解。
  - **零成交**：`record_cancellation` 以正股数为前提，零成交方案无股份可
    核减，保持 Completed 终态（不阻塞新方案、日终不失败）。
- `approve_issuer_repurchase`：`established_on = registry.settled_on()`（原
  `plan.approved_on`）；专户事实已存在时跳过重绑；唯一性拒绝文案改为如实
  （「既有方案完成后的首个日终自动注销（默认注销策略），之后才能批准新
  方案」）。
- `company_contract_views.rs`：回购 readiness 唯一性 blocker 文案与
  ActivePlanStage 注释对齐 H 批口径（Completed＝过渡态）。
- `player_proposals.rs`：玩家回购提案构造 plan 时固定
  `completion_policy: CancelOnCompletion`（不开放为玩家参数）。

### Web（`apps/web/src/save/schema/corporate-actions.ts`）

- `RepurchaseCompletionPolicy` 类型（`"CancelOnCompletion"`）+
  `IssuerRepurchasePlan.completion_policy` 必填字段 + parser 域同步（`exact`
  键集 + `oneOf` 域；公告载荷 `IssuerRepurchaseAnnouncement` 内嵌同一 plan
  解析，单一 parser 两处覆盖）。typegen 新增
  `apps/web/src/types/generated/RepurchaseCompletionPolicy.ts`（186 项导出）。

## 测试（红→绿）

引擎 `session/issuer_repurchase_session_tests.rs`（红证据＝行为缺失断言失败，
`.tmp/company-system/repurchase-closure/red-run2.log`、`red-reasons.log`；
绿 `green-final.log` 10/10）：

1. `completed_repurchase_auto_cancels_at_next_day_end`——完成→次日日终自动
   注销：状态机 Cancelled、cancelled_shares=全部成交、总股本/注册资本按面值
   核减、名册专户核减、回购账户持仓清零、Simple 账面事实回填、不除权、
   restore 深等。
2. `new_repurchase_plan_is_approvable_after_auto_cancellation`——自动注销后
   同证券新方案可提（红因＝「immutable once set」）、新额度合成入账、两份
   账面声明事实。
3. `player_repurchase_proposal_midgame_reaches_auto_cancellation`——开局
   数日（多日终）后玩家回购提案受理（红因＝「invalid issuer repurchase
   account facts」established_on 守卫）、全链路撮合→完成→次日日终自动注销。
4. `auto_cancellation_defers_when_repurchase_order_rests_at_day_end`——
   竞态守卫：完成判定后在途委托残留的日终不注销、休市日持续跳过、委托消解
   后的首个日终注销。测试注入卖向委托（预约股份）：买向残留与
   「live buy reservation exceeds account cash」不变量冲突、下一 tick 即
   显式失败——正常引擎时序下买向残留结构性不可能，守卫防御 restore/时序
   错位。
5. `zero_fill_completed_plan_stays_terminal_without_blocking_new_plans`——
   零成交方案保持 Completed 终态、日终不失败、不阻塞新方案。
6. 既有 `repurchase_completion_withdraws_unused_synthetic_funds_and_
   cancellation_shrinks_capital` 按 H 批契约改写：完成判定当日日终即回收
   （原为过窗后）；手动注销在过渡窗口内（次日终之前）执行；其后日终不重复
   自动注销（cancelled_on 不改写）；restore 深等保留。
7. F 批 `repurchase_readiness_allows_new_plan_when_existing_is_completed_
   not_cancelled`（company_contract_views_tests）按过渡态口径更新：完成当日
   过渡窗口内 readiness 满足 + 自动注销后（Cancelled）readiness 仍满足。

Web：`corporate-actions-schema.test.ts` 新增处置策略域用例（合法值通过、
未知变体拒绝、缺字段拒绝）+ fixture 补 `completion_policy`（33/33）。

## 验证

| 项 | 结果 | 证据 |
| --- | --- | --- |
| 红灯 | 5 新用例全部因正确原因失败（无自动注销／immutable facts／established_on 守卫） | `red-run2.log`、`red-reasons.log` |
| 回购组 | `issuer_repurchase_session_tests` 10/10 | `green-final.log` |
| 受影响引擎组（并行，各 10000ms deadline） | company::issuer_repurchase 5、probe 7、player_proposal 12、contract_views 13、corporate_actions 19、company_simple_session 37、share_registry 28、error_classification 10、capabilities 10、mechanism_switch 3、simple_preferences_session 16、rights_offering_session 14、share_split 8、notices 11、dividend_tax_mode 20、cash_dividend 35、ex_reference_price 24、company::simple 122 全绿；`session::failure` 3 败经 pristine HEAD 独立 worktree 复跑计数一致（环境性日历家族，M 批台账同登记） | `gr-*.log`、`gr2-*.log`、`gr4-*.log` |
| `session::persistence` | 31 过/13 败——**pristine HEAD（`071bf263` 独立 worktree 复跑）逐项对照：13 个失败用例名完全一致、零新增**（全部为 `saved_runtime_tests` envelope/费用审计既有家族） | `gr2-session--persistence.log`、`persistence-failures.txt`、`.tmp/head-names2.txt` 对照 |
| clippy | `cargo clippy -p engine --lib --tests` 警告集合与 pristine HEAD **完全一致**（零新增） | `clippy-current-summary.txt` 与 `.tmp/clippy-head-summary.txt` diff 为空 |
| Web 受影响文件（并行） | corporate-actions-schema 33、company-contract-views 17、player-proposals 12、mechanism-switches 3、simple-finance 12、system 16、complete-save-schema 7、save-schema-contract 7、company-contracts 34 全绿 | `web-*.log`、`web2-*.log` |
| typegen | `export_bindings` 186 项全过；新增 `RepurchaseCompletionPolicy.ts`；`check-generated-types` 仅因新文件未提交报 untracked（提交流程预期） | `typegen-count.log`、`typegen-check.log` |
| tsc | `tsc -b apps/web --force` 0 错误 | `tsc.log` |
| oxlint | 本批 2 个 web 文件 0 警告 | `oxlint.log` |
| `cargo check --workspace --all-targets --exclude stock-market-game` | 通过（仅 main 既有 warning） | `workspace-check.log` |
| rustfmt | 本批新增代码块零格式差异（`cargo fmt -p engine --check` 逐文件与 pristine HEAD 基线对照：session.rs 45→45、回购测试文件余量均为 main 既有漂移；未运行 `cargo fmt --all`） | `fmt-check-final.txt` 与 HEAD 对照 |

## 已知边界与遗留

- **处置策略扩展位**：`RepurchaseCompletionPolicy` 仅 `CancelOnCompletion`
  一变体；新增变体时必须同步 `auto_cancel_completed_repurchases` 分派逻辑与
  Web parser 域（枚举文档注释已声明），不得静默落空。
- **在途委托竞态守卫为防御性**：正常引擎时序下市场日终清空订单簿（且买向
  残留与现金预约不变量冲突、显式失败），该守卫覆盖 restore/时序错位场景；
  注销不迟到只延后。
- **名册结算日登记快照冲突**：`established_on = settled_on` 在结算日恰有
  分红／配股登记快照时被既有守卫拒绝（结构性事实冲突如实上抛）——该日提案
  需等下一结算日。
- **`session::persistence` 13 败**：pristine HEAD 独立 worktree 复跑逐项对照
  完全一致（零新增），属既有基线。
- 手动注销 API `execute_issuer_repurchase_cancellation` 保留（过渡窗口内可用，
  自动注销复用同一实现）；未暴露 wasm/宿主命令面（与 M 批一致）。

## 独立复核门禁（CLAUDE.md）

本 diff 须由未实施本批的 subagent 按 CLAUDE.md 复核：大 A 语义依据、最小范围、
边界测试与跨层语义漂移。复核结论回填于本节（或另存
`repurchase-closure-review.md`）。

## 文件清单

- 引擎：`packages/engine/src/company/issuer_repurchase.rs`（枚举+字段+文档）、
  `company/issuer_repurchase_tests.rs`、`session.rs`（established_on/bind-once/
  文案/日终自动注销）、`session/issuer_repurchase_session_tests.rs`（5 新用例+
  既有用例契约更新）、`session/issuer_repurchase_transaction_probe_tests.rs`、
  `session/player_proposals.rs`、`session/company_contract_views.rs`、
  `session/company_contract_views_tests.rs`。
- Web：`apps/web/src/save/schema/corporate-actions.ts`、
  `apps/web/src/save/corporate-actions-schema.test.ts`、
  `apps/web/src/types/generated/RepurchaseCompletionPolicy.ts`（typegen 正规生成）。
- 文档：`docs/trading-rules.md`（回购节 H 批段 + 唯一性口径 + 玩家提案节边界
  作废）、`agents/company-system/{repurchase-closure,current-handoff,
  implementation-checklist}.md`。
