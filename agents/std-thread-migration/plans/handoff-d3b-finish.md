# 交接：D3b 收尾（第 9 任→第 10 任，2026-10-16）

## 1. 当前状态

- **HEAD**：`5a4040ca`（refactor: D3b 件 4 基建——复审宿主 trait 化）。
  工作区**干净**（git status 非未跟踪=0；.tmp 不入库）。
- D3a 九轮整改全部闭合（九审通过）。D3b 进度：件 1 裁剪✓、件 2 注入
  通道✓、件 3 撤销路由✓、件 5 个人态注入✓、件 4 基建✓+接线暂缓。
- 数字基线：默认 lib **1602/0/11**；env=1=**39**（名单见
  `.tmp/sol-mid-d3-red-evidence-r3/env1-d3b3.txt`：chain_restructure 11 +
  其他 28——后者多属 D5 域）。翻转批 1-5 = 10/13/14/14/15 全绿。
- 本会话提交链（D3b 段）：f7f4c85a(件1)→e83f091e(件2/3)→3d638cb2(件5)
  →5a4040ca(件4基建)。

## 2. 件 4 剩余：校准接线

**根因已定位（勿重新排查）**：组内缺 `record_review`/quiet 门控——
session 版 `apply_plan_lifecycle_actions`（decision_chain.rs:560）对
每个动作**先** `plans.record_review(plan_id, day, price, acquired)` +
`record_resource_review(plan_id, resources)` 再 apply；quiet 判定
（lifecycle.rs:523-537）依赖簿内 `last_review_signal_score_bp /
last_review_price / last_review_acquired_count / last_review_resources`
与本次比对——组内未做则每拍都"非 quiet"→每拍修订、计划身份漂移。

**修法**：group.rs 的 `apply_lifecycle_action`（已起草，allow(dead_code)）
在 ExecutionState/Revise/Terminate/Observe/Restructure 每分支 apply 前
补两行 record_review（输入=host.review_facts + host.review_resources——
session 版 decision_chain.rs:576-589 同式）。

**接线位**：run_chain_decision 内 observe 之后、开仓循环之前——被回退
的消费块形态见 git 历史（`git show 5a4040ca` 前一版工作树；或在
`let assessments: Vec<_> = ...` 之后重写）：组装 PlanLifecycleReview
{account, market, plans, held_by_code, equity, belief:&out.belief,
chain_params, trading_day, lot} + GroupReviewHost → collect → 逐条
apply_lifecycle_action。

**回归 2 例复现路径**（接线后必须全绿再提交）：
- `d3b3_paused_plan_with_child_routes_targeted_cancel`：红形态=复审撤买
  分支（Buy+score≤0→Terminate）抢先终止 Paused 计划，quote 前置分支
  （件 3）拿不到 Cancel。修 record_review 后 score 波动被 quiet 门控
  吸收；若仍抢先，检查 paused 分支顺序（lifecycle.rs:355 Paused 分支
  在撤买分支之前——复审对 Paused 计划走恢复评估不走撤买）。
- `solmid_execution_leg_lifecycle_matches_independent_fills`：红形态=
  独立源对拍多出 (AccountId(4), StockId(0)) Buy filled=0——每拍修订/
  重开仓导致计划身份漂移。record_review 修复后 filled 语义应回稳。

## 3. GroupReviewHost 起草位与调试发现

- 位置：group.rs `struct GroupReviewHost<'a>`（约 :2678）+ impl
  PlanReviewHost（urgency_at_view 的组内实现——风险观察
  build_account_risk_observation+assess_existing_reduction_urgency；
  institution_behavior_shared_host——成本基自组账户 invested/recovered
  推导）+ `apply_lifecycle_action`（:约1917 前）。
- **零量条目过滤**（调试发现）：组 shares 含清仓零量面——
  RiskPositionInput 要求 qty>0，不过滤则 panic
  `ZeroPositionQuantity`（RiskPositionInput 校验）。
