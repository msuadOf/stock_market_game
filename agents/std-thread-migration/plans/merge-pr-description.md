# PR 描述骨架：exp/std-thread-model → main（std-thread 运行时迁移）

> 用途：合并 main 时的 PR 描述底稿。`<PLACEHOLDER>` 共 6 处，合并时以终态数字回填；
> 「已归档参考值」仅供回填时核对量级，不得直接冒充终值。性能 A/B 与战役数字为
> 已归档第一阶段终测实值（`evidence/perf-p2/perf-phase1-final.txt`，非占位）。
> 行文所引归档均相对仓库根 `agents/std-thread-migration/`。

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
而非方案必要代价；P0–P2 恢复战役第一阶段收官：**A 71→1409.2 t/s、B 4.3→302.7 t/s
（300 tick，70.4×；1000 tick 长跑 3.18→501.5，resting 增长税消除）**。翻转后收口链：
集合竞价单一清算价修复（41716af9，A 股制度级对齐）与玩家市价链路五提交闭环
（35fa8909→21b1e88b，Sol 五轮终审通过）；Sol 视图模型终裁
（`reviews/sol-view-model-verdict.txt`）认定双写补偿非语义必需、**视图内生立为合并后
下一战役**；协议宿主 step_frame 生产入口接轨 <STEPFRAME_NOTE>（终验收 §二.5 必查项）。

## 2. 关键数字（合并时回填）

| 项 | 旧值 | 新值 | 依据/已归档参考 |
|---|---|---|---|
| 分支规模 | — | `<BRANCH_STATS>`（已归档参考：21b1e88b 时 301 commits / 932 files（+207401/−25930），类型分布 feat 59 / fix 62 / docs 76 / perf 34 / test 31 / refactor 30 / chore 5 / build 1 / revert 2 / merge 1；step_frame 批落地后重取） | `git log main..HEAD`（merge-base `a6001494`） |
| 翻转批 env=1 失败 | 106 | 0 | `evidence/flip-106-failures.txt` → `evidence/env1-current-full.txt` 审计链 |
| 生产缺省 | 旧管线 | std-thread runtime（`step_runtime_enabled` 缺省 true，翻转提交 `7419c39b`） | `packages/engine/src/runtime/mod.rs` |
| 全量回归（lib，默认与 env=1 双绿） | — | `<REGRESSION_FINAL>`（已归档参考：1642 通过 / 0 失败 / 11 ignored 双模式 + 集成 93 绿 @21b1e88b） | 交接 `handoff-stepframe-integration.md` §1 |
| 口径 A：裸引擎探针（runtime_s5_probe，3000 tick/10 组×2000/5 股，直跑 World） | 71 t/s（翻转后复测） | **1409.2 t/s**（5 轮 1374.4–1440.0，采于 1f5863cd；守恒逐位 5/5 轮） | `evidence/perf-p2/perf-phase1-final.txt`；链 71→1253.5（P1-B）→1358（tag `perf/bare-1358x`）→1409.2 |
| 口径 B：全语义（driver_semantics_probe + driver-semantics-setup.json，1 组×20000 纯 retail，经 runtime driver） | 4.3 t/s@300 tick / 3.18@1000 tick（翻转后复测） | **302.7 t/s@300 tick**（5 轮 290.8–308.2）+ **501.5@1000 tick 长跑**（零衰减）——采于 1f5863cd | 同上账本；链 4.3→78.1（P2-2b）→309.1（P2-4b）→302.7（正式窗） |
| 性能恢复战役第一阶段（Sol 归因终裁 P0–P2 路线图） | A 71 t/s、B 4.3 t/s | **A 71→1409.2；B 4.3→302.7（70.4×）、1000 tick 3.18→501.5（157.7×）**；测量窗 load 15.6–18.7 外部负载在途、如实登记不择轮（净机窗 load~6 时 B300 曾 309–315） | `evidence/perf-p2/perf-phase1-final.txt`；`reviews/sol-perf-attribution-verdict.txt` |
| 集合竞价结算价+玩家市价路由 | 竞价成交按 maker 申报价结算（双边同价 fixture 巧合掩盖）；玩家市价/符号价意图静默丢弃、Lowest 误挂涨停 | 单一清算价结算（41716af9）+ 真路由/保护边界/逐请求回执/序号游标贯通/commit_from 缓存域交接（35fa8909→21b1e88b 五提交） | Sol 专项复核+五轮终审通过；红证 `.tmp/sol-103-red-evidence/`（合并前归档至 `evidence/`） |
| 里程碑 tag 链（复核锚点） | — | `perf/proto-962x`（7842a13b 原型 962）/ `perf/bare-627x`（5412fd10 P1 后 627–689）/ `perf/bare-1358x`（816cddb3 战役中点 1358）/ `perf/fullsem-300x`（6dcc25b4 P2-4b 后 B 300+） | `git tag -l 'perf/*'` |
| 原型机制口径（不可与 A/B 并列） | rayon 形态 359 t/s | 所有权转移臂 962 t/s@10 组（纯机制税 ~8%） | ADR-0043 §2 三臂数据 |
| Sol 独立复核轮数 | — | `<SOL_ROUNDS>`（已归档参考：verdict 原文 6 份 + D3a~D3 逐审链 + 翻转两轮 + 性能归因终裁 + P2 独立复核轮 + 视图模型终裁 + 竞价专项 + 市价链路五轮——回填时按台账逐轮计） | §5 审查指引 |
| 实现 agent 换人次数 | — | `<ROTATIONS>`（已归档参考：≥11 任，handoffs 归档第 6→11 任交接；step_frame 批第 15→16 任） | `handoffs/` |
| 复核修复的真缺陷数 | — | `<DEFECTS>`（含翻转复审两阻断、P2 复审两阻断、D5 终审四轮、D3a 中点整改 5 项、市价五轮各批阻断项——回填时按台账逐条计） | `ledgers/multi-tick-design.md`、`evidence/perf-p2/MANIFEST.md` §P2 复核 |

