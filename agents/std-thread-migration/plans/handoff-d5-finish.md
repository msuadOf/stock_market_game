# 交接：D5 清零收尾（第 10 任→第 11 任，2026-10-16）

## 1. 当前状态

- **HEAD**：`d7c75e2b`（自成交拍板 A——双轨 fixture+restore 链五层深修）。
  工作区**干净**（git status 全空；.tmp 不入库）。
- 数字基线：默认 lib **1605/0/11**；env=1=**27**；翻转批 1-5 = **5/5** 全绿
  （10/13/14/14/15）。
- 本会话提交链（D5 段，自 5a4040ca 后共 15 个）：
  9451e435(件4红证)→93fe882a(复审接线)→8f682d53(fixture 适配)→
  e9587555(qty0)→1d9fd529(日界)→8e801ceb(复核修复)→0be48150(存档族)→
  d755dfb1(adopt 直入)→fb39814e(#2 evaluate)→c2f5bc0f(翻转批适配)→
  ed3bacc3(#1 Trade 契约)→b3e7bffe(#3 envelope)→bd3aa85a(#5 pin)→
  d7c75e2b(自成交 A)。
- 两轮独立复核全过（code-review agent：件 4 批+D5 #1/#2/#3 批——
  1 阻断+7 建议全部修复并复验确认）。

## 2. 剩余 27 例分域清单

**主台账**：`.tmp/d5-triage.md`（全程逐域更新至 d7c75e2b，含每域判型、
红因、修法方向、拍板记录）。此处只列域与计数：

| 域 | 例数 | 状态 |
|---|---|---|
| real_step restore 最后一层（持仓双计） | 1 | 见 §4 |
| retail belief 链专项（#4） | 5 | retail_analysis 1+t3_boundary 3+npc_working_quote 1——见 §3 |
| attention 公开可见性门禁（#7） | 3 | desk 竞价隐藏簿/开盘揭示语义迁移——**必须与披露面同捆做**，翻转前硬门 |
| #6 改造 | 3 | two_npc（staged 消费面 runtime 已有+断言读面换 world_public_views）/cross_stock（MutatePlanBook+external 挂单——Trade 契约已腿方向化，断言写 taker=买腿）/live_plan（restore 价格历史已修 fault2010——看剩什么，可能已被 d7c75e2b 连带清） |
| #8 采纳机制 | 2 | ordinary_adoption ×2——组内无外来单认领计划腿面；小迁移（record_child_submitted 外来触发）或登记 |
| epoch 双轨 | 1 | 见 §3 依赖注记 |
| envelope/信封域残余 | 2 | book_auction_boundary（信封批预算）+fee 审计深断言——envelope 投影已落地，看具体断言差 |
| 旧管线机制 pin 后剩面 | 10 | committable 2/npc_prep 2/quota 1/npc_request 1 已 pin 跳过（env=1 绿）——**27 计数含默认侧仍跑的**；env=1 实际红名单以命令为准 |

**开工第一步**：跑 `ENGINE_STEP_RUNTIME=1 cargo test -p engine --lib` 取
当前精确名单（上表计数是台账推断，可能因 d7c75e2b 的连带修复有出入）。

## 3. retail 链专项开工位（#4，5 例）

**已证归因**（多轮前）：retail 位的 belief 消费链未迁移——组内 retail 走
decide_data（纯策略），不消费 disclosure/information/entry 创建。

**开工位**：
- 组内 belief_states 目前只装 inst 位（chain_local 判定=belief_states
  有值）。retail 位需组内 belief 态+disclosure 投递消费+个人分析
  （session 版 capture_retail_analysis 的生产链为参照——
  `session/decision_chain/retail_analysis.rs`）。
- **纠缠已解**：epoch rng 调度公式已真迁（fb39814e——组内
  evaluate_attention 与旧管线 evaluate_candidate_with_signal 同式），
  retail 专项不再被调度公式阻塞。
- **epoch 双轨例**（1 例）的遗留深因=staged 消费 seam 与组 evaluate 的
  排期交错+快照一拍差（live 轨组构造于 step_frame 内 vs twin 构造于
  restore 后）——建议在 retail 专项做完后单独收口（同属调度/观察时序域）。
- retail_analysis 测试（`public_step_consumes_retail_beliefs_and_replays_
  after_restore`）直改 session belief_participants[AccountId(1)] 后 step
  断言 entry 创建——组内等价需 retail 位 belief 演进+回写。

## 4. restore 链最后一层（real_step 1 例）

**已收口五层**：fault2010（desk recent 种子）→2226（adopt 清旧簿+双域
一致）→2218（build_ledger saved-only 重建）→2220（证明空窗登记——下一
runtime_step 由 desk 权威重建）→事件对拍。save+restore+双轨 step 事件
**已逐位相等**。

**剩最后一层**：`restored continuation must commit: 同拍投影持仓下穿：
AccountId(1) StockId(0) qty=-1`（apply_same_tick_fill_projection）。
- **定位**：restore 后 desk 种子卖单（从 ledger 键恢复，100 股）在续拍
  成交时，组初始持仓投影（from_session 的 initial_shares=session
  positions=100）被 desk 成交回写扣减 100 → 0，但同拍投影再扣一次 → -1。
- **修向**：desk 种子单成交的扣减次序——组初始持仓已含"挂出中"的单量
  语义 vs desk 成交回写视为增量扣减，两者双计。对照未 restore 的 live
  轨（同拍无此错）：live 轨组持仓从构造期投影起演进（卖 200 成 100 →
  组 100），desk remaining=100；restore 轨组初始=save 时的 100（投影）
  +desk 种子 100——live 轨的组持仓与 desk 单是**同一演化的两面**，
  restore 重建时两面各自从 save 恢复后成交扣减双计。修法二选一：
  A) restore 后的 desk 种子成交回写走"替换"而非"扣减"（组持仓以 desk
  权威重算）；B) from_session 的 initial_shares 在 restore 形态下扣去
  ledger 种子量（单量不双承载）。建议 A（组持仓权威已在 desk 侧）。

