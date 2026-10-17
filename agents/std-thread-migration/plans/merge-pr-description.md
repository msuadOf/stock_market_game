# PR 描述骨架：exp/std-thread-model → main（std-thread 运行时迁移）

> 用途：合并 main 时的 PR 描述底稿。`<PLACEHOLDER>` 共 6 处，合并时以终测/终审数字回填；
> 「已归档参考值」仅供回填时核对量级，不得直接冒充终值。行文所引归档均相对仓库根
> `agents/std-thread-migration/`。

---

## 1. 总述

本分支把 engine 的 tick 执行模型从 rayon 线程池整体迁移为 **std::thread 所有权转移运行时**
（ADR-0043）：G 个 NPC 组线程 + N 个每股交易部线程，现金/股票以可 move 实体
（`Entity` 五类）在下单时**实际扣出**随消息转移（非 escrow 预留），全 `std::sync::mpsc`
零锁零共享，tick 两阶段（组内决策∥ → 交易部撮合∥ → commit 前 `converge_round` 唯一
同步），失败原子性由三层 working overlay + 单点裁决承载；代码本体零线程数概念，CPU
限制交 OS（cgroup/taskset）。迁移背景是 rayon 宽度税与串行脊实测（128w 档 −43%~−63%、
16 核仅 42% CPU）。语义侧以「A 股语义映射表 30 项 + Abort 回滚等价 + 现金/股份守恒 +
T+1/整手/费用完整口径」为合同，13 条有意分歧逐条登记（trading-rules.md + ADR-0042）；
翻转批曾暴露 106 个 session 侧失败，经全局域迁移（D0-D5）清零后 `step()` 缺省已翻转
std-thread runtime（`ENGINE_STEP_RUNTIME=0` 为逃生门，wasm32 恒串行回退）。翻转后性能
一度回退（裸口径 71 t/s、全语义 4.3 t/s），Sol 归因终裁认定 70–93% 为可回收实现浪费
而非方案必要代价，P0–P2 恢复战役落地后性能账本见 §2。

## 2. 关键数字（合并时回填）

| 项 | 旧值 | 新值 | 依据/已归档参考 |
|---|---|---|---|
| 分支规模 | — | 272 commits / 910 files（+200824/−25905），merge-base `a6001494`；类型分布 feat 59 / fix 55 / docs 64 / refactor 30 / perf 28 / test 27 / chore 4 / build 1 | `git log main..HEAD` |
| 翻转批 env=1 失败 | 106 | 0 | `evidence/flip-106-failures.txt` → `evidence/env1-current-full.txt` 审计链 |
| 生产缺省 | 旧管线 | std-thread runtime（`step_runtime_enabled` 缺省 true，翻转提交 `7419c39b`） | `packages/engine/src/runtime/mod.rs` |
| 全量回归（lib，默认与 env=1 双绿） | — | `<REGRESSION_FINAL>`（已归档参考：1613 通过 / 0 失败 / 11 ignored） | `evidence/perf-p2/p2-3a-green.txt` |
| 口径 A：裸引擎探针（runtime_s5_probe，3000 tick/20000 NPC/5 股） | 71 t/s（翻转后复测） | `<PERF_A>`（已归档参考：P1-B 后 mean 1253.5，轮次 1220.9/1296.8/1242.8） | `evidence/perf-p1/MANIFEST.md`；独占复测 `29464a50` |
| 口径 B：全语义含事件域（max_speed_probe，经 GameSession.step） | 4.3 t/s（翻转后复测） | `<PERF_B>`（已归档参考：P2-2b 78.1 mean@300 tick、69.5@1000 tick，P2-3a 再 +5%/+5.1%） | `evidence/perf-p2/MANIFEST.md` |
| 原型机制口径（不可与 A/B 并列） | rayon 形态 359 t/s | 所有权转移臂 962 t/s@10 组（纯机制税 ~8%） | ADR-0043 §2 三臂数据 |
| Sol 独立复核轮数 | — | `<SOL_ROUNDS>`（已归档参考：verdict 原文 6 份 + 台账内 D3a 中点~八审逐审链 + D3 四~九审 + 翻转两轮 + 性能归因终裁 + P2 独立复核轮） | §5 审查指引 |
| 实现 agent 换人次数 | — | `<ROTATIONS>`（已归档参考：≥11 任，handoffs 归档第 6→11 任交接） | `handoffs/` |
| 复核修复的真缺陷数 | — | `<DEFECTS>`（含翻转复审两阻断、P2 复审两阻断、D5 终审四轮、D3a 中点整改 5 项等——回填时按台账逐条计） | `ledgers/multi-tick-design.md`、`evidence/perf-p2/MANIFEST.md` §P2 复核 |

> A/B 两口径负载不同构（A=纯 retail、无 driver；B=含 inst/hot、竞价与事件上行），
> 禁止跨口径相减或换算（`plans/flip-runbook.md` §3）。

## 3. 语义合同：有意分歧与决策记录

- **有意分歧总账**：`plans/intentional-divergences.md` — 13 条有意分歧
  （A1–A13，其中 8 条已入 trading-rules.md / ADR，5 条为台账登记）+ 4 条旧管线
  登记缺口（B1–B4，随旧管线退役消亡）+ 11 组/15 站点 pin 等价锚（C1–C11，旧管线
  专有机制面，批 9 退役时随语义迁移）。合并前逐条核对登记状态与该总账一致。