> A/B 两口径负载不同构（A=纯 retail 直跑 World、无 driver；B=1 组×20000 经 runtime
> driver 全语义），禁止跨口径相减或换算（`plans/flip-runbook.md` §3）。B 300 tick
> 全处集合竞价申报窗（fees=0），不可当连续竞价成本代表；数字采于 1f5863cd，其后
> 市价链路/step_frame 批若复测以新账本覆盖。

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
  按当时事实更新「后果」第 6 条（翻转现状与 step_frame 接轨态）**。
- **wasm32 串行回退（方向 A，分歧 A12）**：wasm 宿主无 OS 线程，
  `step_runtime_enabled` 在 `target_arch="wasm32"` 恒 false，Web 目标 Stage 1 保持
  串行旧管线形态（engine/web-wasm 双包 wasm32 check 零错，`c5bb4a0b`；
  编译级断言 `wasm_serial_smoke` 承载可达性）。native 缺省翻转不影响 wasm。
- 批 10 文档同步（ADR-0043 落库、trading-rules 七条、architecture 八处）已随
  `7209fdfd` 入库；集合竞价单一清算价随 `41716af9` 登记 trading-rules.md（沪深
  依据附引用）；市价申报回执面以 §5 市价五轮送审链为复核权威。

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
- **合并后第一战役=视图内生读面**（`reviews/sol-view-model-verdict.txt` §4 排序）：
  owner 原生维护读面+统一发布版本，删除会话镜像安装链与双写补偿——不属本 PR
  范围，以其为前置的批 9 拆分口径见终验收 §三.3。
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
| 性能恢复战役红绿证据与终测账本 | `evidence/perf-p1/MANIFEST.md`（P1-A/B/C）、`evidence/perf-p2/MANIFEST.md`（P2-1a…4b + P2 独立复核轮两阻断修复）、`evidence/perf-p2/perf-phase1-final.txt`（第一阶段正式终测：A 1409.2/B 302.7/501.5+binary SHA+测量窗声明） |
| 市价链路与竞价修复 Sol 五轮终审 | 送审提示词链（五轮）：`.tmp/sol-auction-fix-review-prompt.txt` → `.tmp/sol-market-{fix-recheck,third,fourth,fifth}-review-prompt.txt`；红证 `.tmp/sol-103-red-evidence/`（sol2nd–sol5th 家族，含完整 panic 栈）——**合并前归档至 `reviews/`+`evidence/` 并回填本行指针** |
| 视图模型终裁 | `reviews/sol-view-model-verdict.txt`（e0258c2b 归档：双写质疑部分成立、视图内生=合并后下一战役、step_frame 生产入口分叉发现与收尾排序） |
| step_frame 接轨批 | 交接 `.tmp/handoff-stepframe-integration.md`（调用链地图/边界坑清单/验收口径）；前置 `plans/step-frame-integration-prerequisites.md`；批尾与两路径对照结果以终验收 §二.5 为准（合并前归档交接稿并回填指针） |
| 历史红证据（MANIFEST+SHA） | `evidence/sol-mid-d3-red-evidence-r3/`、`evidence/sol3-red-evidence/`、`evidence/flip-106-*` |
| 实现轮换交接 | `handoffs/handoff-*.md`（第 6→11 任） |
| 方案设计权威 | `design-docs/migration-design/F-converged-architecture.md`（收敛架构）+ `G-batch-plan.md`（批次计划） |
| 终验收 | `reviews/sol-final-acceptance-prompt.txt`（送审面：翻转+战役批链+性能账本+step_frame 必查项；verdict 合并前归档至 `plans/` 并在此回填指针） |

---
*骨架生成于 2026-10-17（基线 HEAD=`32db0c6c`，272 commits）；2026-10-18 更新（基线
HEAD=`21b1e88b`=301 commits，市价链路五提交后；step_frame 接轨批在途，落地后回填
`<STEPFRAME_NOTE>` 并重取分支规模）。占位符共 6 处：`<BRANCH_STATS>`、
`<REGRESSION_FINAL>`、`<SOL_ROUNDS>`、`<ROTATIONS>`、`<DEFECTS>`、`<STEPFRAME_NOTE>`。*
