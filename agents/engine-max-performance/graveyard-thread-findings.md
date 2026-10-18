# 批级 checkpoint 析构移交专用线程（graveyard）findings

日期：2026-10-07。依据：串行脊柱研究 section-3（诊断 A/B 实测 +4.1%，`batch_checkpoint_drop` ~132µs/tick 全为旧账户页 jemalloc free）。

## 实现

- **engine 侧**（`packages/engine/src/session/deferred_drop.rs`、`session.rs`）：复用 71dd7825 的池外专用线程基建（OnceLock 惰性线程 + mpsc + 发送失败退化内联析构），零新线程系统。公开 `engine::session::defer_destruction`（所有权移交入口）与 `#[doc(hidden) deferred_destruction_count]`（宿主测试观测）。
- **server 侧**（`apps/server/src/actor.rs` `run_protocol_batch` 尾部）：成功路径把批级回滚锚点 checkpoint 移交专用线程析构。错误路径仍走 `rollback_cycle`（消费 checkpoint 做回滚）不变。析构无观察副作用，失败原子性/首错序零影响。
- 架构合规：无亲和性钉扎、无池内许可预留、平级 OS 线程（用户"位置性预留也算硬编码"原则的形态 b）。

## TDD

红→绿：`batch_checkpoint_drop_is_deferred_to_dedicated_thread`——**零 tick 批**（无 commit 移交干扰）下 `deferred_destruction_count` 增量 ≥1，专证 checkpoint 交接为生产形态（试验 A"测试形态≠生产形态"教训的直接应用）。红灯为行为失败（增量 0），非编译失败。

## 全量对照

- engine lib：**1321 过/8 败**，失败名单与 C 移植基线（`.tmp/e-c-port/port-fails.txt`）**逐字一致**。
- server lib：35 过/0 败（含新测试）。

## 基准（锚定交错 astart,(h,c)×5,aend，核段 32-47 起始占用 0.9%，worker=4，3000 tick）

| | 数值 |
|---|---|
| 基线 SHA | 129e5dfc…（HEAD 6114ae13 隔离 worktree 构建） |
| candidate SHA | 7c8a72f5…（工作区构建，verify-build 通过） |
| h1..h5 | 305.55 / 314.31 / 319.33 / 300.93 / 312.47 |
| c1..c5 | 330.80 / 305.13 / 330.44 / 329.21 / 333.08 |
| 锚漂 | −1.44%（≤5% ✓） |

- d=C/H = [1.0826, 0.9708, 1.0348, 1.0940, 1.0660]；**mean(d)=+4.96%**、**4/5 正**；mean−1=4.96% ≥ 2SE=4.42% ✓；区间 H[300.9,319.3] vs C[305.1,333.1] 未分离（**如实登记**；判定口径与 worker=4、试验 C 移植、X1 翻正一致：≥2% 且 ≥4/5 且显著性过）。
- 与研究 +4.1% 诊断一致（本批略高在波动带内）。正式口径 ~321x → **~337x**（推算）。

## 遗留与登记

- mpsc 无界队列无背压：与 71dd7825 同一边界（每批一个 checkpoint，析构远快于提交，见 deferred-drop-thread-findings.md 边界备注）。
- 独立复核待主会话安排（重点：engine 公开入口最小性、零 tick 批断言充分性、错误路径不变）。
- 研究路线 2（删外层构建复用首步内部 checkpoint 为批锚点，+0.4%）未做——收益小，待后续顺手。
