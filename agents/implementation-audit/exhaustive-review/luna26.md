# 任务 20–22 独立全文复核

- 复核基线：产品提交 `08e4fc7`（题定与合并版本一致）；在 `.worktree/implementation-reaudit` 只读核查。
- 完整阅读 `.omo/evidence/company-information-npc-intentions/task-20-review.md`（92 行）、`task-21-review.md`（24 行）、`task-22-review.md`（29 行），均从首行至 EOF，共 145 行；并读根 `AGENTS.md`、`docs/principles.md`。复核计划 K5/K5a/K6 相关章节、当前 typed errors、测试断言及生产 callers。
- 未运行测试/构建，未执行 Git 写入，未改产品文件；本记录是唯一新增文件。

## 章节覆盖矩阵

| 全文范围 | 对照对象 | 复核结果 |
|---|---|---|
| task-20 §1–2（隔离测试数字、三问） | 本次限定产品复核不复跑历史命令；源码范围与计划映射 | 历史数字只作为当时证据，不推定为当前基线。旧审查将任务20模块实现 APPROVE 与“后续生产接线待办”分开，逻辑成立。 |
| task-20 §3–4（K5 锚点、验收/失败电池） | 计划 K5 第136行；`experience/feedback/{inputs,lifecycle}.rs`；`tests/experience_feedback/` | 日期衰减、被套观察、三时钟守卫及typed失败在库层有实现/断言；与当前 Retail legacy caller 不是同一结论，见 G08。 |
| task-20 §5–6（偏离裁决、交接义务） | 计划 K5 与任务22–23、25–26生产接线 | 原文明确派生读数/日期写入要后续接入；任务20 APPROVE 未宣称链路闭环。接线债至今仍可由生产调用确认。 |
| task-20 §7–8（质量探针、结论） | 只接受其提交时隔离复验范围 | APPROVE 对 `4e830c5` 的模块与指定测试成立；不能外推为当前产品完整交付或全链路无缺。 |
| task-21 全文（首轮 REJECT、修复、二审） | `plans/revision.rs:54–85`、`tests/plans.rs:1227` 起 | 超有效期豁免仅传给到期/日终，时间回拨及非生命周期事件仍受守卫；历史不可达缺陷已修，旧 REJECT 不应恢复。 |
| task-22 全文（首轮 REJECT、逐项修复、二审） | `plans/allocation.rs`、`plans/candidates/targets.rs`、`tests/plan_allocation/` | DuplicatePlanId、100股转换约束、半偶tie及u32溢出都有对应实现/测试；拒绝的i128中间溢出测试在公开有界输入下确无可达构造，不构成缺测。 |

## 旧 REJECT 复核

### Task 21：到期事件不可达

旧审查指出事件一律拒绝越过 `last_valid_trading_day`，而 `expire()` 又要求越过期限，导致有效期后无法终止。现行 `TradingPlan::ensure_event_allowed` 将 `allow_beyond_horizon` 作为显式参数；仅 `expire` / 日终清理路径豁免期限，终态和事件时间回拨仍在同一守卫中校验（`packages/engine/src/plans/revision.rs:54–85`）。测试覆盖有效期内误到期为回拨错误，以及观察、成交、修订、暂停越界仍返回 `EventBeyondHorizon`（`packages/engine/tests/plans.rs:1227–1260`）。原缺陷已修复，边界范围没有被扩大。

### Task 22：分配排序确定性、A股手数与舍入边界

旧审查三项均可在当前代码闭环：

- 重复 `PlanId` 在排序前经 `BTreeSet` 检查并返回 `AllocationError::DuplicatePlanRequest`（`packages/engine/src/plans/allocation.rs:44–53`、`allocation/types.rs:68`）；真实测试断言重复项返回typed错误（`packages/engine/tests/plan_allocation/failures.rs:151–170`）。排序末键为 `plan_id`（`allocation.rs:56–63`）。
- 公开目标股数换算拒绝非 `A_SHARE_BOARD_LOT` 参数，并按100股整手向下转换（`packages/engine/src/plans/candidates/targets.rs:47–68`）；不把可配置订单大小与A股申报手数概念混淆。
- 正负评分半偶tie有直接测试（`tests/plan_allocation/gold/scoring.rs:38`）；现金储备分/其他金额tie与本次读到的同套测试需以后续对应条目核对，task-22原文称其存在，但本轮未确认每一项测试名，故不把原报告的“所有tie均已直接覆盖”提升为当前独立证实。此处为证据保留，不构成已复现的新缺陷。
- 股数超 `u32` 显式错误有测试（`tests/plan_allocation/failures.rs:137–149`）。在分子来自有界bp、权益/价格有界i64金额、缩放常数固定时，报告所述的公共输入无法逼近 `i128::MAX`，不要求不可达的中间溢出fixture。

