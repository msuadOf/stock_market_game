VERDICT: APPROVE

# 任务 20 独立复核 — 「将真实经历接入信心、忍耐与风险压力」

- 复核对象：commit `4e830c5` (`feat(engine): 用真实经历调整个体耐心与风险行为`)，branch `codex/feat/web-ui-polish`，parent `3bb5096`
- 复核人：独立 subagent（未参与实现）。计划勾选状态不作为证据；本文件为实际门禁。
- 隔离执行：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-20 4e830c5`（成功，无需 git archive 兜底）。主树零 cargo / 零 git 写。
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t20r-{00..07}-*.txt`、`t20r-04-full.txt`、`t20r-05-check.txt`。

## 1. 隔离重跑记录（命令 + exit code + 数量）

| # | 命令（worktree 内） | exit | 结果 |
|---|---|---|---|
| 1 | `cargo test -p engine --test experience_feedback` | 0 | **25 passed / 0 failed / 0 ignored** |
| 2 | `cargo test -p engine --test experience` | 0 | **7 passed / 0 failed / 0 ignored**（既有套件原样，未被触碰） |
| 3 | `cargo test -p engine`（全量） | 0 | **695 个测试条目 = 691 passed + 4 ignored + 0 failed**（27 个测试二进制 + doc-tests 0）。4 个 ignored 为 session 既有 release-mode 压力门（2/5/10 万账户 + 2 万散户成本探针），设计如此 |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | 四 crate 全部 Finished，无警告输出 |

关键套件（隔离复验）：extraction_replay **3/3 绿**（含 `identical_construction_replays_bit_identical`）；technical_memory **33/33**（任务 19 兄弟模块无回归）；behavior **35/35**；plans **33/33**；session 116（112+4i，含 `restore_rejects_inconsistent_retail_experience_lifecycles` 等既有守卫）。

### 数量对账（任务指令预期 "≈693 = 660 + 33"）
- 实际：695 = 670（4e830c5 既有基线，含已提交的任务 19/21 产物）+ **25**（experience_feedback 新增）。
- 指令估计的两个分量都不准：新增套件是 **25** 不是 33（plans 与 technical_memory 各有 33，疑为误读来源）；基线是 670 不是 660。总数 695 ≈ 693 属巧合接近，无真实缺口。
- 与 worker 证据（task-20-happy.txt，715 条目 = 711 过 + 4 忽略）差异 = **+20 real_estate_accounting**，系并发 sibling agent 未提交文件在其主树被 cargo 自动发现；不在 4e830c5 中，我的隔离树正确地没有它。两边全绿，完全可对账。
- 双方独立运行均报 experience_feedback 25/25、experience 7/7 —— worker 证据与隔离复验一致。

## 2. AGENTS.md 三问

### Q1 是否符合大 A 语义，依据是否可靠
**符合。** 本改动不触任何交易所规则（涨跌停/T+1/撮合/费用均零改动），领域语义是 ADR-0013/K5（计划行 136）既定的个体心理模型：120 市场分钟退出冷静期是游戏既定心理机制而非交易所制度，`POST_EXIT_COOLDOWN_MINUTES = 120` 未动。K1 三时钟纪律（公历日 / 市场分钟 / 交易日序各自单调、互不换算）延续 calendar 模块既有建模，calendar 套件 10/10 绿。无跨板块规则平均化、无声默 fallback；所有非法输入/时序均为类型化 `ExperienceError`（7 个新变体，全部有测试命中）。长期被套、失败衰减均以「本人观察/真实成交」为锚，不虚构行情经历。依据链：K5 行 136 锚点 + ADR-0013 + notepad learnings 固定口径，可靠。

### Q2 改动是否为需求所必需、是否保持最小范围
**是。** 9 文件全部位于 experience 模块及其新测试目录；唯一既有文件 experience.rs 净 +40/−2。−2 是任务 19 遗留的两行模块文档占位（「经历内部结构…由任务 20 继续」）被更新为 feedback 子模块登记——**无任何代码行被删除**（diff hunk @@ -2,15 +2,22 @@ 核实）。behavior/、session/、plans/、price_memory.rs 零触碰。失败确认严格镜像 legacy 计数增点（`record_fill_dated` 在调用 `record_fill_with_order` 前预判、成功后登记），无第二条计数路径。清仓 cooldown 从 legacy 字段读回（`self.stocks[code].cooldown_until_market_minute`），不二次计算/二次溢出路径。复用而非复制：`consecutive_failed_buys` / `peak_equity` / 成本由读取方传入，未另建账户损益副本。

### Q3 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度
**未发现阻断项。** 边界电池齐备（见 §4 失败路径表）。跨层：行为接缝零改动（diff 不含 behavior 文件）+ 会话接线显式推迟到 25/26 + 存档恢复 validate 推迟到 27 —— 三处推迟均在模块文档、commit message、issues.md §3 登记，非静默。复杂度：写入三段式（纯守卫→legacy 原样→提交段）沿用 ADR-0013 风格；facts/derived 分离避免第二份会漂移的状态。两个非阻断观察见 §6。

