# fixtures2 实施状态

本批实现仅调整 integration tests 的 fixture 状态所有权与调用；不新增生产 API，不改交易规则、单位、会计重述或披露排期。

已阅读根 AGENTS、principles/testing/architecture/open-questions，以及 ADR-0002、ADR-0016、ADR-0019、ADR-0025；本批没有对开放产品问题定路线。各动作文件全文阅读后实施。

## 逐动作

| 动作 | 状态 | 文件 | 状态 owner / 方法 | 真实 caller | 定向行为测试（待 root 执行） | 未完成 |
|---|---|---|---|---|---|---|
| hosts-R2-N05 | 独立静态复核通过；Rust待验证 | packages/engine/tests/civil_clock.rs | SpringFestivalScenario 拥有 GameSession 与 registered_due；new_spring_festival、own_due；日结仍显式调用 GameSession | 同文件原春节场景 9 处消费者；保留原地 T+1、休市、RNG、错误 rollback 和 observer retry 断言 | civil_clock / skipping_a_day_with_unprocessed_dues_is_rejected；rejected_day_end_preserves_disclosure_observer_for_retry | root 编译及短测 |
| hosts-R2-N06 | 独立静态复核通过；Rust待验证 | packages/engine/tests/fundamental_beliefs/main.rs、centers.rs、failures/mod.rs、gold.rs、per_share.rs | FundamentalBeliefCase 拥有 Scenario、NpcInformationState、BeliefMarket、BeliefBook 与 BeliefIssuerInputs 具名发行人输入；new、acquire、apply_cause；每次临时构造 context/inputs | gold 2 用例、failures 4 用例、per_share 行情 spy 与 centers serde 用例 | fundamental_beliefs / failures::unacquired_material_is_rejected；gold::earnings_multiple_end_to_end_gold（另有 per_share::float_shares_never_enter_valuation_and_market_untouched） | root 编译及短测 |
| hosts-R2-N08 | 独立静态复核通过；Rust待验证 | packages/engine/tests/industry_reports/correction_restatement.rs | CorrectionScenario 拥有 Books、ClosingEngine、MemberId 与原报告字节；through_correction、close_month、close_year | correction_does_not_leak_into_later_periods；serde 对照仍显式解出 Books/ClosingEngine | industry_reports / correction_restatement::correction_does_not_leak_into_later_periods；correction_restatement::restatement_worksheet_survives_serde_round_trip | root 编译及短测 |
| hosts-R2-N10 | 独立静态复核通过；Rust待验证 | packages/engine/tests/plans.rs | PlanScenario 拥有 PlanBook 与主 PlanId；new_buy、from_open、plan、accept_and_fill(OrderId, qty, trading_day, child_complete)；先 Accepted 后 Filled | 原 book_with_buy_plan 与 link_and_fill 消费者；serde 多计划在初始 fill 后显式解出 book/id | plans / real_fills_reaching_share_target_complete_the_plan；fill_beyond_target_is_rejected | root 编译及短测 |
| hosts-R2-N11 | 独立静态复核通过；Rust待验证 | packages/engine/tests/publications/failures/mod.rs、announcement_failures.rs、publication_failures.rs | Base 继续独占 ClosingEngine/PublicLibrary/scope/排期输入；valid_q1、publication_request(period) | 所有原 base/request 消费者；错误输入仍由各测试显式篡改 | publications / failures::publication_failures::correction_link_rejected_on_mismatch；failures::announcement_failures::announcement_timing_rejected | root 编译及短测 |
| hosts-R2-N12 | 独立静态复核通过；Rust待验证 | packages/engine/tests/session.rs | TestOrderSaveFixture 只持有一个 SaveSlot；new、from_save、save_mut、add_position、with_resting_sellers、消费式 restore。restore 内同步 envelope/cursor 并保留原 reservation 断言，只 restore 一次 | 原同步函数 6 入口（卖方批量构造并入方法）；所有原 session_with_resting_sellers 消费者；player_session_with_position 使用 builder 编辑 position | session / order_save_fixture_restores_envelopes_cursors_and_reservations；continuous_multi_fill_charges_one_minimum_commission_per_account_batch | root 编译及短测 |

## 验证记录与限制

- 12 个源文件均是 actions.files；没有新增 fixture 源文件，没有 Git 写操作，没有运行 Cargo 或完整回归。
- rustfmt 直接解析/格式化 12 个源文件通过；git diff --check 通过。此证据只是格式/语法检查，不是类型检查或测试结果。
- 静态核对原有 190 个相关 #[test] 名称全部保留；原 assert/assert_eq/assert_ne/panic 数量全部保留。session 新增一个针对跨投影不变量的短 case（5 个 assert_eq）：同账户买/卖挂单的现金/股份预留、2 个 envelope、稀疏 FIFO seq 与历史 cursor 不回退。
- 新增 fixture 契约用例尚未实跑，未取得 TDD red/green 执行证据；生产重构未在此批新增。实际 Cargo 构建与所有定向短测由 root 统一运行。
- 根据 domain 消息，将真实 Account/Position reads 改为 getter（civil_clock/session），同步 TradingPlan 同名 getter（plans）；SaveSlot、AccountSnap、PositionSnap 同名字段保持 DTO 访问，未将 fixture 合法同步带入外部坏档恢复路径。
- 原测试的 current save/checkpoint 断言仍使用既有内存 SaveSlot API；不以本 fixture 声称存在日内用户持久化。
- 独立完整 diff 审查已通过，报告见 ../review-fixtures2.md；F1及两个兼容增量再次复核通过，无未修有效发现。Rust 类型检查及短测尚未执行，当前不宣称最终完成。

## 独立复核修订轨迹

- 复核指出 civil_clock 的 rejected_day_end_preserves_disclosure_observer_for_retry 英文注释被机械替换出无意义的 scenario.session 文案。本次仅将该用例 3 条 Given/When/Then 注释改为自然中文，未全文件翻译，未改行为。
- 主动收敛 FundamentalBeliefCase::new：使用 BeliefIssuerInputs { kind, total_issued_shares } 表达发行人估值口径，参数从 8 个减少到 7 个；全部 8 个 caller 保留显式 CompanyKind::Industrial / ISSUED_SHARES / 原 RNG 和获知时点，不增加 allow。
- 另组实施后 TradingPlan 字段转为私有。本文件同步同名 getter；原 restore fixture 的 terminated.status 单字段直写，改为 serde JSON 只设置同一 status 值后恢复。没有应用额外 PlanEvent，没有改变其余字段或 from_parts 校验输入。
- 上述增量经过 rustfmt 和 scoped diff --check；未运行 Cargo。已向原 reviewer 发送增量复核任务，修后独立复核已通过。

- 六项结构化台账同步写入 status.json；均标为独立静态 review 通过、Rust root 验证待执行。

## Root 最终代表性验证（2026-10-03）

本组最终状态以 status.json 的 final_validation 为准。root build07 与 all-targets check08 均通过；root 选定精确 Rust case 的逐条结果已按 source 映射，WS 两个 bind sandbox EPERM 保留初始失败记录并由沙箱外原断言 2/2 通过核销。Writer 5/5 与桌面 CLI 5/5 通过。未选中的旧测试、完整 suite、API doctest（actors）及真实 matrix/E2E/性能未据此声称执行。历史“worker 未运行/待 root”段落保留为过程证据，当前没有未关闭实施或独立复核发现。
