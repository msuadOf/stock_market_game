# E9 临时容器 churn 进度档（判据已过，试点在途）

## 阶段一：churn 地图（判据通过）

数据源：`.tmp/expl-hotpath2/`（callgrind fair-sched 432.5M IR + gdb 1811 组采样）+ 本轮补充分析（脚本与输出在 `.tmp/e9/`）。

**callgrind 归属链**（400 tick 窗口）：
- SaveSlot 析构 142.7M IR（33%）← Arc::drop_slow ← BTreeMap::drop（嵌套）
- clone_for_tick_shadow 双调用方：protocol checkpoint 211.2M + pipeline::plan_tick 111.7M（合计 ~75% IR）
- 其余 churn：BeliefBook 26.2M+15.5M、CompanyOperations 23.2M、ExperienceFeedback 22.1M、SaveSnapshot 10.7M、CommittableSessionState 26.1M

**线程归属**（gdb 1811 采样，4 worker）：
- actor（串行脊柱）清醒 654 采样中：clone_subtree 138 + drop 71 + dying_next 29 + drop_slow 14 + sdallocx 16 + tcache_flush 13 + memcpy 21 + clone_for_tick_shadow 18 ≈ **churn 占脊柱清醒时间 ~54%，占总 wall 采样 ~19.5%**
- worker（并行车道）叶帧大头是睡眠/自旋/get/验证，churn 极少 → 与 P0-dirtykey 教训一致：车道 churn 不值钱，**脊柱 churn 才是刀口**
- 判据 X ≈ 19.5% ≥ 4% 门槛 → **方向活**

**双克隆现场**（`session/protocol/civil/session.rs:263-283` `step_frame`）：
1. `self.checkpoint()` → `try_clone_for_checkpoint` → `clone_for_tick_shadow`（深克隆整个 GameSession，脊柱）
2. `game.step_frame()` → `execute_authoritative_tick` → `plan_tick` 内再克隆工作影子
3. 成功路径 `drop(checkpoint)`——保险克隆整份当场丢弃（脊柱析构 = SaveSlot drop 大头）

## 阶段二试点：惰性 checkpoint（在途）

方案：step_frame/step_frame_with_commit_evidence 的 checkpoint 只克隆便宜字段（fact_cursor/published_runtime/pending_save_candidates/day_end_save/intraday——全是 Arc/浅），game 不克隆；失败回滚=恢复便宜字段+清 poison。语义依据：shadow 架构保证"权威状态只接收最终不可失败的 CommitTick 交换"，失败路径权威未被改动（除 poison 标志）——用红灯测试锁住："失败 step_frame 回滚后 game 与 tick 前引用深相等"。

- [x] 判据与地图
- [ ] 红灯测试：失败回滚后 game 深相等（先证今天靠全量恢复通过）
- [ ] 惰性实现（只动 step_frame 热路径；end_civil_day 等边界保持全量克隆——用户明确边界无所谓）
- [ ] 定向短测（checkpoint/rollback/poison/civil session 全 filter）
- [ ] 正式 probe + verify-build + 锚定交错配对（worker=4、<10% 占用核段、≥5 对）
- [ ] 独立复核（无派生权限则报主会话）
- [ ] 提交或撤回 + findings + current-state

## 风险登记

- 若审计发现有隐藏的"提交前权威变更"路径（ingress fork、verification_evidence），试点失败撤回，如实记录。
- rollback 语义变化点：今天回滚把 game 也换回克隆（健康态），惰性版改为清 poison——两种语义若在既有测试里有分歧（如 game 内其他字段被动过），红灯会暴露。