## 3. K5 行 136 逐锚点核验

| 锚点 | 实现 | 测试证据 |
|---|---|---|
| 复用失败买入、成本、浮盈回吐、冷静期、账户峰值 | 计数/峰值复用 legacy 字段；giveback 逻辑未触碰 | behavior `observed_profit_giveback...` 35/35 绿 |
| 新经历状态：失败事件日期 + 忍耐/信心/风险压力 | 事实入状态（`failure_events`/`stocks`/`exit_records`），档位读取时派生 | reads.rs 全 7 测 |
| 日历时间与市场分钟分开 | `ExperienceMoment{civil_date, market_minute, trading_day}` 三分量各自单调、互不换算 | clocks.rs 三独立回拨拒绝 + market_minute 测试断言峰值未被推进 |
| 实际买入失败由成交后本人观察确认；部分成交同订单去重 | adverse = 观察 ≤ 买入价 95%（`observe_position_dated` 镜像）或亏损卖出首笔；`first_fill_of_sell_order` 去重 | `partial_fills_of_one_order_confirm_failure_exactly_once` |
| 每 20 交易日无新受挫减弱一档，不删真实亏损 | `failure_influence = counter − (as_of−最近受挫)/20`，饱和于 0，计数与登记不动 | `failure_influence_decays_one_tier_per_twenty_trading_days`（3→2→1→0→0）+ 事实保留断言 |
| 清仓保留账户经历 | `exit_records` 追加式；账户级计数不清 | `reentry_after_clearing_keeps_calm_down_history` |
| 120 市场分钟冷静期保持 | ExitRecord.cooldown 从 legacy 字段读回（=分钟+120） | 同上测试断言 `12+120` 且活跃冷却语义原样 |
| 长期被套 = 持满 20 交易日 + 低于成本的本人观察 | `held_days ≥ 20 && last_own_observation.price < 读取方传入权威成本`；成交价亦本人亲历价 | `long_stuck_requires_twenty_trading_days_and_below_cost_observation`、`fills_themselves_are_own_observations...`、`opening_allocation_counts_toward_long_stuck` |

## 4. 计划验收/失败电池逐条核验（均有真实断言，非口头声明）

Acceptance（seam.rs / main.rs）：
- 同损益不同经历受控风格选择不同 → `same_pnl_chooses_differently`：fresh → `TryBuy/Pullback`，scarred（3 次真实受挫，经**新写入路径**构建）→ `Watch/LowConfidence`；同权益/同风格/同 FixedRng。走既有 `decide_retail_position_with_experience`，读缝为 heuristics.rs `apply_experience_confidence`（`consecutive_failed_buys ≥ 2` → 1/(n+1) 重试概率）——与断言值精确吻合。
- 补仓不清除账户损失 → `adding_to_a_position...`（计数、失败登记、入场周期三保留）。
- 清仓再入不抹冷静期历史 → `reentry_after_clearing...`（exit_records 保留 + cooldown_until 不变）。
- 未成交计划不记失败 → `unfilled_or_cancelled_orders...`（无写入路径可感知未成交订单 77）。
- 保留非恐慌者不必止损 → `panic_exits_the_same_loss_that_long_term_holds`（Panic→Exit vs LongTerm→Hold，同状态同损失）。

QA failure 指定四类（failures/）：
- 部分成交重复计次 → `partial_fills...exactly_once` ✓
- 未来经历 → `as_of_before_recorded...on_both_clocks`（三时钟分量各拒 + is_long_stuck 同守卫）✓
- 丢失持仓生命周期 → `buy_from_zero_over_an_active_entry` / `exit_without_an_active_entry` / `observation_without_an_active_entry` ✓
- 非法峰值/衰减时间回拨 → `civil/market_minute/trading_day_clock_rejects_backwards_events`（峰值未推进断言）+ 衰减即 as_of 守卫 ✓
- 加项：非正金额、分钟溢出不动 feedback、篡改存档 `validate()` 拒绝、多股隔离（A/B 生命周期不串扰）。

## 5. 登记偏离裁决（§6 指定）

