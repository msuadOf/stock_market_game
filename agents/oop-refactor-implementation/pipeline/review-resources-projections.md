# Pipeline resources 与 projections 独立复核

- 复核日期：2026-10-03。
- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`；检查工作区相对该基线的完整 diff。
- 复核者：独立 subagent `review_resources`，未参与本批实现。
- 权威范围：`assigned-actions.json` 中 `engine-pipeline-01-A01`、`engine-pipeline-10-A01`、`pipeline-R2-N05/N07/N11/N12/N13`。
- 限制：未修改产品文件，未运行 Cargo、编译或测试，未派生其他 agent；结论属于源码与测试的静态复核，不代表运行验收通过，也不能据此确认 TDD 的红绿历史。

## 检查范围与依据

全文读取并检查以下文件及其完整 diff：

- `account_validation.rs`、`account_validation_tests.rs`。
- `transition.rs`、`ledger_validation.rs`；后者相对指定基线没有 diff，作为独立 validator 对照全文读取。
- `settlement.rs`、`settlement_tests.rs`、`account_settlement.rs`、`account_settlement_tests.rs`。
- `retail_projection.rs`、`retail_projection_tests.rs`、`retail_projection_persistence_tests.rs`。
- `decision_snapshot_capture.rs`、`decision_snapshot_capture_tests.rs`、`decision_snapshot_tests.rs`。

另读取 `decision_snapshot.rs`、`institutional_experience_projection.rs`、NPC queue/candidate commit 的相关生产调用；核对既有 `transition_tests.rs`、账户 driver 的资源竞争/原子重试测试和 ledger 的卖费、交付、链校验测试。

依据为仓库 `AGENTS.md`、`docs/principles.md`、相关架构与开放问题、ADR-0017 顶部修订及数据流/分歧 #9、ADR-0018 §7、`docs/trading-rules.md`。现行规则文档列有交易所 2026 版规则与适用日期，并明确中国结算费用表最近访问 404 的证据边界。本次没有修改交易制度、费用参数、撮合次序或官方规则适用范围，因此没有开展新的联网官方材料复核；不把卖费封顶描述为真实交易所清算规则。

## 结论

本次指定范围未发现需要修复的有效问题，可以通过独立静态复核。依据可追溯，改动符合既有大 A 语义及明确登记的游戏简化；实现符合 assigned-actions 的 receiver/value object 迁移范围，没有新增交易限制、持久化权威副本或依赖。

1. **AccountBudget 的单位与不可回补边界保持。** `account_validation.rs:1116` 从原不可变 `DecisionResourceSnapshot` 读取现金；逐股股份仍为 `u32`。`adopt_cash_lane`（1148）只写 Money 现金，`adopt_shares_lane`（1152）只写对应 StockCode 的股数。worker 合并仍在可丢弃 round，首错导致整轮不提交；Cash/Share/Cancel 调度、two-pass OrderId 与拒绝不占预算行为没有改变。未新增读取 P4 释放或实时账户余额的路径。
2. **卖费 cap、分项顺序与历史欠费保持。** `transition.rs:197` 将原算法逐句迁入 `SellerChargeAllocation`，仍按累计 nominal 减 charged 得到 unpaid，逐腿以 gross 封顶，依次 commission → stamp_tax → transfer_fee，净交付仍非负；卖单现金 envelope 与 spent.cash 恒为零。`ledger_validation.rs:291` 继续独立复算 cap 和优先级，没有共享 producer 的结果代替校验，也未删除累计字段、交付或资源方程。
3. **Settlement 的 checked 首错顺序保持。** `settlement.rs:151` 仍按 gross、commission、stamp_tax、transfer_fee、qty 顺序累计，再检查 receipt count；非 Fill/零数量 Fill 的 skip 位点不变。账户并行、账户/股票结果整理、同账户同股 Buy-before-Sell、clone_for_shadow 与 applied_groups 顺序不变。准备全部成功后才返回账户 patch；两个经验 projection 成功后才安装事务容器。`account_settlement.rs` 的 BeliefParticipantState 接线只替换 belief，保留该 participant 的其他状态。
4. **AccountFillProjection 不重新结算。** 平均价继续以 cents/share 向零截断，实际费用从 charged 累计，机构 invested/recovered 与实际 fee history 的清仓收益逻辑保持。`retail_projection.rs:418` 先扫描全部 job 的 Result，`finish`（624）仅保存 `final_position_error`，caller（422）随后处理对账错误；未把较早账户对账错误提前到另一个账户的 Fill 错误之前。watchlist 仍按 Settlement 后持仓修剪。
5. **机构模式与 moment 一起传递。** 私有 `InstitutionalFacts(ExperienceMoment)` 取代分离的 Option/expect；两个生产入口签名不变，不新增运行时合法状态禁令。civil_date、market_minute、trading_day 原样传入 dated Fill。
6. **capture 仍保持同源事实及原失败时点。** `CapturedExperienceObservation` 可表达 NoExperience、Retail experience/risk/self positions，以及非 Retail experience/self positions/无 risk。观察 equity 和 position peaks、prune、SelfView、risk、strategy 的顺序保持；`decision_snapshot_capture.rs:400` 的 SelfView 先于 risk，risk 先于 strategy。snapshot/urgency 校验成功后才写回 experience；capture 的生产调用位置仍为完整 candidate 的下一 tick NPC queue，不实施额外观察版本迁移。
7. **SelfView 现金含义没有漂移。** `SelfViewCashReservations::record`（436）先累计 reserved 再按原阶段标记累计 replaceable，`available_cash`（444）仍为 raw_cash − reserved + replaceable。连续/开盘可撤窗口由 caller 判断，closing/PreOpen 不变；SelfView 的 replaceable 现金没有进入 P1 执行预算。

## 测试与证据边界

静态读取确认保留已有跨股票现金竞争、同股股份竞争、独立 Share worker、拒单不占预算、fatal 重试、卖费欠费追收、独立 ledger 伪造拒绝、Fill 幂等与保存恢复前缀、Buy-before-Sell、job 错误先于 final_position_error 等覆盖。

新增测试固定了卖费各分项 cap 边界及组件超额/合计 overflow；多持仓 equity、更新后 peaks、market_minute 和 T+1 SelfView；非 Retail experience 合法组合；缺市场/资金 overflow 不安装 experience；SelfView/risk/strategy 的错误优先级；各阶段混合工作单可替换子集；平均价截断、多股票 watchlist、机构缺实际 fee history、越量卖出、零量 Fill 的既有局部边界；Settlement 后续坏 receipt、逐项实际卖费累计和四容器失败原子性。

未发现由本次提取引入但缺少必要边界保护的场景。N07 建议的 producer/validator 全分项参数化对照没有新增统一测试，目前是 producer cap 边界测试与保留的独立 ledger 优先级/交付拒绝测试分别覆盖；这是可补充证据，不构成本次算法逐句迁移的阻断问题。零数量 Fill 在局部 Settlement 测试中保持 skip，而 upstream ledger `validate_audit` 会拒绝无 cumulative movement 的 Fill；新测试没有把该局部 skip 扩大为生产合法收据政策。

所有测试均未由本复核者执行。实现方仍须报告实际测试命令、并发参数、deadline 和结果；此复核不能替代该运行证据。

## 最终 SHA256 证据门禁

由原独立复核者 `/root/implement_pipeline/review_resources` 亲自计算并签认当前文件 SHA256，见 [review-resources-projections-manifest.json](review-resources-projections-manifest.json)。完整基线 diff 固定在 [review-resources-projections-final.diff](review-resources-projections-final.diff)，其 SHA256 与每个文件的 diff SHA256 均登记在 manifest。

全部 13 个实际 changed 生产/测试文件的当前 Git blob 与上次完整 diff 工具输出的 new blob identity 一致，diff 行数统计也一致，确认自上次审查未发现源码或测试内容漂移；因此不存在待补读增量。`ledger_validation.rs` 仍与基线逐字相同，列作独立校验对照。上次未保存 SHA256，本次没有编造旧 SHA256，而是明确记录先前工具输出的 blob prefix 与当前完整 blob/SHA256。

本门禁只新增/更新 review 工作文件，未修改产品源码或测试，未运行测试；parent 的全验证不由本记录重复执行或冒认。