原 REJECT 指出的有效问题已修。task-22 APPROVE 的范围结论可信；对现金舍入tie单项，本次仅依据原文记载，不声称独立找到其断言。

## 计划与生产链逐章核验

计划 K5 明确新经历的失败事件日期、三种时间尺度、每20交易日衰减及长持有低于成本的本人观察判定（`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:130–136`）；K5a 要求分析/信心与经历联动（同文件 `:145–156`）；K6 对本人账户计划续行优先级和真实可用现金预算作出承诺（`:160–170`）。下面区分实现模块、调用构造和生产消费，不用库测试代替实际caller。

### G08 — Retail 日期经历链未接入

**成立，限定为 Retail 生产路径。** dated API 及其typed错误在库层已有，不能说反馈能力未实现；但会话真实 Retail观察在 `decision_snapshot_capture.rs:345–349` 调用只有 `market_minute` 的 `observe_position`，真实成交投影在 `retail_projection.rs:518–528` 调用 `record_fill_with_order`。这些路径未提供含 civil date / trading day 的 `ExperienceMoment`，也就不能向 dated反馈状态登记事件日期并按交易日消费 `failure_influence`。库API存在并不能把 legacy writer 变成 dated writer。

反证边界：机构路径有单独 `InstitutionalFacts(moment)` 分支（`retail_projection.rs:332–338,529`）且有自己的信心/风险逻辑；不把机构遗漏归到 G08，也不建议在机构路径重复扣失败影响。当前可确认的缺口是 Retail 调用者与 K5日期语义不一致，而不是A股交易制度实现错误。

### G38 — 新机会类别在生产请求中未出现

**成立，限账户内部软预算请求分类。** 分配器实现了 `RiskReduction > ExistingPlan > NewOpportunity` 的排序及信心、`PlanId`破同分（`packages/engine/src/plans/allocation.rs:112–137`），类型也有三类（`allocation/types.rs:16–21`）。但实际 `decision_chain.rs:968–978` 卖请求和 `:1232–1240` 买请求都硬编码 `AllocationClass::ExistingPlan`，请求来自活跃计划的下一子单；生产链没有把新机会作为相应类别传入。因此`NewOpportunity`排序分支无法由当前生产请求触发，不能据分配器单测宣称K6分类优先级已兑现。

反证边界：这只证明同一账户请求分类未接上，不证明跨账户共享预算或撮合顺序有错；同样不将 `AllocationExperience::default()` 独立报成缺陷，机构经验可以已在上游消费，重复扣减会改变语义。

## 新候选与反证

- 未确认超出 G08/G38 的新产品候选。typed失败、去重排序、手数转换及交易计划期限边界均有具体守卫或测试，不因旧审查过程曾有缺陷而重复登记。
- task-20 的“全部测试断言存在”是历史评审对当时范围的结论；当前本轮只核验上述定向断言和caller，不把未运行测试表述为通过。
- task-22 对金额舍入tie的全面断言覆盖，本轮未逐项定位到现金储备tie测试，列为证据限制待查，不足以单独认定遗漏。

## 结论

Task 21、22 首轮 REJECT 所列修复在当前基线上可由实现与边界测试证实；其二审 APPROVE 没有掩盖已修复问题。Task 20 的模块级 APPROVE 仍然成立于其提交范围，但日期经历到 Retail 会话调用链尚未接通（G08）；K6 allocator 新机会分类在生产请求也仍全部落为 `ExistingPlan`（G38）。本轮静态复核未运行测试，不提供运行态通过结论。