1. **读侧输入未接入目标/紧迫度（计划字面「将字段真正接入目标/紧迫度输入」）→ 裁决：可接受，非欠交付。** 依赖矩阵 20 阻塞 22/23：目标/紧迫度消费方在 22/23 才存在，现在接入等于前置越界。本提交给出完整读侧契约 inputs.rs：`failure_influence(as_of)->u16`（含衰减）、`is_long_stuck(code, cost, as_of)->bool`（成本由调用方权威传入，无复制）、`experience_drawdown_from_peak(equity)->Option<f64>`（无峰值诚实 None）——22/23 无需重开 experience 模块即可消费，且 as_of 双时钟守卫已就位。缝测试证明新写入路径确实驱动既有行为分叉（scarred 状态经 `record_fill_dated`/`observe_position_dated` 构建，经 `consecutive_failed_buys` 读缝产生 Watch/LowConfidence）——新字段并非全然休眠。**休眠部分如实登记**：`failure_events` 日期/epoch/exit_records 及三个派生读取在 4e830c5 无生产调用方（grep 全 src 仅定义处），已在 commit message、issues.md §3、learnings「任务 22/23 接线时直接查这里」三处显式移交。残余风险：若 22/23 不消费则衰减/被套成死代码——由计划依赖与 learnings 指引缓解，非本提交缺陷。
2. **`#[serde(default, skip_serializing_if = "ExperienceFeedback::is_empty")]`（issues §2 请裁决）→ 裁决：正确选择。** 首次全量跑确曾红（FNV 4093535087516938092 ≠ 锚 1702442567969422992，issues §5 与 learnings 交叉印证）；该选择使旧档字节不变、锚测试**未改一字**而回绿——我隔离复验 extraction_replay 3/3 绿。旧格式缺 feedback 字段经 `serde(default)` 恢复空反馈，有专测（`feedback_state_survives_serde_roundtrip_and_old_saves_default_it`）。K1 不留迁移器纪律成立；20k 散户存档体积接线前零增量（B03 关切）。TS 端可选字段已为任务 29 登记。
3. **experience.rs 309 纯行超 250 天花板 → 裁决：接受登记。** 任务 19 已 290 纯行（b1f638d 复核 APPROVE 先例 +7）；本任务规格强制在模块入口加字段/错误变体/接线，净增 ~19 行，新逻辑全部落在 feedback{,/lifecycle,/inputs}.rs（144/145/37 纯行）。建议：下次再触碰 experience.rs 既有内容时先按责任拆分（登记既有，不阻断）。
4. **-2 删除行 → 已核：仅两行过时模块文档**（任务 19 的「由任务 20 继续」占位），无代码删除。
5. **commit message 三项声明全部属实**：(a) 行为接缝零改动——diff 无 behavior 文件，分叉经既有函数验收；(b) 衰减/被套/风险压力留给 22/23——grep 证实零生产调用方；(c) 默认空反馈不序列化、旧档字节与锚不变——serde 属性 + extraction_replay 绿 + 旧档默认测试。

## 6. 非阻断发现（移交后续任务，无需改本提交）

1. **25/26 接线陷阱（预期性提示，非缺陷）**：legacy 与 `*_dated` 写入路径混用会使持仓生命周期守卫以 `ActiveEntryAlreadyExists`/`NoActiveEntry` 类型化爆炸（如经 legacy 清仓后 dated 再建仓）。这是刻意的响亮拒绝而非静默腐化（铁律 2 正确姿势），但 25/26 接线必须**一次性全量切换**三类调用（fill/observe/init），不得半切换。已在模块文档隐含，此处显式留给 25/26。
2. **22/23 消费义务**：`apply_experience_confidence` 仍读未衰减的原始 `consecutive_failed_buys`（issues §3 有意过渡态）；22/23 须决定是否切换 `failure_influence`，否则衰减特性不可被行为观察。

## 7. 质量探针汇总

- 确定性：experience 模块 grep `rand|Instant|SystemTime|std::time|std::fs|std::net|unsafe` → **零命中**。
- 250 行天花板（git diffstat 权威值）：feedback.rs 229、lifecycle.rs 193、inputs.rs 55、main.rs 241、seam.rs 246、reads.rs 193、failures/mod.rs 244、clocks.rs 133 —— 全部 ≤250；experience.rs 384 原始/309 纯行超限已登记（§5.3）。
- 任务 21 教训核查：全部合法流程可达（建仓→加仓→观察→清仓→再入在 main/reads 测试中完整走通）；守卫拒绝后状态原样（多处显式断言），无合法流被死锁。
- TDD：worker 证据含 red 运行 exit 101（104 个 E0432/E0599/E0609，全部指向任务 20 API 面）→ green；时间线（测试 03:27-03:32、实现 03:35-03:36、证据 03:38、提交 03:42）自洽。red 日志为摘录，无法独立重放时点，但 API 名与最终面吻合且绿态已独立复验——接受。
- 不予拒绝项（按指令）：clippy behavior/decision.rs:239 既有项（任务 42 登记）；sibling real_estate 未提交噪音（不在 4e830c5）。

## 8. 结论

三问全部通过；指定验收/失败电池每条有真实断言；三项登记偏离经裁决均可接受且移交路径清晰；隔离重跑四命令全绿（25/7/695(691+4i)/check×4 crates）。**APPROVE**。任务 20 门禁满足，22/23 依赖解锁。

— 独立复核 subagent，2026-09-11。工作树 `wt-review-20` 保留于 Temp 供抽查（只读，不影响主树）。