## 5. 已知坑增量（本轮新增）

1. **install 段序**：runtime_step_with_install 内的 envelope 投影/诊断
   派生段必须在 install_outcome **之后**（sol3/sol3b 的安装失败注入测试
   依赖 install 前段不产生副作用——此前段序错把注入测试挤成 Ok）。
2. **from_session 改 &mut**（adopt 后清旧簿需要）波及 **52 处调用点**——
   已全改（`from_session(&X)`→`from_session(&mut X)`），新调用点注意。
3. **双轨 fixture 模式**（自成交拍板 A 的产物）：runtime 分支走双账户
   种子卖（seed_order_for_test——enqueue 仅玩家位）；默认分支保留原
   单账户 enqueue。三例已落地（fee_audit×2+real_step）——新 fixture
   遇"旧管线结算 epoch"面时同款分流。
4. **observe 门禁的 fixture 生态**（fb39814e 后）：缺省决策概率化——
   翻转批固定 seed 断言改"候选序搜索"（batch3×3+batch4）；新 fixture
   需要确定性决策时用 ForceDue 的 ensure_observe（force_due_for_test_at
   默认 true）或 attention_base=1.0。
5. **attention 门禁（#7）必须与披露面同捆**——A 股集合竞价时段不揭示
   买卖盘是真实交易制度面，协调者明示翻转前硬门，不可永久搁置。
6. **数量计数以命令为准**：台账域计数是推断（d7c75e2b 连带修复可能有
  出入——live_plan 的 fault2010 已修，先跑名单确认）。

## 6. 清零后队列

1. env=1 清零（27→0）后**立即停**（不翻默认）。
2. 接 **wasm 方向 A**：`.tmp/wasm-gap-verification.md`（engine wasm32
   编译必红 25 错——去掉 world 的 wasm 门控让其编译，session 在 wasm 上
   保持 runtime_driver=None 走串行管线；约 0.5~1 天；翻转批 P4 硬门；
   检查用 +nightly-2026-09-05）。
3. 然后停等协调者编排翻转批（`.tmp/flip-runbook.md` 已备：默认翻转+
   全量回归+关 Echo 终测+Sol 终验收+五风险清单）。
4. simulation-diagnostics 编译失修（c79f051c 起 17 错）单独立项、翻转
   批后处理，不混入 D5。
5. maker/taker 消费面：ADR-0042 已登记（docs/decisions/0042——无撞号，
   0041 是 stock-numeric-identity）；清单 `.tmp/maker-taker-consumer-scan.md`；
   Event.ts 重生成待翻转批统一做。

## 硬性纪律（沿承）

- 红证据：完整 cargo test 输出+命令+SHA，存 `.tmp/sol-d3b-d5-red-evidence/`。
- 汇报只写断言到的面；失败就说失败。
- 每批独立 subagent 复核（CLAUDE.md 门禁）。
- 一件一提交，显式 pathspec（禁 git add -A）。