- **ADR-0042**（`docs/decisions/0042-trade-maker-taker-leg-semantics.md`，accepted）：
  Trade maker/taker 字段语义统一为腿方向（maker≡卖腿 / taker≡买腿）；A 股无
  maker/taker 费差，腿方向无歧义且费用归户唯一化；前端消费面 30 处全量扫描零适配。
- **ADR-0043**（`docs/decisions/0043-std-thread-ownership-runtime.md`，draft）：
  本 PR 的运行时模型权威记录（拓扑/所有权转移/tick 协议/失败原子性/锁步不变量/
  同拍交付双写/panic 运输协议/OS 级 CPU 限制）。**合并时须把状态 draft→accepted 并
  按当时事实更新「后果」第 6 条（翻转现状）**。
- **wasm32 串行回退（方向 A，分歧 A12）**：wasm 宿主无 OS 线程，
  `step_runtime_enabled` 在 `target_arch="wasm32"` 恒 false，Web 目标 Stage 1 保持
  串行旧管线形态（engine/web-wasm 双包 wasm32 check 零错，`c5bb4a0b`；
  编译级断言 `wasm_serial_smoke` 承载可达性）。native 缺省翻转不影响 wasm。
- 批 10 文档同步（ADR-0043 落库、trading-rules 七条、architecture 八处）已随
  `7209fdfd` 入库。

## 4. 风险与回滚

- **运行时逃生门**：部署侧设 `ENGINE_STEP_RUNTIME=0` 即整进程回旧管线
  （`OnceLock` 首读生效，须进程启动前设置；批 9 外部清理时移除该门）。
- **源级回滚**：翻转提交 `7419c39b` 为单一目的提交，`git revert` 即净回旧管线缺省；
  回滚后验证矩阵见 `plans/flip-runbook.md` §5.1（V1–V3）。
- **回滚决策依据**：正确性事故（守恒红/事件语义错）→ 立即回滚；性能不达预期
  **不构成回滚理由**，走负结果归因流程。
- **已知性能债（P3/P4 剩余项，Sol 归因终裁路线图）**：P3=事件单遍派发、desk 公共簿
  聚合与 mirror/ledger 跨 tick 增量维护、chain 决策路径/个人态克隆链；
  P4=组数缺省推导标定（50~100 股/128 核场景）、分片与 affinity——按 profile 数据
  排序，不在本 PR 范围。路线图与依据：`reviews/sol-perf-attribution-verdict.txt` §2。
- **迁移期已知限制**（ADR-0043「后果」如实登记）：严格所有权下单轮资金不可复用
  （撤单释放次拍可用，trading-rules 已登记）；散户经历成交事件面 runtime 专项测试
  欠（分歧 A5，semantic-traceability R31）；simulation-diagnostics 面编译失修
  （先于本批，单独立项）。

## 5. 审查指引（复核链归档位置）

| 阶段 | verdict / 记录归档 |
|---|---|
| 总索引 | `agents/std-thread-migration/README.md`（plans/reviews/handoffs/ledgers/evidence/design-docs 导览） |
| 主台账（D0–D5 全链 + A/B 拍板 + Sol 中点~八审逐审记录） | `ledgers/multi-tick-design.md` |
| D3 迁移链 Sol verdict | `plans/sol-mid-d3-verdict.md`、`plans/sol-mid-d3-recheck-verdict.md`、`plans/sol-third-d3-verdict.md`（四~九审送审提示词在 `reviews/sol-{fourth..ninth}-d3-review-prompt.txt`，结论记录于台账） |
| 翻转批 Sol 复核 | `plans/sol-flip-review-verdict.md`、`plans/sol-recheck-verdict.md`（有意分歧 A2/A3 裁决来源） |
| 翻转执行与回滚手册 | `plans/flip-runbook.md`（前置检查/命令矩阵/两口径终测协议/Sol 终验收范围） |
| 性能负结果归因终裁 | `reviews/sol-perf-attribution-verdict.txt`（70–93% 可回收浪费判定 + P0–P4 路线图）；负结果链 `evidence/bisect-summary.txt` |
| 性能恢复战役红绿证据 | `evidence/perf-p1/MANIFEST.md`（P1-A/B/C）、`evidence/perf-p2/MANIFEST.md`（P2-1a/1b/2a/2b/3a + P2 独立复核轮两阻断修复记录） |
| 历史红证据（MANIFEST+SHA） | `evidence/sol-mid-d3-red-evidence-r3/`、`evidence/sol3-red-evidence/`、`evidence/flip-106-*` |
| 实现轮换交接 | `handoffs/handoff-*.md`（第 6→11 任） |
| 方案设计权威 | `design-docs/migration-design/F-converged-architecture.md`（收敛架构）+ `G-batch-plan.md`（批次计划） |
| 终验收 | `reviews/sol-final-acceptance-prompt.txt`（送审面；verdict 合并前归档至 `plans/` 并在此回填指针） |

---
*骨架生成于 2026-10-17，基线 HEAD=`32db0c6c`（main..HEAD 272 commits）。占位符共 6 处：
`<PERF_A>`、`<PERF_B>`、`<REGRESSION_FINAL>`、`<SOL_ROUNDS>`、`<ROTATIONS>`、`<DEFECTS>`。*