- `assess_institution_behavior`/`InstitutionBehavior` 已提级 pub(crate)
  （institutional_behavior.rs；session.rs 有 re-export——组内路径
  `crate::session::InstitutionBehavior`）。
- PlanLifecycleReview 已 pub(crate) 化（字段 pub(crate)——组内直接结构
  体字面量组装；session capture 保持私有构造）。

## 4. 11 例 fixture 适配清单与三注入通道

**通道用法**（全部 driver 方法，cfg(test)，下行组 0）：
- `driver.install_personal_for_test(local, snapshot)`——完整个人态六件
  快照（BeliefParticipantSnapshot；从 collect_belief 读面克隆后改）。
  session 版 personal.install 的等价。
- `driver.apply_plan_events_for_test(vec![(PlanId, PlanEvent)])`——组簿
  事件（Paused/Revised/Terminated...；session 版 commit_injected_plan_
  roots 的计划事件面）。
- `driver.force_due_for_test_at(&[account], tick)`——强制到期。

**chain_restructure 11 例**（env1-d3b3.txt 名单）分两类：
- 撤销/暂停族（约 6 例：mid_day_termination×2/mid_day_reversal/
  urgency_acceleration/urgency_paused_child/urgency_recovery_neutral）：
  fixture 改造=seeded 场景后经 install_personal 注入修订 belief → 强制
  到期 → 断言 OrderCanceled/计划状态。件 3 路由已就位。
- 复审族（约 5 例：institution_small_signal/revisions_patience/
  net_profitable_exit/known_drawdown/boundary_mixed_pause）：依赖件 4
  接线完成（真实复审路径跑通）后适配——session 版断言的
  plan.review() 字段组内簿全有（同 PlanBook 类型）。
- 注意：session 侧 `personal.install`+`commit_injected_plan_roots` 在
  runtime 下**被组投影覆盖**（每拍 set_runtime_personal_state 整态
  覆盖）——必须走组内通道，改 session 侧无效。

## 5. D5 范围提醒（D3b 后）

env=1 的 28 例非 chain 失败多属 D5：persistence/saved_runtime（3）、
epoch_alignment（1）、retail_analysis（1）、attention（3）、
authoritative_tick（5）、t3_boundary（3）、plan_owner（3）、
committable_state（2）等——旧管线专有事件面/staged 机制/存档域。
已知欠账：retail belief 链未迁移；runtime_hold_experience_init 入场
参考价近似（buy_vwap 已修——五审；加仓经验迁移仍近似登记 D5）。
代码位：drive.rs step_with_capture（投影链）、failure.rs
runtime_install_baseline（回滚面）。

## 6. 已知坑汇总

- **红证格式硬性**：完整 cargo test 输出（断言原文+left/right 或 panic
  原文）+命令+源码 SHA（突变型注明基准+突变；测试提交先于实现提交的
  可审计形态）。空文件/纯汇总=自动打回；两人栽过。
- **共享面提取完整性**（件 3 教训）：撤买/暂停分支留在 session 包装器
  （generate_next_plan_quote 头段）——共享 decide_plan_quote 不含该面，
  组内直调不触达。提取共享函数时核对**全部调用点**的分支覆盖。
- **GameSession::new 清 staged 槽**（防串扰）：注入通道测试的孪生构造
  必须先于主 queue——红绿必须同 fixture 同构造序。
- **flake 家族**：npc_decisions（queue_attempt_seq 竞争）、watchdog、
  panic_transport 偶发 ±1——单跑复验绿即登记，勿当回归。
- **fixture 前提类坑**：fault1015（order_size 须 lot 整数倍）；inst 无
  company setup 的 fault；belief 位下标=id_map 位置（玩家 0/retail 1/
  inst 2..）；玩家 grant_position 在组投影外（世界账户不感知——
  用 retail 对手位）。
- **铁律**：报告"覆盖"只写断言到的面（七审勘正过）；突变红 fixture
  判别力受限时如实登记（05-foreign 先例）。
