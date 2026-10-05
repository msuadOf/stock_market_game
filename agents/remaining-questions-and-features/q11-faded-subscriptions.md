# Q11 公开订阅与 G43 淡出契约复核

G43 的现行已决范围是：候选仅来自本人持仓、关注列表、活动计划或本人重新发现；历史 `BeliefBook`、已知材料和经历保留，不会自动恢复候选资格。原始证据见 [luna03](../implementation-audit/exhaustive-review/luna03.md)，既有真实 Root 的修复与独立签核见 [个人策略复核](../implementation-gap-implementation/personal-strategy-review.md)。作者已全文阅读这些记录及正式 K6 相关契约。

Q11 提交 `2bf5b2a` 增加日终即时订阅时，在 `deliver_public_information` 中无条件将 `belief.entry_stocks()` 加入订阅集合。该行让已清仓、无关注条目、无活动计划的历史信念股票继续获知新公告，违反 G43，并非获得授权的新产品规则。总账独立复核指出这一问题，Q11 非作者复核者重新检查后确认有效并承认之前漏审；不能以原先9个短测通过推翻本次发现。

## 代表性短测与生产范围

先新增 `session::notices::tests::day_end_information_subscription_requires_current_holding_watchlist_or_active_plan`，保持错误生产行不动，由 Root 统一编译并记录真实业务红。该 case 使用4个独立小 `GameSession`：

- `faded`：曾经实际获知并建立本人信念，随后无持仓、关注或活动计划；新 `ContractWon` 来自真实 `CompanyOperations`，通过公开 `end_civil_day` 的18:00派发，不能进入本人信息集；旧信息和信念完整保留。
- `held`：真实开局流通股份分配建立持仓，移除关注后仍须接收相关消息，不以测试伪造成交建立持仓。
- `watched`：只保留本人的真实关注条目，仍须接收相关消息。
- `active_plan`：无持仓/关注但仍有实际 `PlanBook` 活动计划，仍须接收相关消息；计划不被伪装成成交或冻结资金。

每个分支保存并完整恢复 `GameSession`，核对本人信息及信念。Root实际执行1个case，在0.20秒内出现业务红：淡出者的新公告首次获知时点为 `Some(2030-01-05 18:00)`，应为 `None`；日志 `.tmp/checklist-wave3/q11-faded-subscription-red.log`。这不是编译失败或0项测试假绿。生产修复仅去掉历史信念的无条件订阅兜底，不删除旧信息、替换信念或弱化正例；进一步在公开前明确断言各分支真实资格，持仓正例必须恰有开局100股、关注与活动计划正例必须实际存在，淡出者三类资格必须全部不存在。Root通过 `build8 --jobs 32` 刷新编译（23.47秒），随后以exact名称执行此case，四个分支及各自完整恢复全部通过；日志明确为1 passed、0 failed，执行1.62秒，证据 `.tmp/checklist-wave3/session-notices-tests-day_end_information_subscription_requires_current_holding_watchlist_or_active_plan-stage-final.log`，符合普通测试10秒硬上限。

同时复查其余取材：Institution/ Retail 通过 `root_candidate_codes`，其集合为本人持仓、关注、活动计划与本人实际重新发现，没有历史 `belief` 兜底；公开获知因果记录只在正式新增本人 `Acquisition` 后写入；交易诊断的来源报告只来自当次真实候选使用的报告。`decision_chain_diagnostics` 的历史信念数量计数只是只读展示，不会登记获知或创建候选。

## 门禁与限制

真实业务红、单行生产修复及绿色重跑均已取得证据。非作者已完整审查修复、测试及两份记录，并亲读最终绿色日志，确认实际1项通过、真实红证据保留且无并行任务污染，完成本修复独立门禁；`git diff --check` 通过。提交由 Root 统一收口，仅包含 `notices.rs` 和两份Q11工作记录，不混并行的宿主receiver或Q22改动。不改变 A 股撮合、T+1、金额、股数、公开时点、资金来源或日终持久化；没有新增存档字段、版本或兼容逻辑。本修复只做代表性短测，不执行完整回归。
