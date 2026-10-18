# 交接：D5 第 11 任收尾（2026-10-16，HEAD=c5bb4a0b）

## 1. 最终状态

- **env=1：27→9**；默认 lib **1605/0/11**；翻转批 1-5 **5/5 全绿**
  （10/13/14/14/15）；**wasm 方向 A 已落地**（engine/web-wasm 双包
  wasm32 check 零错+test build 零错+突变复核红）。
- 本任提交链（8 个）：
  6e265f92(pin 补齐 6 例)→e026df4b(restore 最后一层)→1047e693(ledger
  校验分支+3 pin)→4b33eee2(retail 链组内化)→cf2af935(回滚基线扩)
  →035489a7(two_npc 读面)→c5bb4a0b(wasm A)。
- 红/绿证据：`.tmp/sol-d5-red-evidence/`（env1-27 全名单、01-restore
  红/绿、env1-final-11th、wasm 双包绿+突变注记）。

## 2. 已修分域与根因（供复核）

| 域 | 例数 | 根因/修法 |
|---|---|---|
| 旧管线专有 pin | 6 | **第 10 任报告失实**：committable×2 只加注记无跳过块、npc_prep×2/quote_expiry/book_auction 完全没 pin——补 env=1 跳过块（默认侧真跑全绿） |
| restore 最后一层 | 1 | save 契约仅存 charged——restore 硬编码 remaining=1 股占位；扩 audit 数值面（remaining/filled/limit/purse）+组态投影六件（卖余量入 initial_shares/买 purse 扣现金+登记/frozen_today/DeskMarketSeed 日内行情种子/accepted_delivered 不重放/seed 混入回传逆运算） |
| envelope/注入面连带 | 4 | runtime 形态（旧簿空）误入 book↔ledger 对拍（validate 分支）；restructure/unified×2/unified_post=step_with_commit_evidence+commit_injected 注入面旧管线专有（pin+登记；**新 pin 判定用 step_runtime_enabled()——翻转改缺省后自动正确，既有 var_os 判定的翻转适配归翻转批**） |
| retail 链 | 6 | **组内化**（见 §3）；t3 的 full_day 红根因=world_day_end_for_step 死代码未接线（日界拍 desk 残留） |
| compute_context | 1 | 回滚基线缺 belief_participants/retail_experience/history_reads（retail 链写入面泄漏） |
| two_npc | 1 | staged 消费面早已迁移；纯旧簿读面——新增 cfg(test) 读面 runtime_resting_book_for_test |

## 3. retail 链组内化（第 11 任主件，复核重点）

- **投影**：belief 位放开机构限定；BeliefParticipantSnapshot 扩
  retail_experience（构造投影+回写**合并**——merged_with_group_authority
  保组外键）；RootProjection.session_seed；NpcProfile.retail_style。
- **retail_analysis 参数化重构**：观察/信号/成本三核改
  RetailAnalysisCtx（view 依赖一次性提取）——session 薄壳+组线程同核。
- **组内 run_retail_decision**（group.rs）：经验行 capture 演进（equity/
  持仓 dated/首建）→观察 core→behavior 决策（decide_retail_position_
  with_experience+apply_personal_analysis+risk）→trace 收集→组态写回。
  own/risk 自 ctx.positions 同源（build_self_view 零量股集合漂移 panic）。
- **trace 上行**（EchoRetailTraces 通道）+driver 段投影 session.
  last_retail_decisions；Start 清组内缓冲（瞬态语义）。
- **DeferDueForTest**（cfg(test)）：组内"无 due tick"形态通道。
- **首买经历 init 延后**（个人态回写后执行）——整态覆盖不再冲掉。

## 4. 剩余 9 例分域（下一任开工位）

| 域 | 例数 | 开工位 |
|---|---|---|
| attention 公开可见性门禁 | 3 | **真迁中件**（翻转前硬门）：断言面=旧簿（resting_order_count/snapshot bids/asks 门禁）+公开信号（imbalance 不吃隐藏单）+player_working_orders。runtime 缺：①session 侧公开投影（竞价相位隐藏/揭示——book_public_cutoff_sequences 语义）②组内观察 imbalance 的相位门禁③desk 簿的 session 私有镜像或断言读面迁移（runtime_resting_book_for_test 已有）。红形态：resting_order_count=0/索引越界 |
| pre_open 拒收回执 | 1 | authoritative_tick :439（竞价相位拒收无 IntentRejected 回执事件）——desk 竞价拒收回执核对（event_bridge 的 receipts→拒收投影） |
| ordinary_adoption ×2 | 1+1 | 采纳机制（外来单认领计划腿）——组内 record_child_submitted 的外来触发等价 or 登记（台账 #8 原判） |
| cross_stock | 1 | #6 改造（MutatePlanBook 通道+external 挂单）；Trade 契约已腿方向化（断言写 taker=买腿） |
| live_plan | 1 | restore 后二拍 Trade 缺失（continuous_tick_transaction :577）——restore 链已修五层+save 契约，重跑判型（可能与 restore 最后一层同族或独立） |
| epoch terminal | 1 | epoch_alignment :1172（rng 终态 left=(129,…) right=(149,…)）——判型关键：**retail 链已组内化（rng=npc_seed 派生同式）——重跑可能已变化**；若仍差=组内/旧管线 rng 消耗序差（决策流不回写、attention 流回写——查 A 类任务的 attention 消耗点） |

## 5. 已知坑增量（本轮）

1. **fixture grant 时序**：driver 构造后的 grant_position 组不感知
   （投影外注入）——挂单被组端 sellable 校验跳过（sol6th 先例）。
2. **observe 门采样非断言面**：runtime 首拍快照（构造期空簿）与
   session 簿 signal 面不同——fixture 用 ForceDue（ensure_observe）或
   两拍形态；**直改 base_probability 会撞 save fault2210**（策略/关注
   概率一致性校验）。
3. **retail behavior 语义**：风控行为（止损/止盈减仓）不被到达门控
   ——"到达率 0 零单"类断言需收窄为买入受理；NPC 到达不再保证成交
   （等价锚=restore 双轨对拍，不逐位对拍旧管线——构造期观察面差异）。
4. **pin 判定两式并存**：新 pin 用 step_runtime_enabled()（翻转安全）；
   既有 var_os 判定在翻转（缺省改 true）后会失效/反转——**翻转批必须
   统一**（翻转提交里批量替换或 R1 前处理）。
5. **flaky 家族新增**：watchdog_timeout（偶发）、deferred_drop 的
   disconnected（竞态族）——单跑复验绿即登记。

## 6. 队列（沿承+更新）

1. 剩余 9 例清零（§4 顺序）→ 复核（CLAUDE.md 门禁：未实施 subagent
   独立审 diff——**本任 8 提交尚未复核**，翻转前必须补）。
2. wasm 方向 A 已落地；CI 面（ci.yml 的 wasm 构建）建议翻转批前实跑
   一次核实（.tmp/wasm-check-target 的本地 check 已过）。
3. 翻转批（.tmp/flip-runbook.md）：注意 §5.4 的 pin 判定统一+第 10 任
   报告失实教训（env=1 计数以命令为准）。
4. simulation-diagnostics 编译失修（c79f051c 起 17 错）翻转批后处理。
